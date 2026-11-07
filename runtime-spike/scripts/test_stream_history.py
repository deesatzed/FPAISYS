"""Focused tests of independent stream-transition reconstruction."""
import json
import sqlite3
import struct
import unittest

from verify_ledger import _verify_stream_history


class StreamHistoryTests(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.db.executescript("""
            CREATE TABLE event_stream_receipts(name,capacity,generation,prior,next,observation_id);
            CREATE TABLE measured_stream_receipts(name,capacity,generation,prior,next,observation_id);
            CREATE TABLE event_streams(name,capacity,generation,latencies);
            CREATE TABLE measured_streams(name,capacity,generation,state);
            CREATE TABLE event_observations(id,prior_json,event_json,champion_us,champion_error);
        """)

    def test_legacy_rollover_rejects_wrong_transition(self):
        empty = b""
        first = struct.pack(">d", 3.0)
        second = struct.pack(">d", 5.0)
        self.db.executemany("INSERT INTO event_stream_receipts VALUES(?,?,?,?,?,?)", [
            ("s", 1, 0, empty, empty, None), ("s", 1, 1, empty, first, 1),
            ("s", 1, 2, first, second, 2)])
        self.db.executemany("INSERT INTO event_observations VALUES(?,?,?,?,?)", [
            (1, "{}", '{"metric":3.0}', 1, None), (2, "{}", '{"metric":5.0}', 1, None)])
        self.db.execute("INSERT INTO event_streams VALUES(?,?,?,?)", ("s", 1, 2, second))
        self.assertEqual(_verify_stream_history(self.db, "event_stream_receipts", False), 1)
        self.db.execute("UPDATE event_observations SET event_json='{\"metric\":4.0}' WHERE id=2")
        with self.assertRaisesRegex(ValueError, "transition mismatch"):
            _verify_stream_history(self.db, "event_stream_receipts", False)

    def test_measured_failure_and_success_preserve_last_error(self):
        def state(total, error, metric=None, duration=None):
            count = 0 if metric is None else 1
            return b"SES2" + struct.pack(">IQII", 1, total, error, count) + (b"" if count == 0 else struct.pack(">dQ", metric, duration))
        zero, first, second = state(0, 0), state(1, 500, 3.0, 12), state(2, 500, 5.0, 9)
        self.db.executemany("INSERT INTO measured_stream_receipts VALUES(?,?,?,?,?,?)", [
            ("s", 1, 0, zero, zero, None), ("s", 1, 1, zero, first, 1),
            ("s", 1, 2, first, second, 2)])
        snapshots = [dict(state_semantics="measured-v2", count=0, mean=0.0, p99_us=0, last_error=0),
                     dict(state_semantics="measured-v2", count=1, mean=3.0, p99_us=12, last_error=500)]
        self.db.executemany("INSERT INTO event_observations VALUES(?,?,?,?,?)", [
            (1, json.dumps(snapshots[0]), '{"metric":3.0}', 12, "trap"),
            (2, json.dumps(snapshots[1]), '{"metric":5.0}', 9, None)])
        self.db.execute("INSERT INTO measured_streams VALUES(?,?,?,?)", ("s", 1, 2, second))
        self.assertEqual(_verify_stream_history(self.db, "measured_stream_receipts", True), 1)
        self.db.execute("UPDATE event_observations SET champion_us=8 WHERE id=2")
        with self.assertRaisesRegex(ValueError, "transition mismatch"):
            _verify_stream_history(self.db, "measured_stream_receipts", True)


if __name__ == "__main__":
    unittest.main()
