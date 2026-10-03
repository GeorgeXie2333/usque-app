"""Exercise SDK pin enforcement and command failures without running toolchains."""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import linux_dev


@unittest.skipUnless(sys.platform == "linux", "Linux development entry point")
class LinuxDevTests(unittest.TestCase):
    def test_native_proxy_keeps_its_invocation_name(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "rustup"
            target.write_text('#!/bin/sh\nprintf "%s" "$0"\n')
            target.chmod(0o755)
            proxy = Path(directory) / "cargo"
            proxy.symlink_to(target)
            executable = linux_dev.linux_executable(proxy)
            # This owned fixture models rustup's executable-name dispatch.
            result = subprocess.run(  # noqa: S603
                [executable], check=True, capture_output=True, text=True
            )
            self.assertEqual(result.stdout, str(proxy))

    def test_windows_commands_and_symlink_targets_are_rejected(self):
        for path in (
            "/mnt/c/tools/flutter",
            "/opt/usque-test/pwsh.exe",
            "/opt/usque-test/flutter.bat",
        ):
            with self.assertRaisesRegex(ValueError, "Linux executable"):
                linux_dev.linux_executable(path)
        with tempfile.TemporaryDirectory() as directory:
            link = Path(directory) / "flutter"
            link.symlink_to("/mnt/c/tools/flutter")
            with self.assertRaisesRegex(ValueError, "Linux executable"):
                linux_dev.linux_executable(link)

    def test_flutter_revision_mismatch_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            workflow = root / ".github/workflows/ci.yml"
            workflow.parent.mkdir(parents=True)
            workflow.write_text("  FLUTTER_VERSION: 3.44.7\n  FLUTTER_COMMIT: " + "a" * 40 + "\n")
            with patch.object(linux_dev, "ROOT", root), patch.object(linux_dev, "run") as run:
                run.return_value = json.dumps(
                    {"frameworkVersion": "3.44.7", "frameworkRevision": "b" * 40}
                )
                with self.assertRaisesRegex(ValueError, "does not match CI"):
                    linux_dev.verify_flutter(root / "flutter")
                run.return_value = json.dumps(
                    {"frameworkVersion": "3.44.7", "frameworkRevision": "a" * 40}
                )
                linux_dev.verify_flutter(root / "flutter")

    def test_linux_sdk_must_come_from_local_properties(self):
        with tempfile.TemporaryDirectory() as directory:
            gui = Path(directory)
            properties = gui / "android/local.properties"
            properties.parent.mkdir()
            with patch.object(linux_dev, "GUI", gui):
                with self.assertRaisesRegex(ValueError, "Create"):
                    linux_dev.flutter_sdk()
                properties.write_text("flutter.sdk=C:\\\\SDK\\\\flutter\n")
                with self.assertRaisesRegex(ValueError, "absolute Linux path"):
                    linux_dev.flutter_sdk()
                properties.write_text("flutter.sdk=/opt/usque-test/flutter\n")
                with patch.object(linux_dev, "linux_executable") as executable:
                    self.assertEqual(linux_dev.flutter_sdk(), Path("/opt/usque-test/flutter"))
                    executable.assert_called_once_with(Path("/opt/usque-test/flutter/bin/flutter"))

    def test_failed_command_keeps_its_exit_status_and_stops_the_check(self):
        failure = subprocess.CalledProcessError(17, ["flutter", "analyze"])
        with patch.object(linux_dev, "run", side_effect=failure) as run:
            with self.assertRaises(subprocess.CalledProcessError) as result:
                linux_dev.flutter(Path("/opt/usque-test/flutter"))
            self.assertEqual(result.exception.returncode, 17)
            self.assertEqual(run.call_count, 1)
        with (
            patch.object(linux_dev, "flutter_sdk", side_effect=failure),
            patch("sys.argv", ["linux_dev.py", "check", "flutter"]),
        ):
            self.assertEqual(linux_dev.main(), 17)


if __name__ == "__main__":
    unittest.main()
