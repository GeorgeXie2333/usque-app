"""Check the cloud installer's early guards without any downloads or installation."""

import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent / "setup_codex_cloud.sh"


@unittest.skipUnless(sys.platform == "linux", "Cloud installer shell guards require Linux")
class CodexCloudTests(unittest.TestCase):
    def invoke(self, *args, enabled=True, platform="Linux", architecture="x86_64"):
        with tempfile.TemporaryDirectory() as directory:
            command = Path(directory) / "uname"
            command.write_text(
                f'#!/bin/sh\nif [ "$1" = -s ]; then echo {platform}; else echo {architecture}; fi\n'
            )
            command.chmod(0o755)
            environment = dict(os.environ)
            environment.update(
                PATH=directory + ":/usr/bin:/bin",
                USQUE_CLOUD_TOOLS_DIR=directory,
            )
            environment.pop("USQUE_CODEX_CLOUD", None)
            if enabled:
                environment["USQUE_CODEX_CLOUD"] = "1"
            # All fixture paths fail before apt, rustup, SDK or repository writes.
            return subprocess.run(  # noqa: S603
                ["/bin/bash", str(SCRIPT), *args],
                env=environment,
                text=True,
                capture_output=True,
                check=False,
            )

    def test_opt_in_required(self):
        result = self.invoke(enabled=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Set USQUE_CODEX_CLOUD=1", result.stderr)

    def test_unknown_mode_rejected(self):
        result = self.invoke("refresh")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no arguments", result.stderr)

    def test_other_platform_rejected(self):
        result = self.invoke(platform="Darwin")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("requires Linux", result.stderr)

    def test_other_architecture_rejected(self):
        result = self.invoke(architecture="aarch64")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("requires amd64", result.stderr)

    def test_external_cache_path_rejected(self):
        result = self.invoke()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("outside the repository, under /workspace", result.stderr)


if __name__ == "__main__":
    unittest.main()
