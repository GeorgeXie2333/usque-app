"""Source identity and staged-DLL checks; no SDK, compiler or network access."""

import json
import shutil
import struct
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import flutter_windows_engine as engine


class WindowsEngineIdentityTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        recipe = json.loads(engine.RECIPE.read_text(encoding="utf-8"))
        for item in recipe["patches"]:
            destination = self.root / item["path"]
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(engine.ROOT / item["path"], destination)
        self.recipe_path = self.root / "recipe.json"
        self.recipe_path.write_text(json.dumps(recipe), encoding="utf-8")
        self.recipe, self.recipe_hash = engine.load_recipe(self.recipe_path, self.root)
        self.dll = self.root / "flutter_windows.dll"
        header = bytearray(128)
        header[:2] = b"MZ"
        struct.pack_into("<I", header, 0x3C, 96)
        header[96:100] = b"PE\0\0"
        struct.pack_into("<H", header, 100, engine.MACHINES["x64"])
        self.dll.write_bytes(header)
        self.manifest = self.root / "engine-build.json"
        self.record = {
            "schema": 1,
            "recipe_sha256": self.recipe_hash,
            "engine_commit": self.recipe["engine_commit"],
            "required_framework_commit": self.recipe["framework_commit"],
            "architecture": "x64",
            "flutter_windows_dll_sha256": engine.digest(header),
            "source_build_exit_code": 0,
        }
        self.write_record()

    def write_record(self):
        self.manifest.write_text(json.dumps(self.record), encoding="utf-8")

    def verify_staged(self, architecture="x64"):
        return engine.verify_staged(
            self.dll, self.manifest, architecture, self.recipe_path, self.root
        )

    def test_exact_build_identity_is_accepted(self):
        self.assertEqual(self.verify_staged()["recipe_sha256"], self.recipe_hash)

    def test_directory_selection_uses_capacity_and_creates_fresh_owned_roots(self):
        normal_temp = self.root / "temp"
        roomy = self.root / "localappdata"
        normal_temp.mkdir()
        roomy.mkdir()
        old = roomy / "usque-flutter-engine-existing"
        old.mkdir()
        sentinel = old / "keep"
        sentinel.write_text("unchanged", encoding="utf-8")

        def usage(path):
            return SimpleNamespace(free=(14 if path == normal_temp else 80) * 1024**3)

        with patch.object(engine.shutil, "disk_usage", side_effect=usage):
            first = engine.create_build_directory([normal_temp, roomy], self.recipe_path, self.root)
            second = engine.create_build_directory(
                [normal_temp, roomy], self.recipe_path, self.root
            )
        self.assertNotEqual(first["path"], second["path"])
        self.assertEqual(Path(first["path"]).parent, roomy)
        marker = json.loads(
            (Path(first["path"]) / "usque-engine-build-root.json").read_text(encoding="utf-8")
        )
        self.assertEqual(marker["Kind"], "usque-flutter-engine-v1")
        self.assertEqual(marker["RecipeSha256"], self.recipe_hash)
        self.assertEqual(marker["MinimumFreeBytes"], 40 * 1024**3)
        self.assertEqual(sentinel.read_text(encoding="utf-8"), "unchanged")
        self.assertEqual(list(normal_temp.iterdir()), [])

    def test_insufficient_capacity_does_not_create_or_delete_directories(self):
        parent = self.root / "too-small"
        parent.mkdir()
        existing = parent / "keep"
        existing.write_text("unchanged", encoding="utf-8")
        with patch.object(
            engine.shutil, "disk_usage", return_value=SimpleNamespace(free=40 * 1024**3 - 1)
        ):
            with self.assertRaisesRegex(ValueError, "at least 40 GiB"):
                engine.create_build_directory([parent], self.recipe_path, self.root)
        self.assertEqual(list(parent.iterdir()), [existing])
        self.assertEqual(existing.read_text(encoding="utf-8"), "unchanged")

    def test_directory_selection_validates_recipe_before_creating_a_root(self):
        parent = self.root / "fresh"
        parent.mkdir()
        patch_path = self.root / self.recipe["patches"][0]["path"]
        patch_path.write_bytes(patch_path.read_bytes() + b"changed")
        with self.assertRaisesRegex(ValueError, "patch hash mismatch"):
            engine.create_build_directory([parent], self.recipe_path, self.root)
        self.assertEqual(list(parent.iterdir()), [])

    def test_changed_patch_is_rejected_before_staging(self):
        path = self.root / self.recipe["patches"][0]["path"]
        path.write_bytes(path.read_bytes() + b"\n")
        with self.assertRaisesRegex(ValueError, "patch hash mismatch"):
            self.verify_staged()

    def test_relative_paths_reject_traversal_and_drive_paths(self):
        for name in ("../outside", "/absolute", "C:/elsewhere", "dir\\file", "./noncanonical"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                engine.relative_file(self.root, name)

    def test_local_engine_assets_must_all_exist_and_be_nonempty(self):
        recipe = {"required_release_files": ["flutter_windows.dll", "gen/platform.dill"]}
        with self.assertRaisesRegex(ValueError, "missing or empty"):
            engine.verify_release_files(self.root, recipe)
        platform = self.root / "gen" / "platform.dill"
        platform.parent.mkdir()
        platform.write_bytes(b"")
        with self.assertRaisesRegex(ValueError, "missing or empty"):
            engine.verify_release_files(self.root, recipe)
        platform.write_bytes(b"kernel")
        hashes = engine.verify_release_files(self.root, recipe)
        self.assertEqual(hashes["gen/platform.dill"], engine.digest(b"kernel"))
        self.assertEqual(hashes["flutter_windows.dll"], engine.digest(self.dll.read_bytes()))

    def test_wrong_architecture_and_tampered_dll_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "wrong PE architecture"):
            self.verify_staged("arm64")
        self.dll.write_bytes(self.dll.read_bytes() + b"changed")
        with self.assertRaisesRegex(ValueError, "verified build output"):
            self.verify_staged()

    def test_failed_stale_and_wrong_source_build_records_are_rejected(self):
        original = dict(self.record)
        for field, value in (
            ("source_build_exit_code", 1),
            ("source_build_exit_code", False),
            ("source_build_exit_code", None),
            ("recipe_sha256", "0" * 64),
            ("engine_commit", "0" * 40),
            ("required_framework_commit", "0" * 40),
        ):
            with self.subTest(field=field, value=value):
                self.record = {**original, field: value}
                self.write_record()
                with self.assertRaises(ValueError):
                    self.verify_staged()

    def test_malformed_pe_offsets_do_not_pass_machine_validation(self):
        for offset in (0, 120, 0xFFFFFFFF):
            with self.subTest(offset=offset):
                header = bytearray(self.dll.read_bytes())
                struct.pack_into("<I", header, 0x3C, offset)
                self.dll.write_bytes(header)
                with self.assertRaises(ValueError):
                    engine.pe_machine(self.dll)

    def test_source_verification_requires_exact_changes_and_patched_bytes(self):
        source_name = "engine/src/example.cc"
        source = self.root / source_name
        source.parent.mkdir(parents=True)
        source.write_bytes(b"patched\r\n")
        recipe = {
            "engine_commit": "1" * 40,
            "patched_source_lf_sha256": {source_name: engine.digest(b"patched\n")},
        }
        with patch.object(engine, "git_output", side_effect=[recipe["engine_commit"], source_name]):
            self.assertEqual(engine.verify_source(self.root, recipe)["engine_commit"], "1" * 40)
        with patch.object(engine, "git_output", side_effect=["2" * 40]):
            with self.assertRaisesRegex(ValueError, "revision differs"):
                engine.verify_source(self.root, recipe)
        for changes in ("", source_name + "\nengine/src/unreviewed.cc"):
            with patch.object(engine, "git_output", side_effect=[recipe["engine_commit"], changes]):
                with self.assertRaisesRegex(ValueError, "missing or unexpected"):
                    engine.verify_source(self.root, recipe)
        source.write_bytes(b"unpatched\n")
        with patch.object(engine, "git_output", side_effect=[recipe["engine_commit"], source_name]):
            with self.assertRaisesRegex(ValueError, "source differs"):
                engine.verify_source(self.root, recipe)

    def test_framework_validation_uses_the_engine_pin_consumed_by_flutter(self):
        pin = self.root / "bin" / "internal" / "engine.version"
        pin.parent.mkdir(parents=True)
        pin.write_text(self.recipe["engine_commit"] + "\n", encoding="utf-8")
        with patch.object(engine, "git_output", return_value=self.recipe["framework_commit"]):
            self.assertEqual(
                engine.verify_framework(self.root, self.recipe)["required_engine_commit"],
                self.recipe["engine_commit"],
            )
            pin.write_text("0" * 40 + "\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "different engine revision"):
                engine.verify_framework(self.root, self.recipe)
        with patch.object(engine, "git_output", return_value="0" * 40):
            with self.assertRaisesRegex(ValueError, "framework checkout differs"):
                engine.verify_framework(self.root, self.recipe)


if __name__ == "__main__":
    unittest.main()
