"""Compare experimental synthetic container results with an application review preview."""
import argparse
import hashlib
import json
import os
import re
import stat
import struct
import subprocess
from pathlib import Path

from synthetic_container_lifecycle import _synthetic_request


def verify_receipts(entries, image, host, *, snapshots=None):
    names, containers = set(), set()
    daemon_id = None
    captured = []
    if snapshots is not None and (not isinstance(snapshots, list) or len(snapshots) != len(entries)):
        raise ValueError("invalid receipt snapshot count")
    for index, (entry, event, window) in enumerate(entries):
        path = Path(entry["receipt_path"])
        try:
            if snapshots is None:
                if not hasattr(os, "O_NOFOLLOW"):
                    raise ValueError("safe receipt opening unavailable")
                fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | getattr(os, "O_NONBLOCK", 0))
                with os.fdopen(fd, "rb") as file:
                    info = os.fstat(file.fileno())
                    if not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o077:
                        raise ValueError("invalid receipt permissions")
                    payload = file.read(4097)
            else:
                payload = snapshots[index].encode("utf-8")
            if len(payload) > 4096:
                raise ValueError("receipt too large")
            def unique_pairs(pairs):
                result = {}
                for key, value in pairs:
                    if key in result:
                        raise ValueError("duplicate receipt field")
                    result[key] = value
                return result
            receipt = json.loads(payload, object_pairs_hook=unique_pairs)
            request, artifact = _synthetic_request(entry["candidate"], event, window)
            actual = entry["actual"]
            output = struct.pack("<IdI", actual["decision"], actual["score"], actual["reason"])
            name, container = receipt["name"], receipt["container_id"]
            if (set(receipt) != {"schema", "name", "image", "state", "daemon_id", "docker_host", "candidate", "artifact_sha256", "request_sha256", "container_id", "result_status", "output_sha256"}
                    or receipt["schema"] != "sese-synthetic-container-intent-v4" or receipt["state"] != "removed"
                    or receipt["result_status"] != "valid_verdict" or receipt["image"] != image or receipt["docker_host"] != host
                    or not isinstance(receipt["daemon_id"], str) or not re.fullmatch(r"[A-Za-z0-9:_.-]{1,128}", receipt["daemon_id"])
                    or (daemon_id is not None and daemon_id != receipt["daemon_id"])
                    or not isinstance(name, str) or not re.fullmatch(r"sese-synthetic-[0-9a-f]{32}", name)
                    or path.name != name + ".json" or name in names
                    or not isinstance(container, str) or not re.fullmatch(r"[0-9a-f]{64}", container) or container in containers
                    or receipt["candidate"] != entry["candidate"]
                    or receipt["artifact_sha256"] != hashlib.sha256(artifact).hexdigest()
                    or entry["artifact_sha256"] != receipt["artifact_sha256"]
                    or receipt["request_sha256"] != hashlib.sha256(request).hexdigest()
                    or receipt["output_sha256"] != hashlib.sha256(output).hexdigest()):
                raise ValueError("receipt binding mismatch")
            names.add(name)
            containers.add(container)
            daemon_id = receipt["daemon_id"]
            captured.append(payload.decode("utf-8"))
        except (OSError, KeyError, AttributeError, TypeError, ValueError, json.JSONDecodeError, struct.error) as error:
            raise ValueError(f"invalid container receipt: {error}") from error
    return captured


def _same(left, right, fields):
    for key in fields:
        a, b = left[key], right[key]
        if key in ("metric", "mean", "score"):
            if struct.pack("<d", a) != struct.pack("<d", b):
                return False
        elif key == "decision":
            decisions = {"allow": 0, "throttle": 1, "quarantine": 2, "escalate": 3}
            if isinstance(a, str):
                a = decisions.get(a)
            if isinstance(b, str):
                b = decisions.get(b)
            if type(a) is not int or type(b) is not int or a not in range(4) or a != b:
                return False
        elif type(a) is not type(b) or a != b:
            return False
    return True


