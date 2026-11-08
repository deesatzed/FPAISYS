"""Experimental built-in synthetic container probe; never use for untrusted code.

The application uses it for non-promotional synthetic experiments. It does not
attest Docker, the image, the host, or cleanup after a daemon failure.
"""
import math
import json
import hashlib
import fcntl
import os
import re
import selectors
import stat
import struct
import subprocess
import tempfile
import time
import uuid
from pathlib import Path

# Fixed built-in module, matching the Rust synthetic throttle artifact.
_THROTTLE = bytes.fromhex(
    "0061736d01000000"
    "010f0160087e7c7e7f7e7e7c7f037f7c7f03020100071501116576616c7561"
    "74655f6576656e745f763100000a0a0108004101200620070b"
)
_WINDOW_PROBE = bytes.fromhex(
    "0061736d01000000010f0160087e7c7e7f7e7e7c7f037f7c7f030201000715"
    "01116576616c756174655f6576656e745f763100000a12011000410020012000"
    "baa02002baa020030b"
)
SYNTHETIC_CANDIDATES = ("allow", "throttle", "quarantine", "window-probe")


def _synthetic_request(candidate, event, window):
    decisions = {"allow": 0, "throttle": 1, "quarantine": 2}
    if not isinstance(candidate, str) or candidate not in SYNTHETIC_CANDIDATES:
        raise ValueError("unknown synthetic candidate")
    if not isinstance(event, (tuple, list)) or len(event) != 4 or not isinstance(window, (tuple, list)) or len(window) != 4:
        raise ValueError("invalid synthetic event or window")
    timestamp, entity, metric, flags = event
    count, mean, p99, last_error = window
    for value, maximum in [(timestamp, 2**64 - 1), (entity, 2**64 - 1), (flags, 2**32 - 1),
                           (count, 2**64 - 1), (p99, 2**64 - 1), (last_error, 2**32 - 1)]:
        if type(value) is not int or not 0 <= value <= maximum:
            raise ValueError("invalid synthetic integer")
    try:
        if type(metric) not in (int, float) or type(mean) not in (int, float):
            raise ValueError("invalid synthetic float")
        metric, mean = float(metric), float(mean)
    except OverflowError as exc:
        raise ValueError("invalid synthetic float") from exc
    if not math.isfinite(metric) or not math.isfinite(mean):
        raise ValueError("invalid synthetic float")
    artifact = (_WINDOW_PROBE if candidate == "window-probe" else
                _THROTTLE[:-6] + bytes([decisions[candidate]]) + _THROTTLE[-5:])
    return struct.pack("<QdQIQQdI", count, mean, p99, last_error, timestamp, entity, metric, flags) + artifact, artifact


def _docker_env():
    return {key: value for key, value in os.environ.items() if not key.startswith("DOCKER_")}


def _validate_host(host):
    if not isinstance(host, str) or not host.startswith("unix:///") or len(host) > 1024:
        raise ValueError("Docker endpoint must be a local unix socket URI")
    path = host[len("unix://"):]
    if (path == "/" or path.startswith("//") or os.path.normpath(path) != path or any(c in path for c in "\x00\n\r?#%")):
        raise ValueError("Docker endpoint must be a local unix socket URI")
    return host


def _docker_read(args, *, limit=4096, deadline=None):
    """Read a bounded Docker response; failures and deadlines are unknown."""
    now = time.monotonic()
    deadline = min(now + 5, deadline) if deadline is not None else now + 5
    if now >= deadline:
        return None
    try:
        proc = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=_docker_env())
    except OSError:
        return None
    output = bytearray()
    try:
        os.set_blocking(proc.stdout.fileno(), False)
        with selectors.DefaultSelector() as selector:
            selector.register(proc.stdout, selectors.EVENT_READ)
            eof = False
            while not (eof and proc.poll() is not None):
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    return None
                if not eof:
                    for key, _ in selector.select(min(remaining, 0.05)):
                        chunk = os.read(key.fd, limit + 1 - len(output))
                        if not chunk:
                            selector.unregister(proc.stdout)
                            eof = True
                            break
                        output.extend(chunk)
                        if len(output) > limit:
                            return None
                else:
                    time.sleep(min(remaining, 0.01))
        if proc.returncode != 0:
            return None
        try:
            return output.decode("utf-8").strip()
        except UnicodeDecodeError:
            return None
    except OSError:
        return None
    finally:
        if proc.poll() is None:
            try:
                proc.kill()
            except ProcessLookupError:
                pass
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                pass
        proc.stdout.close()


