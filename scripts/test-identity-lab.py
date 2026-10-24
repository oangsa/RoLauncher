"""Portable tests only; never opens CNG or changes Windows principals/ACLs."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("identity_lab", Path(__file__).with_name("identity-lab.py"))
lab = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lab)


class RecoveryAccounting(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture_root = Path(__file__).resolve().parent.parent / "target" / "identity-lab-fixtures"
        cls.fixture_root.mkdir(parents=True, exist_ok=True)

    def receipt(self, root, name="receipt.jsonl", **changes):
        record = {"schema_version": 1,
                  "key": {"name": "RoLauncher.IdentityPoC.12345678-1234-4234-8234-123456789abc",
                          "provider": "software", "scope": "machine", "blob": [1, 2]},
                  "owner_sid_reference": [1] * 32, "phase": "finalize_attempted",
                  "cleanup": "incomplete", "native_status": 0x80090016}
        record.update(changes)
        path = root / name
        path.write_text(json.dumps(record) + "\n", encoding="utf-8")
        return path

    def test_incomplete_and_torn_receipts_block_reuse_without_native_operations(self):
        with tempfile.TemporaryDirectory(dir=self.fixture_root) as directory:
            root = Path(directory)
            self.receipt(root)
            (root / "torn.jsonl").write_text("{torn", encoding="utf-8")
            report, _ = lab.account([root])
            self.assertEqual(report["incomplete"], 2)
            self.assertTrue(report["reuse_blocked"])
            self.assertFalse(report["native_key_operations"])

    def test_changed_identity_and_public_evidence_remain_incomplete(self):
        with tempfile.TemporaryDirectory(dir=self.fixture_root) as directory:
            root = Path(directory)
            path = self.receipt(root, cleanup="succeeded", native_status=0)
            original = path.read_text(encoding="utf-8")
            changed = json.loads(original)
            changed["key"]["blob"] = [9, 9]
            path.write_text(original + json.dumps(changed) + "\n", encoding="utf-8")
            report, _ = lab.account([root])
            self.assertEqual(report["incomplete"], 1)
            self.assertEqual(report["cleanup_succeeded"], 0)

    def test_manifest_is_exclusive_and_detects_changes_without_exposing_identifiers(self):
        with tempfile.TemporaryDirectory(dir=self.fixture_root) as directory:
            root = Path(directory)
            path = self.receipt(root, cleanup="succeeded", native_status=0)
            report, records = lab.account([root, root])
            self.assertEqual(report["receipt_count"], 1)
            manifest = root / "local-baseline.json"
            lab.save_baseline(manifest, records)
            with self.assertRaises(FileExistsError):
                lab.save_baseline(manifest, [])
            self.assertEqual(lab.compare_baseline(manifest)["changed_or_missing"], 0)
            path.write_text("{torn", encoding="utf-8")
            comparison = lab.compare_baseline(manifest)
            self.assertEqual(comparison["changed_or_missing"], 1)
            self.assertNotIn(str(path), json.dumps(comparison))

    def test_source_isolation_and_hosted_runner_boundary(self):
        self.assertEqual(lab.check_sources(Path(__file__).resolve().parent.parent), [])

    def test_ambiguous_native_status_cannot_be_counted_as_confirmed_deletion(self):
        with tempfile.TemporaryDirectory(dir=self.fixture_root) as directory:
            root = Path(directory)
            self.receipt(root, cleanup="succeeded", native_status=0x80090016)
            report, _ = lab.account([root])
            self.assertEqual(report["cleanup_succeeded"], 0)
            self.assertEqual(report["incomplete"], 1)
            self.assertTrue(report["reuse_blocked"])


if __name__ == "__main__":
    unittest.main()
