"""Beta admission and ordering regression checks; no network or installs."""

import shutil
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import linux_package
import release_contract


class BetaReleaseTests(unittest.TestCase):
    tag = "v0.3.3-beta.2"
    commit = "a" * 40

    def test_only_exact_completed_release_push_is_admitted(self):
        run = {
            "path": ".github/workflows/ci.yml",
            "head_sha": self.commit,
            "head_branch": "release/" + self.tag,
            "event": "push",
            "status": "completed",
            "conclusion": "success",
        }
        release_contract.verify_beta_ci({"workflow_runs": [run]}, self.commit, self.tag)
        for field, values in {
            "path": [".github/workflows/build.yml"],
            "head_sha": ["b" * 40],
            "head_branch": ["main", "dev", "release/v0.3.3-beta.1", "release/v0.3.3-beta.3"],
            "event": ["workflow_dispatch", "pull_request"],
            "status": ["queued", "in_progress"],
            "conclusion": ["failure", "cancelled", "skipped", None],
        }.items():
            for value in values:
                with (
                    self.subTest(field=field, value=value),
                    self.assertRaises(release_contract.ContractError),
                ):
                    release_contract.verify_beta_ci(
                        {"workflow_runs": [{**run, field: value}]}, self.commit, self.tag
                    )
        for field in run:
            incomplete = dict(run)
            incomplete.pop(field)
            with self.assertRaises(release_contract.ContractError):
                release_contract.verify_beta_ci(
                    {"workflow_runs": [incomplete]}, self.commit, self.tag
                )
        for runs in ({}, {"workflow_runs": []}, {"workflow_runs": [0]}):
            with self.assertRaises(release_contract.ContractError):
                release_contract.verify_beta_ci(runs, self.commit, self.tag)
        for tag in ("v0.3.3", "v0.3.3-beta.0", "v0.3.3-beta.01", "v0.3.3-rc.1"):
            with self.assertRaises(release_contract.ContractError):
                release_contract.verify_beta_ci({"workflow_runs": [run]}, self.commit, tag)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "runs.json"
            release_contract.write_json(path, {"workflow_runs": [run]})
            self.assertEqual(
                release_contract.main(
                    [
                        "verify-beta-ci",
                        "--runs",
                        str(path),
                        "--commit",
                        self.commit,
                        "--tag",
                        self.tag,
                    ]
                ),
                0,
            )
        with self.assertRaises(release_contract.ContractError):
            release_contract.verify_main_ci({"workflow_runs": [run]}, self.commit)

    def test_previous_beta_success_cannot_admit_the_current_candidate(self):
        previous = {
            "path": ".github/workflows/ci.yml",
            "head_sha": self.commit,
            "head_branch": "release/v0.3.3-beta.1",
            "event": "push",
            "status": "completed",
            "conclusion": "success",
        }
        with self.assertRaises(release_contract.ContractError):
            release_contract.verify_beta_ci({"workflow_runs": [previous]}, self.commit, self.tag)

    def test_stable_workflow_preserves_both_release_gates(self):
        workflow = (linux_package.ROOT / ".github/workflows/release.yml").read_text()
        self.assertIn("prerelease: false", workflow)
        self.assertIn("make_latest: true", workflow)
        self.assertIn("draft: false", workflow)
        self.assertIn("verify-main-ci", workflow)
        self.assertIn("verify-beta-ci", workflow)
        for name in ("ci.yml", "codeql.yml"):
            self.assertIn(
                "'release/**'", (linux_package.ROOT / ".github/workflows" / name).read_text()
            )

    def test_beta_rpm_metadata_keeps_signed_semver_and_rejects_unmapped_header(self):
        for architecture in linux_package.ARCHITECTURES:
            fields = {
                "%{NAME}": "usque",
                "%{VERSION}": "0.3.3~beta.1",
                "%{RELEASE}": "1",
                "%{ARCH}": linux_package.ARCHITECTURES[architecture][1],
            }
            for tag, script in linux_package.rpm_scriptlets("0.3.3-beta.1", architecture).items():
                fields[f"%{{{tag}}}"] = script + "\n"
                fields[f"[%{{{tag}PROG}}\\n]"] = "/bin/sh\n"

            def query(arguments, fields=fields):
                return fields.get(arguments[3], "absent")

            with patch.object(linux_package, "run", side_effect=query):
                linux_package.inspect_rpm_metadata(Path("inert.rpm"), "0.3.3-beta.1", architecture)
                fields["%{VERSION}"] = "0.3.3-beta.1"
                with self.assertRaises(ValueError):
                    linux_package.inspect_rpm_metadata(
                        Path("inert.rpm"), "0.3.3-beta.1", architecture
                    )

    @unittest.skipUnless(shutil.which("dpkg"), "native dpkg required; enforced on Linux CI")
    def test_native_debian_beta_sorting(self):
        versions = ["0.3.3-beta.1", "0.3.3-beta.2", "0.3.3-beta.10", "0.3.3"]
        for older, newer in zip(versions[:-1], versions[1:], strict=True):
            linux_package.run(
                [
                    "dpkg",
                    "--compare-versions",
                    linux_package.distribution_version(older),
                    "lt",
                    linux_package.distribution_version(newer),
                ]
            )

    def test_canonical_beta_order_and_distribution_identity(self):
        versions = ["0.3.2", "0.3.3-beta.1", "0.3.3-beta.2", "0.3.3-beta.10", "0.3.3"]
        self.assertEqual(sorted(versions, key=linux_package.release_version), versions)
        self.assertEqual(linux_package.distribution_version(versions[1]), "0.3.3~beta.1")
        self.assertEqual(linux_package.distribution_version("0.3.3"), "0.3.3")
        self.assertEqual(
            linux_package.artifact_name(versions[1], "arm64", "rpm"),
            "usque-v0.3.3-beta.1-linux-arm64.rpm",
        )
        # Execute only the generated pure comparator, never a package hook.
        hook = linux_package.preinstall("deb", versions[1])
        comparator = hook[
            hook.index("def release_version(value):") : hook.index("installed_version =")
        ]
        namespace = {}
        # Only our generated pure comparator and fixed synthetic values, never a package hook.
        exec("import re\n" + comparator, namespace)  # noqa: S102
        for value in versions:
            self.assertEqual(
                namespace["release_version"](value), linux_package.release_version(value)
            )
        for value in (
            "0.3.3-beta.0",
            "0.3.3-beta.01",
            "0.3.3~beta.1",
            "0.3.3-rc.1",
            "0.3.3\n",
            "00.3.3",
            "0.3.3-beta.18446744073709551616",
        ):
            with self.subTest(value=value), self.assertRaises(ValueError):
                linux_package.release_version(value)
            with self.subTest(hook_value=value), self.assertRaises(SystemExit):
                namespace["release_version"](value)
