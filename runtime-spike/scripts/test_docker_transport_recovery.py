"""Live socket-interruption test for the synthetic Docker lifecycle."""
import hashlib
import json
import os
from pathlib import Path
import re
import select
import shutil
import socket
import struct
import subprocess
import tempfile
import threading
import time
import unittest

from synthetic_container_lifecycle import inspect_pending, recover_pending, run_synthetic


class SocketProxy:
    """Test-only Unix socket forwarder, stopped without touching the daemon."""
    def __init__(self, path, upstream, stop_marker, acknowledged):
        self.path, self.upstream = str(path), upstream
        self.marker, self.acknowledged = stop_marker, acknowledged
        self.stop = threading.Event()
        self.listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.listener.bind(self.path)
        self.listener.listen(32)
        self.listener.settimeout(0.05)
        self.thread = threading.Thread(target=self.serve, daemon=True)
        self.thread.start()

    def serve(self):
        try:
            while not self.stop.is_set() and not self.marker.exists():
                try:
                    client, _ = self.listener.accept()
                except socket.timeout:
                    continue
                threading.Thread(target=self.forward, args=(client,), daemon=True).start()
        finally:
            self.stop.set()
            self.listener.close()
            self.acknowledged.touch()

    def forward(self, client):
        upstream = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            upstream.connect(self.upstream)
            readers = [client, upstream]
            while readers and not self.stop.is_set():
                readable, _, _ = select.select(readers, [], [], 0.05)
                for source in readable:
                    data = source.recv(65536)
                    if not data:
                        readers.remove(source)
                        (upstream if source is client else client).shutdown(socket.SHUT_WR)
                    else:
                        (upstream if source is client else client).sendall(data)
        except OSError:
            pass
        finally:
            client.close()
            upstream.close()

    def close(self):
        self.stop.set()
        self.thread.join(timeout=3)
        if self.thread.is_alive():
            raise RuntimeError("proxy did not stop")
        os.unlink(self.path)


@unittest.skipUnless(os.environ.get("SESE_TEST_IMAGE") and os.environ.get("SESE_TEST_DOCKER_HOST"),
                     "requires a local synthetic worker image and Docker socket")
class DockerTransportRecoveryTests(unittest.TestCase):
    def test_interrupted_socket_withholds_verdict_then_recovers_owned_container(self):
        docker = shutil.which("docker")
        host = os.environ["SESE_TEST_DOCKER_HOST"]
        self.assertTrue(host.startswith("unix:///"))
        upstream = host[len("unix://"):]
        image = os.environ["SESE_TEST_IMAGE"]
        container_id = None
        proxy = None
        with tempfile.TemporaryDirectory(prefix="sese-socket-", dir="/tmp") as directory:
            root = Path(directory)
            journal = root / "journal"
            journal.mkdir()
            sock = root / "docker.sock"
            stop, ack = root / "stop", root / "stopped"
            worker_output = root / "worker-output"
            endpoint = "unix://" + str(sock)
            proxy = SocketProxy(sock, upstream, stop, ack)
            wrapper = root / "docker-wrapper"
            wrapper.write_text("#!/usr/bin/env python3\nimport os,sys,subprocess,time\na=sys.argv[1:]\n"
                + "if len(a)>2 and a[2]=='start':\n"
                + " r=subprocess.run([" + repr(docker) + "]+a,stdout=subprocess.PIPE)\n"
                + " open(" + repr(str(worker_output)) + ",'wb').write(r.stdout)\n"
                + " sys.stdout.buffer.write(r.stdout); sys.stdout.buffer.flush()\n"
                + " open(" + repr(str(stop)) + ",'w').write(str(r.returncode))\n"
                + " end=time.monotonic()+5\n"
                + " while not os.path.exists(" + repr(str(ack)) + ") and time.monotonic()<end: time.sleep(.01)\n"
                + " sys.exit(r.returncode)\n"
                + "os.execv(" + repr(docker) + ",[" + repr(docker) + "]+a)\n")
            wrapper.chmod(0o700)
            try:
                with self.assertRaisesRegex(RuntimeError, "cleanup unconfirmed") as error:
                    run_synthetic(image, docker=str(wrapper), journal_dir=journal,
                                  docker_host=endpoint, timeout=20)
                self.assertEqual(stop.read_text(), "0")
                self.assertEqual(worker_output.read_bytes(), struct.pack("<IdI", 1, 3.0, 4))
                match = re.search(r"container_id=([0-9a-f]{64})", str(error.exception))
                self.assertIsNotNone(match)
                container_id = match.group(1)
                receipt = next(journal.glob("sese-synthetic-*.json"))
                self.assertEqual(json.loads(receipt.read_text())["state"], "pending")
                self.assertEqual(inspect_pending(receipt, docker=docker)["status"], "unknown")
                with self.assertRaisesRegex(RuntimeError, "unresolved synthetic container receipt"):
                    run_synthetic(image, docker=docker, journal_dir=journal,
                                  docker_host=endpoint, timeout=3)
                proxy.close()
                proxy = None
                stop.unlink()
                ack.unlink()
                proxy = SocketProxy(sock, upstream, stop, ack)
                observed = inspect_pending(receipt, docker=docker)
                self.assertEqual(observed["container_id"], container_id)
                recover_pending(receipt, container_id=container_id, daemon_id=observed["daemon_id"],
                                docker_host=endpoint, image=image,
                                receipt_sha256=hashlib.sha256(receipt.read_bytes()).hexdigest(),
                                actor="synthetic-test", docker=docker)
                self.assertEqual(json.loads(receipt.read_text())["state"], "removed")
                remaining = subprocess.check_output([docker, "--host", host, "ps", "-aq", "--no-trunc",
                                                     "--filter", "id=" + container_id], text=True, timeout=10)
                self.assertEqual(remaining.strip(), "")
                container_id = None
                result = run_synthetic(image, docker=docker, journal_dir=journal,
                                       docker_host=endpoint, timeout=20)
                self.assertEqual(result["verdict"], (1, 3.0, 4))
            finally:
                if proxy is not None:
                    proxy.close()
                if container_id is not None:
                    subprocess.run([docker, "--host", host, "rm", "-f", container_id],
                                   capture_output=True, timeout=10)
                for receipt in journal.glob("sese-synthetic-*.json"):
                    try:
                        record = json.loads(receipt.read_text())
                        name = record.get("name", "")
                        if record.get("image") != image or not re.fullmatch(r"sese-synthetic-[0-9a-f]{32}", name):
                            continue
                        owned = subprocess.check_output([docker, "--host", host, "ps", "-aq", "--no-trunc",
                            "--filter", "label=io.sese.synthetic-probe=" + name,
                            "--filter", "ancestor=" + image], text=True, timeout=10).splitlines()
                        for item in owned:
                            if re.fullmatch(r"[0-9a-f]{64}", item):
                                subprocess.run([docker, "--host", host, "rm", "-f", item], capture_output=True, timeout=10)
                    except (OSError, ValueError, subprocess.SubprocessError) as error:
                        raise RuntimeError(f"test container cleanup unconfirmed: {receipt}") from error
