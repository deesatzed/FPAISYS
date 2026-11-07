"""Independent synthetic signature checks; optional cryptography dependency."""
import importlib.util
import sqlite3
import unittest

from verify_ledger import load_signature_authorities, verify_attestation_signatures


@unittest.skipUnless(importlib.util.find_spec("cryptography"), "cryptography not installed")
class SyntheticSignatureTests(unittest.TestCase):
    def setUp(self):
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
        from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
        import hashlib
        import struct
        self.db = sqlite3.connect(":memory:")
        self.db.executescript("CREATE TABLE event_attestation_receipts(trace_authority TEXT,outcome_authority TEXT,trace_payload BLOB,outcome_payload BLOB,trace_signature BLOB,outcome_signature BLOB,authority_digest TEXT); CREATE TABLE event_preference_receipts(authority TEXT,payload BLOB,signature BLOB,authority_digest TEXT); PRAGMA user_version=37;")
        self.keys = [Ed25519PrivateKey.from_private_bytes(bytes([n])*32) for n in (11,12,13)]
        self.config = {"source":("trace-source", self.keys[0].public_key().public_bytes(Encoding.Raw,PublicFormat.Raw)),"observer":("outcome-observer", self.keys[1].public_key().public_bytes(Encoding.Raw,PublicFormat.Raw)),"reviewer":("preference-reviewer", self.keys[2].public_key().public_bytes(Encoding.Raw,PublicFormat.Raw))}
        self.pin = "a"*64
        def sign(key, name, role, payload):
            fields = [name.encode(),role.encode()]
            message = b"sese-evidence-attestation-v1\0" + b"".join(struct.pack(">I",len(f))+f for f in fields) + struct.pack(">Q",len(payload)) + hashlib.sha256(payload).digest()
            return key.sign(message)
        self.db.execute("INSERT INTO event_attestation_receipts VALUES(?,?,?,?,?,?,?)",("source","observer",b"trace",b"outcome",sign(self.keys[0],"source","trace-source",b"trace"),sign(self.keys[1],"observer","outcome-observer",b"outcome"),self.pin))
        self.db.execute("INSERT INTO event_preference_receipts VALUES(?,?,?,?)",("reviewer",b"pair",sign(self.keys[2],"reviewer","preference-reviewer",b"pair"),self.pin))

    def tearDown(self): self.db.close()

    def test_valid_and_changed_signed_payload(self):
        self.assertEqual(verify_attestation_signatures(self.db,self.config,self.pin),3)
        self.db.execute("UPDATE event_attestation_receipts SET trace_payload=?",(b"changed",))
        with self.assertRaisesRegex(ValueError,"signature"):
            verify_attestation_signatures(self.db,self.config,self.pin)

    def test_authority_role_and_configuration_are_bound(self):
        bad = dict(self.config)
        bad["source"] = ("outcome-observer",bad["source"][1])
        with self.assertRaises(ValueError): verify_attestation_signatures(self.db,bad,self.pin)
        with self.assertRaises(ValueError): verify_attestation_signatures(self.db,self.config,"b"*64)

    def test_pinned_authority_file_rejects_duplicates_and_excludes_revoked(self):
        import hashlib
        import json
        import tempfile
        from pathlib import Path
        values = [{"id": name, "role": role, "public_key": key.hex(), "revoked": name == "reviewer"} for name,(role,key) in self.config.items()]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "authorities.json"
            def load(items):
                data = json.dumps({"schema":"sese-authority-set-v1","authorities":items}).encode()
                path.write_bytes(data)
                return load_signature_authorities(path,hashlib.sha256(data).hexdigest())
            result = load(values)
            self.assertNotIn("reviewer",result)
            with self.assertRaises(ValueError): load(values + [values[0]])
            with self.assertRaises(ValueError): load_signature_authorities(path,"0"*64)

    def test_missing_preference_authority_fails(self):
        bad = dict(self.config)
        del bad["reviewer"]
        with self.assertRaises(ValueError): verify_attestation_signatures(self.db,bad,self.pin)


if __name__ == "__main__": unittest.main()
