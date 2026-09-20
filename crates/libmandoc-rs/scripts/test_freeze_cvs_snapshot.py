#!/usr/bin/env python3
"""Offline regression tests for the maintainer-only CVS snapshot freezer."""

from datetime import datetime, timezone
import json
import os
from pathlib import Path
import shutil
import stat
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import freeze_cvs_snapshot as freezer


class SnapshotFreezeTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="mant-freeze-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "target").mkdir()
        scripts = self.root / "crates/libmandoc-rs/scripts"
        scripts.mkdir(parents=True)
        (scripts / "cvs-ssh").write_text("#!/bin/sh\nexit 99\n")
        (scripts / "cvs-ssh").chmod(0o755)
        upstream = self.root / "crates/libmandoc-rs/upstream"
        upstream.mkdir()
        (upstream / "FILES").write_text(f"{'0' * 64}\t1.1\told.c\n")
        self.fake_cvs = self.root / "fake-cvs"
        self.fake_cvs.write_text(
            "#!/usr/bin/env python3\n"
            "import os, pathlib, shutil, sys\n"
            "if sys.argv[1:] == ['--version']:\n"
            " print('fake CVS 1.0'); raise SystemExit\n"
            "counter = pathlib.Path(os.environ['FAKE_CVS_COUNTER'])\n"
            "number = int(counter.read_text()) if counter.exists() else 0\n"
            "counter.write_text(str(number + 1))\n"
            "source = pathlib.Path(os.environ['FAKE_CVS_SOURCE_A' if number == 0 else 'FAKE_CVS_SOURCE_B'])\n"
            "index = sys.argv.index('checkout')\n"
            "destination = pathlib.Path.cwd() / sys.argv[index + 4]\n"
            "shutil.copytree(source, destination)\n"
        )
        self.fake_cvs.chmod(self.fake_cvs.stat().st_mode | stat.S_IXUSR)
        self.first = self.root / "fixture-a"
        self.second = self.root / "fixture-b"
        self.make_checkout(self.first)
        shutil.copytree(self.first, self.second)
        self.counter = self.root / "counter"
        self.cutoff = datetime(2026, 9, 20, 12, 34, 56, tzinfo=timezone.utc)

    def make_checkout(self, root: Path, source=b"source\n", revision="1.2") -> None:
        for relative in (Path(), Path("sub"), Path("regress")):
            directory = root / relative
            (directory / "CVS").mkdir(parents=True)
        (root / "source.c").write_bytes(source)
        (root / "sub/item.txt").write_text("item\n")
        (root / "regress/case.in").write_text("case\n")
        (root / "CVS/Entries").write_text(
            f"/source.c/{revision}/Thu Sep 20 12:00:00 2026//\n"
            "D/sub////\nD/regress////\nD\n"
        )
        (root / "sub/CVS/Entries").write_text(
            "/item.txt/1.4/Thu Sep 20 12:00:00 2026//\nD\n"
        )
        (root / "regress/CVS/Entries").write_text(
            "/case.in/1.7/Thu Sep 20 12:00:00 2026//\nD\n"
        )

    def environment(self):
        return patch.dict(os.environ, {
            "FAKE_CVS_COUNTER": str(self.counter),
            "FAKE_CVS_SOURCE_A": str(self.first),
            "FAKE_CVS_SOURCE_B": str(self.second),
        })

    def run_freeze(self, suffix: str):
        self.counter.unlink(missing_ok=True)
        work = self.root / "target" / f"work-{suffix}"
        output = self.root / "target" / f"output-{suffix}"
        with self.environment():
            report = freezer.freeze(
                self.root, str(self.fake_cvs), self.cutoff, work, output
            )
        return report, work, output

    def test_freeze_is_repeatable_and_separates_regress_from_shipping_manifest(self):
        first, _, output_a = self.run_freeze("a")
        second, _, output_b = self.run_freeze("b")
        self.assertEqual(first["archiveSha256"], second["archiveSha256"])
        self.assertEqual((output_a / "FILES").read_bytes(), (output_b / "FILES").read_bytes())
        inventory = json.loads((output_a / "CVS_INVENTORY.json").read_text())
        self.assertEqual(inventory["shippingCount"], 2)
        self.assertEqual(inventory["regressCount"], 1)
        self.assertIn("regress/case.in", inventory["files"])
        self.assertNotIn("regress/case.in", (output_a / "FILES").read_text())
        self.assertIn("regress/case.in", (output_a / "REGRESS_FILES").read_text())
        with tarfile.open(output_a / first["archive"]) as archive:
            self.assertIn("mandoc/regress/case.in", archive.getnames())
        self.assertEqual(first["inventoryDiff"]["added"], ["source.c", "sub/item.txt"])
        self.assertEqual(first["inventoryDiff"]["removed"], ["old.c"])

    def test_independent_checkout_revision_or_content_drift_is_rejected(self):
        (self.second / "source.c").write_bytes(b"changed\n")
        entries = self.second / "CVS/Entries"
        entries.write_text(entries.read_text().replace("/1.2/", "/1.3/"))
        work = self.root / "target/work-drift"
        output = self.root / "target/output-drift"
        with self.environment(), self.assertRaisesRegex(ValueError, "checkouts disagree"):
            freezer.freeze(self.root, str(self.fake_cvs), self.cutoff, work, output)
        self.assertFalse(output.exists())
        self.assertTrue(work.exists())

    def test_inventory_rejects_missing_entries_untracked_files_and_links(self):
        cases = []
        missing = self.root / "missing"
        shutil.copytree(self.first, missing)
        (missing / "sub/CVS/Entries").unlink()
        cases.append(missing)
        untracked = self.root / "untracked"
        shutil.copytree(self.first, untracked)
        (untracked / "extra.c").write_text("extra")
        cases.append(untracked)
        linked = self.root / "linked"
        shutil.copytree(self.first, linked)
        (linked / "link.c").symlink_to("source.c")
        cases.append(linked)
        for checkout in cases:
            with self.subTest(checkout=checkout), self.assertRaises(ValueError):
                freezer.inventory(checkout)

    def test_inventory_rejects_case_collisions(self):
        checkout = self.root / "collision"
        shutil.copytree(self.first, checkout)
        (checkout / "Source.c").write_text("collision")
        entries = checkout / "CVS/Entries"
        entries.write_text(entries.read_text().replace(
            "D/sub////", "/Source.c/1.1/Thu Sep 20 12:00:00 2026//\nD/sub////"
        ))
        with self.assertRaisesRegex(ValueError, "case-colliding"):
            freezer.inventory(checkout)

    def test_existing_candidate_is_never_replaced(self):
        output = self.root / "target/existing"
        output.mkdir()
        marker = output / "keep"
        marker.write_text("original")
        with self.assertRaisesRegex(ValueError, "must not already exist"):
            freezer.freeze(
                self.root, str(self.fake_cvs), self.cutoff,
                self.root / "target/new-work", output,
            )
        self.assertEqual(marker.read_text(), "original")

    def test_paths_outside_repository_target_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "below"):
            freezer.freeze(
                self.root, str(self.fake_cvs), self.cutoff,
                self.root / "work", self.root / "target/output",
            )


if __name__ == "__main__":
    unittest.main()
