"""Engine package completeness, trust boundaries and safe source-build fallback."""

import copy
import fnmatch
import json
import stat
import struct
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

import flutter_windows_engine as engine
import flutter_windows_engine_package as package


class EnginePackageTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.recipe, self.recipe_hash = engine.load_recipe()
        self.build_root = self.root / "build"
        self.archive = self.root / "engine.zip"
        self.destination = self.root / "restored"
        self.make_outputs("x64")

    def make_outputs(self, architecture):
        license_file = self.build_root / "flutter/engine/src/flutter/LICENSE"
        license_file.parent.mkdir(parents=True, exist_ok=True)
        license_file.write_bytes(b"Fixture Flutter license notice")
        self.output = (
            self.build_root / "flutter/engine/src/out" / self.recipe["architectures"][architecture]
        )
        for name in self.recipe["required_release_files"]:
            path = self.output / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"fixture " + name.encode())
        header = bytearray(128)
        header[:2] = b"MZ"
        struct.pack_into("<I", header, 0x3C, 96)
        header[96:100] = b"PE\0\0"
        struct.pack_into("<H", header, 100, engine.MACHINES[architecture])
        (self.output / "flutter_windows.dll").write_bytes(header)
        for name in (
            "flutter_windows.dll.pdb",
            "flutter_windows.dll.exp",
            "flutter_export.h",
            "vcruntime140.dll",
            "cpp_client_wrapper/core_implementations.cc",
            "gen/dart-pkg/sky_engine/lib/ui/ui.dart",
            "shader_lib/flutter/runtime_effect.glsl",
        ):
            target = self.output / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(b"fixture " + name.encode())
        self.sdk = self.build_root / (
            f"flutter/engine/src/flutter/prebuilts/windows-{architecture}/dart-sdk"
        )
        for name in (
            "bin/dart.exe",
            "bin/dartaotruntime.exe",
            "bin/snapshots/frontend_server_aot.dart.snapshot",
        ):
            target = self.sdk / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(b"fixture SDK")
        self.record = {
            "schema": 1,
            "recipe_sha256": self.recipe_hash,
            "engine_commit": self.recipe["engine_commit"],
            "required_framework_commit": self.recipe["framework_commit"],
            "architecture": architecture,
            "source_build_exit_code": 0,
            "engine_tests": "passed" if architecture == "x64" else "not_run",
            "flutter_windows_dll_sha256": engine.digest(header),
            "release_file_sha256": engine.verify_release_files(self.output, self.recipe),
            "flutter_windows_pdb_sha256": package.file_digest(
                self.output / "flutter_windows.dll.pdb"
            ),
        }
        self.receipt = self.build_root / f"engine-build-{architecture}.json"
        self.write_record()

    def write_record(self):
        self.receipt.write_text(json.dumps(self.record), encoding="utf-8")

    def pack(self, architecture="x64"):
        return package.pack(self.build_root, architecture, self.archive)

    def rewrite_archive(self, transform):
        with zipfile.ZipFile(self.archive) as source:
            entries = {name: source.read(name) for name in source.namelist()}
        transform(entries)
        with zipfile.ZipFile(self.archive, "w") as destination:
            for name, data in entries.items():
                destination.writestr(name, data)

    def trusted_artifact(self):
        self.pack()
        self.outer = self.root / "download.zip"
        with zipfile.ZipFile(self.outer, "w") as archive:
            archive.write(self.archive, "engine.zip")
        self.artifact = {
            "id": 123,
            "name": package.identity("x64")["artifact_name"],
            "expired": False,
            "digest": "sha256:" + package.file_digest(self.outer),
            "size_in_bytes": self.outer.stat().st_size,
            "workflow_run": {"id": 456},
        }
        self.run = {
            "id": 456,
            "event": "push",
            "status": "completed",
            "conclusion": "success",
            "head_branch": "dev",
            "path": package.WORKFLOW,
            "head_sha": "a" * 40,
            "repository": {"full_name": package.REPOSITORY},
            "head_repository": {"full_name": package.REPOSITORY},
        }

    def download(self, *arguments, output=None):
        if output is not None:
            output.write(self.outer.read_bytes())
        return ""

    def test_complete_round_trip_for_both_architectures(self):
        for architecture in ("x64", "arm64"):
            with self.subTest(architecture=architecture):
                if architecture == "arm64":
                    self.make_outputs(architecture)
                    self.archive = self.root / "arm64.zip"
                    self.destination = self.root / "restored-arm64"
                result = self.pack(architecture)
                package.unpack(self.archive, architecture, self.destination)
                self.assertEqual(result["package_sha256"], package.file_digest(self.archive))
                restored = (
                    self.destination
                    / "flutter/engine/src/out"
                    / self.recipe["architectures"][architecture]
                )
                self.assertEqual(
                    (restored / "flutter_windows.dll").read_bytes(),
                    (self.output / "flutter_windows.dll").read_bytes(),
                )
                self.assertTrue((restored / "cpp_client_wrapper/core_implementations.cc").is_file())
                self.assertTrue((restored / "gen/dart-pkg/sky_engine/lib/ui/ui.dart").is_file())
                self.assertTrue((restored / "dart-sdk/bin/dart.exe").is_file())
                self.assertFalse((self.destination / "depot_tools").exists())

    def test_identity_tracks_recipe_and_build_tools_but_normalizes_checkout_newlines(self):
        fixture = self.root / "repository"
        files = set(package.INPUT_FILES) | {"tool/flutter_windows_engine_manifest.json"}
        files.update(item["path"] for item in self.recipe["patches"])
        for name in files:
            target = fixture / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes((engine.ROOT / name).read_bytes())
        before = package.identity("x64", fixture)
        script = fixture / "tool/build_flutter_windows_engine.ps1"
        contents = script.read_bytes().replace(b"\r\n", b"\n")
        script.write_bytes(contents.replace(b"\n", b"\r\n"))
        self.assertEqual(before, package.identity("x64", fixture))
        script.write_bytes(contents + b"# changed\n")
        self.assertNotEqual(
            before["input_sha256"], package.identity("x64", fixture)["input_sha256"]
        )

    def test_missing_sdk_stale_assets_symbols_and_test_results_cannot_be_published(self):
        original = copy.deepcopy(self.record)
        for field, value in (
            ("recipe_sha256", "0" * 64),
            ("engine_tests", "not_run"),
            ("flutter_windows_pdb_sha256", "0" * 64),
            ("release_file_sha256", {}),
        ):
            with self.subTest(field=field):
                self.record = {**original, field: value}
                self.write_record()
                destination = self.root / f"bad-{field}.zip"
                with self.assertRaises(ValueError):
                    package.pack(self.build_root, "x64", destination)
        self.record = original
        self.write_record()
        (self.sdk / "bin/dart.exe").unlink()
        with self.assertRaisesRegex(ValueError, "missing a build support file"):
            self.pack()

    def test_archive_tampering_and_wrong_inputs_leave_no_destination(self):
        self.pack()
        pristine = self.archive.read_bytes()
        for transform in (
            lambda entries: entries.update({"extra.exe": b"unexpected"}),
            lambda entries: entries.pop("engine-build-x64.json"),
            lambda entries: entries.update({"engine-build-x64.json": b"tampered"}),
            lambda entries: entries.update({package.PACKAGE_METADATA: b"{}"}),
        ):
            self.archive.write_bytes(pristine)
            self.rewrite_archive(transform)
            with self.assertRaises(ValueError):
                package.unpack(self.archive, "x64", self.destination)
            self.assertFalse(self.destination.exists())
        self.archive.write_bytes(pristine)
        with self.assertRaisesRegex(ValueError, "requested build inputs"):
            package.unpack(self.archive, "arm64", self.destination)

    def test_existing_destination_is_preserved(self):
        self.pack()
        self.destination.mkdir()
        sentinel = self.destination / "keep"
        sentinel.write_bytes(b"unchanged")
        with self.assertRaisesRegex(ValueError, "must not exist"):
            package.unpack(self.archive, "x64", self.destination)
        self.assertEqual(sentinel.read_bytes(), b"unchanged")

    def test_unsafe_zip_paths_links_case_collisions_and_expansion_are_rejected(self):
        for name in ("../outside", "C:/outside", "dir\\file", "CON.txt", "dir/file."):
            with self.subTest(name=name):
                # ZipInfo normalizes backslashes when creating a Windows ZIP;
                # exercise the path guard directly as well as serialized names.
                with self.assertRaises(ValueError):
                    package.checked_name(name)
                if "\\" not in name:
                    with zipfile.ZipFile(self.archive, "w") as archive:
                        archive.writestr(name, b"unsafe")
                    with zipfile.ZipFile(self.archive) as archive, self.assertRaises(ValueError):
                        package.archive_entries(archive)
        entry = zipfile.ZipInfo("link")
        entry.external_attr = (stat.S_IFLNK | 0o777) << 16
        with zipfile.ZipFile(self.archive, "w") as archive:
            archive.writestr(entry, b"outside")
        with zipfile.ZipFile(self.archive) as archive, self.assertRaises(ValueError):
            package.archive_entries(archive)
        with zipfile.ZipFile(self.archive, "w") as archive:
            archive.writestr("file", b"one")
            archive.writestr("FILE", b"two")
        with zipfile.ZipFile(self.archive) as archive, self.assertRaises(ValueError):
            package.archive_entries(archive)
        with (
            zipfile.ZipFile(self.archive) as archive,
            patch.object(package, "MAX_EXPANDED_BYTES", 1),
        ):
            with self.assertRaisesRegex(ValueError, "expansion limit"):
                package.archive_entries(archive)

    def test_absent_and_expired_artifacts_are_explicit_source_build_misses(self):
        for artifacts in (
            [],
            [{"name": package.identity("x64")["artifact_name"], "expired": True}],
        ):
            with (
                patch.object(package, "api", return_value={"artifacts": artifacts}),
                patch.object(package, "gh") as gh,
            ):
                result = package.restore("x64", self.destination)
            self.assertFalse(result["restored"])
            gh.assert_not_called()
            self.assertFalse(self.destination.exists())

    def test_pr_fork_untrusted_branch_wrong_workflow_and_failed_runs_are_never_reused(self):
        self.trusted_artifact()
        original = copy.deepcopy(self.run)
        for field, value in (
            ("event", "pull_request"),
            ("event", "pull_request_target"),
            ("head_branch", "feature"),
            ("path", ".github/workflows/build.yml"),
            ("head_repository", {"full_name": "someone/fork"}),
            ("repository", {"full_name": "someone/fork"}),
            ("status", "in_progress"),
            ("conclusion", "failure"),
            ("head_sha", "invalid"),
        ):
            with self.subTest(field=field, value=value):
                run = {**original, field: value}
                with (
                    patch.object(package, "api", side_effect=[{"artifacts": [self.artifact]}, run]),
                    patch.object(package, "gh") as gh,
                ):
                    result = package.restore("x64", self.destination)
                self.assertFalse(result["restored"])
                gh.assert_not_called()

    def test_provenance_is_checked_before_unpacking_with_exact_workflow_ref_and_commit(self):
        self.trusted_artifact()
        with (
            patch.object(package, "api", side_effect=[{"artifacts": [self.artifact]}, self.run]),
            patch.object(package, "gh", side_effect=self.download) as gh,
        ):
            result = package.restore("x64", self.destination)
        self.assertTrue(result["restored"])
        arguments = gh.call_args_list[1].args
        self.assertEqual(arguments[:2], ("attestation", "verify"))
        for flag, value in (
            ("--repo", package.REPOSITORY),
            ("--signer-workflow", f"{package.REPOSITORY}/{package.WORKFLOW}"),
            ("--source-ref", "refs/heads/dev"),
            ("--source-digest", self.run["head_sha"]),
            ("--signer-digest", self.run["head_sha"]),
        ):
            self.assertEqual(arguments[arguments.index(flag) + 1], value)
        self.assertIn("--deny-self-hosted-runners", arguments)
        self.assertEqual(result["artifact_id"], 123)
        self.assertTrue((self.destination / "engine-restore.json").is_file())

    def test_tampered_download_and_failed_attestation_fail_instead_of_source_fallback(self):
        self.trusted_artifact()
        original = self.artifact["digest"]
        self.artifact["digest"] = "sha256:" + "0" * 64
        with (
            patch.object(package, "api", side_effect=[{"artifacts": [self.artifact]}, self.run]),
            patch.object(package, "gh", side_effect=self.download),
        ):
            with self.assertRaisesRegex(ValueError, "immutable digest"):
                package.restore("x64", self.destination)
        self.assertFalse(self.destination.exists())
        self.artifact["digest"] = original

        def reject(*arguments, output=None):
            if arguments[0] == "attestation":
                raise ValueError("Attestation rejected")
            return self.download(*arguments, output=output)

        with (
            patch.object(package, "api", side_effect=[{"artifacts": [self.artifact]}, self.run]),
            patch.object(package, "gh", side_effect=reject),
            patch.object(package, "unpack") as unpack,
        ):
            with self.assertRaisesRegex(ValueError, "Attestation rejected"):
                package.restore("x64", self.destination)
        unpack.assert_not_called()
        self.assertFalse(self.destination.exists())

    def test_producer_triggers_cover_every_reuse_input_and_never_accept_pr_code(self):
        workflow = (engine.ROOT / package.WORKFLOW).read_text(encoding="utf-8")
        trigger = workflow.split("concurrency:", 1)[0]
        self.assertNotIn("pull_request", trigger)
        self.assertNotIn("secrets.", workflow)
        self.assertIn("github.repository == 'GeorgeXie2333/usque-app'", workflow)
        self.assertIn("github.ref == 'refs/heads/main'", workflow)
        self.assertIn("github.ref == 'refs/heads/dev'", workflow)
        self.assertIn("-RunTests:$runEngineTests", workflow)
        patterns = [
            line.strip()[2:] for line in trigger.splitlines() if line.strip().startswith("- ")
        ]
        inputs = set(package.INPUT_FILES) | {"tool/flutter_windows_engine_manifest.json"}
        inputs.update(item["path"] for item in self.recipe["patches"])
        for name in inputs:
            with self.subTest(name=name):
                self.assertTrue(any(fnmatch.fnmatchcase(name, pattern) for pattern in patterns))

    def test_release_requires_attested_engine_and_pr_access_stays_read_only(self):
        release = (engine.ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        self.assertIn("tool/flutter_windows_release_engine.py restore", release)
        self.assertIn("tool/flutter_windows_release_engine.py verify-staged", release)
        consumer = (engine.ROOT / ".github/workflows/build.yml").read_text(encoding="utf-8")
        self.assertNotIn(": write", consumer)
        self.assertNotIn("secrets.", consumer)
        self.assertIn("tool/flutter_windows_release_engine.py verify-staged", consumer)

    def test_windows_workflows_install_pinned_python_before_engine_steps(self):
        for name in (
            package.WORKFLOW,
            ".github/workflows/build.yml",
            ".github/workflows/release.yml",
        ):
            with self.subTest(workflow=name):
                workflow = (engine.ROOT / name).read_text(encoding="utf-8")
                # Inspect the Windows job, not another platform's Python setup.
                job = (
                    workflow
                    if name == package.WORKFLOW
                    else workflow.split("\n  windows:\n", 1)[1].split("\n  android:\n", 1)[0]
                )
                setup = job.index("uses: actions/setup-python@")
                self.assertIn("id: python", job[:setup])
                self.assertIn('python-version: "3.12.10"', job)
                if name != ".github/workflows/release.yml":
                    native = job.index("& ./tool/build_flutter_windows_engine.ps1")
                    self.assertLess(setup, native)
                    block = job[native:].split("\n      - name:", 1)[0]
                    self.assertIn('-Python "${{ steps.python.outputs.python-path }}"', block)
                else:
                    self.assertLess(
                        setup, job.index("tool/flutter_windows_release_engine.py restore")
                    )
                if name == package.WORKFLOW:
                    fixture = job.index("& ./tool/test_flutter_windows_timer.ps1")
                    self.assertLess(setup, job.index("python -m unittest"))
                    self.assertIn(
                        '-Python "${{ steps.python.outputs.python-path }}"',
                        job[fixture:].split("\n", 1)[0],
                    )


if __name__ == "__main__":
    unittest.main()
