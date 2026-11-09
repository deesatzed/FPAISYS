"""Checkpoint coverage checks for the one-command event-cycle verification."""
import sqlite3
import tempfile
import unittest
from pathlib import Path

from verify_ledger import digest, verify


class EventCycleCheckpointTests(unittest.TestCase):
    def test_cycle_requires_a_checkpoint_covering_the_current_head(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "ledger.db"
            conn = sqlite3.connect(path)
            conn.execute("CREATE TABLE ledger_events(seq INTEGER,kind TEXT,reference INTEGER,payload TEXT,created_ns TEXT,prev TEXT,hash TEXT)")
            first = digest("GENESIS", "event", 1, "one", "1")
            second = digest(first, "event", 2, "two", "2")
            conn.executemany("INSERT INTO ledger_events VALUES(?,?,?,?,?,?,?)", [
                (1, "event", 1, "one", "1", "GENESIS", first),
                (2, "event", 2, "two", "2", first, second),
            ])
            conn.commit(); conn.close()
            self.assertTrue(verify(path, 1, first)["checkpoint_match"])
            with self.assertRaisesRegex(ValueError, "checkpoint does not cover ledger head"):
                verify(path, 1, first, require_head=True)
            self.assertTrue(verify(path, 2, second, require_head=True)["checkpoint_match"])


if __name__ == "__main__":
    unittest.main()
