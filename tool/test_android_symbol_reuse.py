"""Run the APK workflow scripts with a fake compiler; never build or sign an APK."""

from __future__ import annotations

import os
import shutil
import subprocess
import tempfile
import textwrap
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASH = (
    str(Path(os.environ.get("ProgramFiles", "C:/Program Files")) / "Git/bin/bash.exe")
    if os.name == "nt"
    else shutil.which("bash")
)
WORKFLOWS = {
    "build.yml": "Build split and universal Release APKs without release secrets",
    "release.yml": "Build signed split and universal APKs",
}
SYMBOLS = {f"app.{abi}.symbols" for abi in ("android-arm", "android-arm64", "android-x64")}
COMPILER = r"""
fixture_calls=0
flutter() {
  fixture_calls=$((fixture_calls + 1))
  local output=''
  for argument in "$@"; do
    case "$argument" in --split-debug-info=*) output="${argument#*=}" ;; esac
  done
  test -n "$output" || return 91
  printf '%s\n' "$output" >> "$RUNNER_TEMP/compiler-inputs.txt"
  mkdir -p "$output"
  if [ "$FIXTURE_MODE" != missing ]; then
    for abi in android-arm android-arm64 android-x64; do
      printf 'build-%s' "$fixture_calls" > "$output/app.$abi.symbols"
    done
  fi
  printf 'inert private material' > "$output/never-copy.keystore"
  if [ "$FIXTURE_MODE" = fail-second ] && [ "$fixture_calls" -eq 2 ]; then
    return 17
  fi
}
"""


def workflow_script(filename: str, step: str) -> str:
    workflow = (ROOT / ".github/workflows" / filename).read_text(encoding="utf-8")
    block = workflow.split(f"      - name: {step}\n", 1)[1].split("\n      - name:", 1)[0]
    return textwrap.dedent(block.split("        run: |\n", 1)[1])


@unittest.skipUnless(BASH and Path(BASH).is_file(), "native bash required for workflow fixtures")
class AndroidSymbolReuseTests(unittest.TestCase):
    def run_workflow(self, filename: str, mode: str = "success"):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        result = subprocess.run(  # noqa: S603 - Fixed local Bash with fake Flutter, no key or SDK access.
            [
                BASH,
                "--noprofile",
                "--norc",
                "-c",
                COMPILER + workflow_script(filename, WORKFLOWS[filename]),
            ],
            cwd=root,
            env={**os.environ, "RUNNER_TEMP": root.as_posix(), "FIXTURE_MODE": mode},
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
        return root, result

    def test_reuse_keeps_both_symbol_snapshots_and_excludes_other_files(self):
        for filename in WORKFLOWS:
            with self.subTest(workflow=filename):
                root, result = self.run_workflow(filename)
                self.assertEqual(result.returncode, 0, result.stderr)
                inputs = (root / "compiler-inputs.txt").read_text().splitlines()
                self.assertEqual(len(inputs), 2)
                self.assertEqual(inputs[0], inputs[1])
                for number, kind in enumerate(("split", "universal"), start=1):
                    snapshot = root / "flutter-symbols-android" / kind
                    self.assertEqual({path.name for path in snapshot.iterdir()}, SYMBOLS)
                    for name in SYMBOLS:
                        self.assertEqual((snapshot / name).read_text(), f"build-{number}")

    def test_missing_symbols_stop_before_the_next_build(self):
        for filename in WORKFLOWS:
            with self.subTest(workflow=filename):
                root, result = self.run_workflow(filename, "missing")
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(len((root / "compiler-inputs.txt").read_text().splitlines()), 1)
                self.assertFalse(any((root / "flutter-symbols-android/split").iterdir()))

    def test_failed_build_cannot_publish_a_new_symbol_snapshot(self):
        for filename in WORKFLOWS:
            with self.subTest(workflow=filename):
                root, result = self.run_workflow(filename, "fail-second")
                self.assertEqual(result.returncode, 17, result.stderr)
                self.assertFalse(any((root / "flutter-symbols-android/universal").iterdir()))
                for name in SYMBOLS:
                    self.assertEqual(
                        (root / "flutter-symbols-android/split" / name).read_text(), "build-1"
                    )


if __name__ == "__main__":
    unittest.main()
