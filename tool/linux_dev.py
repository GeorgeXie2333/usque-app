"""Run workstation-safe Linux development commands with the repository SDK pins."""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GUI = ROOT / "apps/usque_gui"


def linux_executable(path: str | Path) -> str:
    requested = Path(path).absolute()
    resolved = requested.resolve()
    for candidate in (requested, resolved):
        if re.match(r"/mnt/[a-z]/", str(candidate)) or candidate.suffix.lower() in {
            ".exe",
            ".bat",
            ".cmd",
        }:
            raise ValueError(f"A Linux executable is required: {candidate}")
    if not resolved.is_file() or not os.access(resolved, os.X_OK):
        raise ValueError(f"Executable not found: {resolved}")
    # Rustup proxies dispatch by executable name; validate their targets without
    # replacing cargo/rustc/rust-analyzer with the resolved rustup command.
    return str(requested)


def command(name: str) -> str:
    found = shutil.which(name)
    if found is None:
        raise ValueError(f"Install the Linux command '{name}' first.")
    return linux_executable(found)


def flutter_sdk() -> Path:
    properties = GUI / "android/local.properties"
    if not properties.is_file():
        raise ValueError(
            "Create apps/usque_gui/android/local.properties with flutter.sdk and sdk.dir."
        )
    for line in properties.read_text(encoding="utf-8").splitlines():
        if line.startswith("flutter.sdk="):
            value = line.partition("=")[2].replace("\\:", ":").replace("\\\\", "\\")
            sdk = Path(value)
            if not sdk.is_absolute():
                raise ValueError("flutter.sdk must be an absolute Linux path.")
            linux_executable(sdk / "bin/flutter")
            return sdk.resolve()
    raise ValueError("flutter.sdk is missing from Android local.properties.")


def run(args: list[str], cwd: Path = ROOT, *, capture: bool = False) -> str:
    print(f"[{cwd.relative_to(ROOT) or '.'}] {' '.join(args)}", flush=True)
    # Arguments are fixed command vectors; no shell or credential interpolation.
    result = subprocess.run(  # noqa: S603
        args,
        cwd=cwd,
        check=True,
        text=True,
        stdout=subprocess.PIPE if capture else None,
    )
    return result.stdout if capture else ""


def verify_flutter(sdk: Path) -> None:
    workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    version = re.search(r"^  FLUTTER_VERSION: ([\d.]+)$", workflow, re.MULTILINE)
    commit = re.search(r"^  FLUTTER_COMMIT: ([a-f0-9]{40})$", workflow, re.MULTILINE)
    if version is None or commit is None:
        raise ValueError("Flutter pins are missing from CI.")
    actual = json.loads(run([str(sdk / "bin/flutter"), "--version", "--machine"], capture=True))
    if actual.get("frameworkVersion") != version[1] or actual.get("frameworkRevision") != commit[1]:
        raise ValueError("Flutter version/revision does not match CI; use the pinned SDK.")


def rust() -> None:
    cargo = command("cargo")
    pin = re.search(
        r'^channel = "([\d.]+)"$', (ROOT / "rust-toolchain.toml").read_text(), re.MULTILINE
    )
    if pin is None or run([cargo, "--version"], capture=True).split()[1] != pin[1]:
        raise ValueError("Cargo does not match rust-toolchain.toml.")
    run([cargo, "fmt", "--all", "--check"])
    run([cargo, "clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"])
    run([cargo, "test", "--workspace", "--all-targets", "--locked"])


def flutter(sdk: Path) -> None:
    executable = str(sdk / "bin/flutter")
    run([executable, "pub", "get", "--enforce-lockfile"], GUI)
    run(
        [str(sdk / "bin/dart"), "format", "--output=none", "--set-exit-if-changed", "lib", "test"],
        GUI,
    )
    run([executable, "analyze", "--no-pub"], GUI)
    run([executable, "test", "--no-pub", "--exclude-tags", "golden"], GUI)


