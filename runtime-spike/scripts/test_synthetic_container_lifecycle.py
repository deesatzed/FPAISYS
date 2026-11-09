import os
import io
import tarfile
import json
import hashlib
import fcntl
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
import struct
import unittest
from unittest import mock
from pathlib import Path

from synthetic_container_lifecycle import _configuration_matches, _synthetic_request, inspect_pending, run_synthetic


class ContainerLifecycleTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory(prefix="sese-lifecycle-receipts-")
        self.addCleanup(directory.cleanup)
        self.journal_dir = directory.name
        self.docker_host = os.environ.get("SESE_TEST_DOCKER_HOST", "unix:///nonexistent/docker.sock")

    def make_pending(self, path):
        """Construct an unresolved creation intent for the journal and recovery tests."""
        record = json.loads(path.read_text())
        record.update(schema="sese-synthetic-container-intent-v4", state="pending")
        path.write_text(json.dumps(record))

    def test_original_request_reaches_lifecycle_only_if_it_matches_fixed_candidate(self):
        import synthetic_container_lifecycle as lifecycle
        request, artifact = _synthetic_request("throttle", (1, 42, 3.0, 4), (0, 0.0, 0, 0))
        with mock.patch.object(lifecycle, "_run_synthetic_locked", return_value={"ok": True}) as execute:
            result = run_synthetic("sha256:" + "0" * 64, journal_dir=self.journal_dir,
                                   docker_host=self.docker_host, request_bytes=request)
            self.assertEqual(result, {"ok": True})
            self.assertEqual(execute.call_args.kwargs["request"], request)
            self.assertEqual(execute.call_args.kwargs["artifact"], artifact)
            with self.assertRaisesRegex(ValueError, "request does not match"):
                run_synthetic("sha256:" + "0" * 64, journal_dir=self.journal_dir,
                              docker_host=self.docker_host, request_bytes=request + b"x")
            with self.assertRaisesRegex(ValueError, "request does not match"):
                run_synthetic("sha256:" + "0" * 64, journal_dir=self.journal_dir,
                              docker_host=self.docker_host, request_bytes=bytearray(request))
            self.assertEqual(execute.call_count, 1)

    def test_worker_archive_pin_rejects_tampering_links_and_extra_files(self):
        from synthetic_container_lifecycle import _worker_archive_matches
        payload = b"known synthetic worker"
        pin = hashlib.sha256(payload).hexdigest()
        def archive(entries):
            out = io.BytesIO()
            with tarfile.open(fileobj=out, mode="w") as tar:
                for name, data, link in entries:
                    info = tarfile.TarInfo(name)
                    if link:
                        info.type = tarfile.SYMTYPE
                        info.linkname = "elsewhere"
                        tar.addfile(info)
                    else:
                        info.size = len(data)
                        tar.addfile(info, io.BytesIO(data))
            return out.getvalue()
        good = archive([("event-worker", payload, False)])
        self.assertTrue(_worker_archive_matches(good, pin))
        self.assertFalse(_worker_archive_matches(good, "0" * 64))
        self.assertFalse(_worker_archive_matches(archive([("other", payload, False)]), pin))
        self.assertFalse(_worker_archive_matches(archive([("event-worker", payload, True)]), pin))
        self.assertFalse(_worker_archive_matches(archive([("event-worker", payload, False), ("extra", payload, False)]), pin))
        self.assertFalse(_worker_archive_matches(good[:100], pin))

    def test_pinned_pending_and_removed_receipts_keep_conservative_status(self):
        from synthetic_container_lifecycle import _check_journal
        name = "sese-synthetic-" + "a" * 32
        path = Path(self.journal_dir) / (name + ".json")
        request, artifact = _synthetic_request("throttle", (1, 42, 3.0, 4), (0, 0.0, 0, 0))
        record = {"schema": "sese-synthetic-container-intent-v5", "name": name,
                  "image": "sha256:" + "b" * 64, "state": "pending", "daemon_id": "daemon",
                  "docker_host": self.docker_host, "candidate": "throttle",
                  "artifact_sha256": hashlib.sha256(artifact).hexdigest(),
                  "request_sha256": hashlib.sha256(request).hexdigest(),
                  "expected_worker_sha256": "c" * 64, "worker_pin_status": "unconfirmed"}
        path.write_text(json.dumps(record)); path.chmod(0o600)
        self.assertEqual(inspect_pending(path)["status"], "unknown")
        directory = os.open(self.journal_dir, os.O_RDONLY)
        try:
            with self.assertRaisesRegex(RuntimeError, "unresolved"):
                _check_journal(directory)
            record.update(state="removed", container_id="d" * 64, result_status="failed", output_sha256=None)
            path.write_text(json.dumps(record)); _check_journal(directory)
            record.update(result_status="valid_verdict", output_sha256="e" * 64)
            path.write_text(json.dumps(record))
            with self.assertRaisesRegex(RuntimeError, "unresolved"):
                _check_journal(directory)
            record["worker_pin_status"] = "matched"
            path.write_text(json.dumps(record)); _check_journal(directory)
        finally:
            os.close(directory)

    def test_invalid_worker_pin_is_rejected_before_container_work(self):
        with self.assertRaisesRegex(ValueError, "worker pin"):
            run_synthetic("sha256:" + "0" * 64, journal_dir=self.journal_dir,
                          docker_host=self.docker_host, expected_worker_sha256="not-a-digest")
        self.assertEqual(list(Path(self.journal_dir).iterdir()), [])

    def test_internal_lifecycle_rejects_untrusted_request_before_any_container_work(self):
        import synthetic_container_lifecycle as lifecycle
        request, artifact = _synthetic_request("throttle", (1, 42, 3.0, 4), (0, 0.0, 0, 0))
        with self.assertRaisesRegex(ValueError, "request does not match"):
            lifecycle._run_synthetic_locked("sha256:" + "0" * 64, journal_dir=self.journal_dir,
                docker_host=self.docker_host, deadline=time.monotonic() + 1, candidate="throttle",
                event=(1, 42, 3.0, 4), window=(0, 0.0, 0, 0), request=request + b"x", artifact=artifact)
        self.assertEqual(list(Path(self.journal_dir).iterdir()), [])

    def test_configuration_check_rejects_missing_and_relaxed_restrictions(self):
        container, image, name = "a" * 64, "sha256:" + "b" * 64, "sese-synthetic-" + "c" * 32
        report = {"Id": container, "Image": image, "Mounts": [],
                  "State": {"Status": "created", "Running": False},
                  "Config": {"Image": image, "User": "65534:65534", "OpenStdin": True,
                             "Labels": {"io.sese.synthetic-probe": name}},
                  "HostConfig": {"NetworkMode": "none", "ReadonlyRootfs": True, "CapDrop": ["ALL"],
                                 "CapAdd": None, "SecurityOpt": ["no-new-privileges"], "Privileged": False,
                                 "PidsLimit": 16, "Memory": 134217728, "MemorySwap": 134217728,
                                 "NanoCpus": 1000000000, "Binds": None, "VolumesFrom": None, "GroupAdd": None,
                                 "Devices": None, "DeviceRequests": None, "PidMode": "", "IpcMode": "private", "UTSMode": ""}}
        check = lambda obj: _configuration_matches(json.dumps(obj), container, image, name)
        self.assertTrue(check(report))
        for section, field, value in [("HostConfig", "NetworkMode", "bridge"), ("HostConfig", "ReadonlyRootfs", False),
                ("HostConfig", "CapDrop", []), ("HostConfig", "CapAdd", ["SYS_ADMIN"]),
                ("HostConfig", "SecurityOpt", ["seccomp=unconfined"]), ("HostConfig", "Memory", 0),
                ("HostConfig", "MemorySwap", -1), ("HostConfig", "PidsLimit", 0),
                ("HostConfig", "NanoCpus", 0), ("HostConfig", "Privileged", True),
                ("HostConfig", "PidMode", "host"), ("HostConfig", "IpcMode", "host"),
                ("HostConfig", "Binds", ["/host:/host"]), ("HostConfig", "GroupAdd", ["0"]), ("Config", "User", "0"),
                ("State", "Running", True)]:
            with self.subTest(field=field):
                changed = json.loads(json.dumps(report))
                changed[section][field] = value
                self.assertFalse(check(changed))
                del changed[section][field]
                self.assertFalse(check(changed))
        self.assertFalse(_configuration_matches("not json", container, image, name))

    def test_required_worker_profile_cannot_be_bypassed_by_entrypoint_or_arguments(self):
        container, image, name = "a" * 64, "sha256:" + "b" * 64, "sese-synthetic-" + "c" * 32
        report = {"Id": container, "Image": image, "Mounts": [],
                  "State": {"Status": "created", "Running": False},
                  "Config": {"Image": image, "User": "65534:65534", "OpenStdin": True,
                             "Entrypoint": ["/event-worker", "--require-isolation-v1"], "Cmd": None,
                             "Env": ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"],
                             "Labels": {"io.sese.synthetic-probe": name}},
                  "HostConfig": {"NetworkMode": "none", "ReadonlyRootfs": True, "CapDrop": ["ALL"],
                                 "CapAdd": None, "SecurityOpt": ["no-new-privileges"], "Privileged": False,
                                 "PidsLimit": 16, "Memory": 134217728, "MemorySwap": 134217728,
                                 "NanoCpus": 1000000000, "Binds": None, "VolumesFrom": None, "GroupAdd": None,
                                 "Devices": None, "DeviceRequests": None, "PidMode": "", "IpcMode": "private", "UTSMode": ""}}
        check = lambda obj: _configuration_matches(json.dumps(obj), container, image, name, require_worker_profile=True)
        self.assertTrue(check(report))
        for field, value in [("Entrypoint", ["/event-worker"]), ("Entrypoint", ["/bin/sh"]),
                             ("Cmd", ["--ignore-isolation"]), ("Entrypoint", None),
                             ("Env", ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin", "LD_PRELOAD=/tmp/inject.so"]),
                             ("Env", None)]:
            changed = json.loads(json.dumps(report))
            changed["Config"][field] = value
            self.assertFalse(check(changed))
            del changed["Config"][field]
            self.assertFalse(check(changed))

    @unittest.skipUnless(os.environ.get("SESE_TEST_CANARY_IMAGE"), "requires a worker image with the isolation self-check")
    def test_required_worker_profile_runs_and_rejects_entrypoint_override_before_start(self):
        docker = shutil.which("docker")
        image = os.environ["SESE_TEST_CANARY_IMAGE"]
        result = run_synthetic(image, docker=docker, journal_dir=self.journal_dir,
                               docker_host=self.docker_host, require_worker_profile=True)
        self.assertEqual(result["verdict"], (1, 3.0, 4))
        marker = Path(self.journal_dir) / "worker-started"
        wrapper = Path(self.journal_dir) / "docker-profile-wrapper"
        for override in ("--entrypoint=/bin/sh", "--env=LD_PRELOAD=/tmp/inject.so", "--group-add=0"):
            with self.subTest(override=override):
                wrapper.write_text("#!/usr/bin/env python3\nimport os,sys\na=sys.argv[1:]\n"
                                   + "if len(a)>2 and a[2]=='start': open(" + repr(str(marker)) + ",'w').close()\n"
                                   + "if len(a)>2 and a[2]=='create': a.insert(-1," + repr(override) + ")\n"
                                   + "os.execv(" + repr(docker) + ",[" + repr(docker) + "]+a)\n")
                wrapper.chmod(0o700)
                with self.assertRaisesRegex(RuntimeError, "configuration unconfirmed or mismatched"):
                    run_synthetic(image, docker=str(wrapper), journal_dir=self.journal_dir,
                                  docker_host=self.docker_host, require_worker_profile=True)
                self.assertFalse(marker.exists())

    @unittest.skipUnless(os.environ.get("SESE_TEST_CANARY_IMAGE"), "requires a worker image with the isolation self-check")
    def test_worker_self_check_accepts_profile_and_rejects_relaxed_restrictions(self):
        docker = shutil.which("docker")
        base = [docker, "--host", self.docker_host]
        request, _ = _synthetic_request("throttle", (1, 42, 3.0, 4), (0, 0.0, 0, 0))
        cases = [("none", "128m", True, True, True), ("bridge", "128m", True, True, False),
                 ("none", "256m", True, True, False), ("none", "128m", False, True, False),
                 ("none", "128m", True, False, False)]
        for network, memory, readonly, no_new_privs, should_succeed in cases:
            with self.subTest(network=network, memory=memory, readonly=readonly, no_new_privs=no_new_privs):
                args = ["create", "--pull=never", "--interactive", "--network=" + network, "--cap-drop=ALL",
                        "--pids-limit=16", "--memory=" + memory, "--memory-swap=" + memory, "--cpus=1", "--user=65534:65534"]
                if readonly:
                    args.append("--read-only")
                if no_new_privs:
                    args.append("--security-opt=no-new-privileges")
                created = subprocess.check_output(base + args + [os.environ["SESE_TEST_CANARY_IMAGE"]], text=True, timeout=15).strip()
                self.assertRegex(created, r"^[0-9a-f]{64}$")
                try:
                    result = subprocess.run(base + ["start", "--attach", "--interactive", created],
                                            input=request, capture_output=True, timeout=20)
                    self.assertEqual(result.returncode == 0, should_succeed)
                    if should_succeed:
                        self.assertEqual(struct.unpack("<IdI", result.stdout), (1, 3.0, 4))
                    else:
                        self.assertEqual(result.stdout, b"")
                finally:
                    subprocess.run(base + ["rm", "-f", created], stdout=subprocess.DEVNULL,
                                   stderr=subprocess.DEVNULL, timeout=10, check=True)

    @unittest.skipUnless(os.environ.get("SESE_TEST_CANARY_IMAGE"), "requires a worker image with the isolation self-check")
    def test_worker_self_check_rejects_added_group(self):
        docker = shutil.which("docker")
        base = [docker, "--host", self.docker_host]
        request, _ = _synthetic_request("throttle", (1, 42, 3.0, 4), (0, 0.0, 0, 0))
        args = ["create", "--pull=never", "--interactive", "--network=none", "--read-only",
                "--cap-drop=ALL", "--security-opt=no-new-privileges", "--pids-limit=16",
                "--memory=128m", "--memory-swap=128m", "--cpus=1", "--user=65534:65534",
                "--group-add=0", os.environ["SESE_TEST_CANARY_IMAGE"]]
        created = subprocess.check_output(base + args, text=True, timeout=15).strip()
        self.assertRegex(created, r"^[0-9a-f]{64}$")
        try:
            result = subprocess.run(base + ["start", "--attach", "--interactive", created],
                                    input=request, capture_output=True, timeout=20)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, b"")
        finally:
            subprocess.run(base + ["rm", "-f", created], stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL, timeout=10, check=True)

    @unittest.skipUnless(os.environ.get("SESE_TEST_CANARY_IMAGE") and os.environ.get("SESE_TEST_WORKER_SHA256"), "requires a synthetic worker image and test pin")
    def test_readonly_root_denies_worker_replacement_after_pin_check(self):
        docker = shutil.which("docker")
        image = os.environ["SESE_TEST_CANARY_IMAGE"]
        pin = os.environ["SESE_TEST_WORKER_SHA256"]
        base = [docker, "--host", self.docker_host]
        marker = Path(self.journal_dir) / "copy-result"
        worker = Path(self.journal_dir) / "replacement"
        worker.write_text("#!/bin/sh\ncat >/dev/null\nprintf '\\002\\000\\000\\000\\000\\000\\000\\000\\000\\000\\010\\100\\004\\000\\000\\000'\n")
        worker.chmod(0o755)
        wrapper = Path(self.journal_dir) / "docker-race-wrapper"
        wrapper.write_text("#!/usr/bin/env python3\nimport os,sys,subprocess\na=sys.argv[1:]\n"
            + "if len(a)>2 and a[2]=='start':\n"
            + " r=subprocess.run([" + repr(docker) + ", '--host', a[1], 'cp', " + repr(str(worker)) + ", a[-1]+':/event-worker'], capture_output=True)\n"
            + " open(" + repr(str(marker)) + ",'w').write(str(r.returncode))\n"
            + "os.execv(" + repr(docker) + ",[" + repr(docker) + "]+a)\n")
        wrapper.chmod(0o700)
        result = run_synthetic(image, docker_host=self.docker_host, docker=str(wrapper),
                               journal_dir=self.journal_dir, require_worker_profile=True,
                               expected_worker_sha256=pin, timeout=30)
        self.assertNotEqual(marker.read_text(), "0")
        self.assertEqual(result["verdict"], (1, 3.0, 4))
        receipt = json.loads(Path(result["receipt_path"]).read_text())
        self.assertEqual(receipt["worker_pin_status"], "matched")
        # The same replacement must work when the container root is writable.
        cid = subprocess.check_output(base + ["create", "--network=none", "--cap-drop=ALL",
            "--security-opt=no-new-privileges", "--pids-limit=16", "--memory=128m", "--memory-swap=128m",
            "--cpus=1", "--user=65534:65534", image], text=True, timeout=15).strip()
        try:
            copy = subprocess.run(base + ["cp", str(worker), cid + ":/event-worker"], capture_output=True, timeout=15)
            self.assertEqual(copy.returncode, 0, copy.stderr)
        finally:
            subprocess.run(base + ["rm", "-f", cid], check=True, timeout=15, stdout=subprocess.DEVNULL)

    @unittest.skipUnless(os.environ.get("SESE_TEST_IMAGE"), "requires a local synthetic worker image")
    def test_refuses_changed_network_before_starting_worker(self):
        docker = shutil.which("docker")
        marker = Path(self.journal_dir) / "worker-started"
        wrapper = Path(self.journal_dir) / "docker-wrapper"
        wrapper.write_text("#!/usr/bin/env python3\nimport os,sys\na=sys.argv[1:]\n"
                           + "if len(a)>2 and a[2] in ('run','start'): open(" + repr(str(marker)) + ",'w').close()\n"
                           + "if len(a)>2 and a[2] in ('run','create'): a[a.index('--network=none')]='--network=bridge'\n"
                           + "os.execv(" + repr(docker) + ",[" + repr(docker) + "]+a)\n")
        wrapper.chmod(0o700)
        with self.assertRaisesRegex(RuntimeError, "configuration"):
            run_synthetic(os.environ["SESE_TEST_IMAGE"], docker=str(wrapper),
                          journal_dir=self.journal_dir, docker_host=self.docker_host, timeout=30)
        self.assertFalse(marker.exists(), "worker was started before configuration verification")
        receipt = json.loads(next(Path(self.journal_dir).glob("sese-synthetic-*.json")).read_text())
        self.assertEqual(receipt["state"], "removed")
        self.assertEqual(receipt["result_status"], "failed")

    @unittest.skipUnless(os.environ.get("SESE_TEST_IMAGE"), "requires a local synthetic worker image")
    def test_inspection_obeys_evaluation_deadline_and_cleans_created_container(self):
        docker = shutil.which("docker")
        marker = Path(self.journal_dir) / "worker-started"
        wrapper = Path(self.journal_dir) / "slow-inspect"
        wrapper.write_text("#!/usr/bin/env python3\nimport os,sys,time\na=sys.argv[1:]\n"
                           + "if len(a)>2 and a[2]=='start': open(" + repr(str(marker)) + ",'w').close()\n"
                           + "if len(a)>4 and a[2:5]==['inspect','--type','container']: time.sleep(8)\n"
                           + "os.execv(" + repr(docker) + ",[" + repr(docker) + "]+a)\n")
        wrapper.chmod(0o700)
        started = time.monotonic()
        with self.assertRaises(RuntimeError):
            run_synthetic(os.environ["SESE_TEST_IMAGE"], docker=str(wrapper), timeout=2,
                          journal_dir=self.journal_dir, docker_host=self.docker_host)
        self.assertLess(time.monotonic() - started, 4.5, "inspection ignored the evaluation deadline")
        self.assertFalse(marker.exists())
        receipt = json.loads(next(Path(self.journal_dir).glob("sese-synthetic-*.json")).read_text())
        self.assertEqual((receipt["state"], receipt["result_status"]), ("removed", "failed"))

    def test_invalid_caller_probe_name_fails_before_receipt_or_backend(self):
        with self.assertRaisesRegex(ValueError, "probe name"):
            run_synthetic("sha256:" + "0" * 64, journal_dir=self.journal_dir, docker_host=self.docker_host,
                          docker="/nonexistent/docker", probe_name="other-container")
        self.assertEqual(list(Path(self.journal_dir).iterdir()), [])

    @unittest.skipUnless(os.environ.get("SESE_TEST_VOLUME_IMAGE"), "requires a local image declaring a volume")
    def test_image_declared_volume_is_rejected_before_container_launch(self):
        # Ensure an accidental launch in the regression test removes its anonymous volume.
        wrapper = Path(self.journal_dir) / "docker-cleanup-volumes"
        launch_marker = Path(self.journal_dir) / "launched"
        wrapper.write_text("#!/bin/sh\nif [ \"$3\" = rm ]; then exec " + shlex.quote(shutil.which("docker"))
                           + " \"$1\" \"$2\" rm -fv \"$5\"; fi\nif [ \"$3\" = create ]; then touch "
                           + shlex.quote(str(launch_marker)) + "; fi\nexec " + shlex.quote(shutil.which("docker")) + " \"$@\"\n")
        wrapper.chmod(0o700)
        with self.assertRaisesRegex(RuntimeError, "image declares volumes"):
            run_synthetic(os.environ["SESE_TEST_VOLUME_IMAGE"], docker=str(wrapper), journal_dir=self.journal_dir,
                          docker_host=self.docker_host)
        self.assertFalse(launch_marker.exists())

    def test_rejects_remote_endpoint_before_contacting_docker(self):
        with self.assertRaisesRegex(ValueError, "local unix"):
            run_synthetic("sha256:" + "0" * 64, docker_host="tcp://example.invalid:2375", docker="/nonexistent/docker", journal_dir=self.journal_dir)
        self.assertEqual(list(Path(self.journal_dir).iterdir()), [])

    def test_unknown_candidate_and_invalid_event_fail_before_backend(self):
        image = "sha256:" + "0" * 64
        for candidate, event in [("custom", (1, 2, 3.0, 4)), ("allow", (1, 2, float("nan"), 4)), ("allow", (1, 2, 3.0, -1))]:
            with self.assertRaises(ValueError):
                run_synthetic(image, candidate=candidate, event=event, docker_host=self.docker_host, docker="/nonexistent/docker", journal_dir=self.journal_dir)
        self.assertEqual(list(Path(self.journal_dir).iterdir()), [])

    @unittest.skipUnless(os.environ.get("SESE_TEST_IMAGE"), "requires an explicitly selected local Docker image")
    def test_real_builtins_handle_distinct_synthetic_events_and_windows(self):
        image = os.environ["SESE_TEST_IMAGE"]
        for candidate, decision in [("allow", 0), ("throttle", 1), ("quarantine", 2)]:
            result = run_synthetic(image, candidate=candidate, event=(10, 9, -2.5, 7), window=(3, 1.5, 12, 1), docker_host=self.docker_host, journal_dir=self.journal_dir)
            self.assertEqual(result["verdict"], (decision, -2.5, 7))
            self.assertEqual(result["candidate"], candidate)
            record = json.loads(Path(result["receipt_path"]).read_text())
            self.assertEqual(record["state"], "removed")
            self.assertEqual(record["candidate"], candidate)
            self.assertEqual(record["artifact_sha256"], result["artifact_sha256"])

    def test_preflight_failure_records_no_creation(self):
        image = "sha256:" + "0" * 64
        with self.assertRaisesRegex(RuntimeError, "unconfirmed"):
            run_synthetic(image, docker="/nonexistent/docker", journal_dir=self.journal_dir, docker_host=self.docker_host)
        records = list(Path(self.journal_dir).glob("*.json"))
        self.assertEqual(len(records), 1)
        record = json.loads(records[0].read_text())
        self.assertEqual(record["image"], image)
        self.assertEqual(record["state"], "not_created")
        self.assertEqual(record["candidate"], "throttle")
        self.assertEqual(record["artifact_sha256"], "98dd40bc296ce558a939e9c72600a6e95cd1d4105d6758df48e4becb80bbcabe")
        self.assertEqual(record["request_sha256"], "86a17193bc133c6fd6add5f4ac1407da2cc7b6ac8b89631522a65995401a01d1")
        self.assertRegex(record["name"], r"^sese-synthetic-[0-9a-f]{32}$")
        self.assertEqual(records[0].stat().st_mode & 0o777, 0o600)
        result = inspect_pending(records[0], docker="/nonexistent/docker")
        self.assertEqual(result["status"], "not_created")
        self.assertEqual(result["name"], record["name"])

    def test_failed_preflight_does_not_block_a_later_attempt(self):
        image = "sha256:" + "0" * 64
        for _ in range(2):
            with self.assertRaisesRegex(RuntimeError, "daemon identity unconfirmed"):
                run_synthetic(image, docker="/nonexistent/docker", journal_dir=self.journal_dir,
                              docker_host=self.docker_host)
        records = list(Path(self.journal_dir).glob("sese-synthetic-*.json"))
        self.assertEqual(len(records), 2)
        self.assertTrue(all(json.loads(p.read_text())["state"] == "not_created" for p in records))

    def test_malformed_preflight_remains_blocking(self):
        image = "sha256:" + "0" * 64
        with self.assertRaisesRegex(RuntimeError, "daemon identity unconfirmed"):
            run_synthetic(image, docker="/nonexistent/docker", journal_dir=self.journal_dir,
                          docker_host=self.docker_host)
        receipt = next(Path(self.journal_dir).glob("sese-synthetic-*.json"))
        record = json.loads(receipt.read_text())
        record["artifact_sha256"] = "0" * 64
        receipt.write_text(json.dumps(record))
        with self.assertRaisesRegex(RuntimeError, "unresolved synthetic container receipt"):
            run_synthetic(image, docker="/nonexistent/docker", journal_dir=self.journal_dir,
                          docker_host=self.docker_host)

    @unittest.skipUnless(os.environ.get("SESE_TEST_IMAGE"), "requires a local synthetic worker image")
    def test_successful_run_after_failed_preflight_needs_no_recovery(self):
        image = os.environ["SESE_TEST_IMAGE"]
        with self.assertRaisesRegex(RuntimeError, "daemon identity unconfirmed"):
            run_synthetic(image, docker="/nonexistent/docker", journal_dir=self.journal_dir,
                          docker_host=self.docker_host)
        result = run_synthetic(image, docker=shutil.which("docker"), journal_dir=self.journal_dir,
                               docker_host=self.docker_host, timeout=20)
        self.assertEqual(result["verdict"], (1, 3.0, 4))
        states = [json.loads(p.read_text())["state"] for p in Path(self.journal_dir).glob("sese-synthetic-*.json")]
        self.assertCountEqual(states, ["not_created", "removed"])

    @unittest.skipUnless(os.environ.get("SESE_TEST_IMAGE"), "requires a local synthetic worker image")
    def test_creation_intent_precedes_real_create_and_ambiguous_creation_blocks_retry(self):
        docker = shutil.which("docker")
        marker = Path(self.journal_dir) / "create-observed.json"
        wrapper = Path(self.journal_dir) / "ambiguous-create"
        wrapper.write_text("#!/usr/bin/env python3\nimport json,os,pathlib,subprocess,sys\na=sys.argv[1:]\n"
            + "if len(a)>2 and a[2]=='create':\n"
            + " n=a[a.index('--name')+1]; p=pathlib.Path(" + repr(self.journal_dir) + ")/(n+'.json'); r=json.loads(p.read_text());\n"
            + " pathlib.Path(" + repr(str(marker)) + ").write_text(json.dumps(r));\n"
            + " x=subprocess.run([" + repr(docker) + "]+a,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL); sys.exit(1 if x.returncode==0 else x.returncode)\n"
            + "os.execv(" + repr(docker) + ",[" + repr(docker) + "]+a)\n")
        wrapper.chmod(0o700)
        container_id = None
        try:
            with self.assertRaisesRegex(RuntimeError, "creation.*unconfirmed"):
                run_synthetic(os.environ["SESE_TEST_IMAGE"], docker=str(wrapper),
                              journal_dir=self.journal_dir, docker_host=self.docker_host, timeout=20)
            before = json.loads(marker.read_text())
            self.assertEqual(before["schema"], "sese-synthetic-container-intent-v4")
            self.assertEqual(before["state"], "pending")
            self.assertIsInstance(before["daemon_id"], str)
            receipt = Path(self.journal_dir) / (before["name"] + ".json")
            observed = inspect_pending(receipt, docker=docker)
            self.assertEqual(observed["status"], "observed")
            self.assertFalse(observed["running"])
            container_id = observed["container_id"]
            with self.assertRaisesRegex(RuntimeError, "unresolved synthetic container receipt"):
                run_synthetic(os.environ["SESE_TEST_IMAGE"], docker=docker,
                              journal_dir=self.journal_dir, docker_host=self.docker_host)
        finally:
            if container_id is None and marker.exists():
                name = json.loads(marker.read_text())["name"]
                found = subprocess.run([docker, "--host", self.docker_host, "inspect", "--format",
                                        '{{.Id}}|{{index .Config.Labels "io.sese.synthetic-probe"}}', name],
                                       capture_output=True, text=True, timeout=10)
                if found.returncode == 0:
                    found_id, label = found.stdout.strip().split("|", 1)
                    if label == name and re.fullmatch(r"[0-9a-f]{64}", found_id):
                        container_id = found_id
            if container_id:
                subprocess.run([docker, "--host", self.docker_host, "rm", "-f", container_id],
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True, timeout=10)

    def test_unresolved_intent_blocks_another_attempt(self):
        image = "sha256:" + "0" * 64
        with self.assertRaisesRegex(RuntimeError, "unconfirmed"):
            run_synthetic(image, docker="/nonexistent/docker", journal_dir=self.journal_dir, docker_host=self.docker_host)
        before = sorted(Path(self.journal_dir).glob("sese-synthetic-*.json"))
        self.make_pending(before[0])
        with self.assertRaisesRegex(RuntimeError, "unresolved synthetic container receipt"):
            run_synthetic(image, docker="/nonexistent/docker", journal_dir=self.journal_dir, docker_host=self.docker_host)
        self.assertEqual(sorted(Path(self.journal_dir).glob("sese-synthetic-*.json")), before)

    def test_recovery_command_rejects_unknown_observation_without_modification(self):
        image = "sha256:" + "0" * 64
        with self.assertRaisesRegex(RuntimeError, "unconfirmed"):
            run_synthetic(image, docker="/nonexistent/docker", journal_dir=self.journal_dir, docker_host=self.docker_host)
        receipt = next(Path(self.journal_dir).glob("sese-synthetic-*.json"))
        self.make_pending(receipt)
        before = receipt.read_bytes()
        inspected = subprocess.run([sys.executable, str(Path(__file__).parent / "synthetic_container_recover.py"), str(receipt), "--inspect-only", "--docker", "/nonexistent/docker"], capture_output=True, text=True, timeout=10)
        self.assertEqual(inspected.returncode, 0, inspected.stderr)
        report = json.loads(inspected.stdout)
        self.assertEqual(report["status"], "unknown")
        self.assertEqual(report["receipt_sha256"], hashlib.sha256(before).hexdigest())
        command = [sys.executable, str(Path(__file__).parent / "synthetic_container_recover.py"), str(receipt),
                   "--container-id", "a" * 64, "--daemon-id", "unconfirmed", "--docker-host", self.docker_host,
                   "--image", image, "--receipt-sha256", hashlib.sha256(before).hexdigest(), "--actor", "test-operator",
                   "--docker", "/nonexistent/docker", "--remove-observed-container"]
        omitted = subprocess.run(command[:-1], capture_output=True, text=True, timeout=10)
        self.assertNotEqual(omitted.returncode, 0)
        self.assertIn("required", omitted.stderr)
        self.assertEqual(receipt.read_bytes(), before)
        result = subprocess.run(command, capture_output=True, text=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("container observation unknown", result.stderr)
        self.assertEqual(receipt.read_bytes(), before)
        self.assertEqual(list(Path(self.journal_dir).glob(".sese-recovery-*.json")), [])

    def test_parallel_attempt_waits_then_retries_after_preflight_failure(self):
        marker = Path(self.journal_dir) / "started"
        release = Path(self.journal_dir) / "release"
        wrapper = Path(self.journal_dir) / "docker-wrapper"
        wrapper.write_text("#!/bin/sh\ntouch " + shlex.quote(str(marker)) + "\nwhile [ ! -e " + shlex.quote(str(release)) + " ]; do /bin/sleep 0.02; done\nexit 1\n")
        wrapper.chmod(0o700)
        code = "import sys; from synthetic_container_lifecycle import run_synthetic; run_synthetic(sys.argv[1], journal_dir=sys.argv[2], docker_host=sys.argv[3], docker=sys.argv[4])"
        args = [sys.executable, "-c", code, "sha256:" + "0" * 64, self.journal_dir, self.docker_host, str(wrapper)]
        first = subprocess.Popen(args, cwd=Path(__file__).parent, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        second = None
        try:
            deadline = time.monotonic() + 10
            while not marker.exists() and time.monotonic() < deadline:
                time.sleep(0.02)
            self.assertTrue(marker.exists())
            second = subprocess.Popen(args, cwd=Path(__file__).parent, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            time.sleep(0.25)
            self.assertIsNone(second.poll(), "second attempt did not wait for journal lock")
        finally:
            release.touch()
            _, first_error = first.communicate(timeout=10)
            if second is not None:
                _, second_error = second.communicate(timeout=10)
        self.assertNotEqual(first.returncode, 0, first_error.decode())
        self.assertNotEqual(second.returncode, 0)
        self.assertIn("daemon identity unconfirmed", second_error.decode())
        self.assertEqual(len(list(Path(self.journal_dir).glob("sese-synthetic-*.json"))), 2)

    def test_journal_lock_wait_has_deadline(self):
        fd = os.open(self.journal_dir, os.O_RDONLY)
        fcntl.flock(fd, fcntl.LOCK_EX)
        code = "import sys; from synthetic_container_lifecycle import run_synthetic; run_synthetic(sys.argv[1], journal_dir=sys.argv[2], docker_host=sys.argv[3], docker='/nonexistent/docker', timeout=0.1)"
        try:
            try:
                child = subprocess.run([sys.executable, "-c", code, "sha256:" + "0" * 64, self.journal_dir, self.docker_host], cwd=Path(__file__).parent, capture_output=True, text=True, timeout=2)
            except subprocess.TimeoutExpired:
                self.fail("journal lock wait exceeded the configured deadline")
        finally:
            os.close(fd)
        self.assertNotEqual(child.returncode, 0)
        self.assertIn("journal lock deadline exceeded", child.stderr)
        self.assertEqual(list(Path(self.journal_dir).glob("sese-synthetic-*.json")), [])

    def test_unsafe_or_malformed_receipts_block_before_backend(self):
        name = "sese-synthetic-" + "a" * 32 + ".json"
        for variant in ("malformed", "duplicate", "symlink", "public"):
            with self.subTest(variant=variant), tempfile.TemporaryDirectory(dir=self.journal_dir) as directory:
                path = Path(directory) / name
                if variant == "symlink":
                    target = Path(directory) / "target"
                    target.write_text("{}")
                    path.symlink_to(target)
                else:
                    path.write_text('{"state":"removed","state":"removed"}' if variant == "duplicate" else "{}")
                    path.chmod(0o644 if variant == "public" else 0o600)
                with self.assertRaisesRegex(RuntimeError, "unresolved synthetic container receipt"):
                    run_synthetic("sha256:" + "0" * 64, docker="/nonexistent/docker", journal_dir=directory, docker_host=self.docker_host)
                self.assertEqual(len(list(Path(directory).glob("sese-synthetic-*.json"))), 1)

    def test_incomplete_recovery_blocks_even_if_receipt_looks_removed(self):
        from synthetic_container_lifecycle import _synthetic_request
        name = "sese-synthetic-" + "a" * 32
        image = "sha256:" + "0" * 64
        request, artifact = _synthetic_request("allow", (1, 42, 3.0, 4), (0, 0.0, 0, 0))
        receipt = Path(self.journal_dir) / (name + ".json")
        receipt.write_text(json.dumps({"schema":"sese-synthetic-container-intent-v4", "name":name, "image":image,
            "state":"removed", "daemon_id":"daemon", "docker_host":self.docker_host, "candidate":"allow",
            "artifact_sha256":hashlib.sha256(artifact).hexdigest(), "request_sha256":hashlib.sha256(request).hexdigest(),
            "container_id":"b" * 64, "result_status":"failed", "output_sha256":None}))
        receipt.chmod(0o600)
        audit = Path(self.journal_dir) / (".sese-recovery-" + "c" * 32 + ".json")
        audit.write_text(json.dumps({"schema":"sese-synthetic-container-recovery-v2", "status":"started", "actor":"operator",
            "receipt_name":receipt.name, "receipt_sha256":"d" * 64, "container_id":"b" * 64, "daemon_id":"daemon",
            "docker_host":self.docker_host, "image":image, "started_unix_ns":"1", "synthetic":True,
            "execution_authenticated":False, "promotion_evidence":False}))
        audit.chmod(0o600)
        with self.assertRaisesRegex(RuntimeError, "unresolved synthetic recovery"):
            run_synthetic(image, journal_dir=self.journal_dir, docker_host=self.docker_host, docker="/nonexistent/docker")
        self.assertEqual(len(list(Path(self.journal_dir).glob("sese-synthetic-*.json"))), 1)
        legacy = Path(self.journal_dir) / (".sese-recovery-" + "f" * 32 + ".json")
        legacy.write_text(json.dumps({"schema":"sese-synthetic-container-recovery-v1", "status":"removed"})); legacy.chmod(0o600)
        with self.assertRaisesRegex(RuntimeError, "unresolved synthetic recovery"):
            run_synthetic(image, journal_dir=self.journal_dir, docker_host=self.docker_host, docker="/nonexistent/docker")
        completed = json.loads(audit.read_text())
        completed.update(status="completed", finished_unix_ns="2", finished_receipt_sha256=hashlib.sha256(receipt.read_bytes()).hexdigest())
        result = Path(self.journal_dir) / (".sese-recovery-" + "e" * 32 + ".json")
        correct_digest = completed["finished_receipt_sha256"]
        completed["finished_receipt_sha256"] = "f" * 64
        result.write_text(json.dumps(completed)); result.chmod(0o600)
        with self.assertRaisesRegex(RuntimeError, "unresolved synthetic recovery"):
            run_synthetic(image, journal_dir=self.journal_dir, docker_host=self.docker_host, docker="/nonexistent/docker")
        completed["finished_receipt_sha256"] = correct_digest
        result.write_text(json.dumps(completed))
        with self.assertRaisesRegex(RuntimeError, "daemon identity unconfirmed"):
            run_synthetic(image, journal_dir=self.journal_dir, docker_host=self.docker_host, docker="/nonexistent/docker")

    def test_unsafe_or_malformed_recovery_records_block_launch(self):
        name = ".sese-recovery-" + "a" * 32 + ".json"
        for variant in ("malformed", "duplicate", "symlink", "public"):
            with self.subTest(variant=variant), tempfile.TemporaryDirectory(dir=self.journal_dir) as directory:
                path = Path(directory) / name
                if variant == "symlink":
                    target = Path(directory) / "target"
                    target.write_text("{}")
                    path.symlink_to(target)
                else:
                    path.write_text('{"schema":"x","schema":"y"}' if variant == "duplicate" else "{}")
                    path.chmod(0o644 if variant == "public" else 0o600)
                with self.assertRaisesRegex(RuntimeError, "unresolved synthetic recovery"):
                    run_synthetic("sha256:" + "0" * 64, docker="/nonexistent/docker", journal_dir=directory, docker_host=self.docker_host)
                self.assertEqual(list(Path(directory).glob("sese-synthetic-*.json")), [])

    def test_legacy_recovery_audit_is_not_treated_as_v2_completion(self):
        audit = Path(self.journal_dir) / (".sese-recovery-" + "a" * 32 + ".json")
        audit.write_text(json.dumps({"schema":"sese-synthetic-container-recovery-v1", "status":"started"}))
        audit.chmod(0o600)
        with self.assertRaisesRegex(RuntimeError, "daemon identity unconfirmed"):
            run_synthetic("sha256:" + "0" * 64, journal_dir=self.journal_dir, docker_host=self.docker_host, docker="/nonexistent/docker")
        self.assertEqual(json.loads(audit.read_text())["status"], "started")

    def test_pending_inspection_rejects_duplicate_fields_and_symlinks(self):
        name = "sese-synthetic-" + "a" * 32
        path = Path(self.journal_dir) / (name + ".json")
        path.write_text('{"schema":"sese-synthetic-container-intent-v1","name":"' + name + '","image":"sha256:' + "0" * 64 + '","state":"pending","state":"pending"}')
        path.chmod(0o600)
        with self.assertRaises(ValueError):
            inspect_pending(path, docker="/nonexistent/docker")
        link = Path(self.journal_dir) / "link.json"
        link.symlink_to(path)
        with self.assertRaises((ValueError, OSError)):
            inspect_pending(link, docker="/nonexistent/docker")

    def test_legacy_record_without_daemon_binding_cannot_report_observed(self):
        name = "sese-synthetic-" + "a" * 32
        image = "sha256:" + "0" * 64
        path = Path(self.journal_dir) / (name + ".json")
        path.write_text(json.dumps({"schema": "sese-synthetic-container-intent-v1", "name": name, "image": image, "state": "pending"}))
        path.chmod(0o600)
        wrapper = Path(self.journal_dir) / "fake-docker"
        matching = "a" * 64 + "|/" + name + "|" + image + "|" + name + "|true"
        wrapper.write_text("#!/bin/sh\nprintf '%s\\n' " + shlex.quote(matching) + "\n")
        wrapper.chmod(0o700)
        self.assertEqual(inspect_pending(path, docker=str(wrapper))["status"], "unknown")
        path.write_text(json.dumps({"schema": "sese-synthetic-container-intent-v2", "name": name, "image": image, "state": "pending", "daemon_id": "old-daemon"}))
        self.assertEqual(inspect_pending(path, docker=str(wrapper))["status"], "unknown")

    def test_rejects_unpinned_image_before_contacting_docker(self):
        with self.assertRaisesRegex(ValueError, "image digest"):
            run_synthetic("untrusted:latest", docker="/nonexistent/docker", journal_dir=self.journal_dir, docker_host=self.docker_host)

    def test_unavailable_backend_never_returns_success(self):
        with self.assertRaisesRegex(RuntimeError, "unconfirmed"):
            run_synthetic("sha256:" + "0" * 64, docker="/nonexistent/docker", journal_dir=self.journal_dir, docker_host=self.docker_host)

    @unittest.skipUnless(os.environ.get("SESE_TEST_IMAGE"), "requires an explicitly selected local Docker image")
    def test_real_synthetic_run_returns_verdict_and_removes_its_container(self):
        with mock.patch.dict(os.environ, {"DOCKER_CONTEXT": "sese-nonexistent-context", "DOCKER_HOST": "tcp://example.invalid:2375"}):
            result = run_synthetic(os.environ["SESE_TEST_IMAGE"], timeout=15, journal_dir=self.journal_dir, docker_host=self.docker_host)
        self.assertEqual(result["verdict"], (1, 3.0, 4))
        self.assertEqual(len(result["container_id"]), 64)
        remaining = subprocess.run(
            ["docker", "--host", self.docker_host, "ps", "-aq", "--no-trunc", "--filter", "id=" + result["container_id"]],
            capture_output=True, text=True, timeout=5, check=True,
        )
        self.assertEqual(remaining.stdout.strip(), "")
        record = json.loads(Path(result["receipt_path"]).read_text())
        self.assertEqual(record["state"], "removed")
        self.assertEqual(record["container_id"], result["container_id"])
        self.assertEqual(record["result_status"], "valid_verdict")
        self.assertEqual(record["output_sha256"], hashlib.sha256(struct.pack("<IdI", 1, 3.0, 4)).hexdigest())

    @unittest.skipUnless(os.environ.get("SESE_TEST_IMAGE"), "requires an explicitly selected local Docker image")
    def test_cleanup_failure_withholds_verdict_and_reports_owned_container(self):
        actual_docker = shutil.which("docker")
        self.assertIsNotNone(actual_docker)
        container_id = None
        with tempfile.TemporaryDirectory(prefix="sese-cleanup-test-") as directory:
            wrapper = Path(directory) / "docker-wrapper"
            wrapper.write_text("#!/bin/sh\nif [ \"$3\" = rm ]; then exit 1; fi\nexec " + shlex.quote(actual_docker) + " \"$@\"\n")
            wrapper.chmod(0o700)
            try:
                try:
                    result = run_synthetic(os.environ["SESE_TEST_IMAGE"], timeout=15, docker=str(wrapper), journal_dir=self.journal_dir, docker_host=self.docker_host)
                except RuntimeError as error:
                    match = re.search(r"container_id=([0-9a-f]{64})(?:;|$)", str(error))
                    if match:
                        container_id = match.group(1)
                    self.assertIsNotNone(match, str(error))
                    self.assertIn("cleanup unconfirmed", str(error))
                    record = json.loads(next(Path(self.journal_dir).glob("*.json")).read_text())
                    self.assertEqual(record["state"], "pending")
                    with self.assertRaisesRegex(RuntimeError, "unresolved synthetic container receipt"):
                        run_synthetic(os.environ["SESE_TEST_IMAGE"], timeout=15, docker=str(wrapper), journal_dir=self.journal_dir, docker_host=self.docker_host)
                    self.assertEqual(len(list(Path(self.journal_dir).glob("sese-synthetic-*.json"))), 1)
                else:
                    container_id = result["container_id"]
                    self.fail("cleanup failure returned a verdict")
            finally:
                if container_id is not None:
                    subprocess.run([actual_docker, "--host", self.docker_host, "rm", "-f", container_id], capture_output=True, text=True, timeout=10, check=True)
        remaining = subprocess.run(
            [actual_docker, "--host", self.docker_host, "ps", "-aq", "--no-trunc", "--filter", "id=" + container_id],
            capture_output=True, text=True, timeout=5, check=True,
        )
        self.assertEqual(remaining.stdout.strip(), "")

    @unittest.skipUnless(os.environ.get("SESE_TEST_IMAGE"), "requires an explicitly selected local Docker image")
    def test_explicit_recovery_requires_exact_observed_identity_and_unblocks_journal(self):
        from synthetic_container_lifecycle import recover_pending
        actual_docker = shutil.which("docker")
        image = os.environ["SESE_TEST_IMAGE"]
        wrapper = Path(self.journal_dir) / "docker-wrapper"
        wrapper.write_text("#!/bin/sh\nif [ \"$3\" = rm ]; then exit 1; fi\nexec " + shlex.quote(actual_docker) + " \"$@\"\n")
        wrapper.chmod(0o700)
        container_id = None
        try:
            with self.assertRaisesRegex(RuntimeError, "cleanup unconfirmed") as caught:
                run_synthetic(image, timeout=15, docker=str(wrapper), journal_dir=self.journal_dir, docker_host=self.docker_host,
                              expected_worker_sha256=os.environ.get("SESE_TEST_WORKER_SHA256"))
            container_id = re.search(r"container_id=([0-9a-f]{64})(?:;|$)", str(caught.exception)).group(1)
            receipt = next(Path(self.journal_dir).glob("sese-synthetic-*.json"))
            observed = inspect_pending(receipt, docker=actual_docker)
            self.assertEqual(observed["container_id"], container_id)
            if os.environ.get("SESE_TEST_WORKER_SHA256"):
                pending = json.loads(receipt.read_text())
                self.assertEqual(pending["schema"], "sese-synthetic-container-intent-v5")
                self.assertEqual(pending["worker_pin_status"], "unconfirmed")
            before = receipt.read_bytes()
            expected_hash = hashlib.sha256(before).hexdigest()
            inspection = subprocess.run([sys.executable, str(Path(__file__).parent / "synthetic_container_recover.py"), str(receipt), "--inspect-only", "--docker", actual_docker], capture_output=True, text=True, timeout=15)
            self.assertEqual(inspection.returncode, 0, inspection.stderr)
            self.assertEqual(json.loads(inspection.stdout), observed)
            self.assertEqual(observed["receipt_sha256"], expected_hash)
            with self.assertRaisesRegex(ValueError, "identity mismatch"):
                recover_pending(receipt, container_id="0" * 64, daemon_id=observed["daemon_id"], docker_host=self.docker_host, image=image, receipt_sha256=expected_hash, actor="test-operator", docker=actual_docker)
            with self.assertRaisesRegex(ValueError, "receipt changed"):
                recover_pending(receipt, container_id=container_id, daemon_id=observed["daemon_id"], docker_host=self.docker_host, image=image, receipt_sha256="f" * 64, actor="test-operator", docker=actual_docker)
            self.assertEqual(list(Path(self.journal_dir).glob(".sese-recovery-*.json")), [])
            self.assertEqual(receipt.read_bytes(), before)
            self.assertEqual(inspect_pending(receipt, docker=actual_docker)["container_id"], container_id)
            with mock.patch("synthetic_container_lifecycle._write_receipt", side_effect=OSError("test write failure")):
                with self.assertRaisesRegex(RuntimeError, "recovery intent write unconfirmed.*recovery_path="):
                    recover_pending(receipt, container_id=container_id, daemon_id=observed["daemon_id"], docker_host=self.docker_host, image=image, receipt_sha256=expected_hash, actor="test-operator", docker=actual_docker)
            self.assertEqual(receipt.read_bytes(), before)
            self.assertEqual(inspect_pending(receipt, docker=actual_docker)["container_id"], container_id)
            with self.assertRaisesRegex(RuntimeError, "recovery cleanup unconfirmed"):
                recover_pending(receipt, container_id=container_id, daemon_id=observed["daemon_id"], docker_host=self.docker_host, image=image, receipt_sha256=expected_hash, actor="test-operator", docker=str(wrapper))
            self.assertEqual(receipt.read_bytes(), before)
            failed_audit = json.loads(next(Path(self.journal_dir).glob(".sese-recovery-*.json")).read_text())
            self.assertEqual(failed_audit["status"], "started")
            self.assertEqual(inspect_pending(receipt, docker=actual_docker)["container_id"], container_id)
            command = [sys.executable, str(Path(__file__).parent / "synthetic_container_recover.py"), str(receipt),
                       "--container-id", container_id, "--daemon-id", observed["daemon_id"], "--docker-host", self.docker_host,
                       "--image", image, "--receipt-sha256", expected_hash, "--actor", "test-operator",
                       "--docker", actual_docker, "--remove-observed-container"]
            recovered = subprocess.run(command, capture_output=True, text=True, timeout=25)
            self.assertEqual(recovered.returncode, 0, recovered.stderr)
            result = json.loads(recovered.stdout)
            self.assertEqual(result["status"], "removed")
            self.assertFalse(result["execution_authenticated"])
            self.assertFalse(result["promotion_evidence"])
            saved = json.loads(receipt.read_text())
            self.assertEqual(saved["state"], "removed")
            self.assertEqual(saved["result_status"], "failed")
            if os.environ.get("SESE_TEST_WORKER_SHA256"):
                self.assertEqual(saved["worker_pin_status"], "unconfirmed")
                self.assertEqual(saved["expected_worker_sha256"], os.environ["SESE_TEST_WORKER_SHA256"])
            audit = json.loads(Path(result["recovery_path"]).read_text())
            self.assertEqual(audit["actor"], "test-operator")
            self.assertEqual(audit["status"], "completed")
            following = run_synthetic(image, timeout=15, docker=actual_docker, journal_dir=self.journal_dir, docker_host=self.docker_host)
            self.assertNotEqual(following["container_id"], container_id)
        finally:
            if container_id is not None:
                subprocess.run([actual_docker, "--host", self.docker_host, "rm", "-f", container_id], capture_output=True, text=True, timeout=10)

    @unittest.skipUnless(os.environ.get("SESE_TEST_IMAGE"), "requires an explicitly selected local Docker image")
    def test_receipt_write_ambiguity_keeps_recovery_blocked(self):
        import synthetic_container_lifecycle as lifecycle
        actual_docker = shutil.which("docker")
        image = os.environ["SESE_TEST_IMAGE"]
        wrapper = Path(self.journal_dir) / "docker-wrapper"
        wrapper.write_text("#!/bin/sh\nif [ \"$3\" = rm ]; then exit 1; fi\nexec " + shlex.quote(actual_docker) + " \"$@\"\n")
        wrapper.chmod(0o700)
        container_id = None
        try:
            with self.assertRaisesRegex(RuntimeError, "cleanup unconfirmed") as caught:
                run_synthetic(image, timeout=15, docker=str(wrapper), journal_dir=self.journal_dir, docker_host=self.docker_host)
            container_id = re.search(r"container_id=([0-9a-f]{64})(?:;|$)", str(caught.exception)).group(1)
            receipt = next(Path(self.journal_dir).glob("sese-synthetic-*.json"))
            observed = inspect_pending(receipt, docker=actual_docker)
            digest = hashlib.sha256(receipt.read_bytes()).hexdigest()
            original_write = lifecycle._write_receipt
            def fail_after_replacement(path, record, *, create):
                original_write(path, record, create=create)
                if Path(path) == receipt.resolve() and record.get("state") == "removed":
                    raise OSError("simulated sync failure after replacement")
            with mock.patch("synthetic_container_lifecycle._write_receipt", side_effect=fail_after_replacement):
                with self.assertRaisesRegex(RuntimeError, "receipt update unconfirmed"):
                    lifecycle.recover_pending(receipt, container_id=container_id, daemon_id=observed["daemon_id"], docker_host=self.docker_host, image=image, receipt_sha256=digest, actor="test-operator", docker=actual_docker)
            self.assertEqual(json.loads(receipt.read_text())["state"], "removed")
            self.assertEqual(json.loads(next(Path(self.journal_dir).glob(".sese-recovery-*.json")).read_text())["status"], "started")
            with self.assertRaisesRegex(RuntimeError, "unresolved synthetic recovery"):
                run_synthetic(image, timeout=2, docker="/nonexistent/docker", journal_dir=self.journal_dir, docker_host=self.docker_host)
        finally:
            if container_id is not None:
                subprocess.run([actual_docker, "--host", self.docker_host, "rm", "-f", container_id], capture_output=True, text=True, timeout=10)

    @unittest.skipUnless(os.environ.get("SESE_TEST_IMAGE"), "requires an explicitly selected local Docker image")
    def test_changed_daemon_before_cleanup_withholds_verdict_without_removing(self):
        actual_docker = shutil.which("docker")
        self.assertIsNotNone(actual_docker)
        container_id = None
        with tempfile.TemporaryDirectory(prefix="sese-daemon-switch-test-") as directory:
            wrapper = Path(directory) / "docker-wrapper"
            marker = Path(directory) / "info-seen"
            rm_marker = Path(directory) / "rm-called"
            wrapper.write_text(
                "#!/bin/sh\nif [ \"$3\" = info ]; then if [ -e " + shlex.quote(str(marker)) + ".twice ]"
                + "; then echo sese-other-daemon; exit 0; fi; if [ -e " + shlex.quote(str(marker))
                + " ]; then touch " + shlex.quote(str(marker)) + ".twice; else touch " + shlex.quote(str(marker)) + "; fi"
                + "; fi\nif [ \"$3\" = rm ]; then touch " + shlex.quote(str(rm_marker))
                + "; exit 1; fi\nexec " + shlex.quote(actual_docker) + " \"$@\"\n"
            )
            wrapper.chmod(0o700)
            try:
                try:
                    result = run_synthetic(os.environ["SESE_TEST_IMAGE"], timeout=15, docker=str(wrapper), journal_dir=self.journal_dir, docker_host=self.docker_host)
                except RuntimeError as error:
                    match = re.search(r"container_id=([0-9a-f]{64})(?:;|$)", str(error))
                    if match:
                        container_id = match.group(1)
                    self.assertIsNotNone(match, str(error))
                    self.assertIn("daemon changed", str(error))
                    self.assertFalse(rm_marker.exists())
                    record = json.loads(next(Path(self.journal_dir).glob("*.json")).read_text())
                    self.assertEqual(record["state"], "pending")
                else:
                    container_id = result["container_id"]
                    self.fail("daemon mismatch returned a verdict")
            finally:
                if container_id is not None:
                    subprocess.run([actual_docker, "--host", self.docker_host, "rm", "-f", container_id], capture_output=True, text=True, timeout=10, check=True)

    @unittest.skipUnless(os.environ.get("SESE_TEST_TIMEOUT_IMAGE"), "requires an explicitly selected local sleep image")
    def test_real_timeout_removes_its_container_and_returns_no_verdict(self):
        with self.assertRaisesRegex(RuntimeError, "deadline exceeded.*container_id=([0-9a-f]{64})") as caught:
            run_synthetic(os.environ["SESE_TEST_TIMEOUT_IMAGE"], timeout=2, journal_dir=self.journal_dir, docker_host=self.docker_host)
        container_id = re.search(r"container_id=([0-9a-f]{64})(?:;|$)", str(caught.exception)).group(1)
        record = json.loads(next(Path(self.journal_dir).glob("*.json")).read_text())
        self.assertEqual(record["state"], "removed")
        self.assertEqual(record["container_id"], container_id)
        self.assertEqual(record["result_status"], "failed")
        self.assertIsNone(record["output_sha256"])
        remaining = subprocess.run(
            ["docker", "--host", self.docker_host, "ps", "-aq", "--no-trunc", "--filter", "id=" + container_id],
            capture_output=True, text=True, timeout=5, check=True,
        )
        self.assertEqual(remaining.stdout.strip(), "")

    @unittest.skipUnless(os.environ.get("SESE_TEST_TIMEOUT_IMAGE"), "requires an explicitly selected local sleep image")
    def test_intent_survives_parent_death_and_identifies_the_test_container(self):
        image = os.environ["SESE_TEST_TIMEOUT_IMAGE"]
        child = subprocess.Popen(
            [sys.executable, "-c", "import sys; from synthetic_container_lifecycle import run_synthetic; run_synthetic(sys.argv[1], journal_dir=sys.argv[2], docker_host=sys.argv[3], timeout=30)", image, self.journal_dir, self.docker_host],
            cwd=Path(__file__).parent, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        container_id = None
        try:
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                records = list(Path(self.journal_dir).glob("*.json"))
                if records:
                    try:
                        record = json.loads(records[0].read_text())
                    except json.JSONDecodeError:
                        time.sleep(0.05)
                        continue
                    info = subprocess.run(["docker", "--host", self.docker_host, "inspect", record["name"]], capture_output=True, text=True, timeout=3)
                    if info.returncode == 0:
                        details = json.loads(info.stdout)[0]
                        if details["State"]["Running"]:
                            self.assertEqual(details["Config"]["Labels"]["io.sese.synthetic-probe"], record["name"])
                            container_id = details["Id"]
                            break
                self.assertIsNone(child.poll(), "probe exited before container was running")
                time.sleep(0.05)
            self.assertIsNotNone(container_id, "container did not start")
            child.kill()
            child.wait(timeout=5)
            persisted = json.loads(records[0].read_text())
            self.assertEqual(persisted["state"], "pending")
            self.assertEqual(persisted["image"], image)
            inspection = inspect_pending(records[0])
            self.assertEqual(inspection["status"], "observed")
            self.assertEqual(inspection["container_id"], container_id)
            self.assertTrue(inspection["running"])
            wrong_artifact = dict(persisted, artifact_sha256="0" * 64)
            records[0].write_text(json.dumps(wrong_artifact))
            with self.assertRaisesRegex(ValueError, "request identity"):
                inspect_pending(records[0])
            legacy = {key: persisted[key] for key in ("name", "image", "state", "daemon_id", "docker_host")}
            legacy["schema"] = "sese-synthetic-container-intent-v3"
            records[0].write_text(json.dumps(legacy))
            self.assertEqual(inspect_pending(records[0])["status"], "observed")
            records[0].write_text(json.dumps(persisted))
            wrapper = Path(self.journal_dir) / "different-daemon"
            wrapper.write_text("#!/bin/sh\nif [ \"$3\" = info ]; then echo sese-other-daemon; exit 0; fi\nexec " + shlex.quote(shutil.which("docker")) + " \"$@\"\n")
            wrapper.chmod(0o700)
            self.assertEqual(inspect_pending(records[0], docker=str(wrapper))["status"], "unknown")
            original = records[0].read_bytes()
            wrong_image = dict(persisted, image="sha256:" + "0" * 64)
            records[0].write_text(json.dumps(wrong_image))
            self.assertEqual(inspect_pending(records[0])["status"], "unknown")
            wrong_host = dict(persisted, docker_host="unix:///nonexistent/docker.sock")
            records[0].write_text(json.dumps(wrong_host))
            self.assertEqual(inspect_pending(records[0])["status"], "unknown")
            records[0].write_bytes(original)
            after = subprocess.run(["docker", "--host", self.docker_host, "inspect", "--format", "{{.State.Running}}", container_id], capture_output=True, text=True, timeout=5, check=True)
            self.assertEqual(after.stdout.strip(), "true")
        finally:
            if child.poll() is None:
                child.kill()
                child.wait(timeout=5)
            if container_id is not None:
                subprocess.run(["docker", "--host", self.docker_host, "rm", "-f", container_id], capture_output=True, text=True, timeout=10, check=True)
        remaining = subprocess.run(["docker", "--host", self.docker_host, "ps", "-aq", "--no-trunc", "--filter", "id=" + container_id], capture_output=True, text=True, timeout=5, check=True)
        self.assertEqual(remaining.stdout.strip(), "")

    @unittest.skipUnless(os.environ.get("SESE_TEST_FLOOD_IMAGE"), "requires an explicitly selected local output-flood image")
    def test_real_output_flood_is_rejected_and_container_removed(self):
        with self.assertRaisesRegex(RuntimeError, "response exceeds 16 bytes.*container_id=([0-9a-f]{64})") as caught:
            run_synthetic(os.environ["SESE_TEST_FLOOD_IMAGE"], timeout=10, journal_dir=self.journal_dir, docker_host=self.docker_host)
        container_id = re.search(r"container_id=([0-9a-f]{64})(?:;|$)", str(caught.exception)).group(1)
        remaining = subprocess.run(
            ["docker", "--host", self.docker_host, "ps", "-aq", "--no-trunc", "--filter", "id=" + container_id],
            capture_output=True, text=True, timeout=5, check=True,
        )
        self.assertEqual(remaining.stdout.strip(), "")


if __name__ == "__main__":
    unittest.main()
