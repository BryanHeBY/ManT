"""Corpus migration must not redirect fixture gates or grow checkout ledgers."""
from __future__ import annotations

import contextlib
import importlib
import io
import tempfile
import os
from pathlib import Path
import unittest
from unittest.mock import patch

from scripts.roff.lib.fidelity import arguments as fidelity
from scripts.roff.lib.roff_ledger_paths import ROOT


class LedgerPathsTests(unittest.TestCase):
    def test_default_external_and_explicit_paths_across_audit_routes(self):
        for route, filename in (
            ("structure", "STRUCTURE_AUDIT.csv"),
            ("projection", "PROJECTION_AUDIT.csv"),
            ("targets", "TARGET_AUDIT.csv"),
            ("semantics", "SEMANTIC_AUDIT.csv"),
            ("layout", "LAYOUT_AUDIT.csv"),
        ):
            parser = importlib.import_module(f"scripts.roff.audit.audit_roff_{route}").parse_arguments
            for configured in (None, "/external/corpus"):
                env = {} if configured is None else {"MANT_ROFF_CORPUS": configured}
                with self.subTest(route=route, configured=configured), patch.dict(os.environ, env, clear=True):
                    fixture = ROOT / "tests/fixtures/roff" / filename
                    self.assertEqual(parser([]).audit_db, fixture)
                    self.assertEqual(parser(["--fixtures"]).audit_db, fixture)
                    external = (Path(configured) / "audits/mant/current" if configured
                                else ROOT / "target/roff-audits")
                    self.assertEqual(parser(["--manpath", "/manuals"]).audit_db, external / filename)
                    args = parser(["--manpath", "/manuals", "--audit-db", "/chosen/run.csv"])
                    self.assertEqual(args.audit_db, Path("/chosen/run.csv"))
                    if route in ("structure", "projection", "layout"):
                        self.assertEqual(args.fidelity_db, external / "FIDELITY_AUDIT.csv")
                        args = parser(["--manpath", "/manuals", "--fidelity-db", "/chosen/input.csv"])
                        self.assertEqual(args.fidelity_db, Path("/chosen/input.csv"))

    def test_mandoc_defaults_do_not_modify_imported_history(self):
        from scripts.roff.audit.audit_roff_layout import parse_arguments
        with patch.dict(os.environ, {"MANT_ROFF_CORPUS": "/corpus"}, clear=True):
            args = parse_arguments(["--manpath", "/manuals", "--reference-kind", "mandoc"])
            self.assertEqual(args.audit_db, Path("/corpus/audits/mant/current/MANDOC_LAYOUT_AUDIT.csv"))
            self.assertEqual(args.fidelity_db, Path("/corpus/audits/mant/current/MANDOC_FIDELITY_AUDIT.csv"))
            args = parse_arguments(["--fixtures", "--reference-kind", "mandoc"])
            self.assertEqual(args.audit_db, ROOT / "tests/fixtures/roff/MANDOC_LAYOUT_AUDIT.csv")

    def test_fidelity_preserves_optional_recording_and_explicit_source_ledger(self):
        with patch.dict(os.environ, {}, clear=True):
            self.assertIsNone(fidelity.parse_arguments([]).audit_db)
            self.assertIsNone(fidelity.parse_arguments(["--manpath", "/manuals"]).audit_db)
        with patch.dict(os.environ, {"MANT_ROFF_CORPUS": "/corpus"}, clear=True):
            fixture = fidelity.parse_arguments(["--fixtures"])
            self.assertIsNone(fixture.audit_db)
            self.assertEqual(fixture.source_ledger, ROOT / "tests/fixtures/roff/FIDELITY_AUDIT.csv")
            args = fidelity.parse_arguments(["--manpath", "/manuals", "--reference-kind", "mandoc"])
            self.assertEqual(args.audit_db, Path("/corpus/audits/mant/current/MANDOC_FIDELITY_AUDIT.csv"))
            self.assertEqual(args.source_ledger, Path("/corpus/audits/mant/current/FIDELITY_AUDIT.csv"))
            args = fidelity.parse_arguments(["--manpath", "/manuals", "--audit-db", "/chosen/output.csv",
                                             "--source-ledger", "/chosen/source.csv"])
            self.assertEqual(args.audit_db, Path("/chosen/output.csv"))
            self.assertEqual(args.source_ledger, Path("/chosen/source.csv"))

    @unittest.skipUnless(os.name == "posix", "the bounded renderer child requires POSIX")
    def test_layout_child_never_records_fidelity_with_inherited_corpus_configuration(self):
        from scripts.roff.audit.audit_roff_layout import parse_arguments, run_layout_auditor
        with tempfile.TemporaryDirectory(prefix="ledger-child-", dir=ROOT / "target") as directory:
            root = Path(directory)
            corpus = root / "corpus"
            manuals = root / "manuals"
            manuals.mkdir()
            source = manuals / "sample.1"
            source.write_text('.TH SAMPLE 1\n.SH DESCRIPTION\nAlpha beta gamma.\n')
            # Exercise the actual fidelity subprocess and ledger routing with
            # deterministic renderer stand-ins, not product/roff assertions.
            renderer = root / "renderer"
            renderer.write_text('#!/usr/bin/env python3\nprint("Alpha beta gamma.")\n')
            renderer.chmod(0o700)
            ledger = corpus / "audits/mant/current/FIDELITY_AUDIT.csv"
            ledger.parent.mkdir(parents=True)
            previous = (ROOT / "tests/fixtures/roff/FIDELITY_AUDIT.csv").read_bytes()
            with patch.dict(os.environ, {"MANT_ROFF_CORPUS": str(corpus)}):
                arguments = parse_arguments(["--manpath", str(manuals), "--mant", str(renderer),
                                             "--reference", str(renderer), "--timeout", "5"])
                for existing in (False, True):
                    with self.subTest(existing=existing):
                        if existing:
                            ledger.write_bytes(previous)
                        findings, error = run_layout_auditor([source], [manuals], arguments)
                        self.assertIsNone(error)
                        self.assertEqual(len(findings), 1)
                        self.assertEqual(next(iter(findings.values())).status, "clean")
                        self.assertEqual(ledger.read_bytes() if ledger.exists() else None,
                                         previous if existing else None)

    def test_explicit_unrecorded_fidelity_overrides_environment(self):
        with patch.dict(os.environ, {"MANT_ROFF_CORPUS": "/corpus"}, clear=True):
            args = fidelity.parse_arguments(["--manpath", "/manuals", "--no-record"])
            self.assertIsNone(args.audit_db)
            with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
                fidelity.parse_arguments(["--no-record", "--audit-db", "/chosen/file.csv"])


if __name__ == "__main__":
    unittest.main()