def android(sdk: Path) -> None:
    run(
        [
            command("pwsh"),
            "-NoProfile",
            "-File",
            "tool/build_android_rust.ps1",
            "-AbiFilter",
            "arm64-v8a",
            "-CargoAction",
            "clippy",
        ]
    )
    executable = str(sdk / "bin/flutter")
    run([executable, "pub", "get", "--enforce-lockfile"], GUI)
    run([executable, "build", "apk", "--debug", "--config-only", "--no-pub"], GUI)
    gradlew = linux_executable(GUI / "android/gradlew")
    run([gradlew, "--no-daemon", ":app:ktlintCheck"], GUI / "android")
    run([gradlew, "--no-daemon", ":app:testDebugUnitTest", ":app:lintDebug"], GUI / "android")


def python_checks() -> None:
    ruff = command("ruff")
    run([ruff, "check", "tool"])
    run([ruff, "format", "--check", "tool"])
    run([sys.executable, "-m", "unittest", "discover", "-s", "tool", "-p", "test_*.py", "-v"])


def static() -> None:
    for name in ("cargo", "dart", "flutter", "ruff", "buf", "pwsh"):
        command(name)
    run([command("pwsh"), "-NoProfile", "-File", "tool/check_source.ps1"])
    run([sys.executable, "tool/check_repository_policy.py"])
    run([command("actionlint"), "-no-color"])
    run([command("git"), "diff", "--check"])


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    actions = parser.add_subparsers(dest="action", required=True)
    actions.add_parser("doctor", help="Check SDK pins and Linux command origins")
    checks = actions.add_parser(
        "check", help="Run change-scoped checks (Windows gates stay separate)"
    )
    checks.add_argument("scope", choices=("all", "static", "rust", "flutter", "android", "python"))
    actions.add_parser("preview", help="Run the Linux debug UI preview with hot reload")
    actions.add_parser("build-preview", help="Compile the Linux debug UI preview")
    editor = actions.add_parser(
        "edit", help="Open native Neovim with Rust/Dart/Ruff language servers"
    )
    editor.add_argument("files", nargs="*")
    args = parser.parse_args()
    try:
        if sys.platform != "linux":
            raise ValueError("This helper requires Linux or WSL.")
        sdk = flutter_sdk()
        os.environ["PATH"] = str(sdk / "bin") + os.pathsep + os.environ.get("PATH", "")
        verify_flutter(sdk)
        if args.action == "doctor":
            print(f"Flutter SDK: {sdk}")
            for name in (
                "git",
                "cargo",
                "rustc",
                "rust-analyzer",
                "pwsh",
                "ruff",
                "buf",
                "actionlint",
                "java",
                "cmake",
                "ninja",
                "node",
                "npm",
                "nvim",
                "gh",
                "adb",
            ):
                print(f"{name}: {command(name)}")
            print("Windows builds/goldens: not_run here; use the existing Windows CI jobs.")
        elif args.action == "check":
            steps = {
                "rust": rust,
                "flutter": lambda: flutter(sdk),
                "android": lambda: android(sdk),
                "python": python_checks,
                "static": static,
            }
            for scope in steps if args.scope == "all" else (args.scope,):
                steps[scope]()
        elif args.action == "edit":
            run([command("nvim"), "-u", str(ROOT / "tool/neovim.lua"), "--", *args.files])
        else:
            executable = str(sdk / "bin/flutter")
            run([executable, "pub", "get", "--enforce-lockfile"], GUI)
            mode = ["run", "-d", "linux"] if args.action == "preview" else ["build", "linux"]
            run([executable, *mode, "--debug", "--no-pub", "-t", "lib/main_preview.dart"], GUI)
        return 0
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"Linux development command failed: {error}", file=sys.stderr)
        return error.returncode if isinstance(error, subprocess.CalledProcessError) else 1


if __name__ == "__main__":
    raise SystemExit(main())
