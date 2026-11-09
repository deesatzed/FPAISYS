#!/usr/bin/env python3
"""Independently check the S-SESE hash chain against a retained checkpoint.

Optional checks cover synthetic candidate, typed replay, and observation bindings.
Optional signature checks require a pinned authority file and cryptography.
Checkpoint custody and authority trust are not verified.
"""
import argparse
import hashlib
import json
import math
import os
import stat
import struct
from pathlib import Path
import re
import sqlite3
import sys


def digest(previous, kind, reference, payload, timestamp):
    fields = ("sese-ledger-v1", previous, kind, str(reference), payload, timestamp)
    encoded = b"".join(str(len(value.encode("utf-8"))).encode("ascii") + b":" + value.encode("utf-8") for value in fields)
    return hashlib.sha256(encoded).hexdigest()


def encoded_fields(fields):
    return "".join(f"{len(value.encode('utf-8'))}:{value}" for value in fields)


def verify_synthetic_candidates(conn):
    total = 0
    for kind, abi in (("scalar", 1), ("window", 2), ("event", 3)):
        table = f"synthetic_{kind}_candidates"
        event_kind = f"synthetic_{kind}_candidate"
        orphans = conn.execute(
            f"SELECT count(*) FROM ledger_events e LEFT JOIN {table} s ON s.id=e.reference "
            "WHERE e.kind=? AND s.id IS NULL", (event_kind,)
        ).fetchone()[0]
        if orphans:
            raise ValueError("orphan synthetic candidate event")
        rows = conn.execute(
            f"SELECT s.id,s.candidate_id,s.created_unix_ns,c.artifact,c.runtime,c.abi_version,"
            f"e.payload,e.created_ns FROM {table} s "
            "LEFT JOIN candidates c ON c.id=s.candidate_id "
            "LEFT JOIN ledger_events e ON e.kind=? AND e.reference=s.id ORDER BY s.id", (event_kind,)
        )
        seen = set()
        for row_id, candidate, created, artifact, runtime, actual_abi, payload, event_time in rows:
            if row_id in seen:
                raise ValueError("duplicate synthetic candidate event")
            seen.add(row_id)
            if (not isinstance(candidate, str) or not isinstance(created, str)
                    or not isinstance(artifact, bytes) or runtime != "wasmi-experimental"
                    or type(actual_abi) is not int or actual_abi != abi
                    or hashlib.sha256(artifact).hexdigest() != candidate
                    or payload != encoded_fields((f"caller-asserted-synthetic-{kind}-v1", candidate))
                    or event_time != created):
                raise ValueError("synthetic candidate binding mismatch")
            total += 1
    return total


def update_sql_value(hasher, value):
    if value is None:
        hasher.update(b"\x00")
    elif type(value) is int:
        hasher.update(b"\x01" + value.to_bytes(8, "big", signed=True))
    elif isinstance(value, str):
        data = value.encode("utf-8")
        hasher.update(b"\x02" + len(data).to_bytes(8, "big") + data)
    elif isinstance(value, bytes):
        hasher.update(b"\x03" + len(value).to_bytes(8, "big") + value)
    else:
        raise ValueError("unsupported replay evidence type")


def verify_event_replays(conn):
    orphan = conn.execute(
        "SELECT count(*) FROM ledger_events e LEFT JOIN event_replay_runs r ON r.id=e.reference "
        "WHERE e.kind='event_replay' AND r.id IS NULL"
    ).fetchone()[0]
    if orphan:
        raise ValueError("orphan replay event")
    total = 0
    for run_id, semantics in conn.execute("SELECT id,state_semantics FROM event_replay_runs ORDER BY id"):
        if semantics not in ("legacy-v1", "measured-v2"):
            raise ValueError("unknown replay semantics")
        hasher = hashlib.sha256(b"sese-event-replay-evidence-v1\0")
        if semantics == "measured-v2":
            hasher.update(b"measured-v2\0")
        sections = (
            ("run", "SELECT r.id,r.candidate_id,r.pack_version,r.pack_digest,r.passed,r.started_unix_ns,r.environment,c.artifact,c.runtime,c.abi_version FROM event_replay_runs r JOIN candidates c ON c.id=r.candidate_id WHERE r.id=?"),
            ("cases", "SELECT ordinal,case_id,window_json,event_json,expected_json,actual_json,error,passed FROM event_replay_cases WHERE run_id=? ORDER BY ordinal"),
        )
        started = None
        for tag, query in sections:
            tag_bytes = tag.encode("ascii")
            hasher.update(len(tag_bytes).to_bytes(8, "big") + tag_bytes)
            count = 0
            for row in conn.execute(query, (run_id,)):
                count += 1
                if tag == "run":
                    candidate, started, artifact, runtime, abi = row[1], row[5], row[7], row[8], row[9]
                    if (not isinstance(candidate, str) or not isinstance(artifact, bytes)
                            or hashlib.sha256(artifact).hexdigest() != candidate
                            or runtime != "wasmi-experimental" or abi != 3):
                        raise ValueError("invalid replay candidate")
                hasher.update(b"row")
                for value in row:
                    update_sql_value(hasher, value)
            if count == 0 or (tag == "run" and count != 1) or (tag == "cases" and count > 1000):
                raise ValueError("invalid replay evidence count")
            hasher.update(count.to_bytes(8, "big"))
        events = conn.execute("SELECT payload,created_ns FROM ledger_events WHERE kind='event_replay' AND reference=?", (run_id,)).fetchall()
        if len(events) != 1 or events[0] != (encoded_fields(("synthetic-event-replay-v1", hasher.hexdigest())), started):
            raise ValueError("replay evidence binding mismatch")
        total += 1
    return total


def _object(encoded, keys):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError("duplicate JSON field")
            result[key] = value
        return result
    def invalid_constant(_):
        raise ValueError("nonfinite JSON value")
    value = json.loads(encoded, object_pairs_hook=pairs, parse_constant=invalid_constant)
    if not isinstance(value, dict) or set(value) != set(keys):
        raise ValueError("invalid fixture fields")
    return value


def verify_event_replay_timings(conn):
    seen = set()
    total = 0
    for seq, replay_id, payload, created in conn.execute(
            "SELECT seq,reference,payload,created_ns FROM ledger_events WHERE kind='event_replay_timing' ORDER BY seq"):
        if replay_id in seen:
            raise ValueError("duplicate replay timings")
        seen.add(replay_id)
        if not isinstance(payload, str) or len(payload.encode("utf-8")) > 4_194_304:
            raise ValueError("replay timings too large")
        timing = _object(payload, ("schema", "replay_hash", "cases"))
        parent = conn.execute("SELECT e.seq,e.hash,r.started_unix_ns FROM ledger_events e JOIN event_replay_runs r ON r.id=e.reference WHERE e.kind='event_replay' AND e.reference=?", (replay_id,)).fetchall()
        cases = list(conn.execute("SELECT case_id,actual_json,error FROM event_replay_cases WHERE run_id=? ORDER BY ordinal", (replay_id,)))
        if (len(parent) != 1 or seq <= parent[0][0] or created != parent[0][2]
                or timing["schema"] not in ("sese-event-replay-timing-v1", "sese-event-replay-timing-v2") or timing["replay_hash"] != parent[0][1]
                or not isinstance(timing["cases"], list) or not 0 < len(cases) <= 1000 or len(cases) != len(timing["cases"])):
            raise ValueError("replay timing mismatch")
        failed = False
        for case, (case_id, actual, error) in zip(timing["cases"], cases):
            if not isinstance(case, dict) or set(case) != {"case_id", "started_unix_ns", "finished_unix_ns", "elapsed_ns"} or case["case_id"] != case_id:
                raise ValueError("replay timing case mismatch")
            values = [case[key] for key in ("started_unix_ns", "finished_unix_ns", "elapsed_ns")]
            if all(value is None for value in values):
                if timing["schema"] != "sese-event-replay-timing-v2" or not failed or actual is not None or error != "not run after earlier worker failure":
                    raise ValueError("invalid skipped replay timing")
            else:
                for value in values:
                    if (not isinstance(value, str) or not re.fullmatch(r"0|[1-9][0-9]{0,38}", value)
                            or int(value) > 2**128 - 1):
                        raise ValueError("invalid replay timing")
                if timing["schema"] == "sese-event-replay-timing-v2":
                    if failed or error == "not run after earlier worker failure":
                        raise ValueError("invalid replay timing order")
                    if error is not None and actual is None:
                        failed = True
        total += 1
    return total


