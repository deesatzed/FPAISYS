import json
import hashlib
import os
import tempfile
import unittest
from pathlib import Path

from synthetic_container_shadow import compare
from synthetic_container_lifecycle import _synthetic_request


class ShadowTests(unittest.TestCase):
    def test_unavailable_backend_persists_error_and_never_returns_comparison(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(RuntimeError):
                compare("allow", "throttle", event=(1, 42, 3.0, 4), window=(0, 0.0, 0, 0),
                        image="sha256:" + "0" * 64, docker_host="unix:///nonexistent/docker.sock", journal_dir=directory)
            reports = list(Path(directory).glob("sese-shadow-*.json"))
            self.assertEqual(len(reports), 1)
            report = json.loads(reports[0].read_text())
            self.assertEqual(report["state"], "failed")
            self.assertFalse(report["promotion_evidence"])
            self.assertEqual(report["champion"]["status"], "error")
            self.assertEqual(report["shadow"]["status"], "pending")
            self.assertIsNone(report["shadow"]["actual"])
            self.assertTrue(Path(report["champion"]["receipt_path"]).exists())

    @unittest.skipUnless(os.environ.get("SESE_TEST_IMAGE"), "requires local Docker worker")
    def test_real_containers_compare_same_event_with_separate_receipts(self):
        with tempfile.TemporaryDirectory() as directory:
            report = compare("allow", "throttle", event=(9, 42, 3.0, 4), window=(2, 1.5, 7, 0),
                             image=os.environ["SESE_TEST_IMAGE"], docker_host=os.environ["SESE_TEST_DOCKER_HOST"], journal_dir=directory)
            self.assertEqual(report["state"], "completed")
            self.assertEqual(report["champion"]["actual"], {"decision": 0, "score": 3.0, "reason": 4})
            self.assertEqual(report["shadow"]["actual"], {"decision": 1, "score": 3.0, "reason": 4})
            self.assertNotEqual(report["champion"]["receipt_path"], report["shadow"]["receipt_path"])
            for role in ("champion", "shadow"):
                entry = report[role]
                receipt = json.loads(Path(entry["receipt_path"]).read_text())
                request, _ = _synthetic_request(entry["candidate"], (9, 42, 3.0, 4), (2, 1.5, 7, 0))
                self.assertEqual(receipt["request_sha256"], hashlib.sha256(request).hexdigest())
                self.assertEqual(receipt["state"], "removed")
            self.assertEqual(json.loads(Path(report["report_path"]).read_text())["event"], [9, 42, 3.0, 4])
            self.assertFalse(report["promotion_evidence"])


if __name__ == "__main__":
    unittest.main()
