import hashlib
import json
import os
import subprocess
import tempfile
import threading
import time
import unittest
from pathlib import Path

from container_review_run import builtin_for_digest, run
from synthetic_container_lifecycle import _synthetic_request


class ContainerReviewRunTests(unittest.TestCase):
    @unittest.skipUnless(os.environ.get("SESE_TEST_IMAGE"), "requires a local Docker worker image")
    def test_real_application_and_container_runs_produce_guarded_assertion(self):
        root = Path(__file__).resolve().parent.parent
        with tempfile.TemporaryDirectory(prefix="sese-review-run-") as directory:
            db = Path(directory) / "demo.db"
            journal = Path(directory) / "journal"
            journal.mkdir()
            subprocess.run([str(root / "target/debug/sese-event-cycle-demo"), str(root / "target/debug/event-worker"), str(db),
                            "--fixture", str(root / "fixtures/event-synthetic-v1.json"),
                            "--card", str(root / "fixtures/review-card-synthetic-v1.json"),
                            "--followup-fixture", str(root / "fixtures/event-followup-synthetic-v1.json"),
                            "--followup-case", "post-promotion"], input="approve\nrollback\n", text=True,
                           capture_output=True, check=True, timeout=60)
            cli = root / "target/debug/sese-outcome"
            options = dict(outcome_cli=cli, inspector=root / "target/debug/sese-fixture", image=os.environ["SESE_TEST_IMAGE"],
                           docker_host=os.environ["SESE_TEST_DOCKER_HOST"], journal_dir=journal, actor="synthetic-test")
            result = run(db, "typed-cycle", 3, 1, root / "fixtures/event-synthetic-v1.json", **options)
            self.assertTrue(result["matches"])
            self.assertFalse(result["promotion_evidence"])
            self.assertTrue(Path(result["replay_report_path"]).is_file())
            self.assertTrue(Path(result["shadow_report_path"]).is_file())
            assertion = json.loads(subprocess.check_output([str(cli), "event-container-assert-inspect", str(db), str(result["assertion"]["seq"])]))
            self.assertEqual(assertion["evidence_origin"], "caller_asserted")
            self.assertEqual(len(assertion["bundle"]["receipts"]), 3)
            self.assertEqual(json.loads(assertion["bundle"]["replay_report"])["fixture_digest"], result["fixture_digest"])
            self.assertEqual(assertion["replay_report_sha256"], hashlib.sha256(assertion["bundle"]["replay_report"].encode()).hexdigest())
            self.assertEqual(assertion["shadow_report_sha256"], hashlib.sha256(assertion["bundle"]["shadow_report"].encode()).hexdigest())
            replay_saved = json.loads(assertion["bundle"]["replay_report"])
            shadow_saved = json.loads(assertion["bundle"]["shadow_report"])
            receipt_paths = [case["receipt_path"] for case in replay_saved["cases"]]
            receipt_paths += [shadow_saved[role]["receipt_path"] for role in ("champion", "shadow")]
            for path in receipt_paths + [result["replay_report_path"], result["shadow_report_path"], result["bundle_path"]]:
                self.assertEqual(Path(path).parent.resolve(), journal.resolve())
                Path(path).unlink()
            retained = json.loads(subprocess.check_output([str(cli), "event-container-assert-inspect", str(db), str(result["assertion"]["seq"])]))
            self.assertEqual(retained["bundle"], assertion["bundle"])
            before = subprocess.check_output([str(cli), "checkpoint", str(db)])
            captured_checkpoint = json.loads(before)
            verified = subprocess.check_output([os.sys.executable, str(root / "scripts/verify_ledger.py"), str(db),
                                                "--seq", str(captured_checkpoint["seq"]), "--hash", captured_checkpoint["hash"]])
            self.assertTrue(json.loads(verified)["checkpoint_match"])
            verifier = [os.sys.executable, str(root / "scripts/verify_container_capture.py"), str(db),
                        str(result["assertion"]["seq"]), "--seq", str(captured_checkpoint["seq"]), "--hash", captured_checkpoint["hash"]]
            self.assertTrue(json.loads(subprocess.check_output(verifier))["consistent"])
            altered = json.loads(assertion["bundle"]["replay_report"])
            altered["cases"][0]["actual"]["score"] = 123.0
            forged = dict(assertion["bundle"], replay_report=json.dumps(altered))
            forged_path = Path(directory) / "altered-bundle.json"
            forged_path.write_text(json.dumps(forged))
            recorded = json.loads(subprocess.check_output([str(cli), "event-container-capture", str(db), "3", "1", "synthetic-test",
                                                          str(forged_path), str(captured_checkpoint["seq"]), captured_checkpoint["hash"]]))
            before = subprocess.check_output([str(cli), "checkpoint", str(db)])
            forged_checkpoint = json.loads(before)
            rejected = subprocess.run([os.sys.executable, str(root / "scripts/verify_container_capture.py"), str(db),
                                       str(recorded["seq"]), "--seq", str(forged_checkpoint["seq"]), "--hash", forged_checkpoint["hash"]], capture_output=True)
            self.assertNotEqual(rejected.returncode, 0)
            self.assertIn(b"replay case mismatch", rejected.stderr)
            bad_fixture = Path(directory) / "bad.json"
            bad_fixture.write_text('{"schema":"sese-event-fixture-file-v1","version":"v1","state_semantics":"legacy-v1","cases":[]}')
            with self.assertRaises(ValueError):
                run(db, "typed-cycle", 3, 1, bad_fixture, **options)
            self.assertEqual(before, subprocess.check_output([str(cli), "checkpoint", str(db)]))
            checkpoint = json.loads(before)
            existing_reports = set(journal.glob("sese-replay-*.json"))
            intervening = []
            def change_ledger_after_launch():
                deadline = time.monotonic() + 15
                while time.monotonic() < deadline:
                    if set(journal.glob("sese-replay-*.json")) - existing_reports:
                        command = [str(cli), "event-container-assert", str(db), "3", "1", "other-test",
                                   "a" * 64, "b" * 64, str(checkpoint["seq"]), checkpoint["hash"]]
                        intervening.append(subprocess.run(command, capture_output=True, text=True, timeout=30))
                        return
                    time.sleep(0.01)
            thread = threading.Thread(target=change_ledger_after_launch)
            thread.start()
            try:
                with self.assertRaises(subprocess.CalledProcessError) as failure:
                    run(db, "typed-cycle", 3, 1, root / "fixtures/event-synthetic-v1.json", **options)
                self.assertIn(b"ledger changed during comparison", failure.exception.stderr)
            finally:
                thread.join(timeout=30)
            self.assertFalse(thread.is_alive())
            self.assertEqual(len(intervening), 1)
            self.assertEqual(intervening[0].returncode, 0, intervening[0].stderr)
            after = json.loads(subprocess.check_output([str(cli), "checkpoint", str(db)]))
            self.assertEqual(after["seq"], checkpoint["seq"] + 1)

    def test_only_registered_fixed_synthetic_artifacts_are_selected(self):
        for name in ("allow", "throttle", "quarantine", "window-probe"):
            _, artifact = _synthetic_request(name, (1, 2, 3.0, 4), (0, 0.0, 0, 0))
            self.assertEqual(builtin_for_digest(hashlib.sha256(artifact).hexdigest()), name)
        with self.assertRaisesRegex(ValueError, "unknown"):
            builtin_for_digest("0" * 64)


if __name__ == "__main__":
    unittest.main()
