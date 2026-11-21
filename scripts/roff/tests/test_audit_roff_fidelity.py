"""Source-reader injection checks for fidelity orchestration, without renderers."""
import contextlib
import gzip
import hashlib
import io
import json
import lzma
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import call, patch

from scripts.roff.audit import audit_roff_fidelity as AUDIT
from scripts.roff.lib.fidelity import sources


class FidelityDriverTests(unittest.TestCase):
    @unittest.skipUnless(hasattr(os, "mkfifo"), "requires named pipes")
    def test_verified_source_reader_rejects_pipe_symlink_before_reading(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pipe = root / "pipe"
            os.mkfifo(pipe)
            alias = root / "page.1"
            alias.symlink_to(pipe)
            result = subprocess.run(
                [sys.executable, "-c",
                 "from pathlib import Path; import sys; "
                 "from scripts.roff.lib.roff_audit_sources import read_verified_manual; "
                 "print(read_verified_manual(Path(sys.argv[1])))", str(alias)],
                capture_output=True, text=True, timeout=3, check=True,
            )
            self.assertEqual(result.stdout.strip(), "(None, 'manual source is not a regular file')")

    def test_unavailable_source_does_not_run_either_renderer(self):
        page = Path("/absent-fidelity-review/man1/page.1")
        with patch.object(AUDIT, "read_verified_manual", return_value=(None, "broken gzip")) as reader, \
             patch.object(AUDIT, "run_renderer") as render:
            artifact = AUDIT.audit_page(
                page, "page.1", [page.parent.parent], Path("mant"), "man", "man", 1, 4, False,
            )
        self.assertEqual(artifact.finding.status, "hard-failure")
        self.assertIn("broken gzip", artifact.finding.detail)
        self.assertIsNone(artifact.source)
        reader.assert_called_once_with(page)
        render.assert_not_called()

    def test_absent_source_uses_facade_reader_for_both_renderer_inputs(self):
        source = b"Alpha beta gamma delta\n"
        output = "Alpha beta gamma delta\n"
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            page = root / "man1/absent.1"
            self.assertFalse(page.exists())
            with patch.object(AUDIT, "read_verified_manual", return_value=(source, None)) as reader, \
                 patch.object(sources, "source_bytes", side_effect=AssertionError("helper reader used")), \
                 patch.object(AUDIT, "reference_environment", return_value={}), \
                 patch.object(AUDIT, "run_renderer", side_effect=[(0, output, ""), (0, output, "")]) as render:
                artifact = AUDIT.audit_page(
                    page, "absent.1", [root], Path("mant"), "mandoc", "mandoc", 1, 4, False,
                )
            self.assertEqual(artifact.finding.status, "clean")
            self.assertEqual(artifact.source, source)
            self.assertEqual(reader.call_args_list, [call(page), call(page)])
            self.assertEqual(render.call_args_list[0].args[0][1:3], ["--input", str(page)])
            self.assertEqual(render.call_args_list[1].args[3], source)

    def test_redirect_uses_initial_facade_bytes_and_reads_resolved_target(self):
        output = "Alpha beta gamma delta\n"
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            alias = root / "man1/alias.1"
            target = root / "man1/target.1"
            target.parent.mkdir()
            target.touch()
            inputs = {alias: b".so man1/target.1\n", target: output.encode()}
            alias.write_bytes(inputs[alias])
            with patch.object(AUDIT, "read_verified_manual", side_effect=lambda path: (inputs[path], None)) as reader, \
                 patch.object(sources, "source_bytes", side_effect=AssertionError("helper reader used")), \
                 patch.object(AUDIT, "reference_environment", return_value={}), \
                 patch.object(AUDIT, "run_renderer", side_effect=[(0, output, ""), (0, output, "")]) as render:
                artifact = AUDIT.audit_page(
                    alias, "alias.1", [root], Path("mant"), "mandoc", "mandoc", 1, 4, False,
                )
            self.assertEqual(artifact.finding.status, "clean")
            self.assertEqual(reader.call_args_list, [call(alias), call(target), call(target)])
            self.assertEqual(render.call_args_list[0].args[0][1], "manual/1/alias")
            self.assertEqual(render.call_args_list[0].args[2], {
                "MANT_MANPATH": str(root), "MANPATH": str(root),
            })
            self.assertEqual(render.call_args_list[1].args[3], inputs[target])

    def test_reference_command_remains_replaceable_on_driver_facade(self):
        page = Path("/absent/man1/page.1")
        with patch.object(AUDIT, "read_verified_manual", return_value=(b"opaque source", None)), \
             patch.object(AUDIT, "reference_render_command", return_value=(None, None, "injected skip")) as command, \
             patch.object(AUDIT, "run_renderer") as render:
            artifact = AUDIT.audit_page(
                page, "page.1", [page.parent.parent], Path("mant"), "mandoc", "mandoc", 1, 4, False,
            )
        self.assertEqual(artifact.finding.status, "skipped")
        self.assertEqual(artifact.finding.detail, "injected skip")
        self.assertIs(command.call_args.kwargs["source_reader"], AUDIT.verified_reference_source)
        render.assert_not_called()

    def test_invalid_transport_cannot_reuse_or_replace_a_verified_history(self):
        source = b".TH PROBE 1\n.SH DESCRIPTION\nVisible evidence.\n"
        packed = gzip.compress(source)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            page = root / "page.1.gz"
            page.write_bytes(packed)
            database, report = root / "history.csv", root / "report.json"
            args = [
                "--manpath", str(root), "--audit-db", str(database), "--json", str(report),
                "--source-pattern", "Visible evidence", "--reference-kind", "man",
            ]
            clean = AUDIT.AuditArtifact(AUDIT.Finding("page.1.gz", "clean"), source, None, None)
            with patch.object(AUDIT, "validate_tools"), patch.object(AUDIT, "audit_page", return_value=clean) as audit:
                self.assertEqual(AUDIT.main(args), 0)
                accepted = database.read_bytes()
                page.write_bytes(packed + bytes(4))
                for extra in ([], ["--recheck-recorded"], ["--recorded-only"]):
                    self.assertEqual(AUDIT.main([*args, *extra]), 1)
                    self.assertEqual(database.read_bytes(), accepted)
                    findings = json.loads(report.read_text())["findings"]
                    self.assertEqual(len(findings), 1)
                    self.assertEqual(findings[0]["status"], "hard-failure")
                    self.assertIn("could not read source", findings[0]["detail"])
                    self.assertEqual(len(json.loads(report.read_text())["uncovered_sources"]), 1)
                page.write_bytes(packed)
                self.assertEqual(AUDIT.main(args), 0)
                self.assertEqual(database.read_bytes(), accepted)
                self.assertEqual(json.loads(report.read_text())["findings"], [])
                self.assertEqual(audit.call_count, 1)

    def test_bounded_ledger_rotates_without_inventing_a_digest_for_invalid_input(self):
        source = b".TH PROBE 1\n.SH DESCRIPTION\nVisible evidence.\n"
        for option in ("--max-pages", "--max-pages-per-section"):
            for suffix, data in (("xz", lzma.compress(source)), ("gz", gzip.compress(source) + bytes(4))):
                with (self.subTest(option=option, suffix=suffix),
                      tempfile.TemporaryDirectory() as temporary,
                      contextlib.redirect_stdout(io.StringIO()),
                      contextlib.redirect_stderr(io.StringIO())):
                    root = Path(temporary)
                    bad, good = root / f"damaged.1.{suffix}", root / "healthy.1"
                    bad.write_bytes(data)
                    good.write_bytes(source)
                    database, report = root / "audit.csv", root / "latest.json"
                    section = "\0" + "1" if option.endswith("per-section") else ""

                    def rank(seed, path):
                        return hashlib.sha256(f"{seed}{section}\0{path.as_posix()}".encode()).digest()

                    seed = next(seed for seed in range(10_000) if rank(seed, bad) < rank(seed, good))
                    args = ["--manpath", str(root), "--audit-db", str(database), "--json", str(report),
                            option, "1", "--seed", str(seed), "--reference-kind", "man"]
                    clean = AUDIT.AuditArtifact(AUDIT.Finding(good.name, "clean"), source, None, None)
                    with (patch.object(AUDIT, "validate_tools"),
                          patch.object(AUDIT, "audit_page", return_value=clean) as audit):
                        self.assertEqual(AUDIT.main(args), 1)
                        self.assertEqual(AUDIT.main(args), 0)
                        self.assertEqual(AUDIT.main(args), 1)
                        audit.assert_called_once()
                        self.assertEqual(audit.call_args.args[0], good)
                    self.assertEqual([Path(row.path).name for row in AUDIT.read_audit_database(
                        database, "man", "man",
                    ).values()], [good.name])

    def test_reference_resolves_verified_alias_chains_inside_the_hierarchy(self):
        # CVS read.c ROFF_SO, with ASCII and UTF-8, opens the compressed
        # terminal source through either one or two static redirects.
        source = b".TH PROBE 1\n.SH DESCRIPTION\nAuthentic leaf token.\n"
        packed = gzip.compress(source)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "man1").mkdir()
            alias, intermediate, terminal = (root / "man1" / name
                                             for name in ("a.1", "b.1", "leaf.1.gz"))
            terminal.write_bytes(packed)
            intermediate.write_bytes(b".so man1/leaf.1\n")

            def reference(raw):
                alias.write_bytes(raw)
                return sources.reference_render_command(alias, "mandoc", "mandoc", root, raw)

            for raw in (b".so man1/leaf.1\n", b".so man1/b.1\n",
                        b'. soquiet\tman1/leaf.1 \\" trailing\r\n'):
                command, data, reason = reference(raw)
                self.assertIsNotNone(command)
                self.assertEqual(data, source)
                self.assertIsNone(reason)
            for raw, reason in ((b".so man1/a.1\n", "cycle"),
                                (b".so ../outside.1\n", "safe relative path")):
                command, data, detail = reference(raw)
                self.assertIsNone(command)
                self.assertIsNone(data)
                self.assertIn(reason, detail)
            raw = b".so man1/b.1\n"
            terminal.unlink()
            self.assertIn("absent", reference(raw)[2])
            terminal.write_bytes(packed + bytes(4))
            self.assertIn("transport", reference(raw)[2])
            terminal.write_bytes(packed)
            self.assertEqual(reference(raw)[1], source)

    def test_incremental_fidelity_revisits_unchanged_alias_source(self):
        source = b".TH PROBE 1\n.SH DESCRIPTION\nAuthentic leaf token.\n"
        with (tempfile.TemporaryDirectory() as temporary,
              contextlib.redirect_stdout(io.StringIO())):
            root = Path(temporary)
            (root / "man1").mkdir()
            alias, terminal = root / "man1/a.1", root / "man1/leaf.1"
            alias.write_bytes(b".so man1/leaf.1\n")
            terminal.write_bytes(source)
            database, report = root / "audit.csv", root / "latest.json"
            args = ["--manpath", str(root), "--audit-db", str(database), "--json", str(report),
                    "--source-pattern", r"^\.so", "--reference-kind", "man"]
            output = "Authentic leaf token.\n"

            def render(*_args):
                return (0, output, "") if terminal.exists() else (1, "", "redirect target missing")

            with patch.object(AUDIT, "validate_tools"), patch.object(AUDIT, "run_renderer", side_effect=render):
                for expected in ("clean", "hard-failure", "clean"):
                    self.assertEqual(AUDIT.main(args), int(expected == "hard-failure"))
                    findings = json.loads(report.read_text())["findings"]
                    self.assertEqual([(Path(row["path"]).name, row["status"]) for row in findings],
                                     [(alias.name, expected)])
                    if expected == "clean":
                        terminal.unlink()
                    else:
                        terminal.write_bytes(source)
            self.assertEqual(len(AUDIT.read_audit_database(database, "man", "man")), 1)


if __name__ == "__main__":
    unittest.main()
