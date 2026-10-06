"""Exact pristine section-volume observations and conservative frame matching."""

import unittest

from scripts.roff.lib.roff_rendering_frame import prepare_frame


# Exact inputs ran the registered cvs-20261006T140200Z pristine reference
# before these assertions, with ASCII/UTF-8/HTML all successful. Retained
# source/raw receipts: target/audits/cvs-sync-20261006/frame-probes/records.json.
# msec.c::mandoc_a2msec gives exact matches priority, then tries the first
# character; man_validate.c::post_TH keeps an explicit VOL, and
# mdoc_term.c::print_mdoc_head appends the normalized architecture.
# These are UTF-8 stdout bytes decoded as text; no page/body rows were folded.
FIXTURES = {'man-1m': ('.TH PROBE 1m "2026-10-06" "ManT Frame"\n.SH TEST\nBODY\n',
            'PROBE(1m)                   General Commands Manual                  PROBE(1m)\n'
            '\n'
            'T\x08TE\x08ES\x08ST\x08T\n'
            '     BODY\n'
            '\n'
            'ManT Frame                        2026-10-06                         PROBE(1m)\n'),
 'man-3p': ('.TH PROBE 3p "2026-10-06" "ManT Frame"\n.SH TEST\nBODY\n',
            'PROBE(3p)                     Perl Library Manual                    PROBE(3p)\n'
            '\n'
            'T\x08TE\x08ES\x08ST\x08T\n'
            '     BODY\n'
            '\n'
            'ManT Frame                        2026-10-06                         PROBE(3p)\n'),
 'man-3px': ('.TH PROBE 3px "2026-10-06" "ManT Frame"\n.SH TEST\nBODY\n',
             'PROBE(3px)                 Library Functions Manual                 PROBE(3px)\n'
             '\n'
             'T\x08TE\x08ES\x08ST\x08T\n'
             '     BODY\n'
             '\n'
             'ManT Frame                        2026-10-06                        '
             'PROBE(3px)\n'),
 'man-z': ('.TH PROBE z "2026-10-06" "ManT Frame"\n.SH TEST\nBODY\n',
           'PROBE(z)                                                              PROBE(z)\n'
           '\n'
           'T\x08TE\x08ES\x08ST\x08T\n'
           '     BODY\n'
           '\n'
           'ManT Frame                        2026-10-06                          PROBE(z)\n'),
 'mdoc-1m': ('.Dd October 6, 2026\n.Dt PROBE 1m\n.Os ManT Frame\n.Sh TEST\nBODY\n',
             'PROBE(1m)                   General Commands Manual                  PROBE(1m)\n'
             '\n'
             'T\x08TE\x08ES\x08ST\x08T\n'
             '     BODY\n'
             '\n'
             'ManT Frame                      October 6, 2026                      '
             'PROBE(1m)\n'),
 'mdoc-3p': ('.Dd October 6, 2026\n.Dt PROBE 3p\n.Os ManT Frame\n.Sh TEST\nBODY\n',
             'PROBE(3p)                     Perl Library Manual                    PROBE(3p)\n'
             '\n'
             'T\x08TE\x08ES\x08ST\x08T\n'
             '     BODY\n'
             '\n'
             'ManT Frame                      October 6, 2026                      '
             'PROBE(3p)\n'),
 'mdoc-3px': ('.Dd October 6, 2026\n.Dt PROBE 3px\n.Os ManT Frame\n.Sh TEST\nBODY\n',
              'PROBE(3px)                 Library Functions Manual                 PROBE(3px)\n'
              '\n'
              'T\x08TE\x08ES\x08ST\x08T\n'
              '     BODY\n'
              '\n'
              'ManT Frame                      October 6, 2026                     '
              'PROBE(3px)\n'),
 'mdoc-z': ('.Dd October 6, 2026\n.Dt PROBE z\n.Os ManT Frame\n.Sh TEST\nBODY\n',
            'PROBE(z)                             LOCAL                            PROBE(z)\n'
            '\n'
            'T\x08TE\x08ES\x08ST\x08T\n'
            '     BODY\n'
            '\n'
            'ManT Frame                      October 6, 2026                       PROBE(z)\n'),
 'man-1m-explicit': ('.TH PROBE 1m "2026-10-06" "ManT Frame" "Custom Volume"\n.SH TEST\nBODY\n',
                     'PROBE(1m)                        Custom Volume                       '
                     'PROBE(1m)\n'
                     '\n'
                     'T\x08TE\x08ES\x08ST\x08T\n'
                     '     BODY\n'
                     '\n'
                     'ManT Frame                        2026-10-06                         '
                     'PROBE(1m)\n'),
 'man-z-explicit': ('.TH PROBE z "2026-10-06" "ManT Frame" "Custom Volume"\n.SH TEST\nBODY\n',
                    'PROBE(z)                         Custom Volume                        '
                    'PROBE(z)\n'
                    '\n'
                    'T\x08TE\x08ES\x08ST\x08T\n'
                    '     BODY\n'
                    '\n'
                    'ManT Frame                        2026-10-06                          '
                    'PROBE(z)\n'),
 'mdoc-1m-architecture': ('.Dd October 6, 2026\n'
                          '.Dt PROBE 1m AMD64\n'
                          '.Os ManT Frame\n'
                          '.Sh TEST\n'
                          'BODY\n',
                          'PROBE(1m)               General Commands Manual '
                          '(amd64)              PROBE(1m)\n'
                          '\n'
                          'T\x08TE\x08ES\x08ST\x08T\n'
                          '     BODY\n'
                          '\n'
                          'ManT Frame                      October 6, '
                          '2026                      PROBE(1m)\n'),
 'man-1m-body-furniture': ('.TH PROBE 1m "2026-10-06" "ManT Frame"\n'
                           '.SH TEST\n'
                           '.nf\n'
                           'PROBE(1m) General Commands Manual PROBE(1m)\n'
                           'ManT Frame 2026-10-06 PROBE(1m)\n'
                           'BODY\n'
                           '.fi\n',
                           'PROBE(1m)                   General Commands '
                           'Manual                  PROBE(1m)\n'
                           '\n'
                           'T\x08TE\x08ES\x08ST\x08T\n'
                           '     PROBE(1m) General Commands Manual PROBE(1m)\n'
                           '     ManT Frame 2026-10-06 PROBE(1m)\n'
                           '     BODY\n'
                           '\n'
                           'ManT Frame                        '
                           '2026-10-06                         PROBE(1m)\n')}


