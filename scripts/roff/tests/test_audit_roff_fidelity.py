"""Source-reader injection checks for fidelity orchestration, without renderers."""
from pathlib import Path
import tempfile
import unittest
from unittest.mock import call, patch

from scripts.roff.audit import audit_roff_fidelity as AUDIT
from scripts.roff.lib.fidelity import sources


class FidelityDriverTests(unittest.TestCase):
    def test_unavailable_source_does_not_reread_for_hierarchy_selection(self):
        page = Path("/absent-fidelity-review/man1/page.1")
        output = "Alpha beta gamma delta\n"
        with patch.object(AUDIT, "source_bytes", return_value=None) as reader, \
             patch.object(sources, "source_bytes", side_effect=AssertionError("helper reader used")), \
             patch.object(AUDIT, "reference_environment", return_value={}), \
             patch.object(AUDIT, "run_renderer", side_effect=[(0, output, ""), (0, output, "")]) as render:
            artifact = AUDIT.audit_page(
                page, "page.1", [page.parent.parent], Path("mant"), "man", "man", 1, 4, False,
            )
        self.assertEqual(artifact.finding.status, "clean")
        self.assertIsNone(artifact.source)
        reader.assert_called_once_with(page)
        self.assertEqual(render.call_args_list[0].args[0][1:3], ["--input", str(page)])
        self.assertEqual(render.call_args_list[1].args[0], ["man", "-l", str(page)])
        self.assertIsNone(render.call_args_list[1].args[3])

    def test_absent_source_uses_facade_reader_for_both_renderer_inputs(self):
        source = b"Alpha beta gamma delta\n"
        output = "Alpha beta gamma delta\n"
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            page = root / "man1/absent.1"
            self.assertFalse(page.exists())
            with patch.object(AUDIT, "source_bytes", return_value=source) as reader, \
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
            with patch.object(AUDIT, "source_bytes", side_effect=inputs.__getitem__) as reader, \
                 patch.object(sources, "source_bytes", side_effect=AssertionError("helper reader used")), \
                 patch.object(AUDIT, "reference_environment", return_value={}), \
                 patch.object(AUDIT, "run_renderer", side_effect=[(0, output, ""), (0, output, "")]) as render:
                artifact = AUDIT.audit_page(
                    alias, "alias.1", [root], Path("mant"), "mandoc", "mandoc", 1, 4, False,
                )
            self.assertEqual(artifact.finding.status, "clean")
            self.assertEqual(reader.call_args_list, [call(alias), call(target)])
            self.assertEqual(render.call_args_list[0].args[0][1], "manual/1/alias")
            self.assertEqual(render.call_args_list[0].args[2], {
                "MANT_MANPATH": str(root), "MANPATH": str(root),
            })
            self.assertEqual(render.call_args_list[1].args[3], inputs[target])

    def test_reference_command_remains_replaceable_on_driver_facade(self):
        page = Path("/absent/man1/page.1")
        with patch.object(AUDIT, "source_bytes", return_value=b"opaque source") as reader, \
             patch.object(AUDIT, "reference_render_command", return_value=(None, None, "injected skip")) as command, \
             patch.object(AUDIT, "run_renderer") as render:
            artifact = AUDIT.audit_page(
                page, "page.1", [page.parent.parent], Path("mant"), "mandoc", "mandoc", 1, 4, False,
            )
        self.assertEqual(artifact.finding.status, "skipped")
        self.assertEqual(artifact.finding.detail, "injected skip")
        self.assertIs(command.call_args.kwargs["source_reader"], reader)
        render.assert_not_called()


if __name__ == "__main__":
    unittest.main()
