import os
import json
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

from synthetic_container_replay import replay
from synthetic_container_lifecycle import inspect_pending

ROOT = Path(__file__).resolve().parent.parent
INSPECTOR = ROOT / "target/debug/sese-fixture"
FIXTURE = ROOT / "fixtures/event-synthetic-v1.json"


class ContainerReplayTests(unittest.TestCase):
    @unittest.skipUnless(os.environ.get("SESE_TEST_TIMEOUT_IMAGE"), "requires a local sleep image")
    def test_killed_replay_retains_link_to_running_container(self):
        with tempfile.TemporaryDirectory() as directory:
            host = os.environ["SESE_TEST_DOCKER_HOST"]
            child = subprocess.Popen([sys.executable, "synthetic_container_replay.py", str(FIXTURE),
                                      "--inspector", str(INSPECTOR), "--image", os.environ["SESE_TEST_TIMEOUT_IMAGE"],
                                      "--docker-host", host, "--journal-dir", directory],
                                     cwd=ROOT / "scripts", stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            container_id = None
            try:
                deadline = time.monotonic() + 10
                while time.monotonic() < deadline:
                    paths = list(Path(directory).glob("sese-replay-*.json"))
                    if paths:
                        report = json.loads(paths[0].read_text())
                        receipt_path = report["cases"][0]["receipt_path"]
                        if receipt_path:
                            observation = inspect_pending(receipt_path)
                            if observation["status"] == "observed" and observation["running"]:
                                container_id = observation["container_id"]
                                break
                    self.assertIsNone(child.poll(), "replay exited before container started")
                    time.sleep(0.05)
                self.assertIsNotNone(container_id, "container did not start")
                child.kill()
                child.wait(timeout=5)
                report = json.loads(paths[0].read_text())
                self.assertEqual(report["state"], "pending")
                self.assertIsNone(report["passed"])
                self.assertEqual(report["cases"][0]["status"], "running")
                self.assertEqual(inspect_pending(report["cases"][0]["receipt_path"])["container_id"], container_id)
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait(timeout=5)
                if container_id:
                    subprocess.run(["docker", "--host", host, "rm", "-f", container_id], capture_output=True, check=True, timeout=10)
            remaining = subprocess.run(["docker", "--host", host, "ps", "-aq", "--no-trunc", "--filter", "id=" + container_id], capture_output=True, text=True, check=True, timeout=5)
            self.assertEqual(remaining.stdout.strip(), "")

    def test_backend_failure_leaves_inspectable_failed_replay(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(RuntimeError):
                replay(FIXTURE, inspector=INSPECTOR, image="sha256:" + "0" * 64,
                       docker_host="unix:///nonexistent/docker.sock", journal_dir=directory)
            reports = list(Path(directory).glob("sese-replay-*.json"))
            self.assertEqual(len(reports), 1)
            report = json.loads(reports[0].read_text())
            self.assertEqual(report["state"], "failed")
            self.assertFalse(report["passed"])
            self.assertFalse(report["promotion_evidence"])
            self.assertEqual(report["cases"][0]["id"], "throttle-example")
            self.assertEqual(report["cases"][0]["status"], "error")
            self.assertIn("receipt_path=", report["cases"][0]["error"])
            self.assertIsNotNone(report["cases"][0]["receipt_path"])
            self.assertTrue(Path(report["cases"][0]["receipt_path"]).is_file())
            self.assertIsNone(report["cases"][0]["actual"])
            self.assertEqual(reports[0].stat().st_mode & 0o777, 0o600)

    def test_invalid_pack_fails_before_launch(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "bad.json"
            path.write_text('{"schema":"sese-event-fixture-file-v1","version":"v1","state_semantics":"legacy-v1","cases":[]}')
            with self.assertRaisesRegex(ValueError, "fixture"):
                replay(path, inspector=INSPECTOR, image="sha256:" + "0" * 64, docker_host="unix:///nonexistent/docker.sock", journal_dir=directory)
            self.assertEqual([p.name for p in Path(directory).iterdir()], ["bad.json"])

    @unittest.skipUnless(os.environ.get("SESE_TEST_IMAGE"), "requires selected local Docker image")
    def test_real_fixture_passes_matching_candidate_and_fails_wrong_candidate(self):
        with tempfile.TemporaryDirectory() as directory:
            kwargs = dict(inspector=INSPECTOR, image=os.environ["SESE_TEST_IMAGE"], docker_host=os.environ["SESE_TEST_DOCKER_HOST"], journal_dir=directory)
            passed = replay(FIXTURE, candidate="throttle", **kwargs)
            failed = replay(FIXTURE, candidate="allow", **kwargs)
            self.assertTrue(passed["passed"])
            self.assertFalse(failed["passed"])
            self.assertEqual(passed["cases"][0]["actual"], {"decision": 1, "score": 3.0, "reason": 4})
            self.assertEqual(failed["cases"][0]["actual"]["decision"], 0)
            self.assertNotEqual(passed["cases"][0]["receipt_path"], failed["cases"][0]["receipt_path"])
            persisted = json.loads(Path(passed["report_path"]).read_text())
            self.assertEqual(persisted["state"], "completed")
            self.assertEqual(persisted["cases"][0]["actual"], passed["cases"][0]["actual"])
            self.assertEqual(persisted["cases"][0]["event"]["entity_id"], 42)


if __name__ == "__main__":
    unittest.main()
