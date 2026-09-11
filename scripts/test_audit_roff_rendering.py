"""Independent failure-path tests for the development rendering census."""
import argparse
import bz2
import gzip
import importlib.util
import json
import lzma
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("rendering_census", Path(__file__).with_name("audit-roff-rendering.py"))
AUDIT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(AUDIT)


class RenderingCensusTests(unittest.TestCase):
    def inspect(self, source=b"BODY", reference=b"BODY", mant=b"BODY", raw_status="covered"):
        covered = {"status": "covered", "findings": [], "coverage": {"complete": True}}
        raw = {"status": raw_status, "findings": [], "coverage": {"complete": raw_status == "covered"}}
        args = argparse.Namespace(reference=Path("reference"), mant=Path("mant"), width=80, timeout=1)
        with patch.object(AUDIT, "source_bytes", return_value=(source, AUDIT.digest(source))), \
             patch.object(AUDIT, "run_renderer", side_effect=[(0, reference, ""), (0, mant, "")]), \
             patch.object(AUDIT, "prepare_frame", side_effect=lambda text, *_args, **_kwargs: (text, {"status": "covered"})), \
             patch.object(AUDIT, "compare_content", side_effect=[covered, raw]), \
             patch.object(AUDIT, "compare_layout_geometry", return_value=covered):
            return AUDIT.inspect(("input", [{"id": "one"}]), args)[0]

    def test_invalid_utf8_on_either_output_or_source_cannot_be_clean(self):
        self.assertEqual(self.inspect()["status"], "clean")
        for kwargs in ({"source": b"\xff"}, {"reference": b"\xff"}, {"mant": b"\xff"}):
            self.assertEqual(self.inspect(**kwargs)["status"], "review", kwargs)

    def test_uncovered_raw_control_check_cannot_be_clean(self):
        record = self.inspect(raw_status="uncovered")
        self.assertEqual(record["status"], "partial")
        self.assertEqual(record["rawControlCoverage"]["status"], "uncovered")

    def test_transport_hash_and_all_supported_inprocess_compressions(self):
        source = b".TH PROBE 1\nBODY\n"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "source"
            for encoded in (source, gzip.compress(source), bz2.compress(source), lzma.compress(source),
                            lzma.compress(source[:5]) + b"\0" * 4 + lzma.compress(source[5:])):
                path.write_bytes(encoded)
                self.assertEqual(AUDIT.source_bytes(path), (source, AUDIT.digest(encoded)))
            path.write_bytes(lzma.compress(source)[:-1])
            with self.assertRaises(EOFError):
                AUDIT.source_bytes(path)

    def test_decoded_limit_and_xz_dictionary_limit_are_independent(self):
        with patch.object(AUDIT, "MAX_INPUT_BYTES", 4):
            with self.assertRaises(ValueError):
                AUDIT.decode_xz(lzma.compress(b"12345"))
        real = lzma.LZMADecompressor
        with patch.object(AUDIT.lzma, "LZMADecompressor", wraps=real) as decoder:
            self.assertEqual(AUDIT.decode_xz(lzma.compress(b"body")), b"body")
            self.assertEqual(decoder.call_args.kwargs["memlimit"], 64 * 1024 * 1024)

    def test_manifest_snapshot_duplicate_ids_and_empty_selection(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest"
            args = argparse.Namespace(manifest=path, max_pages=None)
            for contents in ("", '{"id":"same","source_path":"x"}\n' * 2):
                path.write_text(contents, encoding="utf-8")
                with self.assertRaises(ValueError):
                    AUDIT.census_inputs(args)
            snapshot = b'{"id":"one","source_path":"x"}\n'
            path.write_bytes(snapshot)
            inputs = AUDIT.census_inputs(args)
            path.write_bytes(b"changed after acquisition")
            self.assertEqual(args._manifest_sha256, AUDIT.digest(snapshot))
            self.assertEqual(inputs[0][1][0]["id"], "one")

    def test_normal_setup_failure_writes_non_green_summary(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "report"
            args = ["audit", "--reference", str(Path(directory) / "absent"), "--reference-id", "test",
                    "--mant", str(Path(directory) / "absent"), "--output", str(output)]
            with patch.object(sys, "argv", args):
                self.assertEqual(AUDIT.main(), 1)
            report = json.loads((output / "summary.json").read_text())
            self.assertEqual(report["status"], "audit-error")
            self.assertFalse(report["coverageComplete"])
            self.assertEqual(report["auditError"]["type"], "FileNotFoundError")

    def test_nonfinite_timeout_is_rejected_before_launch(self):
        for value in ("nan", "inf", "-inf"):
            with patch.object(sys, "argv", ["audit", "--reference", "x", "--reference-id", "test",
                                           "--output", "unused", "--timeout=" + value]), \
                 self.assertRaises(SystemExit) as error:
                AUDIT.main()
            self.assertEqual(error.exception.code, 2)


if __name__ == "__main__":
    unittest.main()
