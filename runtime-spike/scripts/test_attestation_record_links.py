"""Independent semantic checks for retained synthetic claims."""
import sqlite3
import struct
import unittest

from verify_ledger import verify_attestation_payload_record_links, verify_signed_followup_links


def payload(kind, fields, extra=b"", timestamp=15):
    ref = struct.pack(">qq",1,2) + b"a"*64 + b"b"*64 + b"c"*64
    text = b"".join(struct.pack(">I",len(s.encode()))+s.encode() for s in fields)
    return f"sese-synthetic-{kind}-attestation-v1".encode()+b"\0"+ref+extra+text+struct.pack(">Q",timestamp)


class RecordLinkTests(unittest.TestCase):
    def setUp(self):
        self.db=sqlite3.connect(":memory:")
        self.db.executescript("""
        CREATE TABLE event_training_traces(id INTEGER,review_id INTEGER,prompt TEXT,response TEXT,source TEXT,preferred TEXT,rejected TEXT,pair_source TEXT,created_unix_ns TEXT);
        CREATE TABLE event_reviews(id INTEGER,candidate_id TEXT,replay_id INTEGER,created_unix_ns TEXT);
        CREATE TABLE event_replay_runs(id INTEGER,pack_digest TEXT);
        CREATE TABLE event_review_outcomes(review_id INTEGER,status TEXT,source TEXT,created_unix_ns TEXT);
        CREATE TABLE event_attestation_receipts(id INTEGER,trace_id INTEGER,trace_payload BLOB,outcome_payload BLOB);
        CREATE TABLE event_preference_receipts(id INTEGER,trace_id INTEGER,payload BLOB,created_unix_ns TEXT);
        CREATE TABLE ledger_events(seq INTEGER,kind TEXT,reference INTEGER,hash TEXT,payload TEXT,created_ns TEXT);
        PRAGMA user_version=37;
        """)
        self.db.execute("INSERT INTO event_training_traces VALUES(1,1,'prompt','response','source','better','response','reviewer','20')")
        self.db.execute("INSERT INTO event_reviews VALUES(1,?,2,'10')",('a'*64,))
        self.db.execute("INSERT INTO event_replay_runs VALUES(2,?)",('b'*64,))
        self.db.execute("INSERT INTO event_review_outcomes VALUES(1,'validated','observer','30')")
        self.db.execute("INSERT INTO ledger_events VALUES(1,'event_review',1,?,'','')",('c'*64,))
        self.db.execute("INSERT INTO ledger_events VALUES(2,'event_training_trace',1,?,'','')",('d'*64,))
        self.trace=payload('trace',['prompt','response','source','method'],timestamp=15)
        self.outcome=payload('outcome',['observer','method','ref'],b'\x01',timestamp=25)
        self.pref=payload('preference',['prompt','better','response','reviewer','basis'],struct.pack('>q',1)+b'd'*64,timestamp=20)
        self.db.execute("INSERT INTO event_attestation_receipts VALUES(1,1,?,?)",(self.trace,self.outcome))

    def tearDown(self): self.db.close()

    def test_no_preference_is_required_for_trace_and_outcome(self):
        self.assertEqual(verify_attestation_payload_record_links(self.db),2)

    def test_signed_content_must_match_record(self):
        self.db.execute("UPDATE event_training_traces SET response='changed'")
        with self.assertRaisesRegex(ValueError,'content mismatch'):
            verify_attestation_payload_record_links(self.db)

    def test_preference_reference_and_receipt_time_are_checked(self):
        self.db.execute("INSERT INTO event_preference_receipts VALUES(1,1,?,'21')",(self.pref,))
        self.assertEqual(verify_attestation_payload_record_links(self.db),3)
        self.db.execute("UPDATE event_preference_receipts SET created_unix_ns='19'")
        with self.assertRaisesRegex(ValueError,'timestamp mismatch'):
            verify_attestation_payload_record_links(self.db)

    def test_outcome_and_trace_timestamps_are_bounded_by_records(self):
        future_trace = payload('trace',['prompt','response','source','method'],timestamp=21)
        self.db.execute("UPDATE event_attestation_receipts SET trace_payload=?",(future_trace,))
        with self.assertRaisesRegex(ValueError,'timestamp mismatch'):
            verify_attestation_payload_record_links(self.db)
        self.db.execute("UPDATE event_attestation_receipts SET trace_payload=?",(self.trace,))
        early_outcome = payload('outcome',['observer','method','ref'],b'\x01',timestamp=9)
        self.db.execute("UPDATE event_attestation_receipts SET outcome_payload=?",(early_outcome,))
        with self.assertRaisesRegex(ValueError,'timestamp mismatch'):
            verify_attestation_payload_record_links(self.db)

    def test_missing_outcome_or_truncated_payload_fails(self):
        self.db.execute("DELETE FROM event_review_outcomes")
        with self.assertRaises(ValueError): verify_attestation_payload_record_links(self.db)
        self.db.execute("INSERT INTO event_review_outcomes VALUES(1,'validated','observer','30')")
        self.db.execute("UPDATE event_attestation_receipts SET trace_payload=?",(self.trace[:-1],))
        with self.assertRaises(ValueError): verify_attestation_payload_record_links(self.db)

    def test_signed_followup_reference_matches_receipt_and_outcome(self):
        self.db.execute("CREATE TABLE event_followup_receipts(id INTEGER,review_id INTEGER,status TEXT,created_unix_ns TEXT)")
        self.db.execute("INSERT INTO event_followup_receipts VALUES(4,1,'match','22')")
        self.db.execute("INSERT INTO ledger_events VALUES(3,'event_followup_receipt',4,?,'','')",('f'*64,))
        self.db.execute("INSERT INTO ledger_events VALUES(4,'event_outcome',1,?,'','')",('e'*64,))
        signed=payload('outcome',['observer','synthetic-followup-receipt-v1','sese-followup:4:'+('f'*64)],b'\x01',timestamp=25)
        self.db.execute("UPDATE event_attestation_receipts SET outcome_payload=?",(signed,))
        self.assertEqual(verify_signed_followup_links(self.db), (1,0))
        bad=signed.replace(b'f'*64,b'0'*64)
        self.db.execute("UPDATE event_attestation_receipts SET outcome_payload=?",(bad,))
        with self.assertRaisesRegex(ValueError,'follow-up'):
            verify_signed_followup_links(self.db)
        self.db.execute("UPDATE event_attestation_receipts SET outcome_payload=?",(signed,))
        self.db.execute("UPDATE event_followup_receipts SET status='verdict_mismatch'")
        with self.assertRaisesRegex(ValueError,'follow-up'):
            verify_signed_followup_links(self.db)
        self.db.execute("UPDATE event_followup_receipts SET status='match'")
        self.db.execute("UPDATE event_followup_receipts SET review_id=2")
        with self.assertRaises(ValueError): verify_signed_followup_links(self.db)
        self.db.execute("UPDATE event_followup_receipts SET review_id=1,created_unix_ns='26'")
        with self.assertRaisesRegex(ValueError,'follow-up'):
            verify_signed_followup_links(self.db)
        self.db.execute("UPDATE event_followup_receipts SET created_unix_ns='22'")
        self.db.execute("UPDATE ledger_events SET seq=5 WHERE kind='event_followup_receipt'")
        with self.assertRaisesRegex(ValueError,'follow-up'):
            verify_signed_followup_links(self.db)

    def test_unrelated_method_is_counted_as_not_checked(self):
        self.db.execute("CREATE TABLE event_followup_receipts(id INTEGER,review_id INTEGER,status TEXT,created_unix_ns TEXT)")
        self.assertEqual(verify_signed_followup_links(self.db), (0,1))

    def test_wrong_review_reference_fails(self):
        bad=self.trace.replace(b'a'*64,b'e'*64,1)
        self.db.execute("UPDATE event_attestation_receipts SET trace_payload=?",(bad,))
        with self.assertRaisesRegex(ValueError,'reference mismatch'):
            verify_attestation_payload_record_links(self.db)

if __name__ == '__main__': unittest.main()
