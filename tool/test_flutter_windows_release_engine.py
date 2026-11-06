"""Fail-closed checks for the reviewed v0.3.4 engine producer bridge."""

import copy
import json
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import flutter_windows_engine as engine
import flutter_windows_engine_package as package
import flutter_windows_release_engine as release


class ReleaseEngineTests(unittest.TestCase):
    def test_only_the_reviewed_artifact_and_completed_producer_are_accepted(self):
        for architecture, (artifact_id, digest) in release.ARTIFACTS.items():
            with self.subTest(architecture=architecture):
                artifact = {
                    "id": artifact_id,
                    "digest": "sha256:" + digest,
                    "expired": False,
                    "workflow_run": {"id": release.PRODUCER_RUN},
                }
                self.assertEqual(
                    release.pinned_artifacts({"artifacts": [artifact]}, architecture)["artifacts"],
                    [artifact],
                )
                for change in (
                    {"id": 1},
                    {"digest": "sha256:" + "0" * 64},
                    {"expired": True},
                    {"workflow_run": {"id": 1}},
                ):
                    with self.subTest(change=change), self.assertRaises(ValueError):
                        release.pinned_artifacts(
                            {"artifacts": [{**artifact, **change}]}, architecture
                        )
                with self.assertRaises(ValueError):
                    release.pinned_artifacts({"artifacts": [artifact, artifact]}, architecture)
        run = {
            "id": release.PRODUCER_RUN,
            "head_sha": release.PRODUCER,
            "head_branch": "dev",
            "event": "push",
        }
        self.assertEqual(release.pinned_run(run, True), run)
        for change in (
            {"id": 1},
            {"head_sha": "0" * 40},
            {"head_branch": "main"},
            {"event": "pull_request"},
        ):
            with self.subTest(change=change), self.assertRaises(ValueError):
                release.pinned_run({**run, **change}, True)
        with self.assertRaises(ValueError):
            release.pinned_run(run, False)

    def test_code_comparison_ignores_docstrings_but_not_executable_changes(self):
        self.assertEqual(
            release.normalized_python(b'"""old"""\ndef f(): return 1\n'),
            release.normalized_python(b'"""new"""\ndef f(): return 1\n'),
        )
        self.assertNotEqual(
            release.normalized_python(b"def f(): return 1\n"),
            release.normalized_python(b"def f(): return 2\n"),
        )

    def test_only_policy_text_and_two_reviewed_test_methods_may_change(self):
        recipe, _ = engine.load_recipe()
        old_recipe = copy.deepcopy(recipe)
        old_recipe["integration_contract"][
            old_recipe["integration_contract"].index(release.NEW_POLICY)
        ] = release.OLD_POLICY
        names = set(package.INPUT_FILES) | {release.RECIPE_PATH}
        names.update(item["path"] for item in recipe["patches"])
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            candidate = base / "candidate"
            snapshot = base / "snapshot"
            candidate.mkdir()
            snapshot.mkdir()
            for name in names:
                target = candidate / name
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(engine.ROOT / name, target)
            producer_files = {name: (candidate / name).read_bytes() for name in names}

            def git_fixture(_root, *args, **_kwargs):
                name = args[1].split(":", 1)[1]
                if name == release.RECIPE_PATH:
                    return json.dumps(old_recipe).encode()
                return producer_files[name]

            def recipe_fixture(path, _repository):
                data = json.loads(path.read_text(encoding="utf-8"))
                return data, release.LEGACY_RECIPE if path.is_relative_to(snapshot) else "c" * 64

            with (
                patch.object(release, "git_bytes", side_effect=git_fixture),
                patch.object(engine, "load_recipe", side_effect=recipe_fixture),
                patch.object(
                    package,
                    "identity",
                    return_value={
                        "recipe_sha256": release.LEGACY_RECIPE,
                        "input_sha256": release.LEGACY_INPUTS,
                    },
                ),
            ):
                result = release.snapshot_and_check(candidate, snapshot)
                self.assertEqual(result["legacy_recipe_sha256"], release.LEGACY_RECIPE)
                build_script = candidate / "tool/build_flutter_windows_engine.ps1"
                original = build_script.read_bytes()
                build_script.write_bytes(original + b"\n# unexpected build input\n")
                with self.assertRaisesRegex(release.CompatibilityMismatch, "build_flutter_windows"):
                    release.snapshot_and_check(candidate, snapshot)
                build_script.write_bytes(original)
                changed = json.loads((candidate / release.RECIPE_PATH).read_text(encoding="utf-8"))
                changed["engine_commit"] = "0" * 40
                (candidate / release.RECIPE_PATH).write_text(json.dumps(changed), encoding="utf-8")
                with self.assertRaises(release.CompatibilityMismatch):
                    release.snapshot_and_check(candidate, snapshot)

    def test_receipt_binds_candidate_architecture_and_both_producer_identities(self):
        commit = "a" * 40
        artifact_id, digest = release.ARTIFACTS["x64"]
        receipt = {
            "candidate_commit": commit,
            "architecture": "x64",
            "artifact_id": artifact_id,
            "artifact_digest": "sha256:" + digest,
            "producer_commit": release.PRODUCER,
            "producer_run_id": release.PRODUCER_RUN,
            "compatibility": "reviewed-v0.3.4",
            "legacy_recipe_sha256": release.LEGACY_RECIPE,
            "candidate_recipe_sha256": "c" * 64,
            "recipe_sha256": release.LEGACY_RECIPE,
            "input_sha256": release.LEGACY_INPUTS,
            "restored": True,
            "package_sha256": release.PACKAGE_DIGESTS["x64"],
        }
        with (
            tempfile.TemporaryDirectory() as temporary,
            patch.object(engine, "load_recipe", return_value=({}, "c" * 64)),
        ):
            path = Path(temporary) / "receipt.json"
            path.write_text(json.dumps(receipt), encoding="utf-8")
            release.verify_receipt(engine.ROOT, "x64", path, commit)
            for field, value in (
                ("candidate_commit", "b" * 40),
                ("artifact_id", 0),
                ("package_sha256", "bad"),
                ("restored", False),
            ):
                path.write_text(json.dumps({**receipt, field: value}), encoding="utf-8")
                with self.subTest(field=field), self.assertRaises(ValueError):
                    release.verify_receipt(engine.ROOT, "x64", path, commit)

    def test_release_workflow_attests_and_gates_both_internal_receipts(self):
        workflow = (engine.ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        self.assertIn("windows-engine-${{ matrix.variant }}/engine-restore.json", workflow)
        self.assertIn("Verify both release engine provenance records", workflow)
        self.assertIn('--signer-digest "$GITHUB_SHA" --deny-self-hosted-runners', workflow)


if __name__ == "__main__":
    unittest.main()
