"""Verify a synthetic container capture against an externally retained checkpoint."""
import argparse
import hashlib
import json
import re
import sqlite3
from pathlib import Path

from container_review_compare import compare, verify_report_receipts
from verify_ledger import digest, verify_event_fixture, verify_event_observations, verify_event_replays


def _load(value):
    def unique(pairs):
        result = {}
        for key, item in pairs:
            if key in result:
                raise ValueError("duplicate JSON field")
            result[key] = item
        return result
    return json.loads(value, object_pairs_hook=unique)


def verify_capture(database, capture_seq, checkpoint_seq, checkpoint_hash):
    if (type(capture_seq) is not int or type(checkpoint_seq) is not int or capture_seq <= 0
            or checkpoint_seq < capture_seq or not isinstance(checkpoint_hash, str)
            or not re.fullmatch(r"[0-9a-f]{64}", checkpoint_hash)):
        raise ValueError("checkpoint must cover capture")
    conn = sqlite3.connect(Path(database).absolute().as_uri() + "?mode=ro", uri=True)
    try:
        conn.execute("BEGIN")
        previous, sequence, found, capture = "GENESIS", 0, False, None
        for seq, kind, reference, payload, timestamp, link, recorded in conn.execute(
                "SELECT seq,kind,reference,payload,created_ns,prev,hash FROM ledger_events ORDER BY seq"):
            if (type(seq) is not int or type(reference) is not int
                    or not all(isinstance(x, str) for x in (kind, payload, timestamp, link, recorded))
                    or seq != sequence + 1 or link != previous or recorded != digest(previous, kind, reference, payload, timestamp)):
                raise ValueError("ledger chain mismatch")
            if seq == capture_seq:
                if kind != "container_comparison_assertion":
                    raise ValueError("not a container capture")
                capture = (reference, _load(payload), link)
            if seq == checkpoint_seq:
                found = recorded == checkpoint_hash
            sequence, previous = seq, recorded
        if not found or capture is None:
            raise ValueError("checkpoint missing or capture not found")
        reference, value, previous_hash = capture
        expected_keys = {"schema", "replay_id", "observation_id", "candidate_id", "actor", "replay_report_sha256",
                         "shadow_report_sha256", "preview_checkpoint", "evidence_origin", "synthetic", "promotion_evidence", "bundle"}
        if (not isinstance(value, dict) or set(value) != expected_keys
                or value["schema"] != "sese-container-comparison-assertion-v1"
                or value["evidence_origin"] != "caller_asserted" or value["synthetic"] is not True
                or value["promotion_evidence"] is not False or value["replay_id"] != reference
                or type(value["observation_id"]) is not int or not isinstance(value["actor"], str) or not value["actor"].strip()
                or value["preview_checkpoint"] != {"seq": capture_seq - 1, "hash": previous_hash}):
            raise ValueError("invalid capture assertion")
        for kind, identifier in (("event_replay", reference), ("event_observation", value["observation_id"])):
            parents = conn.execute("SELECT seq FROM ledger_events WHERE kind=? AND reference=?", (kind, identifier)).fetchall()
            if len(parents) != 1 or parents[0][0] >= capture_seq:
                raise ValueError("capture parent missing or too late")
        bundle = value["bundle"]
        if (not isinstance(bundle, dict) or set(bundle) != {"schema", "replay_report", "shadow_report", "receipts"}
                or bundle["schema"] != "sese-synthetic-container-comparison-bundle-v1"):
            raise ValueError("invalid capture bundle")
        for key in ("replay_report", "shadow_report"):
            if not isinstance(bundle[key], str) or hashlib.sha256(bundle[key].encode()).hexdigest() != value[key + "_sha256"]:
                raise ValueError("captured report digest mismatch")
        replay, shadow = _load(bundle["replay_report"]), _load(bundle["shadow_report"])
        verify_event_replays(conn)
        verify_event_fixture(conn, reference)
        verify_event_observations(conn)
        row = conn.execute("SELECT candidate_id,pack_version,pack_digest,state_semantics,passed FROM event_replay_runs WHERE id=?", (reference,)).fetchone()
        if row is None or row[0] != value["candidate_id"]:
            raise ValueError("capture replay candidate mismatch")
        cases = []
        for case_id, window, event, expected, actual, error, passed in conn.execute(
                "SELECT case_id,window_json,event_json,expected_json,actual_json,error,passed FROM event_replay_cases WHERE run_id=? ORDER BY ordinal", (reference,)):
            cases.append({"id": case_id, "window": _load(window), "event": _load(event), "expected": _load(expected),
                          "actual": _load(actual) if actual is not None else None, "error": error, "passed": bool(passed)})
        obs = conn.execute("SELECT champion_id,shadow_id,prior_json,event_json,champion_json,champion_error,shadow_json,shadow_error FROM event_observations WHERE id=?", (value["observation_id"],)).fetchone()
        if obs is None:
            raise ValueError("capture observation missing")
        observation = {"id": value["observation_id"], "champion_id": obs[0], "shadow_id": obs[1],
                       "prior": _load(obs[2]), "event": _load(obs[3]), "champion": _load(obs[4]) if obs[4] else None,
                       "champion_error": obs[5], "shadow": _load(obs[6]) if obs[6] else None, "shadow_error": obs[7]}
        preview = {"schema": "sese-synthetic-review-preview-v1", "checkpoint": value["preview_checkpoint"],
                   "replay": {"candidate_id": row[0], "fixture_version": row[1], "fixture_digest": row[2],
                              "state_semantics": row[3], "passed": bool(row[4]), "cases": cases}, "observations": [observation]}
        compare(preview, replay, shadow, value["observation_id"])
        verify_report_receipts(replay, shadow, snapshots=bundle["receipts"])
        return {"schema": "sese-container-capture-verification-v1", "capture_seq": capture_seq,
                "checkpoint_seq": checkpoint_seq, "consistent": True, "synthetic": True,
                "evidence_origin": "caller_asserted", "promotion_evidence": False,
                "execution_authenticated": False, "receipt_permissions_rechecked": False}
    finally:
        conn.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("database")
    parser.add_argument("capture_seq", type=int)
    parser.add_argument("--seq", type=int, required=True)
    parser.add_argument("--hash", required=True)
    args = parser.parse_args()
    print(json.dumps(verify_capture(args.database, args.capture_seq, args.seq, args.hash), sort_keys=True))
