"""Foundation-only Proton pipe contracts; never launch an app or load CEF."""

from __future__ import annotations

import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SWIFTC = shutil.which("swiftc")


@unittest.skipUnless(os.name == "posix" and SWIFTC, "requires an existing POSIX Swift compiler")
class ProtonContractTests(unittest.TestCase):
    def test_framing_route_generations_and_captcha_tokens_without_gui(self):
        with tempfile.TemporaryDirectory(prefix="usque-proton-contract-") as directory:
            executable = Path(directory) / "proton-contract"
            subprocess.run(  # noqa: S603 - existing compiler and Foundation-only fixture sources.
                [
                    SWIFTC,
                    "-swift-version",
                    "5",
                    "-parse-as-library",
                    "-module-cache-path",
                    str(Path(directory) / "module-cache"),
                    str(ROOT / "apps/usque_gui/macos/Runner/ProtonVerificationContract.swift"),
                    str(
                        ROOT
                        / "apps/usque_gui/macos/RunnerTests/ProtonVerificationContractMain.swift"
                    ),
                    "-o",
                    str(executable),
                ],
                check=True,
                capture_output=True,
                timeout=60,
            )
            result = subprocess.run(  # noqa: S603 - pure Foundation contracts, no Cocoa/CEF/Keychain.
                [str(executable)], check=True, capture_output=True, text=True, timeout=5
            )
        self.assertEqual(result.stdout.strip(), "6 Proton verification contract groups passed")


if __name__ == "__main__":
    unittest.main()