def _uint(value, bits):
    if type(value) is not int or not 0 <= value < (1 << bits):
        raise ValueError("invalid fixture integer")
    return value.to_bytes(bits // 8, "big")


def _float_bits(obj, name):
    encoded = obj[name + "_bits"]
    if not isinstance(encoded, str) or not re.fullmatch(r"[0-9a-f]{16}", encoded):
        raise ValueError("invalid fixture float bits")
    raw = bytes.fromhex(encoded)
    value = struct.unpack(">d", raw)[0]
    shown = obj[name]
    if type(shown) not in (float, int) or not math.isfinite(value) or struct.pack(">d", float(shown)) != raw:
        raise ValueError("fixture float mismatch")
    return raw


def _verdict(encoded):
    value = _object(encoded, ("decision", "score", "score_bits", "reason"))
    decision = _uint(value["decision"], 32)
    if value["decision"] > 3:
        raise ValueError("invalid decision")
    return decision + _float_bits(value, "score") + _uint(value["reason"], 32)


def verify_event_fixture(conn, run_id):
    row = conn.execute("SELECT pack_version,pack_digest,passed,environment,state_semantics FROM event_replay_runs WHERE id=?", (run_id,)).fetchone()
    if row is None:
        raise ValueError("missing fixture run")
    version, digest_value, passed, environment, semantics = row
    if not isinstance(version, str) or not version.strip() or type(passed) is not int or passed not in (0, 1) or environment != "synthetic":
        raise ValueError("invalid fixture run")
    domains = {"legacy-v1": b"sese-event-pack-v1\0", "measured-v2": b"sese-event-pack-v2-measured\0"}
    if semantics not in domains:
        raise ValueError("unknown fixture semantics")
    hasher = hashlib.sha256(domains[semantics])
    seen = set()
    count = 0
    all_passed = True
    rows = conn.execute("SELECT ordinal,case_id,window_json,event_json,expected_json,actual_json,error,passed FROM event_replay_cases WHERE run_id=? ORDER BY ordinal", (run_id,))
    for ordinal, case_id, window_json, event_json, expected_json, actual_json, error, case_passed in rows:
        if ordinal != count or not isinstance(case_id, str) or not case_id.strip() or len(case_id.encode("utf-8")) > 256 or case_id in seen or type(case_passed) is not int or case_passed not in (0, 1):
            raise ValueError("invalid fixture case")
        count += 1
        if count > 1000:
            raise ValueError("too many fixture cases")
        seen.add(case_id)
        window = _object(window_json, ("count", "mean", "mean_bits", "p99_us", "last_error"))
        event = _object(event_json, ("timestamp_ns", "entity_id", "metric", "metric_bits", "flags"))
        expected = _verdict(expected_json)
        actual = _verdict(actual_json) if actual_json is not None else None
        if (actual is None) == (error is None) or (error is not None and (not isinstance(error, str) or not error)):
            raise ValueError("invalid case outcome")
        computed = actual == expected
        if computed != bool(case_passed):
            raise ValueError("case result mismatch")
        all_passed = all_passed and computed
        name = case_id.encode("utf-8")
        hasher.update(len(name).to_bytes(8, "big") + name)
        hasher.update(_uint(window["count"], 64) + _float_bits(window, "mean") + _uint(window["p99_us"], 64) + _uint(window["last_error"], 32))
        hasher.update(_uint(event["timestamp_ns"], 64) + _uint(event["entity_id"], 64) + _float_bits(event, "metric") + _uint(event["flags"], 32) + expected)
    if count == 0 or hasher.hexdigest() != digest_value or all_passed != bool(passed):
        raise ValueError("fixture digest or run result mismatch")
    return count


def verify_event_fixtures(conn):
    return sum(verify_event_fixture(conn, row[0]) for row in conn.execute("SELECT id FROM event_replay_runs ORDER BY id"))


def verify_event_workers(conn):
    checked = 0
    unrecorded = 0
    for table, kind, parent, timestamp in (
        ("event_replay_runs", "event_replay_worker", "event_replay", "started_unix_ns"),
        ("event_observations", "event_observation_worker", "event_observation", "observed_unix_ns"),
    ):
        orphan = conn.execute(
            f"SELECT count(*) FROM ledger_events e LEFT JOIN {table} r ON r.id=e.reference "
            "WHERE e.kind=? AND r.id IS NULL", (kind,)
        ).fetchone()[0]
        if orphan:
            raise ValueError("orphan worker provenance event")
        rows = conn.execute(
            f"SELECT r.id,r.worker_path,r.worker_sha256,r.{timestamp},e.payload,e.created_ns "
            f"FROM {table} r LEFT JOIN ledger_events e ON e.kind=? AND e.reference=r.id ORDER BY r.id", (kind,)
        )
        seen = set()
        for run_id, path, sha, created, payload, event_time in rows:
            if run_id in seen:
                raise ValueError("duplicate worker provenance event")
            seen.add(run_id)
            if path is None and sha is None and payload is None and event_time is None:
                unrecorded += 1
                continue
            if not isinstance(path, str) or not path or not isinstance(sha, str) or not re.fullmatch(r"[0-9a-f]{64}", sha) or not isinstance(created, str):
                raise ValueError("invalid worker provenance")
            parents = conn.execute("SELECT hash FROM ledger_events WHERE kind=? AND reference=?", (parent, run_id)).fetchall()
            if len(parents) != 1:
                raise ValueError("missing or duplicate worker parent event")
            expected = encoded_fields(("synthetic-worker-provenance-v1", kind, path, sha, parents[0][0]))
            if payload != expected or event_time != created:
                raise ValueError("worker provenance binding mismatch")
            checked += 1
    return checked, unrecorded


def verify_event_observations(conn):
    orphan = conn.execute("SELECT count(*) FROM ledger_events e LEFT JOIN event_observations o ON o.id=e.reference WHERE e.kind='event_observation' AND o.id IS NULL").fetchone()[0]
    if orphan:
        raise ValueError("orphan observation event")
    total = 0
    for observation_id, domain in conn.execute("SELECT id,domain FROM event_observations ORDER BY id"):
        rows = conn.execute("SELECT o.id,o.champion_id,o.shadow_id,o.prior_json,o.event_json,o.champion_json,o.champion_error,o.shadow_json,o.shadow_error,o.champion_us,o.shadow_us,o.observed_unix_ns,c.artifact,c.runtime,c.abi_version,s.artifact,s.runtime,s.abi_version FROM event_observations o JOIN candidates c ON c.id=o.champion_id JOIN candidates s ON s.id=o.shadow_id WHERE o.id=?", (observation_id,)).fetchall()
        if len(rows) != 1:
            raise ValueError("missing observation candidate")
        row = rows[0]
        for id_col, bytes_col, runtime_col, abi_col in ((1, 12, 13, 14), (2, 15, 16, 17)):
            if (not isinstance(row[id_col], str) or not isinstance(row[bytes_col], bytes)
                    or hashlib.sha256(row[bytes_col]).hexdigest() != row[id_col]
                    or row[runtime_col] != "wasmi-experimental" or row[abi_col] != 3):
                raise ValueError("invalid observation candidate")
        hasher = hashlib.sha256(b"sese-event-observation-evidence-v1\0")
        if domain is not None:
            if not isinstance(domain, str):
                raise ValueError("invalid observation domain")
            data = domain.encode("utf-8")
            hasher.update(b"domain-v1\0" + len(data).to_bytes(8, "big") + data)
        for value in row:
            update_sql_value(hasher, value)
        events = conn.execute("SELECT payload,created_ns FROM ledger_events WHERE kind='event_observation' AND reference=?", (observation_id,)).fetchall()
        if len(events) != 1 or events[0] != (encoded_fields(("synthetic-event-observation-v1", hasher.hexdigest())), row[11]):
            raise ValueError("observation evidence binding mismatch")
        total += 1
    return total


def verify_event_reviews(conn):
    for table, key, kind in (("event_reviews", "id", "event_review"), ("event_review_outcomes", "review_id", "event_outcome")):
        orphan = conn.execute(f"SELECT count(*) FROM ledger_events e LEFT JOIN {table} r ON r.{key}=e.reference WHERE e.kind=? AND r.{key} IS NULL", (kind,)).fetchone()[0]
        if orphan:
            raise ValueError("orphan review or outcome event")
    reviews = 0
    for row in conn.execute("SELECT id,candidate_id,replay_id,observation_id,actor,action,human,bridge,proposal,created_unix_ns FROM event_reviews ORDER BY id"):
        review_id, candidate, replay, observation, actor, action, human, bridge, proposal, created = row
        parents = []
        for kind, reference in (("event_replay", replay), ("event_observation", observation)):
            links = conn.execute("SELECT hash FROM ledger_events WHERE kind=? AND reference=?", (kind, reference)).fetchall()
            if len(links) != 1:
                raise ValueError("missing or duplicate review evidence event")
            parents.append(links[0][0])
        fields = ("synthetic-event-review-v1", candidate, str(replay), str(observation), actor, action, human, bridge, proposal, *parents)
        if not all(isinstance(value, str) for value in fields) or not isinstance(created, str):
            raise ValueError("invalid review fields")
        events = conn.execute("SELECT payload,created_ns FROM ledger_events WHERE kind='event_review' AND reference=?", (review_id,)).fetchall()
        if len(events) != 1 or events[0] != (encoded_fields(fields), created):
            raise ValueError("review evidence binding mismatch")
        reviews += 1
    outcomes = 0
    for review_id, status, actor, source, created in conn.execute("SELECT review_id,status,actor,source,created_unix_ns FROM event_review_outcomes ORDER BY review_id"):
        links = conn.execute("SELECT hash FROM ledger_events WHERE kind='event_review' AND reference=?", (review_id,)).fetchall()
        if len(links) != 1:
            raise ValueError("missing or duplicate outcome review event")
        fields = ("synthetic-event-outcome-v1", status, actor, source, links[0][0])
        if status not in ("validated", "falsified") or not all(isinstance(value, str) for value in fields) or not isinstance(created, str):
            raise ValueError("invalid outcome fields")
        events = conn.execute("SELECT payload,created_ns FROM ledger_events WHERE kind='event_outcome' AND reference=?", (review_id,)).fetchall()
        if len(events) != 1 or events[0] != (encoded_fields(fields), created):
            raise ValueError("outcome evidence binding mismatch")
        outcomes += 1
    return reviews, outcomes


def verify_event_champions(conn):
    orphan = conn.execute("SELECT count(*) FROM ledger_events e LEFT JOIN event_champion_transitions t ON t.id=e.reference WHERE e.kind='event_champion_transition' AND t.id IS NULL").fetchone()[0]
    if orphan:
        raise ValueError("orphan champion transition event")
    states = {}
    transitions = 0
    for row in conn.execute("SELECT id,domain,previous_id,next_id,reference,actor,created_unix_ns,kind FROM event_champion_transitions ORDER BY id"):
        transition_id, domain, previous, next_id, reference, actor, created, kind = row
        if (not all(isinstance(v, str) for v in (domain, previous, next_id, actor, created, kind))
                or not domain or not actor.strip() or type(reference) is not int):
            raise ValueError("invalid champion transition fields")
        fields = ("synthetic-event-champion-v1", domain, previous, next_id, str(reference), actor, kind)
        events = conn.execute("SELECT payload,created_ns FROM ledger_events WHERE kind='event_champion_transition' AND reference=?", (transition_id,)).fetchall()
        if len(events) != 1 or events[0] != (encoded_fields(fields), created):
            raise ValueError("champion transition binding mismatch")
        state = states.get(domain)
        if state is None and kind == "bootstrap" and previous == "" and reference == 0:
            generation = 0
        elif state is not None and kind == "promote" and previous == state[0] and next_id != previous and reference > 0:
            review = conn.execute("SELECT candidate_id,action FROM event_reviews WHERE id=?", (reference,)).fetchone()
            if review != (next_id, "approve"):
                raise ValueError("promotion review mismatch")
            generation = state[1] + 1
        elif (state is not None and kind == "rollback" and previous == state[0]
              and next_id == state[3] and reference == state[2] and state[4] == "promote"):
            generation = state[1] + 1
        else:
            raise ValueError("invalid champion transition sequence")
        states[domain] = (next_id, generation, transition_id, previous, kind)
        transitions += 1
    champions = 0
    for domain, candidate, generation, artifact, runtime, abi in conn.execute("SELECT d.domain,d.champion_id,d.generation,c.artifact,c.runtime,c.abi_version FROM event_champions d LEFT JOIN candidates c ON c.id=d.champion_id"):
        if (domain not in states or (candidate, generation) != states[domain][:2]
                or not isinstance(artifact, bytes) or hashlib.sha256(artifact).hexdigest() != candidate
                or runtime != "wasmi-experimental" or abi != 3):
            raise ValueError("champion state mismatch")
        champions += 1
    if champions != len(states):
        raise ValueError("missing champion state")
    return transitions, champions


def verify_event_traces(conn):
    orphan = conn.execute("SELECT count(*) FROM ledger_events e LEFT JOIN event_training_traces t ON t.id=e.reference WHERE e.kind='event_training_trace' AND t.id IS NULL").fetchone()[0]
    if orphan:
        raise ValueError("orphan training trace event")
    total = 0
    for trace_id, review, actor, source, prompt, response, preferred, rejected, pair_source, created in conn.execute("SELECT id,review_id,actor,source,prompt,response,preferred,rejected,pair_source,created_unix_ns FROM event_training_traces ORDER BY id"):
        def valid(value):
            return isinstance(value, str) and bool(value.strip()) and len(value.encode("utf-8")) <= 16384
        if type(review) is not int or not isinstance(created, str) or not all(valid(v) for v in (actor, source, prompt, response)):
            raise ValueError("invalid training trace fields")
        if preferred is None and rejected is None and pair_source is None:
            pair = ("no-pair", "", "", "")
        elif all(valid(v) for v in (preferred, rejected, pair_source)) and preferred != rejected:
            pair = ("explicit-pair", preferred, rejected, pair_source)
        else:
            raise ValueError("invalid training preference pair")
        links = conn.execute("SELECT hash FROM ledger_events WHERE kind='event_review' AND reference=?", (review,)).fetchall()
        if len(links) != 1:
            raise ValueError("missing or duplicate trace review event")
        fields = ("caller-asserted-synthetic-training-trace-v1", str(review), actor, source, prompt, response, *pair, links[0][0])
        events = conn.execute("SELECT payload,created_ns FROM ledger_events WHERE kind='event_training_trace' AND reference=?", (trace_id,)).fetchall()
        if len(events) != 1 or events[0] != (encoded_fields(fields), created):
            raise ValueError("training trace binding mismatch")
        total += 1
    return total


def _event_link(conn, kind, reference):
    rows = conn.execute("SELECT seq,hash,payload,created_ns FROM ledger_events WHERE kind=? AND reference=?", (kind, reference)).fetchall()
    if len(rows) != 1:
        raise ValueError("missing or duplicate evidence event")
    return rows[0]


def _shown_float(value):
    if type(value) not in (int, float) or not math.isfinite(value):
        raise ValueError("invalid observed float")
    return struct.pack(">d", float(value))


def _followup_result(window_json, event_json, expected_json, prior_json, observed_json, actual_json, error, semantics):
    window = _object(window_json, ("count", "mean", "mean_bits", "p99_us", "last_error"))
    event = _object(event_json, ("timestamp_ns", "entity_id", "metric", "metric_bits", "flags"))
    prior_keys = ("count", "mean", "p99_us", "last_error")
    if semantics == "measured-v2":
        prior_keys += ("state_semantics",)
    prior = _object(prior_json, prior_keys)
    if semantics == "measured-v2" and prior["state_semantics"] != semantics:
        raise ValueError("follow-up state semantics mismatch")
    observed = _object(observed_json, ("timestamp_ns", "entity_id", "metric", "flags"))
    same_input = (
        _uint(window["count"], 64) == _uint(prior["count"], 64)
        and _float_bits(window, "mean") == _shown_float(prior["mean"])
        and _uint(window["p99_us"], 64) == _uint(prior["p99_us"], 64)
        and _uint(window["last_error"], 32) == _uint(prior["last_error"], 32)
        and _uint(event["timestamp_ns"], 64) == _uint(observed["timestamp_ns"], 64)
        and _uint(event["entity_id"], 64) == _uint(observed["entity_id"], 64)
        and _float_bits(event, "metric") == _shown_float(observed["metric"])
        and _uint(event["flags"], 32) == _uint(observed["flags"], 32)
    )
    if not same_input:
        return "input_mismatch"
    if error is not None:
        if not isinstance(error, str) or not error or actual_json is not None:
            raise ValueError("invalid follow-up worker error")
        return "worker_error"
    if actual_json is None:
        raise ValueError("missing follow-up verdict")
    actual = _object(actual_json, ("decision", "score", "reason"))
    decisions = {"allow": 0, "throttle": 1, "quarantine": 2, "escalate": 3}
    if actual["decision"] not in decisions:
        raise ValueError("invalid observed decision")
    verdict = _uint(decisions[actual["decision"]], 32) + _shown_float(actual["score"]) + _uint(actual["reason"], 32)
    return "match" if verdict == _verdict(expected_json) else "verdict_mismatch"


def verify_event_fixture_pins(conn):
    orphan = conn.execute("SELECT count(*) FROM ledger_events e LEFT JOIN event_fixture_pins p ON p.id=e.reference WHERE e.kind='event_fixture_pin' AND p.id IS NULL").fetchone()[0]
    if orphan:
        raise ValueError("orphan event fixture selection")
    total = 0
    for pin, domain, replay, version, digest_value, actor, created in conn.execute("SELECT id,domain,replay_id,pack_version,pack_digest,actor,created_unix_ns FROM event_fixture_pins ORDER BY id"):
        if type(replay) is not int or not all(isinstance(v, str) for v in (domain, version, digest_value, actor, created)):
            raise ValueError("invalid event fixture selection")
        run = conn.execute("SELECT pack_version,pack_digest FROM event_replay_runs WHERE id=?", (replay,)).fetchone()
        if run != (version, digest_value):
            raise ValueError("event fixture selection mismatch")
        run_link = _event_link(conn, "event_replay", replay)
        link = _event_link(conn, "event_fixture_pin", pin)
        fields = ("synthetic-event-fixture-pin-v1", domain, str(replay), version, digest_value, actor, run_link[1])
        if link[2:] != (encoded_fields(fields), created):
            raise ValueError("event fixture selection binding mismatch")
        total += 1
    return total


def verify_followup_promotion_gates(conn):
    """Check selected follow-ups that existed when each promotion occurred.

    This audits present-day gates against retained history; it does not prove
    that a reviewer inspected the selected fixture or establish its quality.
    """
    checked = 0
    for transition, domain, candidate, review in conn.execute(
        "SELECT id,domain,next_id,reference FROM event_champion_transitions WHERE kind='promote' ORDER BY id"
    ):
        transition_seq = _event_link(conn, "event_champion_transition", transition)[0]
        review_seq = _event_link(conn, "event_review", review)[0]
        if review_seq >= transition_seq:
            raise ValueError("approval must precede promotion")
        selections = conn.execute(
            "SELECT id,replay_id FROM event_followup_pins WHERE domain=? AND candidate_id=?",
            (domain, candidate),
        ).fetchall()
        if len(selections) > 1:
            raise ValueError("duplicate follow-up selections")
        if not selections:
            continue
        pin, replay = selections[0]
        selection_seq = _event_link(conn, "event_followup_pin", pin)[0]
        if selection_seq >= transition_seq:
            continue
        if selection_seq >= review_seq:
            raise ValueError("follow-up selection must precede approval")
        result = conn.execute("SELECT passed FROM event_replay_runs WHERE id=?", (replay,)).fetchone()
        if result != (1,):
            raise ValueError("follow-up replay failed")
        checked += 1
    return checked


def verify_event_followups(conn):
    for table, kind in (("event_followup_pins", "event_followup_pin"), ("event_followup_receipts", "event_followup_receipt")):
        orphan = conn.execute(f"SELECT count(*) FROM ledger_events e LEFT JOIN {table} t ON t.id=e.reference WHERE e.kind=? AND t.id IS NULL", (kind,)).fetchone()[0]
        if orphan:
            raise ValueError("orphan follow-up event")
    pins = 0
    for pin, domain, candidate, replay, case_id, actor, created in conn.execute("SELECT id,domain,candidate_id,replay_id,case_id,actor,created_unix_ns FROM event_followup_pins ORDER BY id"):
        if type(replay) is not int or not isinstance(created, str) or any(not isinstance(v, str) or not v.strip() or len(v.encode("utf-8")) > 256 for v in (domain, candidate, case_id, actor)):
            raise ValueError("invalid follow-up selection")
        run = conn.execute("SELECT candidate_id FROM event_replay_runs WHERE id=?", (replay,)).fetchone()
        case = conn.execute("SELECT 1 FROM event_replay_cases WHERE run_id=? AND case_id=?", (replay, case_id)).fetchone()
        if run != (candidate,) or case is None:
            raise ValueError("invalid follow-up selection case")
        replay_link = _event_link(conn, "event_replay", replay)
        link = _event_link(conn, "event_followup_pin", pin)
        fields = ("synthetic-event-followup-selection-v1", domain, candidate, str(replay), case_id, actor, replay_link[1])
        if link[2:] != (encoded_fields(fields), created):
            raise ValueError("follow-up selection binding mismatch")
        pins += 1
    receipts = 0
    for receipt, review, observation, pin, status, created in conn.execute("SELECT id,review_id,observation_id,pin_id,status,created_unix_ns FROM event_followup_receipts ORDER BY id"):
        if status not in ("match", "verdict_mismatch", "input_mismatch", "worker_error") or not isinstance(created, str):
            raise ValueError("invalid follow-up receipt")
        review_link = _event_link(conn, "event_review", review)
        observation_link = _event_link(conn, "event_observation", observation)
        pin_link = _event_link(conn, "event_followup_pin", pin)
        receipt_link = _event_link(conn, "event_followup_receipt", receipt)
        fields = ("synthetic-followup-receipt-v1", str(review), str(observation), str(pin), status, review_link[1], observation_link[1], pin_link[1])
        if receipt_link[2:] != (encoded_fields(fields), created):
            raise ValueError("follow-up receipt binding mismatch")
        selected = conn.execute("SELECT domain,candidate_id,replay_id,case_id FROM event_followup_pins WHERE id=?", (pin,)).fetchone()
        reviewed = conn.execute("SELECT r.candidate_id,r.replay_id,r.action,o.domain,o.champion_id,o.shadow_id,o.champion_json,o.champion_error,o.shadow_json,o.shadow_error,o.worker_sha256,o.prior_json FROM event_reviews r JOIN event_observations o ON o.id=r.observation_id WHERE r.id=?", (review,)).fetchone()
        observed = conn.execute("SELECT champion_id,domain,prior_json,event_json,champion_json,champion_error,worker_sha256 FROM event_observations WHERE id=?", (observation,)).fetchone()
        if selected is None or reviewed is None or observed is None:
            raise ValueError("missing follow-up evidence")
        domain, candidate, follow_replay, case_id = selected
        reviewed_candidate, review_replay, action, review_domain, review_champion, review_shadow, review_champion_result, review_champion_error, review_shadow_result, review_shadow_error, review_worker, review_prior = reviewed
        champion, observation_domain, prior, event, actual, error, observation_worker = observed
        main = conn.execute("SELECT candidate_id,pack_digest,state_semantics,passed,worker_sha256,pack_version FROM event_replay_runs WHERE id=?", (review_replay,)).fetchone()
        follow = conn.execute("SELECT candidate_id,pack_digest,state_semantics,worker_sha256 FROM event_replay_runs WHERE id=?", (follow_replay,)).fetchone()
        if main is None or follow is None:
            raise ValueError("missing follow-up replay")
        if (action != "approve" or reviewed_candidate != candidate or champion != candidate or review_domain != domain or observation_domain != domain
                or main[0] != candidate or follow[0] != candidate or main[3] != 1 or main[1] == follow[1] or main[2] != follow[2]
                or main[4] is None or main[4] != follow[3] or main[4] != observation_worker or main[4] != review_worker
                or review_shadow != candidate or review_champion_result is None or review_shadow_result is None
                or review_champion_error is not None or review_shadow_error is not None):
            raise ValueError("follow-up provenance mismatch")
        review_prior_keys = ("count", "mean", "p99_us", "last_error") + (("state_semantics",) if main[2] == "measured-v2" else ())
        prior_value = _object(review_prior, review_prior_keys)
        if main[2] == "measured-v2" and prior_value["state_semantics"] != main[2]:
            raise ValueError("review state semantics mismatch")
        primary = conn.execute("SELECT id,pack_version,pack_digest FROM event_fixture_pins WHERE domain=?", (domain,)).fetchone()
        if primary is None or primary[1:] != (main[5], main[1]) or _event_link(conn, "event_fixture_pin", primary[0])[0] >= review_link[0]:
            raise ValueError("review fixture selection mismatch")
        main_link = _event_link(conn, "event_replay", review_replay)
        follow_link = _event_link(conn, "event_replay", follow_replay)
        if not (main_link[0] < review_link[0] < observation_link[0] < receipt_link[0] and follow_link[0] < pin_link[0] < review_link[0]):
            raise ValueError("invalid follow-up chronology")
        transition = conn.execute("SELECT t.kind,t.reference,t.next_id,e.seq,t.previous_id FROM event_champion_transitions t JOIN ledger_events e ON e.kind='event_champion_transition' AND e.reference=t.id WHERE t.domain=? AND e.seq<? ORDER BY e.seq DESC LIMIT 1", (domain, observation_link[0])).fetchone()
        if transition is None or transition[:3] != ("promote", review, candidate) or transition[3] <= review_link[0] or transition[4] != review_champion:
            raise ValueError("follow-up has no active reviewed promotion")
        case = conn.execute("SELECT window_json,event_json,expected_json FROM event_replay_cases WHERE run_id=? AND case_id=?", (follow_replay, case_id)).fetchone()
        if case is None or conn.execute("SELECT EXISTS(SELECT 1 FROM event_replay_cases WHERE run_id=? AND window_json=? AND event_json=?)", (review_replay, case[0], case[1])).fetchone()[0]:
            raise ValueError("follow-up case duplicates reviewed input")
        if _followup_result(case[0], case[1], case[2], prior, event, actual, error, follow[2]) != status:
            raise ValueError("follow-up result mismatch")
        receipts += 1
    return pins, receipts


def _measured_state(data, capacity):
    if len(data) < 24 or data[:4] != b"SES2":
        raise ValueError("invalid measured stream state")
    cap, total, error, count = struct.unpack(">IQII", data[4:24])
    if cap != capacity or count != min(total, cap) or len(data) != 24 + 16 * count:
        raise ValueError("invalid measured stream dimensions")
    samples = [struct.unpack(">dQ", data[24 + 16*i:40 + 16*i]) for i in range(count)]
    if any(not math.isfinite(metric) or abs(metric) > 1e12 for metric, _ in samples):
        raise ValueError("invalid measured stream metric")
    return total, error, samples


def _verify_stream_history(conn, table, measured):
    states = {}
    for name, capacity, generation, prior, next_state, observation in conn.execute(
            f"SELECT name,capacity,generation,prior,next,observation_id FROM {table} ORDER BY name,generation"):
        if not 1 <= capacity <= 4096:
            raise ValueError("invalid stream capacity")
        if measured:
            total, last_error, samples = _measured_state(prior, capacity)
        elif len(prior) % 8 or len(next_state) % 8 or len(prior) > 8*capacity or len(next_state) > 8*capacity:
            raise ValueError("invalid legacy stream state")
        previous = states.get(name)
        if previous is None:
            if generation != 0 or observation is not None or (measured and (prior != next_state or total != 0)) or (not measured and (prior or next_state)):
                raise ValueError("invalid initial stream receipt")
        else:
            if previous != (capacity, generation - 1, prior) or observation is None:
                raise ValueError("discontinuous stream receipt")
            row = conn.execute("SELECT prior_json,event_json,champion_us,champion_error FROM event_observations WHERE id=?", (observation,)).fetchone()
            if row is None:
                raise ValueError("missing stream observation")
            prior_json, event_json, duration, error = row
            event = json.loads(event_json)
            metric = event.get("metric") if isinstance(event, dict) else None
            if type(metric) not in (int, float) or not math.isfinite(metric) or abs(metric) > 1e12:
                raise ValueError("invalid stream metric")
            if measured:
                snapshot = json.loads(prior_json)
                mean = 0.0
                for sample, _ in samples:
                    mean += sample
                mean = mean / len(samples) if samples else 0.0
                durations = sorted(value for _, value in samples)
                p99 = durations[min(len(durations) * 99 // 100, len(durations) - 1)] if durations else 0
                if (not isinstance(snapshot, dict) or snapshot.get("state_semantics") != "measured-v2"
                        or snapshot.get("count") != total or type(snapshot.get("mean")) not in (int, float)
                        or struct.pack(">d", snapshot["mean"]) != struct.pack(">d", mean)
                        or snapshot.get("p99_us") != p99 or snapshot.get("last_error") != last_error
                        or type(duration) is not int or duration < 0 or total == 2**64 - 1):
                    raise ValueError("measured observation state mismatch")
                samples = (samples + [(float(metric), duration)])[-capacity:]
                next_error = 500 if error is not None else last_error
                expected = b"SES2" + struct.pack(">IQII", capacity, total + 1, next_error, len(samples))
                expected += b"".join(struct.pack(">dQ", sample, elapsed) for sample, elapsed in samples)
            else:
                if metric < 0:
                    raise ValueError("invalid legacy stream metric")
                expected = (prior[8:] if len(prior) == capacity * 8 else prior) + struct.pack(">d", metric)
            if expected != next_state:
                raise ValueError("stream transition mismatch")
        states[name] = (capacity, generation, next_state)
    stream_table, column = ("measured_streams", "state") if measured else ("event_streams", "latencies")
    actual = {name: (capacity, generation, state) for name, capacity, generation, state in
              conn.execute(f"SELECT name,capacity,generation,{column} FROM {stream_table}")}
    if any(actual.get(name) != state for name, state in states.items()) or (measured and len(actual) != len(states)):
        raise ValueError("current stream state mismatch")
    return len(states)


def verify_event_stream_receipts(conn):
    counts = []
    for table, kind, marker in (("event_stream_receipts", "event_stream_receipt", "synthetic-event-stream-v1"),
                                ("measured_stream_receipts", "measured_stream_receipt", "synthetic-measured-stream-v2")):
        orphan = conn.execute(f"SELECT count(*) FROM ledger_events e LEFT JOIN {table} r ON r.id=e.reference WHERE e.kind=? AND r.id IS NULL", (kind,)).fetchone()[0]
        if orphan:
            raise ValueError("orphan stream receipt event")
        count = 0
        for receipt, name, capacity, generation, prior, next_state, observation, created in conn.execute(
                f"SELECT id,name,capacity,generation,prior,next,observation_id,created_unix_ns FROM {table} ORDER BY id"):
            if (not isinstance(name, str) or not name or type(capacity) is not int or capacity <= 0
                    or type(generation) is not int or generation < 0 or not isinstance(prior, bytes)
                    or not isinstance(next_state, bytes) or not isinstance(created, str)
                    or (observation is not None and type(observation) is not int)):
                raise ValueError("invalid stream receipt")
            observation_hash = _event_link(conn, "event_observation", observation)[1] if observation is not None else ""
            fields = (marker, name, str(capacity), str(generation), hashlib.sha256(prior).hexdigest(),
                      hashlib.sha256(next_state).hexdigest(), str(observation) if observation is not None else "", observation_hash)
            link = _event_link(conn, kind, receipt)
            if link[2:] != (encoded_fields(fields), created):
                raise ValueError("stream receipt binding mismatch")
            count += 1
        histories = _verify_stream_history(conn, table, kind == "measured_stream_receipt")
        if kind == "event_stream_receipt":
            legacy_unreceipted = conn.execute("SELECT count(*) FROM event_streams").fetchone()[0] - histories
        counts.append(count)
    return (*counts, legacy_unreceipted)


def verify_event_preference_receipts(conn):
    if conn.execute("PRAGMA user_version").fetchone()[0] < 37:
        return None
    orphan = conn.execute("SELECT count(*) FROM ledger_events e LEFT JOIN event_preference_receipts a ON a.id=e.reference WHERE e.kind='event_preference_receipt' AND a.id IS NULL").fetchone()[0]
    if orphan:
        raise ValueError("orphan preference receipt event")
    count = 0
    for receipt, trace, authority, payload, signature, config, anchor_seq, anchor_hash, created in conn.execute(
            "SELECT id,trace_id,authority,payload,signature,authority_digest,checkpoint_seq,checkpoint_hash,created_unix_ns FROM event_preference_receipts ORDER BY id"):
        if (type(trace) is not int or type(anchor_seq) is not int or anchor_seq <= 0
                or not isinstance(authority, str) or not authority.strip() or len(authority.encode('utf-8')) > 256
                or not isinstance(payload, bytes) or not 1 <= len(payload) <= 131_072
                or not isinstance(signature, bytes) or len(signature) != 64
                or not isinstance(config, str) or not re.fullmatch(r"[0-9a-f]{64}", config)
                or not isinstance(anchor_hash, str) or not re.fullmatch(r"[0-9a-f]{64}", anchor_hash)
                or not isinstance(created, str)):
            raise ValueError("invalid preference receipt")
        trace_link = _event_link(conn, "event_training_trace", trace)
        receipt_link = _event_link(conn, "event_preference_receipt", receipt)
        anchor = conn.execute("SELECT hash FROM ledger_events WHERE seq=?", (anchor_seq,)).fetchone()
        if anchor != (anchor_hash,) or not (trace_link[0] <= anchor_seq < receipt_link[0]):
            raise ValueError("invalid preference checkpoint")
        fields = ("synthetic-preference-receipt-v1", str(trace), authority, hashlib.sha256(payload).hexdigest(),
                  hashlib.sha256(signature).hexdigest(), config, str(anchor_seq), anchor_hash, trace_link[1])
        if receipt_link[2:] != (encoded_fields(fields), created):
            raise ValueError("preference receipt binding mismatch")
        count += 1
    return count


def verify_attestation_signatures(conn, authorities, expected_digest):
    """Verify signatures only; the supplied keys and timestamps are not authenticated."""
    try:
        from cryptography.exceptions import InvalidSignature
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    except ImportError as error:
        raise ValueError("optional cryptography package is required for signature verification") from error
    def check(authority, role, payload, signature, digest_value):
        if digest_value != expected_digest or authority not in authorities:
            raise ValueError("signature authority configuration mismatch")
        actual_role, key = authorities[authority]
        if actual_role != role or not isinstance(payload, bytes) or not 1 <= len(payload) <= 131072 or not isinstance(signature, bytes) or len(signature) != 64:
            raise ValueError("invalid signature input or authority role")
        fields = (authority.encode("utf-8"), role.encode("ascii"))
        message = b"sese-evidence-attestation-v1\0" + b"".join(struct.pack(">I",len(v)) + v for v in fields) + struct.pack(">Q",len(payload)) + hashlib.sha256(payload).digest()
        try:
            Ed25519PublicKey.from_public_bytes(key).verify(signature, message)
        except (InvalidSignature, ValueError) as error:
            raise ValueError("invalid attestation signature") from error
    count = 0
    for source, observer, trace, outcome, trace_sig, outcome_sig, config in conn.execute("SELECT trace_authority,outcome_authority,trace_payload,outcome_payload,trace_signature,outcome_signature,authority_digest FROM event_attestation_receipts"):
        check(source,"trace-source",trace,trace_sig,config)
        check(observer,"outcome-observer",outcome,outcome_sig,config)
        count += 2
    if conn.execute("PRAGMA user_version").fetchone()[0] >= 37:
        for reviewer, payload, signature, config in conn.execute("SELECT authority,payload,signature,authority_digest FROM event_preference_receipts"):
            check(reviewer,"preference-reviewer",payload,signature,config)
            count += 1
    return count


def load_signature_authorities(path, expected_digest):
    if not re.fullmatch(r"[0-9a-f]{64}", expected_digest):
        raise ValueError("invalid authority digest")
    fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as stream:
        if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
            raise ValueError("authority configuration must be a regular file")
        data = stream.read(65537)
    if not data or len(data) > 65536 or hashlib.sha256(data).hexdigest() != expected_digest:
        raise ValueError("authority configuration size or digest mismatch")
    def pairs(items):
        obj = {}
        for key, value in items:
            if key in obj: raise ValueError("duplicate authority field")
            obj[key] = value
        return obj
    value = json.loads(data.decode("utf-8"), object_pairs_hook=pairs)
    if not isinstance(value, dict) or set(value) != {"schema","authorities"} or value["schema"] != "sese-authority-set-v1" or not isinstance(value["authorities"],list) or not 1 <= len(value["authorities"]) <= 128:
        raise ValueError("invalid authority configuration")
    authorities = {}
    seen_keys = set()
    for item in value["authorities"]:
        if not isinstance(item,dict) or set(item) != {"id","role","public_key","revoked"}:
            raise ValueError("invalid authority fields")
        name, role, key, revoked = item["id"], item["role"], item["public_key"], item["revoked"]
        if (not isinstance(name,str) or not name.strip() or name.strip() != name or len(name.encode("utf-8")) > 256
            or name in authorities or role not in ("trace-source","outcome-observer","preference-reviewer")
            or not isinstance(key,str) or not re.fullmatch(r"[0-9a-f]{64}",key) or key in seen_keys or type(revoked) is not bool):
            raise ValueError("invalid authority")
        seen_keys.add(key)
        # Keep revoked identities in the uniqueness check but not in usable authorities.
        authorities[name] = (role, bytes.fromhex(key), revoked)
    return {name:(role,key) for name,(role,key,revoked) in authorities.items() if not revoked}


def verify_event_attestations(conn):
    orphan = conn.execute("SELECT count(*) FROM ledger_events e LEFT JOIN event_attestation_receipts a ON a.id=e.reference WHERE e.kind='event_attestation_receipt' AND a.id IS NULL").fetchone()[0]
    if orphan:
        raise ValueError("orphan attestation receipt event")
    count = 0
    for receipt, trace, source, observer, trace_payload, outcome_payload, trace_signature, outcome_signature, config, anchor_seq, anchor_hash, created in conn.execute(
            "SELECT id,trace_id,trace_authority,outcome_authority,trace_payload,outcome_payload,trace_signature,outcome_signature,authority_digest,checkpoint_seq,checkpoint_hash,created_unix_ns FROM event_attestation_receipts ORDER BY id"):
        if (type(trace) is not int or type(anchor_seq) is not int or anchor_seq <= 0
                or any(not isinstance(v, str) or not v.strip() or len(v.encode('utf-8')) > 256 for v in (source, observer))
                or any(not isinstance(v, bytes) or not 1 <= len(v) <= 131_072 for v in (trace_payload, outcome_payload))
                or any(not isinstance(v, bytes) or len(v) != 64 for v in (trace_signature, outcome_signature))
                or not isinstance(config, str) or not re.fullmatch(r"[0-9a-f]{64}", config)
                or not isinstance(anchor_hash, str) or not re.fullmatch(r"[0-9a-f]{64}", anchor_hash)
                or not isinstance(created, str)):
            raise ValueError("invalid attestation receipt")
        trace_row = conn.execute("SELECT review_id FROM event_training_traces WHERE id=?", (trace,)).fetchone()
        if trace_row is None:
            raise ValueError("missing attestation trace")
        trace_link = _event_link(conn, "event_training_trace", trace)
        outcome_link = _event_link(conn, "event_outcome", trace_row[0])
        receipt_link = _event_link(conn, "event_attestation_receipt", receipt)
        anchor = conn.execute("SELECT hash FROM ledger_events WHERE seq=?", (anchor_seq,)).fetchone()
        if anchor != (anchor_hash,) or not (max(trace_link[0], outcome_link[0]) <= anchor_seq < receipt_link[0]):
            raise ValueError("invalid attestation checkpoint")
        hashes = tuple(hashlib.sha256(value).hexdigest() for value in (trace_payload, outcome_payload, trace_signature, outcome_signature))
        fields = ("synthetic-attestation-receipt-v1", str(trace), source, observer, *hashes, config,
                  str(anchor_seq), anchor_hash, trace_link[1], outcome_link[1])
        if receipt_link[2:] != (encoded_fields(fields), created):
            raise ValueError("attestation receipt binding mismatch")
        count += 1
    return count


def decode_synthetic_attestation(payload, kind):
    """Decode the bounded canonical format; this does not verify a signature."""
    prefix = f"sese-synthetic-{kind}-attestation-v1".encode("ascii") + b"\0"
    if kind not in ("trace", "outcome", "preference") or not payload.startswith(prefix):
        raise ValueError("invalid attestation prefix")
    pos = len(prefix)

    def take(size):
        nonlocal pos
        if size < 0 or pos + size > len(payload):
            raise ValueError("truncated attestation")
        value = payload[pos:pos + size]
        pos += size
        return value

    def integer(fmt):
        return struct.unpack(fmt, take(struct.calcsize(fmt)))[0]

    def text_field():
        size = integer(">I")
        if not 0 < size <= 16_384:
            raise ValueError("invalid attestation field size")
        try:
            value = take(size).decode("utf-8")
        except UnicodeDecodeError as error:
            raise ValueError("invalid attestation UTF-8") from error
        if not value.strip():
            raise ValueError("blank attestation field")
        return value

    def hash_field():
        value = take(64)
        if not re.fullmatch(b"[0-9a-f]{64}", value):
            raise ValueError("invalid attestation hash")
        return value.decode("ascii")

    result = {"review_id": integer(">q"), "replay_id": integer(">q"),
              "candidate_sha256": hash_field(), "fixture_sha256": hash_field(),
              "review_hash": hash_field()}
    if result["review_id"] <= 0 or result["replay_id"] <= 0:
        raise ValueError("invalid attestation reference")
    if kind == "outcome":
        status = take(1)
        if status not in (b"\x01", b"\x02"):
            raise ValueError("invalid attestation outcome")
        result["status"] = "validated" if status == b"\x01" else "falsified"
        names = ("source", "method", "evidence_reference")
    elif kind == "preference":
        result["trace_id"] = integer(">q")
        result["trace_hash"] = hash_field()
        if result["trace_id"] <= 0:
            raise ValueError("invalid attestation trace")
        names = ("prompt", "preferred", "rejected", "source", "comparability_basis")
    else:
        names = ("prompt", "response", "source", "method")
    result.update((name, text_field()) for name in names)
    result["timestamp"] = integer(">Q")
    if result["timestamp"] == 0 or pos != len(payload):
        raise ValueError("invalid attestation timestamp or trailing bytes")
    if kind == "preference" and result["preferred"] == result["rejected"]:
        raise ValueError("identical attestation preference")
    return result


def verify_attestation_payload_record_links(conn):
    """Decode every retained claim and compare its assertions to local records.

    This check neither verifies signatures nor establishes timestamp authenticity.
    """
    def context(trace_id):
        row = conn.execute("SELECT t.review_id,t.prompt,t.response,t.source,t.preferred,t.rejected,t.pair_source,t.created_unix_ns,r.candidate_id,r.replay_id,r.created_unix_ns,p.pack_digest,o.status,o.source,o.created_unix_ns FROM event_training_traces t JOIN event_reviews r ON r.id=t.review_id JOIN event_replay_runs p ON p.id=r.replay_id LEFT JOIN event_review_outcomes o ON o.review_id=r.id WHERE t.id=?", (trace_id,)).fetchone()
        if row is None:
            raise ValueError("attestation records missing")
        review,prompt,response,source,preferred,rejected,pair_source,trace_time,candidate,replay,review_time,fixture,status,outcome_source,outcome_time = row
        review_hash = _event_link(conn,"event_review",review)[1]
        trace_hash = _event_link(conn,"event_training_trace",trace_id)[1]
        common = {"review_id":review,"replay_id":replay,"candidate_sha256":candidate,"fixture_sha256":fixture,"review_hash":review_hash}
        return common, (prompt,response,source,preferred,rejected,pair_source,int(trace_time),int(review_time),status,outcome_source,int(outcome_time) if outcome_time is not None else None,trace_hash)
    def reference(claim, common):
        if any(claim[key] != value for key,value in common.items()):
            raise ValueError("attestation reference mismatch")
    count = 0
    for trace_id, trace_bytes, outcome_bytes in conn.execute("SELECT trace_id,trace_payload,outcome_payload FROM event_attestation_receipts ORDER BY id"):
        common, (prompt,response,source,preferred,rejected,pair_source,trace_time,review_time,status,outcome_source,outcome_time,trace_hash) = context(trace_id)
        trace = decode_synthetic_attestation(trace_bytes,"trace")
        outcome = decode_synthetic_attestation(outcome_bytes,"outcome")
        reference(trace,common); reference(outcome,common)
        if (trace["prompt"],trace["response"],trace["source"]) != (prompt,response,source) or (outcome["status"],outcome["source"]) != (status,outcome_source):
            raise ValueError("attestation content mismatch")
        if outcome_time is None or not (trace["timestamp"] <= trace_time and review_time <= outcome["timestamp"] <= outcome_time):
            raise ValueError("attestation timestamp mismatch")
        count += 2
    if conn.execute("PRAGMA user_version").fetchone()[0] >= 37:
        for trace_id, payload, recorded in conn.execute("SELECT trace_id,payload,created_unix_ns FROM event_preference_receipts ORDER BY id"):
            common, (prompt,response,source,preferred,rejected,pair_source,trace_time,review_time,status,outcome_source,outcome_time,trace_hash) = context(trace_id)
            claim = decode_synthetic_attestation(payload,"preference")
            reference(claim,common)
            if (claim["trace_id"],claim["trace_hash"],claim["prompt"],claim["preferred"],claim["rejected"],claim["source"]) != (trace_id,trace_hash,prompt,preferred,rejected,pair_source):
                raise ValueError("preference content mismatch")
            if not trace_time <= claim["timestamp"] <= int(recorded):
                raise ValueError("preference timestamp mismatch")
            count += 1
    return count


def verify_signed_followup_links(conn):
    """Match claims to receipts; call only after the follow-up receipts were recomputed."""
    checked = 0
    other = 0
    for payload, in conn.execute("SELECT outcome_payload FROM event_attestation_receipts ORDER BY id"):
        claim = decode_synthetic_attestation(payload,"outcome")
        if claim["method"] != "synthetic-followup-receipt-v1":
            other += 1
            continue
        match = re.fullmatch(r"sese-followup:([1-9][0-9]*):([0-9a-f]{64})",claim["evidence_reference"])
        if match is None or int(match.group(1)) > 2**63-1:
            raise ValueError("invalid signed follow-up reference")
        receipt_id = int(match.group(1))
        receipt = conn.execute("SELECT review_id,status,created_unix_ns FROM event_followup_receipts WHERE id=?",(receipt_id,)).fetchone()
        if receipt is None:
            raise ValueError("missing signed follow-up receipt")
        review,status,created = receipt
        link = _event_link(conn,"event_followup_receipt",receipt_id)
        outcome_seq = _event_link(conn,"event_outcome",review)[0]
        expected = {"match":"validated","verdict_mismatch":"falsified"}.get(status)
        if (review != claim["review_id"] or link[1] != match.group(2) or link[0] >= outcome_seq
                or expected is None or expected != claim["status"] or claim["timestamp"] < int(created)):
            raise ValueError("signed follow-up receipt mismatch")
        checked += 1
    return checked,other


def verify_event_export(conn, path, seq, checkpoint_hash):
    def unique_object(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate export field")
            result[key] = value
        return result

    fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as stream:
        if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
            raise ValueError("export must be a regular file")
        data = stream.read(1_048_577)
    if len(data) > 1_048_576:
        raise ValueError("export exceeds size limit")
    exported = json.loads(data.decode("utf-8"), object_pairs_hook=unique_object)
    if not isinstance(exported, dict):
        raise ValueError("invalid export")
    attestation_id, preference_id = exported.get("attestation_receipt_id"), exported.get("preference_receipt_id")
    if any(type(value) is not int or value <= 0 for value in (attestation_id, preference_id)):
        raise ValueError("invalid receipt identifiers")
    a = conn.execute("SELECT trace_id,trace_authority,outcome_authority,trace_payload,outcome_payload,trace_signature,outcome_signature,authority_digest FROM event_attestation_receipts WHERE id=?", (attestation_id,)).fetchone()
    p = conn.execute("SELECT trace_id,authority,payload,signature,authority_digest FROM event_preference_receipts WHERE id=?", (preference_id,)).fetchone()
    if a is None or p is None or a[0] != p[0] or a[7] != p[4]:
        raise ValueError("receipt mismatch")
    if max(_event_link(conn, "event_attestation_receipt", attestation_id)[0], _event_link(conn, "event_preference_receipt", preference_id)[0]) > seq:
        raise ValueError("checkpoint does not cover receipts")
    row = conn.execute("SELECT r.id,r.candidate_id,r.replay_id,r.action,r.actor,p.pack_version,p.pack_digest,t.actor,t.source,t.prompt,t.response,t.preferred,t.rejected,t.pair_source,o.status,o.actor,o.source FROM event_training_traces t JOIN event_reviews r ON r.id=t.review_id JOIN event_replay_runs p ON p.id=r.replay_id JOIN event_review_outcomes o ON o.review_id=r.id WHERE t.id=?", (a[0],)).fetchone()
    if row is None:
        raise ValueError("export records missing")
    review,candidate,replay,action,review_actor,fixture_version,fixture_digest,trace_actor,trace_source,prompt,response,preferred,rejected,pair_source,status,outcome_actor,outcome_source = row
    trace_claim = decode_synthetic_attestation(a[3], "trace")
    outcome_claim = decode_synthetic_attestation(a[4], "outcome")
    preference_claim = decode_synthetic_attestation(p[2], "preference")
    review_link = _event_link(conn, "event_review", review)
    trace_link = _event_link(conn, "event_training_trace", a[0])
    common = {"review_id": review, "replay_id": replay, "candidate_sha256": candidate,
              "fixture_sha256": fixture_digest, "review_hash": review_link[1]}
    for claim in (trace_claim, outcome_claim, preference_claim):
        if any(claim[key] != value for key, value in common.items()):
            raise ValueError("attestation reference mismatch")
    if ((trace_claim["prompt"], trace_claim["response"], trace_claim["source"]) != (prompt, response, trace_source)
            or (outcome_claim["status"], outcome_claim["source"]) != (status, outcome_source)
            or (preference_claim["trace_id"], preference_claim["trace_hash"], preference_claim["prompt"], preference_claim["preferred"], preference_claim["rejected"], preference_claim["source"]) != (a[0], trace_link[1], prompt, preferred, rejected, pair_source)):
        raise ValueError("attestation content mismatch")
    times = conn.execute("SELECT t.created_unix_ns,r.created_unix_ns,o.created_unix_ns FROM event_training_traces t JOIN event_reviews r ON r.id=t.review_id JOIN event_review_outcomes o ON o.review_id=r.id WHERE t.id=?", (a[0],)).fetchone()
    trace_time, review_time, outcome_time = map(int, times)
    if not (trace_claim["timestamp"] <= trace_time and review_time <= outcome_claim["timestamp"] <= outcome_time and preference_claim["timestamp"] >= trace_time):
        raise ValueError("attestation timestamp mismatch")
    expected = {
        "schema":"sese-synthetic-signed-evidence-v1", "export_kind":"inspection_only",
        "candidate":{"sha256":candidate,"replay_id":replay,"fixture_version":fixture_version,"fixture_sha256":fixture_digest},
        "decision":{"review_id":review,"action":action,"actor":review_actor},
        "trace":{"id":a[0],"actor":trace_actor,"source":trace_source,"prompt":prompt,"response":response,"origin":"caller_asserted_synthetic","model_origin_verified":False},
        "preference":{"preferred":preferred,"rejected":rejected,"source":pair_source,"comparability_verified":False},
        "outcome":{"status":status,"actor":outcome_actor,"source":outcome_source,"evidence":"caller_asserted","truth_verified":False},
        "signatures":{"trace":{"authority":a[1],"payload_hex":a[3].hex(),"signature_hex":a[5].hex()},"outcome":{"authority":a[2],"payload_hex":a[4].hex(),"signature_hex":a[6].hex()},"preference":{"authority":p[1],"payload_hex":p[2].hex(),"signature_hex":p[3].hex()}},
        "attestation_receipt_id":attestation_id,"preference_receipt_id":preference_id,
        "checkpoint":{"seq":seq,"hash":checkpoint_hash},"checkpoint_custody_verified":False,
        "authority_config_sha256":a[7],"authority_config_verified":False,
        "observer_independence_verified":False,"reviewer_independence_verified":False,
        "signatures_rechecked":True,"synthetic":True,"training_ready":False,
    }
    if json.dumps(exported, sort_keys=True) != json.dumps(expected, sort_keys=True):
        raise ValueError("export does not match database")
    return True


def verify(database, expected_seq, expected_hash, check_synthetic=False, check_replays=False, check_fixtures=False, check_workers=False, check_observations=False, check_reviews=False, check_champions=False, check_traces=False, check_followups=False, check_stream_receipts=False, check_attestations=False, event_export=None, signature_authorities=None, require_head=False):
    check_attestations = check_attestations or event_export is not None or signature_authorities is not None
    if expected_seq <= 0 or not re.fullmatch(r"[0-9a-f]{64}", expected_hash):
        raise ValueError("invalid checkpoint")
    uri = Path(database).absolute().as_uri() + "?mode=ro"
    conn = sqlite3.connect(uri, uri=True)
    try:
        conn.execute("BEGIN")
        previous = "GENESIS"
        sequence = 0
        found = False
        for seq, kind, reference, payload, timestamp, link, recorded in conn.execute(
            "SELECT seq,kind,reference,payload,created_ns,prev,hash FROM ledger_events ORDER BY seq"
        ):
            if type(seq) is not int or type(reference) is not int or not all(
                isinstance(value, str) for value in (kind, payload, timestamp, link, recorded)
            ):
                raise ValueError("invalid ledger field type")
            if seq != sequence + 1 or link != previous or recorded != digest(previous, kind, reference, payload, timestamp):
                raise ValueError("ledger chain mismatch")
            if seq == expected_seq:
                found = recorded == expected_hash
            sequence = seq
            previous = recorded
        if not found:
            raise ValueError("checkpoint missing or mismatched")
        if require_head and expected_seq != sequence:
            raise ValueError("checkpoint does not cover ledger head")
        checked = verify_synthetic_candidates(conn) if check_synthetic else None
        check_any_replays = check_replays or check_fixtures or check_reviews or check_champions or check_traces or check_followups or check_attestations
        replays = verify_event_replays(conn) if check_any_replays else None
        replay_timings = verify_event_replay_timings(conn) if check_any_replays else None
        fixtures = verify_event_fixtures(conn) if (check_fixtures or check_followups) else None
        workers, unrecorded = verify_event_workers(conn) if (check_workers or check_followups) else (None, None)
        observations = verify_event_observations(conn) if (check_observations or check_reviews or check_champions or check_traces or check_followups or check_stream_receipts or check_attestations) else None
        reviews, outcomes = verify_event_reviews(conn) if (check_reviews or check_champions or check_traces or check_followups or check_attestations) else (None, None)
        transitions, champions = verify_event_champions(conn) if (check_champions or check_followups) else (None, None)
        traces = verify_event_traces(conn) if (check_traces or check_attestations) else None
        fixture_pins = verify_event_fixture_pins(conn) if check_followups else None
        followup_pins, followup_receipts = verify_event_followups(conn) if check_followups else (None, None)
        followup_promotion_gates = verify_followup_promotion_gates(conn) if check_followups else None
        event_stream_receipts, measured_stream_receipts, legacy_unreceipted = verify_event_stream_receipts(conn) if check_stream_receipts else (None, None, None)
        attestation_receipts = verify_event_attestations(conn) if check_attestations else None
        preference_receipts = verify_event_preference_receipts(conn) if check_attestations else None
        export_match = verify_event_export(conn, event_export, expected_seq, expected_hash) if event_export is not None else None
        if signature_authorities is not None:
            uncovered = conn.execute("SELECT count(*) FROM ledger_events WHERE kind IN ('event_attestation_receipt','event_preference_receipt') AND seq>?", (expected_seq,)).fetchone()[0]
            if uncovered:
                raise ValueError("checkpoint does not cover all signature receipts")
        signature_count = verify_attestation_signatures(conn, signature_authorities[0], signature_authorities[1]) if signature_authorities is not None else None
        payload_record_count = verify_attestation_payload_record_links(conn) if signature_authorities is not None else None
        signed_followups, other_outcomes = verify_signed_followup_links(conn) if check_followups and signature_authorities is not None else (None, None)
        return {"chain_valid": True, "checkpoint_match": True, "head_seq": sequence,
                "head_hash": previous, "synthetic_candidates_checked": checked,
                "event_replay_bindings_checked": replays, "event_replay_timing_bindings_checked": replay_timings, "event_fixture_cases_checked": fixtures,
                "event_workers_checked": workers, "event_workers_unrecorded": unrecorded,
                "event_observation_bindings_checked": observations,
                "event_review_bindings_checked": reviews, "event_outcome_bindings_checked": outcomes,
                "event_champion_transitions_checked": transitions, "event_champions_checked": champions,
                "event_trace_bindings_checked": traces,
                "event_followup_pins_checked": followup_pins, "event_followup_receipts_checked": followup_receipts,
                "event_followup_promotion_gates_checked": followup_promotion_gates,
                "event_fixture_pins_checked": fixture_pins,
                "event_stream_receipt_bindings_checked": event_stream_receipts,
                "measured_stream_receipt_bindings_checked": measured_stream_receipts,
                "event_streams_without_receipts": legacy_unreceipted,
                "event_attestation_receipts_checked": attestation_receipts,
                "event_preference_receipts_checked": preference_receipts,
                "event_export_matches_database": export_match,
                "attestation_payload_record_links_checked": True if event_export is not None or payload_record_count is not None else False if check_attestations else None,
                "attestation_payload_record_links_count": payload_record_count,
                "signed_followup_claims_checked": signed_followups,
                "other_outcome_methods_not_checked": other_outcomes,
                "attestation_signatures_checked": True if signature_count is not None else False if check_attestations else None,
                "attestation_signature_count": signature_count,
                "authority_key_strength_strictly_checked": False if signature_count is not None else None,
                "evidence_bindings_checked": False}
    finally:
        conn.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("database")
    parser.add_argument("--seq", type=int, required=True)
    parser.add_argument("--hash", required=True, dest="checkpoint_hash")
    parser.add_argument("--verify-event-cycle", action="store_true", help="check synthetic candidates, typed event followups, traces, and stream receipts")
    parser.add_argument("--verify-synthetic-candidates", action="store_true")
    parser.add_argument("--verify-event-replays", action="store_true")
    parser.add_argument("--verify-event-fixtures", action="store_true")
    parser.add_argument("--verify-event-workers", action="store_true")
    parser.add_argument("--verify-event-observations", action="store_true")
    parser.add_argument("--verify-event-reviews", action="store_true")
    parser.add_argument("--verify-event-champions", action="store_true")
    parser.add_argument("--verify-event-traces", action="store_true")
    parser.add_argument("--verify-event-followups", action="store_true")
    parser.add_argument("--verify-event-stream-receipts", action="store_true")
    parser.add_argument("--verify-event-attestations", action="store_true")
    parser.add_argument("--verify-event-export")
    parser.add_argument("--verify-event-signatures", metavar="AUTHORITIES_JSON")
    parser.add_argument("--authority-sha256")
    args = parser.parse_args()
    if args.verify_event_cycle:
        args.verify_synthetic_candidates = True
        args.verify_event_followups = True
        args.verify_event_traces = True
        args.verify_event_stream_receipts = True
    try:
        if bool(args.verify_event_signatures) != bool(args.authority_sha256):
            raise ValueError("signature verification requires an authority file and digest")
        signature_authorities = (load_signature_authorities(args.verify_event_signatures, args.authority_sha256), args.authority_sha256) if args.verify_event_signatures else None
        print(json.dumps(verify(args.database, args.seq, args.checkpoint_hash, args.verify_synthetic_candidates, args.verify_event_replays, args.verify_event_fixtures, args.verify_event_workers, args.verify_event_observations, args.verify_event_reviews, args.verify_event_champions, args.verify_event_traces, args.verify_event_followups, args.verify_event_stream_receipts, args.verify_event_attestations, args.verify_event_export, signature_authorities, require_head=args.verify_event_cycle), sort_keys=True))
    except (OSError, sqlite3.Error, ValueError, UnicodeError, OverflowError, struct.error, TypeError) as error:
        print(f"ledger verification failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
