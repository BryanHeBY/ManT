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
from scripts.roff.lib.fidelity import text as fidelity_text


class FidelityDriverTests(unittest.TestCase):
    def test_table_source_scan_is_once_per_page(self):
        source = '.TH PROBE 1 "November 25, 2026" "Local"\n.SH DESCRIPTION\nAlpha Beta Gamma Delta\n'
        # The fixed CVS terminal output contains this body row (term.c).
        reference = '     Alpha Beta Gamma Delta\n' * 64
        search = fidelity_text.re.search
        with patch.object(fidelity_text.re, "search", wraps=search) as lookup:
            self.assertEqual(fidelity_text.token_lines(reference, source),
                             [["Alpha", "Beta", "Gamma", "Delta"]] * 64)
        scans = [call for call in lookup.call_args_list if call.args[1] is source]
        self.assertEqual(len(scans), 1)

    def test_footer_preserves_authored_os_and_removes_only_proven_footer(self):
        # CVS man_term.c and mdoc_term.c flush OS, date and title fields;
        # the fixed UTF-8 oracle produced these one-line and split footers.
        for family in ("man", "mdoc"):
            for os_name, footer in (
                ("Uniquehost", "Uniquehost  November 25, 2026 ALPROBE(1)"),
                ("Uniquehost Example Distribution",
                 "Uniquehost Example Distribution\n            November 25, 2026 ALPROBE(1)"),
            ):
                with self.subTest(family=family, os=os_name):
                    if family == "man":
                        source = (f'.TH ALPROBE 1 "November 25, 2026" "{os_name}" "Test Manual"\n'
                                  f'.SH DESCRIPTION\n{os_name}\n')
                    else:
                        source = (f'.Dd November 25, 2026\n.Dt ALPROBE 1\n.Os {os_name}\n'
                                  f'.Sh DESCRIPTION\n{os_name}\n')
                    reference = ("ALPROBE(1)     Test Manual    ALPROBE(1)\n\n"
                                 f"DESCRIPTION\n     {os_name}\n\n{footer}\n")
                    cleaned = fidelity_text.strip_reference_chrome(reference, source)
                    self.assertEqual(cleaned.count(os_name), 1)
                    self.assertNotIn("November", cleaned)
                    finding = lambda output: AUDIT.compare_rendered(
                        "probe", source.encode(), reference, output, "mandoc"
                    ).finding
                    self.assertNotIn("Uniquehost", finding(os_name).missing_tokens)
                    self.assertIn("Uniquehost", finding("Unrelated author text").missing_tokens)

    def test_table_frame_is_local_to_its_proven_region(self):
        source = ('.TH PROBE 1 "November 25, 2026" "Uniquehost"\n.SH DESCRIPTION\n'
                  '.TS\nallbox;\nl l.\nAlpha Beta\tGamma Delta\n.TE\n'
                  'Alpha Beta \\(br Gamma Delta\n')
        # tbl_term.c draws the allbox frame; ordinary term_word emits the
        # escaped prose glyph. Both exact forms were run on the fixed oracle.
        reference = ('     ┌────────────┬─────────────┐\n'
                     '     │ Alpha Beta │ Gamma Delta │\n'
                     '     └────────────┴─────────────┘\n'
                     '     Alpha Beta │ Gamma Delta\n')
        rows = fidelity_text.token_lines(reference, source)
        self.assertIn(["Alpha", "Beta"], rows)
        self.assertIn(["Gamma", "Delta"], rows)
        self.assertIn(["Alpha", "Beta", "Gamma", "Delta"], rows)
        self.assertNotIn("Alpha Beta Gamma Delta", fidelity_text.broken_phrase_candidates(
            rows[-1:], ["Alpha", "Beta", "Gamma", "Delta"], 4))
        self.assertIn("Alpha Beta Gamma Delta", fidelity_text.broken_phrase_candidates(
            rows[-1:], ["Gamma", "Delta", "Alpha", "Beta"], 4))
        self.assertNotIn("Alpha Beta Gamma Delta", fidelity_text.broken_phrase_candidates(
            rows[:-1], ["Gamma", "Delta", "Alpha", "Beta"], 4))

        cell_source = ('.TH PROBE 1 "November 25, 2026" "Uniquehost"\n.SH DESCRIPTION\n'
                       '.TS\nallbox;\nl l.\nAlpha Beta Gamma Delta\tOtherword\n.TE\n')
        cell_reference = ('     ┌────────────────────────┬───────────┐\n'
                          '     │ Alpha Beta Gamma Delta │ Otherword │\n'
                          '     └────────────────────────┴───────────┘\n')
        cell_rows = fidelity_text.token_lines(cell_reference, cell_source)
        self.assertNotIn("Alpha Beta Gamma Delta", fidelity_text.broken_phrase_candidates(
            cell_rows, ["Alpha", "Beta", "Gamma", "Delta", "Otherword"], 4))
        self.assertIn("Alpha Beta Gamma Delta", fidelity_text.broken_phrase_candidates(
            cell_rows, ["Gamma", "Delta", "Alpha", "Beta", "Otherword"], 4))

        double_source = ('.TH PROBE 1 "November 25, 2026" "Uniquehost"\n.SH DESCRIPTION\n'
                         '.TS\ndoublebox;\nl | l.\nAlpha Beta\tGamma Delta\n.TE\n')
        double_reference = ('     ┏━━━━━━━━━━━━┯━━━━━━━━━━━━━┓\n'
                            '     ┃ Alpha Beta │ Gamma Delta ┃\n'
                            '     ┗━━━━━━━━━━━━┷━━━━━━━━━━━━━┛\n')
        self.assertEqual(fidelity_text.token_lines(double_reference, double_source),
                         [["Alpha", "Beta"], ["Gamma", "Delta"]])

    def test_source_proven_url_spelling_and_occurrences(self):
        url = "https://example.org/path?route=abc"
        source = ('.TH PROBE 1 "November 25, 2026" "Uniquehost"\n.SH DESCRIPTION\n'
                  f'.UR {url}\nEvidence\n.UE\n.PP\n.UR {url}\nEvidence\n.UE\n')
        # man_term.c post_UR encloses each target in angles; both occurrences
        # and their query punctuation were observed with the fixed oracle.
        reference = f'Evidence <{url}>\nEvidence <{url}>\n'
        compare = lambda output: AUDIT.compare_rendered(
            "probe", source.encode(), reference, output, "mandoc"
        ).finding
        self.assertNotIn(url, compare(reference).missing_tokens)
        for changed in (url.replace("route", "Route"), url.replace("=", "-"),
                        url.replace("?route=abc", ""), ""):
            with self.subTest(changed=changed):
                self.assertIn(url, compare(reference.replace(url, changed, 1)).missing_tokens)
        bracketed = "https://example.org/a(b)"
        explicit = f'.UR {bracketed}\nEvidence\n.UE\n'
        self.assertEqual(fidelity_text.missing_visible_source_urls(
            explicit, f'<{bracketed}>', 'https://example.org/a(b'), [bracketed])
        wrapped_url = "https://example.org/alpha-beta-gamma-delta-epsilon"
        wrapped_source = ('.TH PROBE 1 "November 25, 2026" "Uniquehost"\n'
                          f'.SH DESCRIPTION\n{wrapped_url}\n')
        wrapped_reference = '     https://example.org/alpha-beta-\n     gamma-delta-epsilon\n'
        self.assertEqual(fidelity_text.missing_visible_source_urls(
            wrapped_source, wrapped_reference, wrapped_url), [])
        self.assertEqual(fidelity_text.missing_visible_source_urls(
            wrapped_source, wrapped_reference, wrapped_url.replace("beta-", "beta", 1)),
            [wrapped_url])

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

    def test_static_alias_comments_and_internal_current_directory_match_product(self):
        # Fixed CVS roff.c:roff_parse_comment stops interpreting the suffix
        # after the first quote comment; Rust Path::components normalizes
        # interior current-directory segments. CVS and the product were also
        # executed against these three exact fixtures before this assertion.
        quote_comment = bytes([92, 34])
        variants = [
            b".so man1/./target.1\n",
            b".so man1/target.1 " + quote_comment + b" ignored " + bytes([92, 35, 10]),
            b".so man1/target.1 " + quote_comment + b" ignored " + bytes([92, 10]),
        ]
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            alias, target = root / "man1/alias.1", root / "man1/target.1"
            target.parent.mkdir()
            target.write_bytes(b"Alpha beta gamma delta\n")
            for source in variants:
                with self.subTest(source=source):
                    alias.write_bytes(source)
                    output = "Alpha beta gamma delta\n"
                    with patch.object(AUDIT, "reference_environment", return_value={}), \
                         patch.object(AUDIT, "run_renderer", side_effect=[(0, output, ""), (0, output, "")]) as render:
                        result = AUDIT.audit_page(alias, "alias.1", [root], Path("mant"), "mandoc", "mandoc", 1, 4, False)
                    self.assertEqual(result.finding.status, "clean")
                    self.assertEqual(render.call_args_list[1].args[3], target.read_bytes())
                    self.assertEqual(result.source, source)

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
