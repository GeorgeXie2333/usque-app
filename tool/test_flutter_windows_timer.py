"""Boundary checks for the frozen, SDK-independent timer fixture preparation."""

import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import flutter_windows_timer as fixture


class TimerFixtureTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name).resolve()
        self.bundle = self.root / "bundle"
        shutil.copytree(fixture.BUNDLE, self.bundle)
        self.root_patch = patch.object(fixture, "ROOT", self.root)
        self.root_patch.start()
        self.addCleanup(self.root_patch.stop)
        self.output = self.root / "target" / "flutter-windows-timer" / "run"

    def update_asset(self, name, data):
        (self.bundle / name).write_bytes(data)
        path = self.bundle / "manifest.json"
        manifest = json.loads(path.read_text(encoding="utf-8"))
        manifest["sha256"][name] = hashlib.sha256(data).hexdigest()
        path.write_text(json.dumps(manifest), encoding="utf-8")

    def test_changed_frozen_input_rejected_before_output_creation(self):
        path = self.bundle / "upstream" / "task_runner_window.cc"
        path.write_bytes(path.read_bytes() + b"// unexpected change\n")
        with self.assertRaisesRegex(ValueError, "hash mismatch"):
            fixture.prepare(self.output, self.bundle)
        self.assertFalse(self.output.exists())

    def test_unlisted_manifest_path_rejected(self):
        path = self.bundle / "manifest.json"
        manifest = json.loads(path.read_text(encoding="utf-8"))
        manifest["sha256"]["../../outside"] = "0" * 64
        path.write_text(json.dumps(manifest), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "manifest or asset paths"):
            fixture.prepare(self.output, self.bundle)
        self.assertFalse(self.output.exists())

    def test_patch_path_cannot_escape_staged_sources(self):
        data = (self.bundle / "candidate.patch").read_bytes()
        self.update_asset("candidate.patch", data.replace(b"b/engine/", b"b/../engine/", 1))
        with self.assertRaisesRegex(ValueError, "only the two frozen"):
            fixture.prepare(self.output, self.bundle)
        self.assertFalse(self.output.exists())

    def test_outside_and_existing_outputs_are_not_modified(self):
        sentinel = self.root / "outside"
        sentinel.write_text("keep", encoding="utf-8")
        for output in (sentinel, self.output.parent, self.output / ".." / "escape"):
            with self.subTest(output=output), self.assertRaisesRegex(ValueError, "new child"):
                fixture.prepare(output, self.bundle)
        self.output.mkdir(parents=True)
        (self.output / "keep").write_text("unchanged", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "new child"):
            fixture.prepare(self.output, self.bundle)
        self.assertEqual(sentinel.read_text(encoding="utf-8"), "keep")
        self.assertEqual((self.output / "keep").read_text(encoding="utf-8"), "unchanged")

    def test_actual_patch_produces_counter_free_candidate_without_touching_inputs(self):
        before = {name: (self.bundle / name).read_bytes() for name in fixture.ASSETS}
        report = fixture.prepare(self.output, self.bundle)
        candidate = (self.output / "candidate_timer.h").read_text(encoding="utf-8")
        original = (self.output / "original_timer.h").read_text(encoding="utf-8")
        patched = self.output / "candidate-source" / fixture.SOURCE_PREFIX / "task_runner_window.cc"
        methods = fixture.extract(
            patched.read_text(encoding="utf-8"),
            "TimerThread::TimerThread(",
            "// Timer used for PollOnce timeout.",
        )
        self.assertIn(methods, candidate)
        self.assertIn("schedule_counter_", original)
        self.assertNotIn("schedule_counter_", candidate)
        self.assertEqual(report["git_apply_check"], "passed")
        self.assertIs(report["candidate_sdk_integrated"], False)
        self.assertEqual(
            before, {name: (self.bundle / name).read_bytes() for name in fixture.ASSETS}
        )

    def test_patch_with_wrong_context_fails_actual_git_apply(self):
        data = (self.bundle / "candidate.patch").read_bytes()
        changed = data.replace(
            b"if (time_point < next_fire_time_)", b"if (time_point < missing_)", 1
        )
        self.assertNotEqual(changed, data)
        self.update_asset("candidate.patch", changed)
        with self.assertRaises(subprocess.CalledProcessError):
            fixture.prepare(self.output, self.bundle)
        self.assertFalse((self.output / "candidate_timer.h").exists())

    def test_inherited_git_environment_cannot_redirect_preparation(self):
        outside = self.root / "unrelated-git-dir"
        environment = {
            "GIT_DIR": str(outside),
            "GIT_WORK_TREE": str(self.root),
            "GIT_INDEX_FILE": str(self.root / "unrelated-index"),
            "GIT_CONFIG_COUNT": "1",
            "GIT_CONFIG_KEY_0": "core.bare",
            "GIT_CONFIG_VALUE_0": "true",
        }
        with patch.dict(os.environ, environment):
            fixture.prepare(self.output, self.bundle)
        self.assertFalse(outside.exists())
        self.assertFalse((self.root / "unrelated-index").exists())
        self.assertTrue((self.output / "candidate-source" / ".git").is_dir())

    @staticmethod
    def complete_results():
        rows = []
        for implementation in ("frozen_original", "patched_candidate"):
            for round_number in range(1, 4):
                rows.append(
                    {
                        "case": "controlled_callback_rearm",
                        "implementation": implementation,
                        "round": round_number,
                        "contract_pass": implementation == "patched_candidate",
                    }
                )
            for case in (
                "earlier_future_notification_after_wait_entry",
                "future_notification_while_parked",
            ):
                rows.append(
                    {
                        "case": case,
                        "implementation": implementation,
                        "contract_pass": implementation == "patched_candidate",
                    }
                )
        for case in (
            "candidate_earliest_wins_during_callback",
            "candidate_callback_self_rearm",
            "candidate_stop_parked",
            "candidate_stop_future_wait",
            "candidate_stop_during_callback",
        ):
            rows.append({"case": case, "pass": True})
        rows.append(
            {
                "summary": "original_expiry_race_and_known_notification_bug_reproduced_candidate_checks_passed",
                "original_expiry_reproductions": 3,
                "original_notification_reproductions": 2,
                "candidate_cases_passed": 10,
                "candidate_sdk_integrated": False,
                "ui_network_or_service_operations": 0,
                "vm_high_cpu_attribution_proven": False,
            }
        )
        return rows

    def test_missing_duplicate_and_false_pass_results_rejected(self):
        complete = self.complete_results()
        cases = {
            "complete": complete,
            "missing case": complete[1:],
            "duplicate": [*complete[:-1], complete[0], complete[-1]],
            "missing summary": complete[:-1],
            "false original pass": [{**complete[0], "contract_pass": True}, *complete[1:]],
            "false integrated": [
                *complete[:-1],
                {**complete[-1], "candidate_sdk_integrated": True},
            ],
            "nonfinite timing": [{**complete[0], "timing": float("nan")}, *complete[1:]],
        }
        path = self.root / "results.jsonl"
        for name, rows in cases.items():
            with self.subTest(name=name):
                path.write_text("\n".join(json.dumps(row) for row in rows) + "\n", encoding="utf-8")
                if name == "complete":
                    self.assertEqual(fixture.validate_results(path)["candidate_cases_passed"], 10)
                else:
                    with self.assertRaises(ValueError):
                        fixture.validate_results(path)


if __name__ == "__main__":
    unittest.main()
