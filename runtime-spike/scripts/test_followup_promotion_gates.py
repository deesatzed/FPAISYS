import sqlite3
import unittest

from verify_ledger import verify_followup_promotion_gates


class FollowupPromotionGatesTest(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.db.executescript("""
          CREATE TABLE event_champion_transitions(id INTEGER, domain TEXT, next_id TEXT, reference INTEGER, kind TEXT);
          CREATE TABLE event_followup_pins(id INTEGER, domain TEXT, candidate_id TEXT, replay_id INTEGER);
          CREATE TABLE event_replay_runs(id INTEGER, passed INTEGER);
          CREATE TABLE ledger_events(seq INTEGER, kind TEXT, reference INTEGER, hash TEXT DEFAULT 'hash', payload TEXT DEFAULT 'payload', created_ns TEXT DEFAULT 'time');
          INSERT INTO event_champion_transitions VALUES(1,'test','candidate',7,'promote');
          INSERT INTO event_replay_runs VALUES(4,1);
          INSERT INTO event_followup_pins VALUES(3,'test','candidate',4);
          INSERT INTO ledger_events(seq,kind,reference) VALUES(2,'event_followup_pin',3),(4,'event_review',7),(5,'event_champion_transition',1);
        """)

    def tearDown(self):
        self.db.close()

    def test_valid_selection_precedes_review(self):
        self.assertEqual(verify_followup_promotion_gates(self.db), 1)

    def test_late_selection_is_rejected(self):
        self.db.execute("UPDATE ledger_events SET seq=4 WHERE kind='event_followup_pin'")
        with self.assertRaisesRegex(ValueError, "follow-up selection.*approval"):
            verify_followup_promotion_gates(self.db)

    def test_failed_selected_replay_is_rejected(self):
        self.db.execute("UPDATE event_replay_runs SET passed=0")
        with self.assertRaisesRegex(ValueError, "follow-up replay failed"):
            verify_followup_promotion_gates(self.db)

    def test_post_promotion_selection_does_not_rewrite_history(self):
        self.db.execute("UPDATE ledger_events SET seq=6 WHERE kind='event_followup_pin'")
        self.assertEqual(verify_followup_promotion_gates(self.db), 0)

    def test_review_must_precede_promotion(self):
        self.db.execute("UPDATE ledger_events SET seq=7 WHERE kind='event_review'")
        with self.assertRaisesRegex(ValueError, "approval must precede promotion"):
            verify_followup_promotion_gates(self.db)

    def test_missing_review_link_fails_closed(self):
        self.db.execute("DELETE FROM ledger_events WHERE kind='event_review'")
        with self.assertRaises(ValueError):
            verify_followup_promotion_gates(self.db)


if __name__ == "__main__":
    unittest.main()
