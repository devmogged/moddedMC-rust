"""Behavioral tests for verify-buja-snapshot.py with throwaway fixture archives."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
import zipfile

SOURCE = Path(__file__).resolve().parents[1] / "verify-buja-snapshot.py"
spec = importlib.util.spec_from_file_location("verify_buja_snapshot", SOURCE)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)


class SnapshotAuditTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        (self.root / "mods").mkdir()
        self.entries = {"a.jar": b"abc-123", "b.jar": b"foo-bar"}
        for filename, content in self.entries.items():
            (self.root / "mods" / filename).write_bytes(content)
        with zipfile.ZipFile(self.root / "mods.zip", "w") as zf:
            for filename, content in self.entries.items():
                zf.writestr("mods/" + filename, content)
            zf.writestr("readme.txt", "non-mod archive entry")

    def tearDown(self):
        self.temp.cleanup()

    def test_identical_bytes_pass(self):
        report = module.verify(self.root, 2)
        self.assertEqual(report["count"], 2)
        self.assertEqual(len(report["files"]), 2)
        self.assertEqual(len(report["index_sha256"]), 64)

    def test_same_size_different_bytes_fail(self):
        (self.root / "mods" / "a.jar").write_bytes(b"xyz-123")
        with self.assertRaisesRegex(ValueError, "SHA-256 content mismatch"):
            module.verify(self.root, 2)

    def test_missing_jar_fails(self):
        (self.root / "mods" / "a.jar").unlink()
        with self.assertRaisesRegex(ValueError, "expected 2 loose JARs"):
            module.verify(self.root, 2)

    def test_filename_mismatch_fails(self):
        (self.root / "mods" / "a.jar").rename(self.root / "mods" / "different.jar")
        with self.assertRaisesRegex(ValueError, "mismatched names"):
            module.verify(self.root, 2)


if __name__ == "__main__":
    unittest.main()
