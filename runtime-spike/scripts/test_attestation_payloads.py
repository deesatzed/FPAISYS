"""Independent tests of the synthetic attestation binary format."""
import struct
import unittest

from verify_ledger import decode_synthetic_attestation


def payload(kind, fields, extra=b"", timestamp=42):
    prefix = f"sese-synthetic-{kind}-attestation-v1".encode() + b"\0"
    ref = struct.pack(">qq", 1, 2) + b"a" * 64 + b"b" * 64 + b"c" * 64
    text = b"".join(struct.pack(">I", len(x.encode())) + x.encode() for x in fields)
    return prefix + ref + extra + text + struct.pack(">Q", timestamp)


class AttestationPayloadTests(unittest.TestCase):
    def test_trace_and_outcome(self):
        trace = decode_synthetic_attestation(payload("trace", ["π", "response", "source", "method"]), "trace")
        self.assertEqual((trace["prompt"], trace["review_id"], trace["timestamp"]), ("π", 1, 42))
        outcome = decode_synthetic_attestation(payload("outcome", ["source", "method", "evidence"], b"\x02"), "outcome")
        self.assertEqual(outcome["status"], "falsified")

    def test_preference_and_invalid_encodings(self):
        extra = struct.pack(">q", 3) + b"d" * 64
        raw = payload("preference", ["prompt", "yes", "no", "source", "basis"], extra)
        self.assertEqual(decode_synthetic_attestation(raw, "preference")["trace_id"], 3)
        for bad in (raw[:-1], raw + b"x", raw.replace(b"yes", b"\xffes"),
                    payload("preference", ["prompt", "same", "same", "source", "basis"], extra),
                    payload("outcome", ["source", "method", "evidence"], b"\x03")):
            with self.assertRaises(ValueError):
                decode_synthetic_attestation(bad, "outcome" if bad.startswith(b"sese-synthetic-outcome") else "preference")


if __name__ == "__main__":
    unittest.main()
