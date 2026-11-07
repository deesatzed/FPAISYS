"""Experimental fixed-candidate shadow comparison."""
import argparse
import hashlib
import json
import time
import uuid
from pathlib import Path

from synthetic_container_lifecycle import SYNTHETIC_CANDIDATES, _synthetic_request, _write_receipt, run_synthetic


def compare(champion, shadow, *, event, window, image, docker_host, journal_dir):
    artifacts = {}
    for name in (champion, shadow):
        _, artifact = _synthetic_request(name, event, window)
        artifacts[name] = hashlib.sha256(artifact).hexdigest()
    directory = Path(journal_dir).resolve(strict=True)
    if not directory.is_dir():
        raise ValueError("journal directory must exist")
    path = directory / ("sese-shadow-" + uuid.uuid4().hex + ".json")
    def candidate(name):
        return {"candidate": name, "artifact_sha256": artifacts[name], "status": "pending",
                "actual": None, "error": None, "receipt_path": None, "started_unix_ns": None, "finished_unix_ns": None}
    report = {"schema": "sese-synthetic-container-shadow-experiment-v1", "synthetic": True,
              "promotion_evidence": False, "state": "pending", "image": image, "docker_host": docker_host,
              "event": list(event), "window": list(window), "champion": candidate(champion), "shadow": candidate(shadow),
              "started_unix_ns": str(time.time_ns()), "finished_unix_ns": None, "report_path": str(path)}
    _write_receipt(path, report, create=True)
    for role in ("champion", "shadow"):
        entry = report[role]
        entry.update(status="running", started_unix_ns=str(time.time_ns()))
        _write_receipt(path, report, create=False)
        def record_intent(receipt_path):
            entry["receipt_path"] = receipt_path
            _write_receipt(path, report, create=False)
        try:
            result = run_synthetic(image, journal_dir=journal_dir, docker_host=docker_host,
                                   candidate=entry["candidate"], event=event, window=window, intent_callback=record_intent)
        except (RuntimeError, ValueError, OSError) as error:
            entry.update(status="error", error=str(error), finished_unix_ns=str(time.time_ns()))
            report.update(state="failed", finished_unix_ns=str(time.time_ns()))
            _write_receipt(path, report, create=False)
            raise RuntimeError(f"synthetic shadow failed; report_path={path}: {error}") from error
        decision, score, reason = result["verdict"]
        entry.update(status="completed", actual={"decision": decision, "score": score, "reason": reason},
                     finished_unix_ns=str(time.time_ns()))
        _write_receipt(path, report, create=False)
    report.update(state="completed", finished_unix_ns=str(time.time_ns()))
    _write_receipt(path, report, create=False)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("champion", choices=SYNTHETIC_CANDIDATES)
    parser.add_argument("shadow", choices=SYNTHETIC_CANDIDATES)
    parser.add_argument("--event", required=True, nargs=4, metavar=("TIMESTAMP_NS", "ENTITY", "METRIC", "FLAGS"))
    parser.add_argument("--window", required=True, nargs=4, metavar=("COUNT", "MEAN", "P99_US", "LAST_ERROR"))
    parser.add_argument("--image", required=True)
    parser.add_argument("--docker-host", required=True)
    parser.add_argument("--journal-dir", required=True)
    args = parser.parse_args()
    event = (int(args.event[0]), int(args.event[1]), float(args.event[2]), int(args.event[3]))
    window = (int(args.window[0]), float(args.window[1]), int(args.window[2]), int(args.window[3]))
    print(json.dumps(compare(args.champion, args.shadow, event=event, window=window, image=args.image,
                             docker_host=args.docker_host, journal_dir=args.journal_dir), indent=2))
