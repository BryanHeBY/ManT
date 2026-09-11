#!/usr/bin/env python3
"""Offline regression tests for the maintainer-only vendor replay boundary."""

import io
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import sync_vendor as vendor


class VendorReplayTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="mant-vendor-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "upstream").mkdir()
        (self.root / "patches").mkdir()

    def source(self, text):
        path = self.root / "upstream" / "SOURCE"
        path.write_text(text)
        return vendor.read_source(path)

    def archive(self, name="mandoc-1.0/source.c", kind=tarfile.REGTYPE):
        path = self.root / "source.tar.gz"
        with tarfile.open(path, "w:gz") as archive:
            entry = tarfile.TarInfo(name)
            entry.type = kind
            entry.linkname = "outside"
            entry.size = 6 if kind == tarfile.REGTYPE else 0
            archive.addfile(entry, io.BytesIO(b"before") if entry.size else None)
        return path

    def release(self, archive):
        return self.source(f"kind = release\nversion = 1.0\nurl = https://example.test/source.tar.gz\n"
                           f"sha256 = {vendor.sha256(archive)}\n")

    def cvs_source(self):
        tree = self.root / "mandoc"
        tree.mkdir()
        (tree / "source.c").write_bytes(b"before")
        manifest = self.root / "upstream" / "FILES"
        manifest.write_text(f"{vendor.sha256(tree / 'source.c')}\t1.2\tsource.c\n")
        source = self.source(
            "kind = cvs\nversion = cvs-20260911\n"
            "cvsroot = :ext:anoncvs@mandoc.bsd.lv:/cvs\nmodule = mandoc\n"
            "date = 2026-09-11 08:00:00 UTC\nroot = mandoc\nmanifest = FILES\n"
            f"manifest_sha256 = {vendor.sha256(manifest)}\n")
        return tree, manifest, source

    def test_source_requires_closed_fields_and_immutable_identity(self):
        archive = self.archive()
        source = self.release(archive)
        self.assertEqual(source["root"], "mandoc-1.0")
        text = (self.root / "upstream" / "SOURCE").read_text()
        for bad in [text + "kind = cvs\n", text + "typo = value\n",
                    text.replace("1.0", "../elsewhere"), text.replace("https:", "http:")]:
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                self.source(bad)
        _, _, source = self.cvs_source()
        text = (self.root / "upstream" / "SOURCE").read_text()
        for date in ["HEAD", "2026-09-11", "2026-19-11 08:00:00 UTC"]:
            with self.subTest(date=date), self.assertRaises(ValueError):
                self.source(text.replace(source["date"], date))

    def test_archive_rejects_wrong_root_traversal_and_links(self):
        for name, kind in [("elsewhere/file", tarfile.REGTYPE),
                           ("mandoc-1.0/../outside", tarfile.REGTYPE),
                           ("/mandoc-1.0/file", tarfile.REGTYPE),
                           ("mandoc-1.0/link", tarfile.SYMTYPE),
                           ("mandoc-1.0/link", tarfile.LNKTYPE),
                           ("mandoc-1.0/pipe", tarfile.FIFOTYPE)]:
            with self.subTest(name=name, kind=kind), self.assertRaises(ValueError):
                vendor.extract_archive(self.archive(name, kind), self.root / "unpack", "mandoc-1.0")
        self.assertFalse((self.root / "unpack").exists())

    def test_archive_rejects_duplicate_members_before_extraction(self):
        archive = self.root / "duplicate.tar"
        with tarfile.open(archive, "w") as stream:
            for _ in range(2):
                entry = tarfile.TarInfo("mandoc-1.0/source.c")
                entry.size = 1
                stream.addfile(entry, io.BytesIO(b"x"))
        with self.assertRaisesRegex(ValueError, "duplicate archive member"):
            vendor.extract_archive(archive, self.root / "unpack", "mandoc-1.0")
        self.assertFalse((self.root / "unpack").exists())

    def test_archive_rejects_windows_special_paths_before_extraction(self):
        for suffix in ["C:/escaped.c", "source.c:stream", "NUL", "CON.c",
                       "sub/LPT1.txt", "COM¹.c", "trailing.", "trailing ",
                       "bad\\name", "bad?name", "bad\x01name"]:
            with self.subTest(suffix=suffix), self.assertRaises(ValueError):
                vendor.extract_archive(self.archive("mandoc-1.0/" + suffix),
                                       self.root / "unpack", "mandoc-1.0")
            self.assertFalse((self.root / "unpack").exists())

    def test_archive_rejects_case_collisions_in_files_and_implicit_directories(self):
        for names in [("Source.c", "source.c"), ("Sub/one.c", "sub/two.c")]:
            archive = self.root / "case-collision.tar"
            with tarfile.open(archive, "w") as stream:
                for name in names:
                    entry = tarfile.TarInfo("mandoc-1.0/" + name)
                    entry.size = 1
                    stream.addfile(entry, io.BytesIO(b"x"))
            with self.subTest(names=names), self.assertRaisesRegex(ValueError, "case-colliding"):
                vendor.extract_archive(archive, self.root / "unpack", "mandoc-1.0")
            self.assertFalse((self.root / "unpack").exists())

    def test_release_checksum_is_checked_before_extraction(self):
        archive = self.archive()
        source = self.release(archive)
        archive.write_bytes(b"modified")
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            vendor.acquire(self.root, source, self.root / "work", archive)
        self.assertFalse((self.root / "work").exists())

    def test_manifest_checks_inventory_contents_and_available_cvs_revisions(self):
        tree, manifest, source = self.cvs_source()
        entries = vendor.read_manifest(manifest, source["manifest_sha256"])
        vendor.verify_cvs_tree(tree, entries)
        (tree / "CVS").mkdir()
        (tree / "CVS" / "Entries").write_text("/source.c/1.2/date//\n")
        vendor.verify_cvs_tree(tree, entries)
        (tree / "CVS" / "Entries").write_text("/source.c/1.3/date//\n")
        with self.assertRaisesRegex(ValueError, "revision mismatch"):
            vendor.verify_cvs_tree(tree, entries)
        (tree / "CVS" / "Entries").unlink()
        (tree / "source.c").write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "content checksum mismatch"):
            vendor.verify_cvs_tree(tree, entries)
        (tree / "source.c").write_bytes(b"before")
        (tree / "unexpected.c").write_bytes(b"extra")
        with self.assertRaisesRegex(ValueError, "inventory mismatch"):
            vendor.verify_cvs_tree(tree, entries)
        with self.assertRaisesRegex(ValueError, "manifest checksum mismatch"):
            vendor.read_manifest(manifest, "0" * 64)

    def test_manifest_rejects_duplicate_and_escaping_paths(self):
        _, manifest, _ = self.cvs_source()
        original = manifest.read_text()
        for text in [original + original, original.replace("source.c", "../source.c"),
                     original.replace("1.2", "HEAD"), ""]:
            manifest.write_text(text)
            with self.subTest(text=text), self.assertRaises(ValueError):
                vendor.read_manifest(manifest, vendor.sha256(manifest))

    def test_manifest_requires_portable_paths_without_case_aliases(self):
        _, manifest, _ = self.cvs_source()
        original = manifest.read_text()
        for name in ["C:/source.c", "source.c:stream", "NUL.c", "trailing.", "trailing "]:
            manifest.write_text(original.replace("source.c", name))
            with self.subTest(name=name), self.assertRaises(ValueError):
                vendor.read_manifest(manifest, vendor.sha256(manifest))
        for names in [("Source.c", "source.c"), ("Sub/one.c", "sub/two.c")]:
            manifest.write_text("".join(original.replace("source.c", name) for name in names))
            with self.subTest(names=names), self.assertRaisesRegex(ValueError, "case-colliding"):
                vendor.read_manifest(manifest, vendor.sha256(manifest))
        for name in ["Makefile", "Makefile.depend", "compat_strlcpy.c", "configure.local.example"]:
            manifest.write_text(original.replace("source.c", name))
            self.assertIn(name, vendor.read_manifest(manifest, vendor.sha256(manifest)))

    def test_cvs_archive_replay_is_offline_and_removes_metadata(self):
        tree, _, source = self.cvs_source()
        (tree / "regress").mkdir()
        (tree / "regress" / "unused").write_text("not vendored")
        archive = self.root / "cvs.tar.gz"
        with tarfile.open(archive, "w:gz") as stream:
            stream.add(tree, arcname="mandoc")
        work = self.root / "work"
        work.mkdir()
        with patch.object(vendor.subprocess, "run") as command:
            staged = vendor.acquire(self.root, source, work, archive)
        command.assert_not_called()
        self.assertEqual(set(vendor.source_files(staged)), {"source.c"})
        self.assertFalse((staged / "regress").exists())

    def test_live_cvs_uses_fixed_date_and_pinned_ssh_wrapper(self):
        tree, _, source = self.cvs_source()
        work = self.root / "work"
        work.mkdir()

        def checkout(argv, *, cwd, env, check):
            self.assertEqual(argv, ["custom-cvs", "-Q", "-d", source["cvsroot"], "checkout",
                                    "-D", source["date"], "-d", "mandoc", "mandoc"])
            self.assertEqual(env["CVS_RSH"], str(self.root / "scripts" / "cvs-ssh"))
            self.assertTrue(check)
            shutil.copytree(tree, cwd / "mandoc")

        with patch.dict(os.environ, {"CVS": "custom-cvs"}), \
                patch.object(vendor.subprocess, "run", side_effect=checkout):
            staged = vendor.acquire(self.root, source, work, None)
        self.assertEqual((staged / "source.c").read_bytes(), b"before")

    @unittest.skipUnless(shutil.which("patch"), "patch utility unavailable")
    def test_already_applied_patch_fails_without_reversing_source(self):
        staged = self.root / "staged"
        staged.mkdir()
        (staged / "source.c").write_bytes(b"after\n")
        (self.root / "patches" / "series").write_text("0001.patch\n")
        (self.root / "patches" / "0001.patch").write_text(
            "--- a/source.c\n+++ b/source.c\n@@ -1 +1 @@\n-before\n+after\n")
        with self.assertRaises(subprocess.CalledProcessError):
            vendor.apply_patches(self.root, staged)
        self.assertEqual((staged / "source.c").read_bytes(), b"after\n")

    def test_duplicate_patch_series_fails_before_any_patch_runs(self):
        (self.root / "patches" / "series").write_text(
            "# ordered fixes\n0001.patch\n\n  0001.patch  \n")
        with patch.object(vendor.subprocess, "run") as command:
            with self.assertRaisesRegex(ValueError, "duplicate patch in series"):
                vendor.apply_patches(self.root, self.root / "staged")
        command.assert_not_called()

    @unittest.skipUnless(shutil.which("patch"), "patch utility unavailable")
    def test_release_patch_replay_and_transactional_install(self):
        archive = self.archive()
        source = self.release(archive)
        work = self.root / "work"
        work.mkdir()
        staged = vendor.acquire(self.root, source, work, archive)
        (self.root / "patches" / "series").write_text("# ordered fixes\n0001.patch\n")
        (self.root / "patches" / "0001.patch").write_text(
            "--- a/source.c\n+++ b/source.c\n@@ -1 +1 @@\n-before\n"
            "\\ No newline at end of file\n+after\n\\ No newline at end of file\n")
        vendor.apply_patches(self.root, staged)
        destination = self.root / "vendor" / "mandoc-1.0"
        destination.mkdir(parents=True)
        (destination / "previous.c").write_bytes(b"old")
        vendor.install_tree(staged, destination)
        self.assertEqual(vendor.compare_trees(staged, destination), [])
        (destination / "source.c").write_bytes(b"drift")
        self.assertEqual(vendor.compare_trees(staged, destination), ["source.c"])

    def test_failed_activation_restores_previous_vendor_tree(self):
        staged = self.root / "staged"
        staged.mkdir()
        (staged / "new.c").write_bytes(b"new")
        destination = self.root / "vendor" / "mandoc-1.0"
        destination.mkdir(parents=True)
        (destination / "old.c").write_bytes(b"old")
        rename = Path.rename

        def fail_activation(path, target):
            if path.name == "replacement":
                raise OSError("injected activation failure")
            return rename(path, target)

        with patch.object(Path, "rename", fail_activation), \
                self.assertRaisesRegex(OSError, "activation failure"):
            vendor.install_tree(staged, destination)
        self.assertEqual(set(vendor.source_files(destination)), {"old.c"})
        self.assertEqual((destination / "old.c").read_bytes(), b"old")


if __name__ == "__main__":
    unittest.main()
