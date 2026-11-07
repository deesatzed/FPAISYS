import unittest

from verify_container_capture import verify_capture


class CaptureVerificationTests(unittest.TestCase):
    def test_checkpoint_must_cover_capture(self):
        with self.assertRaisesRegex(ValueError, "checkpoint"):
            verify_capture("/nonexistent", 2, 1, "0" * 64)


if __name__ == "__main__":
    unittest.main()
