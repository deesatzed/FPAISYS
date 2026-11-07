import os
import struct
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from synthetic_container_lifecycle import _synthetic_request

SCRIPT = Path(__file__).with_name("synthetic_container_worker.py")


class ContainerWorkerTests(unittest.TestCase):
    def test_unknown_artifact_is_rejected_before_any_receipt(self):
        with tempfile.TemporaryDirectory() as directory:
            request, _ = _synthetic_request("allow", (1, 42, 3.0, 4), (0, 0.0, 0, 0))
            result = subprocess.run([sys.executable, str(SCRIPT), "sha256:" + "0" * 64, "unix:///nonexistent/docker.sock", directory,
                                     "/nonexistent/docker", "sese-synthetic-" + "a" * 32, "5"],
                                    input=request[:56] + b"unknown", capture_output=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b"unknown synthetic artifact", result.stderr)
            self.assertEqual(result.stdout, b"")
            self.assertEqual(list(Path(directory).iterdir()), [])

    @unittest.skipUnless(os.environ.get("SESE_TEST_IMAGE"), "requires local Docker worker")
    def test_actual_container_executes_fixed_binary_protocol(self):
        import shutil
        with tempfile.TemporaryDirectory() as directory:
            request, _ = _synthetic_request("throttle", (10, 42, 3.5, 7), (1, 2.0, 3, 4))
            name = "sese-synthetic-" + "b" * 32
            result = subprocess.run([sys.executable, str(SCRIPT), os.environ["SESE_TEST_IMAGE"], os.environ["SESE_TEST_DOCKER_HOST"], directory,
                                     shutil.which("docker"), name, "15"], input=request, capture_output=True, timeout=40, check=True)
            self.assertEqual(struct.unpack("<IdI", result.stdout), (1, 3.5, 7))
            self.assertTrue((Path(directory) / (name + ".json")).is_file())


if __name__ == "__main__":
    unittest.main()
