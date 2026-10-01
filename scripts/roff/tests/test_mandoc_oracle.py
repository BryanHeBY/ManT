#!/usr/bin/env python3
"""Negative and positive tests for the trusted mandoc oracle preflight."""

import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from scripts.roff.oracle import mandoc_oracle
from scripts.roff.oracle import rebuild_reference_mandoc
class OracleIdentityTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="mant-oracle-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        for relative, contents in {
            "crates/libmandoc-rs/upstream/SOURCE": "source lock\n",
            "crates/libmandoc-rs/upstream/FILES": "manifest\n",
            "crates/libmandoc-rs/upstream/CVS_INVENTORY.json": "{}\n",
            "crates/libmandoc-rs/scripts/build_oracle.py": "recipe\n",
            "target/archive.tar.gz": "archive\n",
            "target/build/config.h": "config\n",
            "target/build/config.log": "log\n",
            "target/build/Makefile.local": "make\n",
            "target/oracle": "binary\n",
        }.items():
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(contents)
        self.binary = self.root / "target/oracle"
        self.archive = self.root / "target/archive.tar.gz"
        self.attestation = self.root / "attestation.json"
        registry = self.root / mandoc_oracle.REGISTRY
        registry.parent.mkdir(parents=True, exist_ok=True)
        self.value = mandoc_oracle.create_attestation(
            self.root,
            "cvs-test-linux",
            self.binary,
            "target/archive.tar.gz",
            "crates/libmandoc-rs/scripts/build_oracle.py",
            {
                "configHeader": "target/build/config.h",
                "configLog": "target/build/config.log",
                "makefileLocal": "target/build/Makefile.local",
            },
            ["./configure"],
            ["make", "mandoc"],
            "cc",
            "cc test version",
            ["ascii", "utf8"],
            "crates/libmandoc-rs/upstream/CVS_INVENTORY.json",
        )
        self.write_attestation()

    def write_attestation(self):
        self.attestation.write_text(json.dumps(self.value, sort_keys=True, indent=2) + "\n")
        registry = self.root / mandoc_oracle.REGISTRY
        registry.write_text(json.dumps({
            "schema": "mant.mandoc-oracle-registry/v1",
            "attestations": {
                self.value["identity"]: {
                    "path": str(self.attestation.relative_to(self.root)),
                    "sha256": mandoc_oracle.sha256(self.attestation),
                    "status": "active",
                }
            },
        }, sort_keys=True, indent=2) + "\n")

    def verify(self, **kwargs):
        return mandoc_oracle.preflight(
            self.root,
            kwargs.get("binary", self.binary),
            kwargs.get("archive", self.archive),
            kwargs.get("attestation", self.attestation),
            kwargs.get("identity", "cvs-test-linux"),
            kwargs.get("profile", "utf8"),
        )

    def test_preflight_accepts_the_exact_identity_chain(self):
        evidence = self.verify()
        self.assertEqual(evidence["identity"], "cvs-test-linux")
        self.assertEqual(evidence["binary"]["sha256"], self.value["artifact"]["sha256"])

    def test_swapped_binary_and_archive_are_rejected(self):
        swapped = self.root / "target/swapped"
        swapped.write_text("different")
        with self.assertRaisesRegex(ValueError, "binary"):
            self.verify(binary=swapped)
        with self.assertRaisesRegex(ValueError, "archive"):
            self.verify(archive=swapped)

    def test_wrong_snapshot_recipe_and_build_evidence_are_rejected(self):
        for relative, message in (
            ("crates/libmandoc-rs/upstream/SOURCE", "SOURCE"),
            ("crates/libmandoc-rs/upstream/FILES", "FILES"),
            ("crates/libmandoc-rs/scripts/build_oracle.py", "recipe"),
            ("target/build/config.h", "build evidence"),
        ):
            path = self.root / relative
            original = path.read_text()
            path.write_text("drift")
            with self.subTest(relative=relative), self.assertRaisesRegex(ValueError, message):
                self.verify()
            path.write_text(original)

    def test_identity_profile_platform_and_attestation_are_required(self):
        with self.assertRaisesRegex(ValueError, "identity"):
            self.verify(identity="another")
        with self.assertRaisesRegex(ValueError, "profile"):
            self.verify(profile="html")
        missing = self.root / "missing.json"
        with self.assertRaisesRegex(ValueError, "attestation"):
            self.verify(attestation=missing)
        self.value["platform"]["machine"] = "another-machine"
        self.write_attestation()
        with self.assertRaisesRegex(ValueError, "platform"):
            self.verify()

    def test_unknown_fields_and_false_pristine_claim_are_rejected(self):
        self.value["unexpected"] = True
        self.write_attestation()
        with self.assertRaisesRegex(ValueError, "unknown"):
            self.verify()
        self.value.pop("unexpected")
        self.value["source"]["pristine"] = False
        self.write_attestation()
        with self.assertRaisesRegex(ValueError, "pristine"):
            self.verify()

    def test_unregistered_or_moved_attestation_is_rejected(self):
        copied = self.root / "copied.json"
        copied.write_bytes(self.attestation.read_bytes())
        with self.assertRaisesRegex(ValueError, "trusted registration"):
            self.verify(attestation=copied)
        registry = self.root / mandoc_oracle.REGISTRY
        value = json.loads(registry.read_text())
        value["attestations"] = {}
        registry.write_text(json.dumps(value) + "\n")
        with self.assertRaisesRegex(ValueError, "not registered"):
            self.verify()

    def restoration_fixture(self):
        self.value["profiles"] = ["ascii", "html", "utf8"]
        self.value["recipe"]["buildCommand"] = ["make", "-j4", "mandoc"]
        output = self.root / "target/build"
        (output / "mandoc").write_bytes(self.binary.read_bytes())
        (output / "BUILD.json").write_text("locked build record\n")
        self.value["buildEvidence"]["buildRecord"] = mandoc_oracle.hash_record(
            self.root, "target/build/BUILD.json", "build record",
        )
        self.write_attestation()
        return self.root / rebuild_reference_mandoc.REFERENCE

    def test_restore_rechecks_and_replaces_an_existing_wrong_reference(self):
        reference = self.restoration_fixture()
        reference.parent.mkdir(parents=True)
        reference.write_text("patched vendor binary\n")
        with patch("scripts.roff.oracle.rebuild_reference_mandoc.subprocess.run") as build:
            result = rebuild_reference_mandoc.restore(self.root)
        build.assert_not_called()
        self.assertEqual(reference.read_bytes(), self.binary.read_bytes())
        self.assertEqual(result["profiles"], ["ascii", "html", "utf8"])

    def test_restore_rejects_recipe_drift_even_when_reference_exists(self):
        reference = self.restoration_fixture()
        reference.parent.mkdir(parents=True)
        reference.write_bytes(self.binary.read_bytes())
        (self.root / "crates/libmandoc-rs/scripts/build_oracle.py").write_text("changed recipe")
        with self.assertRaisesRegex(ValueError, "recipe"):
            rebuild_reference_mandoc.restore(self.root)

    def test_restore_checks_archive_before_accepting_reference(self):
        reference = self.restoration_fixture()
        reference.parent.mkdir(parents=True)
        reference.write_bytes(self.binary.read_bytes())
        self.archive.write_text("wrong source snapshot")
        with self.assertRaisesRegex(ValueError, "source archive"):
            rebuild_reference_mandoc.restore(self.root)

    def test_restore_can_copy_an_exact_saved_archive(self):
        reference = self.restoration_fixture()
        saved = self.root / "saved-pristine.tar.gz"
        self.archive.rename(saved)
        rebuild_reference_mandoc.restore(self.root, supplied_archive=saved)
        self.assertEqual(self.archive.read_bytes(), saved.read_bytes())
        self.assertEqual(reference.read_bytes(), self.binary.read_bytes())

    def test_restore_missing_archive_does_not_fall_back_to_vendor(self):
        self.restoration_fixture()
        self.archive.unlink()
        with patch("scripts.roff.oracle.rebuild_reference_mandoc.shutil.which", return_value=None):
            with self.assertRaisesRegex(ValueError, "provide --archive or a CVS client"):
                rebuild_reference_mandoc.restore(self.root)

    def test_restore_never_installs_an_unregistered_rebuild(self):
        reference = self.restoration_fixture()
        candidate = self.root / "target/build/mandoc"
        candidate.write_text("different build")

        def unregistered_build(*args, **kwargs):
            output = self.root / "target/build"
            output.mkdir()
            (output / "mandoc").write_text("unregistered candidate")
            (output / "attestation.json").write_text("{}")

        with patch("scripts.roff.oracle.rebuild_reference_mandoc.subprocess.run", side_effect=unregistered_build):
            with self.assertRaisesRegex(ValueError, "automatic registration is forbidden"):
                rebuild_reference_mandoc.restore(self.root)
        self.assertFalse(reference.exists())


if __name__ == "__main__":
    unittest.main()
