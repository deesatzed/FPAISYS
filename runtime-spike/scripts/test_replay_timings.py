import json
import sqlite3
import unittest

from verify_ledger import verify_event_replay_timings


class ReplayTimingCompatibilityTests(unittest.TestCase):
    def test_legacy_timing_allows_multiple_executed_failures_but_v2_does_not(self):
        conn = sqlite3.connect(":memory:")
        self.addCleanup(conn.close)
        conn.executescript("""
            CREATE TABLE ledger_events(seq INTEGER,kind TEXT,reference INTEGER,payload TEXT,created_ns TEXT,hash TEXT);
            CREATE TABLE event_replay_runs(id INTEGER,started_unix_ns TEXT);
            CREATE TABLE event_replay_cases(run_id INTEGER,ordinal INTEGER,case_id TEXT,actual_json TEXT,error TEXT);
            INSERT INTO event_replay_runs VALUES(1,'1');
            INSERT INTO event_replay_cases VALUES(1,0,'a',NULL,'worker failed');
            INSERT INTO event_replay_cases VALUES(1,1,'b',NULL,'worker failed');
            INSERT INTO ledger_events VALUES(1,'event_replay',1,'parent','1','parent-hash');
        """)
        case = lambda name: {"case_id": name, "started_unix_ns": "1", "finished_unix_ns": "2", "elapsed_ns": "1"}
        payload = {"schema": "sese-event-replay-timing-v1", "replay_hash": "parent-hash", "cases": [case("a"), case("b")]}
        conn.execute("INSERT INTO ledger_events VALUES(2,'event_replay_timing',1,?,'1','timing-hash')", (json.dumps(payload),))
        self.assertEqual(verify_event_replay_timings(conn), 1)
        payload["schema"] = "sese-event-replay-timing-v2"
        conn.execute("UPDATE ledger_events SET payload=? WHERE seq=2", (json.dumps(payload),))
        with self.assertRaisesRegex(ValueError, "invalid replay timing order"):
            verify_event_replay_timings(conn)
        payload["cases"][1] = {"case_id": "b", "started_unix_ns": None, "finished_unix_ns": None, "elapsed_ns": None}
        conn.execute("UPDATE event_replay_cases SET error='not run after earlier worker failure' WHERE ordinal=1")
        conn.execute("UPDATE ledger_events SET payload=? WHERE seq=2", (json.dumps(payload),))
        self.assertEqual(verify_event_replay_timings(conn), 1)
        payload["cases"][0] = {"case_id": "a", "started_unix_ns": None, "finished_unix_ns": None, "elapsed_ns": None}
        conn.execute("UPDATE event_replay_cases SET error='not run after earlier worker failure' WHERE ordinal=0")
        conn.execute("UPDATE ledger_events SET payload=? WHERE seq=2", (json.dumps(payload),))
        with self.assertRaisesRegex(ValueError, "invalid skipped replay timing"):
            verify_event_replay_timings(conn)


if __name__ == "__main__":
    unittest.main()
