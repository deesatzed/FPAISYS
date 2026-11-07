"""Experimental fixed-candidate container replay; not promotion evidence."""
import argparse
import json
import struct
import subprocess
import time
import uuid
from pathlib import Path

from synthetic_container_lifecycle import SYNTHETIC_CANDIDATES, _write_receipt, run_synthetic


def replay(fixture, *, inspector, image, docker_host, journal_dir, candidate="throttle"):
    try:
        result = subprocess.run([str(inspector), "--cases", str(fixture)], capture_output=True, timeout=15, check=True)
        pack = json.loads(result.stdout)
    except (OSError, subprocess.SubprocessError, ValueError) as error:
        raise ValueError(f"fixture inspection failed: {error}") from error
    if (pack.get("schema") != "sese-event-fixture-inspection-v1" or pack.get("executed") is not False
            or not isinstance(pack.get("cases"), list) or not pack["cases"]
            or pack.get("case_count") != len(pack["cases"])):
        raise ValueError("invalid fixture inspection result")
    directory = Path(journal_dir).resolve(strict=True)
    if not directory.is_dir():
        raise ValueError("journal directory must exist")
    report_path = directory / ("sese-replay-" + uuid.uuid4().hex + ".json")
    cases = [{**case, "status": "pending", "actual": None, "passed": None,
              "error": None, "receipt_path": None, "artifact_sha256": None,
              "started_unix_ns": None, "finished_unix_ns": None} for case in pack["cases"]]
    report = {"schema": "sese-synthetic-container-replay-experiment-v2", "synthetic": True,
              "promotion_evidence": False, "version": pack["version"], "state_semantics": pack["state_semantics"],
              "fixture_digest": pack["fixture_digest"], "candidate": candidate, "image": image,
              "docker_host": docker_host, "state": "pending", "passed": None,
              "started_unix_ns": str(time.time_ns()), "finished_unix_ns": None,
              "report_path": str(report_path), "cases": cases}
    _write_receipt(report_path, report, create=True)
    for case in cases:
        event, window, expected = case["event"], case["window"], case["expected"]
        case["status"] = "running"
        case["started_unix_ns"] = str(time.time_ns())
        _write_receipt(report_path, report, create=False)
        def record_intent(path):
            case["receipt_path"] = path
            _write_receipt(report_path, report, create=False)
        try:
            execution = run_synthetic(image, journal_dir=journal_dir, docker_host=docker_host, candidate=candidate,
                                      event=(event["timestamp_ns"], event["entity_id"], event["metric"], event["flags"]),
                                      window=(window["count"], window["mean"], window["p99_us"], window["last_error"]),
                                      intent_callback=record_intent)
        except (RuntimeError, ValueError, OSError) as error:
            case.update(status="error", passed=False, error=str(error), finished_unix_ns=str(time.time_ns()))
            report.update(state="failed", passed=False, finished_unix_ns=str(time.time_ns()))
            _write_receipt(report_path, report, create=False)
            raise RuntimeError(f"synthetic replay failed; report_path={report_path}: {error}") from error
        decision, score, reason = execution["verdict"]
        actual = {"decision": decision, "score": score, "reason": reason}
        passed = (decision == expected["decision"] and reason == expected["reason"]
                  and struct.pack("<d", score) == struct.pack("<d", expected["score"]))
        case.update(status="completed", actual=actual, passed=passed, receipt_path=execution["receipt_path"],
                    artifact_sha256=execution["artifact_sha256"], finished_unix_ns=str(time.time_ns()))
        _write_receipt(report_path, report, create=False)
    report.update(state="completed", passed=all(case["passed"] for case in cases), finished_unix_ns=str(time.time_ns()))
    _write_receipt(report_path, report, create=False)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("fixture")
    parser.add_argument("--inspector", required=True)
    parser.add_argument("--image", required=True)
    parser.add_argument("--docker-host", required=True)
    parser.add_argument("--journal-dir", required=True)
    parser.add_argument("--candidate", choices=SYNTHETIC_CANDIDATES, default="throttle")
    args = parser.parse_args()
    report = replay(args.fixture, inspector=args.inspector, image=args.image, docker_host=args.docker_host,
                    journal_dir=args.journal_dir, candidate=args.candidate)
    print(json.dumps(report, indent=2))
    raise SystemExit(0 if report["passed"] else 1)