BODY = "\nT\bTE\bES\bST\bT\n     BODY\n\n"


class RenderingFrameSectionTests(unittest.TestCase):
    def assert_furniture_removed(self, case, volume, expected=BODY):
        source, raw = FIXTURES[case]
        body, report = prepare_frame(raw, source, reference=True)
        self.assertEqual(report["status"], "covered", case)
        self.assertEqual(report["volume"], volume, case)
        self.assertEqual(body, expected, case)
        self.assertEqual([row["kind"] for row in report["removedRows"]],
                         ["reference-header", "reference-footer"], case)
        self.assertEqual([row["raw"] for row in report["removedRows"]],
                         [raw.splitlines(keepends=True)[0],
                          raw.splitlines(keepends=True)[-1]], case)

    def test_exact_section_precedes_first_character_suffix_fallback(self):
        for dialect in ("man", "mdoc"):
            for section, volume in (("1m", "General Commands Manual"),
                                    ("3p", "Perl Library Manual"),
                                    ("3px", "Library Functions Manual")):
                case = f"{dialect}-{section}"
                with self.subTest(case=case):
                    self.assert_furniture_removed(case, volume)
                    source, raw = FIXTURES[case]
                    _, report = prepare_frame(raw, source, reference=True)
                    self.assertEqual(report["title"], f"PROBE({section})")

    def test_explicit_man_volume_overrides_standard_and_unknown_sections(self):
        for case in ("man-1m-explicit", "man-z-explicit"):
            with self.subTest(case=case):
                self.assert_furniture_removed(case, "Custom Volume")

    def test_mdoc_architecture_stays_attached_to_the_resolved_volume(self):
        self.assert_furniture_removed("mdoc-1m-architecture",
                                     "General Commands Manual (amd64)")

    def test_unknown_sections_remain_partial_and_keep_all_original_rows(self):
        # post_dt's LOCAL fallback and TH's absent volume are not guessed by
        # this conservative mask. Unrecognized furniture stays observable.
        for case in ("man-z", "mdoc-z"):
            source, raw = FIXTURES[case]
            with self.subTest(case=case):
                body, report = prepare_frame(raw, source, reference=True)
                self.assertEqual(report["status"], "partial")
                self.assertIsNone(report["volume"])
                self.assertEqual(body, raw)
                self.assertEqual(report["removedRows"], [])
                self.assertIn("unrecognized-reference-header", report["reasons"])

    def test_author_furniture_spelling_inside_body_is_never_removed(self):
        expected = ("\nT\bTE\bES\bST\bT\n"
                    "     PROBE(1m) General Commands Manual PROBE(1m)\n"
                    "     ManT Frame 2026-10-06 PROBE(1m)\n"
                    "     BODY\n\n")
        self.assert_furniture_removed("man-1m-body-furniture",
                                     "General Commands Manual", expected)

    def test_suffix_fallback_does_not_authorize_mismatched_furniture(self):
        source, raw = FIXTURES["man-1m"]
        changed_header = raw.replace("General Commands Manual", "Unmatched Volume", 1)
        body, report = prepare_frame(changed_header, source, reference=True)
        self.assertEqual(report["status"], "partial")
        self.assertEqual(body, changed_header)
        self.assertEqual(report["removedRows"], [])

        changed_footer = raw.replace("ManT Frame", "Unmatched OS", 1)
        body, report = prepare_frame(changed_footer, source, reference=True)
        self.assertEqual(report["status"], "partial")
        self.assertEqual(body, changed_footer.split("\n", 1)[1])
        self.assertEqual([row["kind"] for row in report["removedRows"]],
                         ["reference-header"])
        self.assertIn("unrecognized-reference-footer", report["reasons"])


if __name__ == "__main__":
    unittest.main()
