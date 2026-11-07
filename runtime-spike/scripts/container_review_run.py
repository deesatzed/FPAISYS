"""Run a synthetic container comparison for an existing application review."""
import argparse
import hashlib
import json
import subprocess
import uuid
from pathlib import Path

from container_review_compare import compare, verify_report_receipts
from synthetic_container_lifecycle import SYNTHETIC_CANDIDATES, _synthetic_request, _write_receipt
from synthetic_container_replay import replay
from synthetic_container_shadow import compare as shadow_compare


def builtin_for_digest(digest):
    for name in SYNTHETIC_CANDIDATES:
        _, artifact = _synthetic_request(name, (1, 2, 3.0, 4), (0, 0.0, 0, 0))
        if hashlib.sha256(artifact).hexdigest() == digest:
            return name
    raise ValueError("unknown synthetic built-in artifact")


def run(database, domain, replay_id, observation_id, fixture, *, outcome_cli, inspector,
        image, docker_host, journal_dir, actor):
    if not isinstance(actor, str) or not actor.strip() or len(actor.encode()) > 256:
        raise ValueError("invalid assertion actor")
    preview_output = subprocess.run([str(outcome_cli), "event-review-preview", str(database), domain, str(replay_id)],
                                    check=True, capture_output=True, timeout=30)
    preview = json.loads(preview_output.stdout)
    observations = [item for item in preview["observations"] if item["id"] == observation_id]
    if len(observations) != 1:
        raise ValueError("observation not found in review preview")
    observation = observations[0]
    champion = builtin_for_digest(observation["champion_id"])
    candidate = builtin_for_digest(preview["replay"]["candidate_id"])
    if observation["shadow_id"] != preview["replay"]["candidate_id"]:
        raise ValueError("observation candidate mismatch")
    if observation["champion_error"] is not None or observation["shadow_error"] is not None:
        raise ValueError("observation contains execution errors")
    e, w = observation["event"], observation["prior"]
    event = (e["timestamp_ns"], e["entity_id"], e["metric"], e["flags"])
    window = (w["count"], w["mean"], w["p99_us"], w["last_error"])
    replay_result = replay(fixture, inspector=inspector, image=image, docker_host=docker_host,
                           journal_dir=journal_dir, candidate=candidate)
    shadow_result = shadow_compare(champion, candidate, event=event, window=window, image=image,
                                   docker_host=docker_host, journal_dir=journal_dir)
    replay_path, shadow_path = Path(replay_result["report_path"]), Path(shadow_result["report_path"])
    replay_bytes, shadow_bytes = replay_path.read_bytes(), shadow_path.read_bytes()
    replay_report, shadow_report = json.loads(replay_bytes), json.loads(shadow_bytes)
    comparison = compare(preview, replay_report, shadow_report, observation_id)
    receipts = verify_report_receipts(replay_report, shadow_report)
    checkpoint = comparison["checkpoint"]
    bundle_path = Path(journal_dir).resolve(strict=True) / ("sese-comparison-bundle-" + uuid.uuid4().hex + ".json")
    bundle = {"schema": "sese-synthetic-container-comparison-bundle-v1", "replay_report": replay_bytes.decode("utf-8"),
              "shadow_report": shadow_bytes.decode("utf-8"), "receipts": receipts}
    _write_receipt(bundle_path, bundle, create=True)
    result = subprocess.run([str(outcome_cli), "event-container-capture", str(database), str(replay_id),
                             str(observation_id), actor, str(bundle_path), str(checkpoint["seq"]), checkpoint["hash"]],
                            check=True, capture_output=True, timeout=30)
    comparison.update(receipt_consistency_checked=True, assertion=json.loads(result.stdout),
                      replay_report_path=str(replay_path), shadow_report_path=str(shadow_path), bundle_path=str(bundle_path))
    return comparison


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("database")
    parser.add_argument("domain")
    parser.add_argument("replay_id", type=int)
    parser.add_argument("observation_id", type=int)
    parser.add_argument("fixture")
    parser.add_argument("--outcome-cli", required=True)
    parser.add_argument("--inspector", required=True)
    parser.add_argument("--image", required=True)
    parser.add_argument("--docker-host", required=True)
    parser.add_argument("--journal-dir", required=True)
    parser.add_argument("--actor", required=True)
    args = parser.parse_args()
    print(json.dumps(run(args.database, args.domain, args.replay_id, args.observation_id, args.fixture,
                         outcome_cli=args.outcome_cli, inspector=args.inspector, image=args.image,
                         docker_host=args.docker_host, journal_dir=args.journal_dir, actor=args.actor), indent=2))