def _daemon_id(docker, host, *, deadline=None):
    value = _docker_read([docker, "--host", host, "info", "--format", "{{.ID}}"], limit=128, deadline=deadline)
    if value is None or not re.fullmatch(r"[A-Za-z0-9:_.-]{1,128}", value):
        return None
    return value


def inspect_pending(receipt_path, *, docker="docker"):
    """Inspect a preflight or pending record without changing it or Docker."""
    path = Path(receipt_path)
    if not hasattr(os, "O_NOFOLLOW"):
        raise OSError("safe receipt opening is unavailable")
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | getattr(os, "O_NONBLOCK", 0))
    with os.fdopen(fd, "rb") as stream:
        metadata = os.fstat(stream.fileno())
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != os.geteuid() or metadata.st_mode & 0o077:
            raise ValueError("receipt must be an owner-only regular file")
        payload = stream.read(4097)
    if len(payload) > 4096:
        raise ValueError("receipt too large")

    def unique_pairs(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate receipt field")
            result[key] = value
        return result

    try:
        record = json.loads(payload, object_pairs_hook=unique_pairs)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ValueError("invalid receipt") from exc
    if not isinstance(record, dict):
        raise ValueError("invalid pending receipt fields")
    version = record.get("schema")
    legacy = version in ("sese-synthetic-container-intent-v1", "sese-synthetic-container-intent-v2")
    expected_fields = {"schema", "name", "image", "state"}
    if version in ("sese-synthetic-container-intent-v2", "sese-synthetic-container-intent-v3", "sese-synthetic-container-intent-v4", "sese-synthetic-container-preflight-v1"):
        expected_fields.add("daemon_id")
    if version in ("sese-synthetic-container-intent-v3", "sese-synthetic-container-intent-v4", "sese-synthetic-container-preflight-v1"):
        expected_fields.add("docker_host")
    if version in ("sese-synthetic-container-intent-v4", "sese-synthetic-container-preflight-v1"):
        expected_fields.update(("candidate", "artifact_sha256", "request_sha256"))
    if set(record) != expected_fields:
        raise ValueError("invalid pending receipt fields")
    name, image = record["name"], record["image"]
    if (version not in ("sese-synthetic-container-intent-v1", "sese-synthetic-container-intent-v2", "sese-synthetic-container-intent-v3", "sese-synthetic-container-intent-v4", "sese-synthetic-container-preflight-v1") or record["state"] != ("not_created" if version == "sese-synthetic-container-preflight-v1" else "pending")
            or not isinstance(name, str) or not re.fullmatch(r"sese-synthetic-[0-9a-f]{32}", name)
            or path.name != name + ".json" or not isinstance(image, str)
            or not re.fullmatch(r"sha256:[0-9a-f]{64}", image)):
        raise ValueError("invalid pending receipt")
    if version in ("sese-synthetic-container-intent-v4", "sese-synthetic-container-preflight-v1"):
        _, artifact = _synthetic_request(record["candidate"], (1, 42, 3.0, 4), (0, 0.0, 0, 0))
        if (record["artifact_sha256"] != hashlib.sha256(artifact).hexdigest()
                or not isinstance(record["request_sha256"], str)
                or not re.fullmatch(r"[0-9a-f]{64}", record["request_sha256"])):
            raise ValueError("invalid synthetic request identity")
    if version == "sese-synthetic-container-preflight-v1":
        _validate_host(record["docker_host"])
        if record["daemon_id"] is not None:
            raise ValueError("invalid preflight receipt")
        return {"status": "not_created", "name": name, "image": image,
                "receipt_sha256": hashlib.sha256(payload).hexdigest(), "docker_host": record["docker_host"]}
    unknown = {"status": "unknown", "name": name, "image": image,
               "receipt_sha256": hashlib.sha256(payload).hexdigest(), "docker_host": record.get("docker_host")}
    if legacy or record["daemon_id"] is None:
        return unknown
    host = _validate_host(record["docker_host"])
    expected_daemon = record["daemon_id"]
    if not isinstance(expected_daemon, str) or not re.fullmatch(r"[A-Za-z0-9:_.-]{1,128}", expected_daemon):
        raise ValueError("invalid daemon identity")
    if _daemon_id(docker, host) != expected_daemon:
        return unknown
    template = '{{.Id}}|{{.Name}}|{{.Image}}|{{index .Config.Labels "io.sese.synthetic-probe"}}|{{.State.Running}}'
    output = _docker_read([docker, "--host", host, "inspect", "--type", "container", "--format", template, name])
    if output is None or _daemon_id(docker, host) != expected_daemon:
        return unknown
    fields = output.split("|")
    if (len(fields) != 5 or not re.fullmatch(r"[0-9a-f]{64}", fields[0])
            or fields[1] != "/" + name or fields[2] != image or fields[3] != name
            or fields[4] not in ("true", "false")):
        return unknown
    return {"status": "observed", "name": name, "image": image, "daemon_id": expected_daemon,
            "container_id": fields[0], "running": fields[4] == "true", "docker_host": host,
            "receipt_sha256": hashlib.sha256(payload).hexdigest()}


def recover_pending(receipt_path, *, container_id, daemon_id, docker_host, image, receipt_sha256, actor, docker="docker"):
    """Explicitly remove one currently observed synthetic container after exact identity checks.

    Docker metadata and the operator's identity are unauthenticated. A missing container is
    unknown, not success. An interrupted recovery retains its pending intent for inspection.
    """
    if (not isinstance(container_id, str) or not re.fullmatch(r"[0-9a-f]{64}", container_id)
            or not isinstance(daemon_id, str) or not re.fullmatch(r"[A-Za-z0-9:_.-]{1,128}", daemon_id)
            or not isinstance(image, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", image)
            or not isinstance(receipt_sha256, str) or not re.fullmatch(r"[0-9a-f]{64}", receipt_sha256)
            or not isinstance(actor, str) or not actor.strip() or len(actor) > 256 or any(ord(c) < 32 for c in actor)):
        raise ValueError("invalid recovery identity")
    host = _validate_host(docker_host)
    receipt_path = Path(receipt_path)
    directory = receipt_path.parent.resolve(strict=True)
    path = directory / receipt_path.name
    directory_fd = os.open(directory, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | os.O_NOFOLLOW)
    try:
        deadline = time.monotonic() + 5
        while True:
            try:
                fcntl.flock(directory_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except (BlockingIOError, InterruptedError):
                if time.monotonic() >= deadline:
                    raise RuntimeError("journal lock deadline exceeded")
                time.sleep(min(0.02, max(0, deadline - time.monotonic())))

        def read_receipt():
            fd = os.open(path.name, os.O_RDONLY | os.O_NOFOLLOW | getattr(os, "O_NONBLOCK", 0), dir_fd=directory_fd)
            with os.fdopen(fd, "rb") as stream:
                meta = os.fstat(stream.fileno())
                if not stat.S_ISREG(meta.st_mode) or meta.st_uid != os.geteuid() or meta.st_mode & 0o077:
                    raise ValueError("unsafe receipt metadata")
                raw = stream.read(4097)
            if len(raw) > 4096 or hashlib.sha256(raw).hexdigest() != receipt_sha256:
                raise ValueError("recovery identity mismatch: receipt changed")
            return raw

        raw = read_receipt()
        observed = inspect_pending(path, docker=docker)
        if (observed.get("status") != "observed" or observed.get("container_id") != container_id
                or observed.get("daemon_id") != daemon_id or observed.get("image") != image):
            raise ValueError("recovery identity mismatch or container observation unknown")
        record = json.loads(raw)
        if record.get("schema") != "sese-synthetic-container-intent-v4" or record.get("docker_host") != host:
            raise ValueError("recovery identity mismatch: endpoint or receipt version")
        read_receipt()
        recovery_path = directory / (".sese-recovery-" + uuid.uuid4().hex + ".json")
        audit = {"schema": "sese-synthetic-container-recovery-v2", "status": "started", "actor": actor,
                 "receipt_name": path.name, "receipt_sha256": receipt_sha256, "container_id": container_id,
                 "daemon_id": daemon_id, "docker_host": host, "image": image, "started_unix_ns": str(time.time_ns()),
                 "synthetic": True, "execution_authenticated": False, "promotion_evidence": False}
        try:
            _write_receipt(recovery_path, audit, create=True)
        except OSError as exc:
            raise RuntimeError(f"recovery intent write unconfirmed; recovery_path={recovery_path}") from exc
        # Recheck the same identity immediately before the explicit removal.
        if inspect_pending(path, docker=docker) != observed or _daemon_id(docker, host) != daemon_id:
            raise RuntimeError(f"recovery identity changed; recovery_path={recovery_path}")
        read_receipt()
        try:
            removal = subprocess.run([docker, "--host", host, "rm", "-f", container_id], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5, env=_docker_env())
        except (OSError, subprocess.SubprocessError) as exc:
            raise RuntimeError(f"recovery cleanup unconfirmed; recovery_path={recovery_path}") from exc
        remaining = _docker_read([docker, "--host", host, "ps", "-aq", "--no-trunc", "--filter", "id=" + container_id], limit=256)
        if removal.returncode != 0 or remaining != "" or _daemon_id(docker, host) != daemon_id:
            raise RuntimeError(f"recovery cleanup unconfirmed; recovery_path={recovery_path}")
        read_receipt()
        record.update(state="removed", container_id=container_id, result_status="failed", output_sha256=None)
        try:
            _write_receipt(path, record, create=False)
        except OSError as exc:
            raise RuntimeError(f"receipt update unconfirmed; recovery_path={recovery_path}; receipt_path={path}") from exc
        final_bytes = (json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n").encode()
        audit.update(status="completed", finished_unix_ns=str(time.time_ns()), finished_receipt_sha256=hashlib.sha256(final_bytes).hexdigest())
        try:
            _write_receipt(recovery_path, audit, create=False)
        except OSError as exc:
            raise RuntimeError(f"recovery result write unconfirmed; recovery_path={recovery_path}") from exc
        return {**audit, "status": "removed", "recovery_path": str(recovery_path), "receipt_path": str(path)}
    finally:
        os.close(directory_fd)


def _write_receipt(path, record, *, create):
    """Write an owner-only record and sync it before the next lifecycle step."""
    payload = (json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n").encode()
    if create:
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0), 0o600)
        with os.fdopen(fd, "wb") as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
    else:
        fd, temporary = tempfile.mkstemp(prefix=".sese-receipt-", dir=path.parent)
        try:
            with os.fdopen(fd, "wb") as stream:
                stream.write(payload)
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(temporary, path)
        finally:
            if os.path.exists(temporary):
                os.unlink(temporary)
    directory = os.open(path.parent, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
    try:
        os.fsync(directory)
    finally:
        os.close(directory)


def _check_recovery_audits(directory_fd, filenames):
    """Require v2 recovery attempts to have a completed, receipt-bound resolution."""
    def unique_pairs(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate recovery field")
            result[key] = value
        return result

    def read_file(filename):
        fd = os.open(filename, os.O_RDONLY | os.O_NOFOLLOW | getattr(os, "O_NONBLOCK", 0), dir_fd=directory_fd)
        with os.fdopen(fd, "rb") as stream:
            meta = os.fstat(stream.fileno())
            if not stat.S_ISREG(meta.st_mode) or meta.st_uid != os.geteuid() or meta.st_mode & 0o077:
                raise ValueError("unsafe recovery metadata")
            raw = stream.read(4097)
        if len(raw) > 4096:
            raise ValueError("oversized recovery record")
        return raw

    started, completed = set(), set()
    for filename in filenames:
        if not (filename.startswith(".sese-recovery-") and filename.endswith(".json")):
            continue
        try:
            if not re.fullmatch(r"\.sese-recovery-[0-9a-f]{32}\.json", filename):
                raise ValueError("invalid recovery filename")
            audit = json.loads(read_file(filename), object_pairs_hook=unique_pairs)
            if not isinstance(audit, dict):
                raise ValueError("invalid recovery record")
            # Historical v1 records predate completion binding; do not reinterpret them.
            if audit.get("schema") == "sese-synthetic-container-recovery-v1":
                continue
            fields = {"schema", "status", "actor", "receipt_name", "receipt_sha256", "container_id", "daemon_id", "docker_host", "image", "started_unix_ns", "synthetic", "execution_authenticated", "promotion_evidence"}
            if audit.get("status") == "completed":
                fields.update(("finished_unix_ns", "finished_receipt_sha256"))
            if (set(audit) != fields or audit.get("schema") != "sese-synthetic-container-recovery-v2"
                    or audit.get("status") not in ("started", "completed") or audit.get("synthetic") is not True
                    or audit.get("execution_authenticated") is not False or audit.get("promotion_evidence") is not False):
                raise ValueError("invalid recovery provenance")
            if (not isinstance(audit["actor"], str) or not audit["actor"].strip() or len(audit["actor"]) > 256
                    or any(ord(c) < 32 for c in audit["actor"])
                    or not isinstance(audit["receipt_name"], str) or not re.fullmatch(r"sese-synthetic-[0-9a-f]{32}\.json", audit["receipt_name"])
                    or not isinstance(audit["receipt_sha256"], str) or not re.fullmatch(r"[0-9a-f]{64}", audit["receipt_sha256"])
                    or not isinstance(audit["container_id"], str) or not re.fullmatch(r"[0-9a-f]{64}", audit["container_id"])
                    or not isinstance(audit["daemon_id"], str) or not re.fullmatch(r"[A-Za-z0-9:_.-]{1,128}", audit["daemon_id"])
                    or not isinstance(audit["image"], str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", audit["image"])
                    or not isinstance(audit["started_unix_ns"], str) or not re.fullmatch(r"[0-9]{1,32}", audit["started_unix_ns"])):
                raise ValueError("invalid recovery identity")
            _validate_host(audit["docker_host"])
            key = tuple(audit[k] for k in ("receipt_name", "receipt_sha256", "container_id", "daemon_id", "docker_host", "image"))
            if audit["status"] == "started":
                started.add(key)
                continue
            if (not isinstance(audit["finished_unix_ns"], str) or not re.fullmatch(r"[0-9]{1,32}", audit["finished_unix_ns"])
                    or not isinstance(audit["finished_receipt_sha256"], str) or not re.fullmatch(r"[0-9a-f]{64}", audit["finished_receipt_sha256"])):
                raise ValueError("invalid recovery completion")
            raw = read_file(audit["receipt_name"])
            receipt = json.loads(raw, object_pairs_hook=unique_pairs)
            if (hashlib.sha256(raw).hexdigest() != audit["finished_receipt_sha256"] or not isinstance(receipt, dict)
                    or receipt.get("state") != "removed" or receipt.get("container_id") != audit["container_id"]
                    or receipt.get("daemon_id") != audit["daemon_id"] or receipt.get("image") != audit["image"]
                    or receipt.get("docker_host") != audit["docker_host"] or receipt.get("result_status") != "failed"):
                raise ValueError("recovery receipt mismatch")
            completed.add(key)
        except (OSError, ValueError, TypeError, KeyError, UnicodeError, json.JSONDecodeError) as exc:
            raise RuntimeError(f"unresolved synthetic recovery: {filename}") from exc
    if started - completed:
        raise RuntimeError("unresolved synthetic recovery: incomplete recovery intent")


def _check_journal(directory_fd):
    """Reject pending, malformed or unverifiable synthetic receipts in this journal."""
    def unique_pairs(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate receipt field")
            result[key] = value
        return result

    # Retry a failed directory sync before accepting any visible completion.
    os.fsync(directory_fd)
    filenames = os.listdir(directory_fd)
    _check_recovery_audits(directory_fd, filenames)
    for filename in filenames:
        if not (filename.startswith("sese-synthetic-") and filename.endswith(".json")):
            continue
        try:
            if not re.fullmatch(r"sese-synthetic-[0-9a-f]{32}\.json", filename):
                raise ValueError("invalid receipt name")
            fd = os.open(filename, os.O_RDONLY | os.O_NOFOLLOW | getattr(os, "O_NONBLOCK", 0), dir_fd=directory_fd)
            with os.fdopen(fd, "rb") as stream:
                meta = os.fstat(stream.fileno())
                if not stat.S_ISREG(meta.st_mode) or meta.st_uid != os.geteuid() or meta.st_mode & 0o077:
                    raise ValueError("unsafe receipt metadata")
                raw = stream.read(4097)
            if len(raw) > 4096:
                raise ValueError("oversized receipt")
            receipt = json.loads(raw, object_pairs_hook=unique_pairs)
            preflight_fields = {"schema", "name", "image", "state", "daemon_id", "docker_host", "candidate", "artifact_sha256", "request_sha256"}
            if isinstance(receipt, dict) and receipt.get("schema") == "sese-synthetic-container-preflight-v1":
                if (set(receipt) != preflight_fields or receipt["state"] != "not_created" or receipt["daemon_id"] is not None
                        or receipt["name"] + ".json" != filename or not isinstance(receipt["image"], str)
                        or not re.fullmatch(r"sha256:[0-9a-f]{64}", receipt["image"])
                        or not isinstance(receipt["request_sha256"], str) or not re.fullmatch(r"[0-9a-f]{64}", receipt["request_sha256"])):
                    raise ValueError("invalid preflight receipt")
                _validate_host(receipt["docker_host"])
                _, artifact = _synthetic_request(receipt["candidate"], (1, 42, 3.0, 4), (0, 0.0, 0, 0))
                if receipt["artifact_sha256"] != hashlib.sha256(artifact).hexdigest():
                    raise ValueError("invalid preflight artifact")
                continue
            fields = {"schema", "name", "image", "state", "daemon_id", "docker_host", "candidate", "artifact_sha256", "request_sha256", "container_id", "result_status", "output_sha256"}
            if not isinstance(receipt, dict) or set(receipt) != fields or receipt["schema"] != "sese-synthetic-container-intent-v4" or receipt["state"] != "removed" or receipt["name"] + ".json" != filename:
                raise ValueError("receipt not confirmed removed")
            if (not isinstance(receipt["image"], str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", receipt["image"])
                    or not isinstance(receipt["daemon_id"], str) or not re.fullmatch(r"[A-Za-z0-9:_.-]{1,128}", receipt["daemon_id"])
                    or not isinstance(receipt["container_id"], str) or not re.fullmatch(r"[0-9a-f]{64}", receipt["container_id"])
                    or not isinstance(receipt["request_sha256"], str) or not re.fullmatch(r"[0-9a-f]{64}", receipt["request_sha256"])):
                raise ValueError("invalid removed receipt")
            _validate_host(receipt["docker_host"])
            _, artifact = _synthetic_request(receipt["candidate"], (1, 42, 3.0, 4), (0, 0.0, 0, 0))
            if receipt["artifact_sha256"] != hashlib.sha256(artifact).hexdigest():
                raise ValueError("invalid artifact identity")
            status, output = receipt["result_status"], receipt["output_sha256"]
            if not ((status == "failed" and output is None) or (status == "valid_verdict" and isinstance(output, str) and re.fullmatch(r"[0-9a-f]{64}", output))):
                raise ValueError("invalid result status")
        except (OSError, ValueError, TypeError, KeyError, UnicodeError, json.JSONDecodeError) as exc:
            raise RuntimeError(f"unresolved synthetic container receipt: {filename}") from exc


def run_synthetic(image, *, journal_dir, docker_host, candidate="throttle", event=(1, 42, 3.0, 4), window=(0, 0.0, 0, 0), timeout=15, docker="docker", intent_callback=None, probe_name=None, require_worker_profile=False):
    """Serialize cooperating runs in one journal; unresolved receipts prevent launch."""
    if not isinstance(image, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", image):
        raise ValueError("image digest must be a full lowercase sha256 identifier")
    if not isinstance(timeout, (float, int)) or not math.isfinite(timeout) or not 0 < timeout <= 60:
        raise ValueError("timeout must be between zero and 60 seconds")
    _validate_host(docker_host)
    _synthetic_request(candidate, event, window)
    if probe_name is not None and (not isinstance(probe_name, str) or not re.fullmatch(r"sese-synthetic-[0-9a-f]{32}", probe_name)):
        raise ValueError("invalid probe name")
    directory = Path(journal_dir).resolve(strict=True)
    if not directory.is_dir():
        raise ValueError("journal directory must exist")
    directory_fd = os.open(directory, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | os.O_NOFOLLOW)
    try:
        lock_deadline = time.monotonic() + timeout
        while True:
            try:
                fcntl.flock(directory_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except (BlockingIOError, InterruptedError):
                remaining = lock_deadline - time.monotonic()
                if remaining <= 0:
                    raise RuntimeError("journal lock deadline exceeded")
                time.sleep(min(0.02, remaining))
        _check_journal(directory_fd)
        return _run_synthetic_locked(image, journal_dir=directory, docker_host=docker_host, candidate=candidate, event=event, window=window, deadline=lock_deadline, docker=docker, intent_callback=intent_callback, probe_name=probe_name, require_worker_profile=require_worker_profile)
    finally:
        os.close(directory_fd)


def _configuration_matches(output, container_id, image, name, *, require_worker_profile=False):
    """Check reported pre-start settings; Docker and its report remain unauthenticated."""
    try:
        obj = json.loads(output)
        host, config, state = obj["HostConfig"], obj["Config"], obj["State"]
        return (obj["Id"] == container_id and obj["Image"] == image
                and obj["Mounts"] == [] and state["Status"] == "created" and state["Running"] is False
                and config["Image"] == image and config["User"] == "65534:65534"
                and (not require_worker_profile or (config["Entrypoint"] == ["/event-worker", "--require-isolation-v1"]
                     and config["Cmd"] in (None, [])
                     and config["Env"] == ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"]))
                and config["OpenStdin"] is True and config["Labels"].get("io.sese.synthetic-probe") == name
                and host["NetworkMode"] == "none" and host["ReadonlyRootfs"] is True
                and host["CapDrop"] == ["ALL"] and host["CapAdd"] in (None, [])
                and host["SecurityOpt"] == ["no-new-privileges"] and host["Privileged"] is False
                and host["PidsLimit"] == 16 and host["Memory"] == 134217728
                and host["MemorySwap"] == 134217728 and host["NanoCpus"] == 1000000000
                and host["Binds"] in (None, []) and host["VolumesFrom"] in (None, [])
                and host["Devices"] in (None, []) and host["DeviceRequests"] in (None, [])
                and host["PidMode"] in ("", "private") and host["IpcMode"] in ("", "private")
                and host["UTSMode"] in ("", "private"))
    except (TypeError, KeyError, ValueError, AttributeError):
        return False


def _run_synthetic_locked(image, *, journal_dir, docker_host, deadline, candidate="throttle", event=(1, 42, 3.0, 4), window=(0, 0.0, 0, 0), docker="docker", intent_callback=None, probe_name=None, require_worker_profile=False):
    """Run a built-in synthetic candidate; return only after verified cleanup."""
    if not re.fullmatch(r"sha256:[0-9a-f]{64}", image):
        raise ValueError("image digest must be a full lowercase sha256 identifier")
    host = _validate_host(docker_host)
    request, artifact = _synthetic_request(candidate, event, window)
    if probe_name is not None and (not isinstance(probe_name, str) or not re.fullmatch(r"sese-synthetic-[0-9a-f]{32}", probe_name)):
        raise ValueError("invalid probe name")
    name = probe_name or "sese-synthetic-" + uuid.uuid4().hex
    directory_path = Path(journal_dir).resolve(strict=True)
    if not directory_path.is_dir():
        raise ValueError("journal directory must exist")
    receipt_path = directory_path / (name + ".json")
    receipt = {"schema": "sese-synthetic-container-preflight-v1", "name": name, "image": image, "state": "not_created", "daemon_id": None, "docker_host": host,
               "candidate": candidate, "artifact_sha256": hashlib.sha256(artifact).hexdigest(), "request_sha256": hashlib.sha256(request).hexdigest()}
    _write_receipt(receipt_path, receipt, create=True)
    if intent_callback is not None:
        intent_callback(str(receipt_path))
    daemon_id = _daemon_id(docker, host, deadline=deadline)
    if daemon_id is None:
        raise RuntimeError(f"Docker daemon identity unconfirmed; receipt_path={receipt_path}")
    volumes = _docker_read([docker, "--host", host, "image", "inspect", "--format", "{{json .Config.Volumes}}", image], deadline=deadline)
    try:
        declared_volumes = json.loads(volumes) if volumes is not None else None
    except json.JSONDecodeError:
        declared_volumes = None
    if volumes is None or declared_volumes not in (None, {}) or volumes not in ("null", "{}"):
        raise RuntimeError(f"image declares volumes or volume configuration unconfirmed; receipt_path={receipt_path}")
    if _daemon_id(docker, host, deadline=deadline) != daemon_id:
        raise RuntimeError(f"Docker daemon changed before launch; receipt_path={receipt_path}")
    receipt.update(schema="sese-synthetic-container-intent-v4", state="pending", daemon_id=daemon_id)
    _write_receipt(receipt_path, receipt, create=False)
    container_id = None
    output = bytearray()
    error = None
    proc = None
    cleanup_verified = False
    args = [
        docker, "--host", host, "create", "--pull=never", "--interactive", "--name", name,
        "--label", "io.sese.synthetic-probe=" + name,
        "--network=none", "--read-only", "--cap-drop=ALL",
        "--security-opt=no-new-privileges", "--pids-limit=16",
        "--memory=128m", "--memory-swap=128m", "--cpus=1",
        "--user=65534:65534", image,
    ]
    try:
        created = _docker_read(args, limit=256, deadline=deadline)
        if created is None or not re.fullmatch(r"[0-9a-f]{64}", created):
            raise RuntimeError("container creation unconfirmed")
        container_id = created
        report = _docker_read([docker, "--host", host, "inspect", "--type", "container", "--format", "{{json .}}", container_id], limit=65536, deadline=deadline)
        if report is None or not _configuration_matches(report, container_id, image, name, require_worker_profile=require_worker_profile) or _daemon_id(docker, host, deadline=deadline) != daemon_id:
            raise RuntimeError("container configuration unconfirmed or mismatched")
        if time.monotonic() >= deadline:
            raise RuntimeError("container deadline exceeded")
        proc = subprocess.Popen([docker, "--host", host, "start", "--attach", "--interactive", container_id], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=_docker_env())
        proc.stdin.write(request)
        proc.stdin.close()
        os.set_blocking(proc.stdout.fileno(), False)
        with selectors.DefaultSelector() as selector:
            selector.register(proc.stdout, selectors.EVENT_READ)
            eof = False
            while not (eof and proc.poll() is not None):
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise RuntimeError("container deadline exceeded")
                if not eof:
                    for key, _ in selector.select(min(remaining, 0.05)):
                        chunk = os.read(key.fd, 17 - len(output))
                        if not chunk:
                            selector.unregister(proc.stdout)
                            eof = True
                            break
                        output.extend(chunk)
                        if len(output) > 16:
                            raise RuntimeError("container response exceeds 16 bytes")
                else:
                    time.sleep(min(remaining, 0.01))
        if proc.returncode != 0:
            raise RuntimeError("container worker failed")
        if len(output) != 16:
            raise RuntimeError("invalid container response length")
        verdict = struct.unpack("<IdI", output)
        if verdict[0] not in (0, 1, 2, 3) or not math.isfinite(verdict[1]):
            raise RuntimeError("invalid synthetic verdict")
    except (OSError, RuntimeError, subprocess.SubprocessError) as exc:
        error = str(exc)
    finally:
        if proc is not None and proc.poll() is None:
            proc.kill()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                error = "Docker client termination unconfirmed"
        if proc is not None and proc.stdout is not None:
            proc.stdout.close()
        if container_id is None:
            error = f"container creation or cleanup unconfirmed; probe name {name}"
        elif _daemon_id(docker, host) != daemon_id:
            error = f"Docker daemon changed; container cleanup unconfirmed: {container_id}"
        else:
            try:
                removal = subprocess.run([docker, "--host", host, "rm", "-f", container_id], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5, env=_docker_env())
                remaining = subprocess.run([docker, "--host", host, "ps", "-aq", "--no-trunc", "--filter", "id=" + container_id], capture_output=True, text=True, timeout=5, env=_docker_env())
                if removal.returncode != 0 or remaining.returncode != 0 or remaining.stdout.strip() or _daemon_id(docker, host) != daemon_id:
                    error = f"container cleanup unconfirmed: {container_id}"
                else:
                    cleanup_verified = True
            except (OSError, subprocess.SubprocessError):
                error = f"container cleanup unconfirmed: {container_id}"
    if cleanup_verified:
        receipt.update(state="removed", container_id=container_id,
                       result_status="valid_verdict" if error is None else "failed",
                       output_sha256=hashlib.sha256(output).hexdigest() if error is None else None)
        try:
            _write_receipt(receipt_path, receipt, create=False)
        except OSError as exc:
            error = f"receipt update unconfirmed: {exc}"
    if error is not None:
        if container_id is not None:
            error += f"; container_id={container_id}"
        error += f"; receipt_path={receipt_path}"
        raise RuntimeError(error)
    return {"verdict": verdict, "candidate": candidate, "artifact_sha256": hashlib.sha256(artifact).hexdigest(), "container_id": container_id, "image": image, "synthetic": True, "receipt_path": str(receipt_path)}
