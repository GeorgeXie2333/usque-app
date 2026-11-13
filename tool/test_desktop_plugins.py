from __future__ import annotations

import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
POLICY = ROOT / "apps/usque_gui/native/desktop_plugins.cmake"
CMAKE = shutil.which("cmake")


@unittest.skipUnless(CMAKE and shutil.which("ninja"), "CMake and Ninja required")
class DesktopPluginTests(unittest.TestCase):
    def fixture(self, root: Path, plugin: str) -> None:
        modules = root / "modules"
        modules.mkdir()
        (modules / "FindJNI.cmake").write_text(
            'message(STATUS "fixture JDK discovered")\n'
            'set(JNI_FOUND TRUE CACHE BOOL "fixture host JDK" FORCE)\n',
            encoding="utf-8",
        )
        (root / "generated_plugins.cmake").write_text(
            'set(PLUGIN_BUNDLED_LIBRARIES "launcher-library;launcher-resource")\n' + plugin,
            encoding="utf-8",
        )
        (root / "CMakeLists.txt").write_text(
            "cmake_minimum_required(VERSION 3.13)\n"
            "project(desktop_plugin_fixture NONE)\n"
            'set(CMAKE_MODULE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/modules" ${CMAKE_MODULE_PATH})\n'
            f'include("{POLICY.as_posix()}")\n'
            "if(USE_DESKTOP_POLICY)\n"
            '  usque_add_desktop_plugins("${CMAKE_CURRENT_SOURCE_DIR}/generated_plugins.cmake")\n'
            "else()\n"
            '  include("${CMAKE_CURRENT_SOURCE_DIR}/generated_plugins.cmake")\n'
            "endif()\n"
            'file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/bundled.txt" "${PLUGIN_BUNDLED_LIBRARIES}")\n'
            'file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/outer-policy.txt" "${CMAKE_DISABLE_FIND_PACKAGE_JNI}")\n',
            encoding="utf-8",
        )

    def configure(self, root: Path, *, policy: bool) -> subprocess.CompletedProcess[str]:
        # Fixed configure arguments and source files in an owned temporary tree.
        return subprocess.run(  # noqa: S603
            [
                CMAKE,
                "-S",
                str(root),
                "-B",
                str(root / "build"),
                "-G",
                "Ninja",
                f"-DUSE_DESKTOP_POLICY={'ON' if policy else 'OFF'}",
            ],
            check=False,
            capture_output=True,
            text=True,
            timeout=30,
        )

    def test_host_jdk_and_cached_discovery_do_not_add_desktop_jni(self) -> None:
        # Model package:jni's optional find_package and bundled-library export.
        plugin = (
            "find_package(JNI COMPONENTS JVM)\n"
            "if(JNI_FOUND)\n"
            "  add_library(jni INTERFACE)\n"
            "  list(APPEND PLUGIN_BUNDLED_LIBRARIES libdartjni.so)\n"
            "endif()\n"
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root, plugin)
            original = self.configure(root, policy=False)
            self.assertEqual(original.returncode, 0, original.stdout + original.stderr)
            self.assertIn("fixture JDK discovered", original.stdout)
            self.assertIn("libdartjni.so", (root / "build/bundled.txt").read_text())

            # Reconfigure the same tree: an old JNI_FOUND cache entry must not
            # resurrect the bridge or remove ordinary plugins from the bundle.
            fixed = self.configure(root, policy=True)
            self.assertEqual(fixed.returncode, 0, fixed.stdout + fixed.stderr)
            self.assertNotIn("fixture JDK discovered", fixed.stdout)
            self.assertEqual(
                (root / "build/bundled.txt").read_text(), "launcher-library;launcher-resource"
            )
            self.assertEqual((root / "build/outer-policy.txt").read_text(), "")

    def test_fresh_desktop_configure_skips_optional_jni(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root, "find_package(JNI COMPONENTS JVM)\n")
            result = self.configure(root, policy=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertNotIn("fixture JDK discovered", result.stdout)

    def test_unconditional_jni_target_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root, "add_library(jni INTERFACE)\n")
            result = self.configure(root, policy=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("desktop plugin graph must not build a JNI runtime", result.stderr)

    def test_other_missing_required_dependencies_still_fail(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root, "find_package(UsqueMissingRequiredPlugin REQUIRED)\n")
            result = self.configure(root, policy=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("UsqueMissingRequiredPlugin", result.stderr)


class DesktopPluginIntegrationTests(unittest.TestCase):
    def test_both_desktop_builds_use_the_shared_plugin_policy(self) -> None:
        for platform in ("linux", "windows"):
            with self.subTest(platform=platform):
                source = (ROOT / f"apps/usque_gui/{platform}/CMakeLists.txt").read_text(
                    encoding="utf-8"
                )
                self.assertIn('include("../native/desktop_plugins.cmake")', source)
                self.assertIn(
                    'usque_add_desktop_plugins("${CMAKE_CURRENT_SOURCE_DIR}/flutter/generated_plugins.cmake")',
                    source,
                )
                self.assertNotIn("include(flutter/generated_plugins.cmake)", source)


if __name__ == "__main__":
    unittest.main()
