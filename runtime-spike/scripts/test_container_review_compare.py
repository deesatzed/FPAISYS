import copy
import hashlib
import json
import struct
import tempfile
import unittest
from pathlib import Path

from container_review_compare import compare, _same, verify_receipts
from synthetic_container_lifecycle import _synthetic_request


class ReviewComparisonTests(unittest.TestCase):
    def test_receipt_binding_detects_altered_request(self):
        with tempfile.TemporaryDirectory() as directory:
            event = (1, 42, 3.0, 4)
            window = (0, 0.0, 0, 0)
            request, artifact = _synthetic_request("throttle", event, window)
            name = "sese-synthetic-" + "a" * 32
            path = Path(directory) / (name + ".json")
            receipt = {"schema": "sese-synthetic-container-intent-v4", "name": name, "state": "removed",
                       "image": "sha256:" + "b" * 64, "docker_host": "unix:///tmp/docker.sock", "daemon_id": "daemon",
                       "candidate": "throttle", "artifact_sha256": hashlib.sha256(artifact).hexdigest(),
                       "request_sha256": hashlib.sha256(request).hexdigest(), "container_id": "c" * 64,
                       "result_status": "valid_verdict", "output_sha256": hashlib.sha256(struct.pack("<IdI", 1, 3.0, 4)).hexdigest()}
            path.write_text(json.dumps(receipt))
            path.chmod(0o600)
            entry = {"candidate": "throttle", "artifact_sha256": receipt["artifact_sha256"],
                     "receipt_path": str(path), "actual": {"decision": 1, "score": 3.0, "reason": 4}}
            verify_receipts([(entry, event, window)], receipt["image"], receipt["docker_host"])
            verify_receipts([(entry, event, window)], receipt["image"], receipt["docker_host"], snapshots=[path.read_text()])
            receipt.update(schema="sese-synthetic-container-intent-v5", expected_worker_sha256="f" * 64, worker_pin_status="matched")
            path.write_text(json.dumps(receipt))
            verify_receipts([(entry, event, window)], receipt["image"], receipt["docker_host"])
            receipt["worker_pin_status"] = "unconfirmed"
            path.write_text(json.dumps(receipt))
            with self.assertRaisesRegex(ValueError, "receipt"):
                verify_receipts([(entry, event, window)], receipt["image"], receipt["docker_host"])
            receipt.update(schema="sese-synthetic-container-intent-v4")
            del receipt["expected_worker_sha256"]
            del receipt["worker_pin_status"]
            path.write_text(json.dumps(receipt))
            with self.assertRaisesRegex(ValueError, "receipt"):
                verify_receipts([(entry, event, window), (entry, event, window)], receipt["image"], receipt["docker_host"])
            second = dict(receipt, name="sese-synthetic-" + "d" * 32, container_id="e" * 64, daemon_id="other-daemon")
            second_path = Path(directory) / (second["name"] + ".json")
            second_path.write_text(json.dumps(second))
            second_path.chmod(0o600)
            with self.assertRaisesRegex(ValueError, "receipt"):
                verify_receipts([(entry, event, window), ({**entry, "receipt_path": str(second_path)}, event, window)], receipt["image"], receipt["docker_host"])
            receipt["request_sha256"] = "0" * 64
            path.write_text(json.dumps(receipt))
            with self.assertRaisesRegex(ValueError, "receipt"):
                verify_receipts([(entry, event, window)], receipt["image"], receipt["docker_host"])

    def test_review_comparison_rejects_changed_shadow_input_and_identity(self):
        event = {"timestamp_ns": 1, "entity_id": 42, "metric": 3.0, "flags": 4}
        window = {"count": 0, "mean": 0.0, "p99_us": 0, "last_error": 0}
        allow = {"decision": 0, "score": 3.0, "reason": 4}
        throttle = {"decision": 1, "score": 3.0, "reason": 4}
        case = {"id": "one", "event": event, "window": window, "expected": throttle, "actual": throttle, "passed": True, "error": None}
        preview = {"schema": "sese-synthetic-review-preview-v1", "checkpoint": {"seq": 1, "hash": "a"},
                   "replay": {"candidate_id": "candidate", "fixture_digest": "digest", "fixture_version": "v1", "state_semantics": "legacy-v1", "passed": True, "cases": [case]},
                   "observations": [{"id": 1, "event": event, "prior": window, "champion_id": "champion", "shadow_id": "candidate", "champion": allow, "shadow": throttle, "champion_error": None, "shadow_error": None}]}
        replay = {"schema": "sese-synthetic-container-replay-experiment-v2", "state": "completed", "promotion_evidence": False, "fixture_digest": "digest", "version": "v1", "state_semantics": "legacy-v1", "passed": True, "image": "image", "docker_host": "host", "cases": [{**case, "status": "completed", "artifact_sha256": "candidate"}]}
        shadow = {"schema": "sese-synthetic-container-shadow-experiment-v1", "state": "completed", "promotion_evidence": False, "image": "image", "docker_host": "host", "event": [1, 42, 3.0, 4], "window": [0, 0.0, 0, 0], "champion": {"status": "completed", "artifact_sha256": "champion", "actual": allow}, "shadow": {"status": "completed", "artifact_sha256": "candidate", "actual": throttle}}
        self.assertTrue(compare(preview, replay, shadow, 1)["matches"])
        changed = copy.deepcopy(shadow)
        changed["event"][2] = 4.0
        with self.assertRaisesRegex(ValueError, "input mismatch"):
            compare(preview, replay, changed, 1)
        changed = copy.deepcopy(shadow)
        changed["shadow"]["artifact_sha256"] = "other"
        with self.assertRaisesRegex(ValueError, "shadow verdict mismatch"):
            compare(preview, replay, changed, 1)

    def test_decisions_and_float_bits_are_compared_exactly(self):
        self.assertTrue(_same({"decision": "allow", "score": 0.0, "reason": 1}, {"decision": 0, "score": 0.0, "reason": 1}, ("decision", "score", "reason")))
        self.assertFalse(_same({"decision": "allow", "score": -0.0, "reason": 1}, {"decision": 0, "score": 0.0, "reason": 1}, ("decision", "score", "reason")))
        self.assertFalse(_same({"decision": "unknown", "score": 0.0, "reason": 1}, {"decision": 0, "score": 0.0, "reason": 1}, ("decision", "score", "reason")))
    def test_mismatched_container_fixture_cannot_match_review(self):
        preview = {"schema": "sese-synthetic-review-preview-v1", "replay": {"fixture_digest": "a"}, "observations": []}
        replay = {"schema": "sese-synthetic-container-replay-experiment-v2", "fixture_digest": "b"}
        with self.assertRaisesRegex(ValueError, "fixture"):
            compare(preview, replay, {}, 1)


if __name__ == "__main__":
    unittest.main()