def compare(preview, replay, shadow, observation_id):
    app = preview["replay"]
    if (preview.get("schema") != "sese-synthetic-review-preview-v1"
            or replay.get("schema") != "sese-synthetic-container-replay-experiment-v2"
            or replay.get("fixture_digest") != app.get("fixture_digest")
            or replay.get("version") != app.get("fixture_version")
            or replay.get("state_semantics") != app.get("state_semantics")):
        raise ValueError("fixture identity mismatch")
    if (replay.get("state") != "completed" or replay.get("promotion_evidence") is not False
            or replay.get("passed") != app.get("passed") or not replay.get("cases")
            or len(replay["cases"]) != len(app["cases"])):
        raise ValueError("replay incomplete or result mismatch")
    for local, container in zip(app["cases"], replay["cases"]):
        if (container.get("status") != "completed" or local["id"] != container["id"]
                or container.get("artifact_sha256") != app["candidate_id"]
                or local["passed"] != container["passed"] or local["error"] is not None
                or not _same(local["event"], container["event"], ("timestamp_ns", "entity_id", "metric", "flags"))
                or not _same(local["window"], container["window"], ("count", "mean", "p99_us", "last_error"))
                or not _same(local["expected"], container["expected"], ("decision", "score", "reason"))
                or not _same(local["actual"], container["actual"], ("decision", "score", "reason"))):
            raise ValueError("replay case mismatch")
    if (shadow.get("schema") != "sese-synthetic-container-shadow-experiment-v1"
            or shadow.get("state") != "completed" or shadow.get("promotion_evidence") is not False
            or shadow.get("image") != replay.get("image") or shadow.get("docker_host") != replay.get("docker_host")):
        raise ValueError("shadow comparison incomplete")
    observations = [item for item in preview["observations"] if item["id"] == observation_id]
    if len(observations) != 1:
        raise ValueError("observation not found")
    observation = observations[0]
    event = dict(zip(("timestamp_ns", "entity_id", "metric", "flags"), shadow["event"]))
    window = dict(zip(("count", "mean", "p99_us", "last_error"), shadow["window"]))
    if (not _same(observation["event"], event, ("timestamp_ns", "entity_id", "metric", "flags"))
            or not _same(observation["prior"], window, ("count", "mean", "p99_us", "last_error"))):
        raise ValueError("observation input mismatch")
    for role in ("champion", "shadow"):
        entry = shadow[role]
        if (entry["status"] != "completed" or entry["artifact_sha256"] != observation[role + "_id"]
                or observation[role + "_error"] is not None
                or not _same(observation[role], entry["actual"], ("decision", "score", "reason"))):
            raise ValueError(f"{role} verdict mismatch")
    if observation["shadow_id"] != app["candidate_id"]:
        raise ValueError("review candidate mismatch")
    return {"schema": "sese-container-review-comparison-v1", "matches": True, "synthetic": True,
            "promotion_evidence": False, "checkpoint": preview["checkpoint"],
            "fixture_digest": app["fixture_digest"], "candidate_id": app["candidate_id"],
            "observation_id": observation_id}


def verify_report_receipts(replay, shadow, *, snapshots=None):
    entries = []
    for case in replay["cases"]:
        event, window = case["event"], case["window"]
        entries.append(({**case, "candidate": replay["candidate"]},
                        (event["timestamp_ns"], event["entity_id"], event["metric"], event["flags"]),
                        (window["count"], window["mean"], window["p99_us"], window["last_error"])))
    for role in ("champion", "shadow"):
        entries.append((shadow[role], shadow["event"], shadow["window"]))
    return verify_receipts(entries, replay["image"], replay["docker_host"], snapshots=snapshots)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("database")
    parser.add_argument("domain")
    parser.add_argument("replay_id", type=int)
    parser.add_argument("observation_id", type=int)
    parser.add_argument("replay_report")
    parser.add_argument("shadow_report")
    parser.add_argument("--outcome-cli", required=True)
    parser.add_argument("--record-actor", help="record caller-asserted report digests in the application ledger")
    args = parser.parse_args()
    output = subprocess.run([args.outcome_cli, "event-review-preview", args.database, args.domain, str(args.replay_id)],
                            check=True, capture_output=True, timeout=30)
    with open(args.replay_report, "rb") as file:
        replay_bytes = file.read()
    with open(args.shadow_report, "rb") as file:
        shadow_bytes = file.read()
    replay = json.loads(replay_bytes)
    shadow = json.loads(shadow_bytes)
    comparison = compare(json.loads(output.stdout), replay, shadow, args.observation_id)
    verify_report_receipts(replay, shadow)
    comparison["receipt_consistency_checked"] = True
    if args.record_actor is not None:
        checkpoint = comparison["checkpoint"]
        recorded = subprocess.run([args.outcome_cli, "event-container-assert", args.database,
                                   str(args.replay_id), str(args.observation_id), args.record_actor,
                                   hashlib.sha256(replay_bytes).hexdigest(), hashlib.sha256(shadow_bytes).hexdigest(),
                                   str(checkpoint["seq"]), checkpoint["hash"]], check=True, capture_output=True, timeout=30)
        comparison["assertion"] = json.loads(recorded.stdout)
    print(json.dumps(comparison, indent=2))
