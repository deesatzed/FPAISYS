"""Controlled demonstration: a worker file hash does not bind its ELF loader."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest
import uuid

from synthetic_container_lifecycle import run_synthetic


@unittest.skipUnless(all(os.environ.get(name) for name in
    ("SESE_TEST_CANARY_IMAGE", "SESE_TEST_TOOLCHAIN_IMAGE", "SESE_TEST_DOCKER_HOST")),
    "requires explicit local worker and Linux arm64 toolchain images")
class WorkerLoaderSubstitutionTests(unittest.TestCase):
    def test_matching_worker_pin_does_not_authenticate_loader(self):
        docker = shutil.which("docker")
        host = os.environ["SESE_TEST_DOCKER_HOST"]
        worker_image = os.environ["SESE_TEST_CANARY_IMAGE"]
        toolchain_image = os.environ["SESE_TEST_TOOLCHAIN_IMAGE"]
        base = [docker, "--host", host]
        tag = "sese-loader-probe:" + uuid.uuid4().hex
        builder = source = None
        tagged = False
        with tempfile.TemporaryDirectory(prefix="sese-loader-test-") as directory:
            root = Path(directory)
            journal = root / "journal"
            journal.mkdir()
            code = root / "loader.c"
            code.write_text('''
void _start(void) {
  char buf[256]; long n;
  do {
    register long x0 __asm__("x0") = 0;
    register long x1 __asm__("x1") = (long)buf;
    register long x2 __asm__("x2") = sizeof(buf);
    register long x8 __asm__("x8") = 63;
    __asm__ volatile("svc 0" : "+r"(x0) : "r"(x1), "r"(x2), "r"(x8) : "memory");
    n = x0;
  } while (n > 0);
  const unsigned char out[16] = {1,0,0,0,0,0,0,0,0,64,83,64,4,0,0,0};
  register long x0 __asm__("x0") = 1;
  register long x1 __asm__("x1") = (long)out;
  register long x2 __asm__("x2") = 16;
  register long x8 __asm__("x8") = 64;
  __asm__ volatile("svc 0" : "+r"(x0) : "r"(x1), "r"(x2), "r"(x8) : "memory");
  __asm__ volatile("mov x0, #0; mov x8, #93; svc 0" ::: "memory");
  __builtin_unreachable();
}
''')
            try:
                builder = subprocess.check_output(base + ["create", "--network=none", "--entrypoint", "/bin/sh",
                    toolchain_image, "-c", "cc -O2 -nostdlib -static -fno-stack-protector /tmp/loader.c -o /tmp/loader"],
                    text=True, timeout=20).strip()
                subprocess.run(base + ["cp", str(code), builder + ":/tmp/loader.c"], check=True, timeout=15, capture_output=True)
                subprocess.run(base + ["start", "-a", builder], check=True, timeout=30, capture_output=True)
                loader = root / "loader"
                subprocess.run(base + ["cp", builder + ":/tmp/loader", str(loader)], check=True, timeout=15, capture_output=True)
                source = subprocess.check_output(base + ["create", worker_image], text=True, timeout=20).strip()
                worker = root / "worker"
                subprocess.run(base + ["cp", source + ":/event-worker", str(worker)], check=True, timeout=15, capture_output=True)
                pin = hashlib.sha256(worker.read_bytes()).hexdigest()
                baseline = run_synthetic(worker_image, docker=docker, docker_host=host, journal_dir=journal,
                                         require_worker_profile=True, expected_worker_sha256=pin, timeout=20)
                self.assertEqual(baseline["verdict"], (1, 3.0, 4))
                subprocess.run(base + ["cp", str(loader), source + ":/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1"],
                               check=True, timeout=15, capture_output=True)
                altered = subprocess.check_output(base + ["commit", source, tag], text=True, timeout=30).strip()
                tagged = True
                result = run_synthetic(altered, docker=docker, docker_host=host, journal_dir=journal,
                                       require_worker_profile=True, expected_worker_sha256=pin, timeout=20)
                self.assertEqual(result["verdict"], (1, 77.0, 4))
                receipt = json.loads(Path(result["receipt_path"]).read_text())
                self.assertEqual(receipt["expected_worker_sha256"], pin)
                self.assertEqual(receipt["worker_pin_status"], "matched")
                self.assertEqual(receipt["state"], "removed")
            finally:
                for receipt in journal.glob("sese-synthetic-*.json"):
                    record = json.loads(receipt.read_text())
                    name = record.get("name", "")
                    if not re.fullmatch(r"sese-synthetic-[0-9a-f]{32}", name):
                        continue
                    owned = subprocess.check_output(base + ["ps", "-aq", "--no-trunc", "--filter",
                        "label=io.sese.synthetic-probe=" + name, "--filter", "ancestor=" + (altered if tagged else worker_image)],
                        text=True, timeout=15).splitlines()
                    for container_id in owned:
                        if re.fullmatch(r"[0-9a-f]{64}", container_id):
                            subprocess.run(base + ["rm", "-f", container_id], check=True, timeout=15, capture_output=True)
                for container_id in (source, builder):
                    if container_id:
                        subprocess.run(base + ["rm", "-f", container_id], check=True, timeout=15, capture_output=True)
                if tagged:
                    subprocess.run(base + ["image", "rm", tag], check=True, timeout=15, capture_output=True)
