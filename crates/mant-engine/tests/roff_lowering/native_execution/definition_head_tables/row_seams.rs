//! Executed alignment and terminal HEAD boundaries with table payloads.

use super::{
    Block, REVIEW_HEADER, lower,
    row_boundaries::{assert_rows, reading_rows},
    whole_word,
};
use mant_ir::{EntryOwner, ResolvedContent, inline_plain_text};
use mant_protocol::{EvidenceBasis, EvidenceClass, ExplanationOptions, ExplanationQuery};

const FOOTER: &str = ".Sh EXAMPLES\nTAIL\n";

fn definition(style: &str, width: &str, head: &str) -> String {
    format!("{REVIEW_HEADER}.Bl -{style} -width {width}\n.It Xo\n{head}.Xc\nENDING\n.El\n{FOOTER}")
}

fn table(layout: &str, content: &str) -> String {
    format!(".TS\n{layout}.\n{content}\n.TE\n")
}

#[test]
fn aligned_first_visible_cell_accounts_for_the_actual_prefix_and_prior_slots() {
    // Exact CVS profiles cover three field widths and the first printable
    // cell in either slot. term_flushln aligns inside the available part
    // of the field; ascii_advance only prints spaces beyond half an en.
    for kind in ["l", "c", "r"] {
        for width in ["4n", "8n", "20n"] {
            for second in [false, true] {
                let layout = if second {
                    format!("l {kind}")
                } else {
                    kind.to_owned()
                };
                let words = if second {
                    "\\&\tXYZ\nA\tWIDESTCELL"
                } else {
                    "XYZ\nWIDESTCELL"
                };
                let source = definition(
                    "hang",
                    width,
                    &format!(".No LONGPREFIX\n.br\n{}", table(&layout, words)),
                );
                let separated = kind != "l" || width == "20n" || (width == "8n" && second);
                let first = if separated {
                    "LONGPREFIX XYZ"
                } else {
                    "LONGPREFIXXYZ"
                };
                let second = if second { "A WIDESTCELL" } else { "WIDESTCELL" };
                let query = lower(&source);
                assert_rows(&query, &[first, second, "ENDING"], &source);
                assert_eq!(
                    whole_word(&query, "XYZ").matches.len(),
                    usize::from(separated),
                    "{source}"
                );
                assert_eq!(
                    whole_word(&query, "LONGPREFIXXYZ").matches.len(),
                    usize::from(!separated),
                    "{source}"
                );
            }
        }
    }
    for (kind, separated) in [("l", false), ("c", false), ("r", true)] {
        // This exact native first cell has only one en of available slack.
        let source = definition(
            "hang",
            "4n",
            &format!(".No LONGPREFIX\n.br\n{}", table(kind, "XYZ\nWIDECELL")),
        );
        let first = if separated {
            "LONGPREFIX XYZ"
        } else {
            "LONGPREFIXXYZ"
        };
        assert_rows(&lower(&source), &[first, "WIDECELL", "ENDING"], &source);
    }
}

#[test]
fn whole_table_origin_and_long_columns_use_the_native_initial_pass() {
    // Fixed CVS term_tbl positions the entire table before applying each
    // layout. LONG adjusts the preceding column pointer on the first pass.
    for options in ["", "center;\n", "expand;\n", "center expand;\n"] {
        for kind in ["l", "c", "r", "n", "a"] {
            for width in ["4n", "8n", "20n"] {
                let source = definition(
                    "hang",
                    width,
                    &format!(".No LONGPREFIX\n.br\n.TS\n{options}{kind}.\nXYZ\nWIDESTCELL\n.TE\n"),
                );
                let separated =
                    options.contains("center") || matches!(kind, "c" | "r" | "n") || width == "20n";
                let first = if separated {
                    "LONGPREFIX XYZ"
                } else {
                    "LONGPREFIXXYZ"
                };
                let query = lower(&source);
                assert_rows(&query, &[first, "WIDESTCELL", "ENDING"], &source);
                assert_eq!(
                    whole_word(&query, "XYZ").matches.len(),
                    usize::from(separated)
                );
            }
        }
    }
    // Ordinary maximum widths, including a centered origin behind the
    // already emitted prefix. A center option alone cannot prove a gap.
    for (measure, width, longest, separated) in [
        ("20n", "4n", "WIDESTCELL", false),
        ("20n", "20n", "WIDESTCELL", true),
        ("20n", "20n", "WIDEWIDEWIDEWIDEWIDEWIDE", false),
        ("30n", "4n", "WIDESTCELL", false),
        ("30n", "8n", "WIDESTCELL", true),
        ("78n", "4n", "WIDESTCELL", true),
    ] {
        let source = definition(
            "hang",
            width,
            &format!(".No LONGPREFIX\n.br\n.ll {measure}\n.TS\ncenter;\nl.\nXYZ\n{longest}\n.TE\n"),
        );
        let first = if separated {
            "LONGPREFIX XYZ"
        } else {
            "LONGPREFIXXYZ"
        };
        assert_rows(&lower(&source), &[first, longest, "ENDING"], &source);
    }
}

#[test]
fn terminal_paragraph_requests_keep_rows_and_their_declaration_evidence() {
    for style in ["tag", "hang", "inset", "ohang"] {
        let run_in = matches!(style, "tag" | "hang");
        for (head, expected) in [
            (
                ".Pp\n.No LABEL\n",
                if style == "ohang" {
                    &["", "LABEL", "ENDING"][..]
                } else {
                    &["", "LABEL ENDING"]
                },
            ),
            (
                ".No LABEL\n.Pp\n.No OTHER\n",
                if run_in {
                    &["LABEL", "OTHER ENDING"]
                } else if style == "inset" {
                    &["LABEL", "", "OTHER ENDING"]
                } else {
                    &["LABEL", "", "OTHER", "ENDING"]
                },
            ),
            (
                ".No LABEL\n.Pp\n",
                if run_in {
                    &["LABEL", "ENDING"]
                } else {
                    &["LABEL", "", "ENDING"]
                },
            ),
            (
                ".No LABEL\n.br\n",
                if style == "hang" {
                    &["LABEL ENDING"]
                } else {
                    &["LABEL", "ENDING"]
                },
            ),
            (
                ".No LABEL\n.sp 1\n",
                if run_in {
                    &["LABEL", "ENDING"]
                } else {
                    &["LABEL", "", "ENDING"]
                },
            ),
        ] {
            let source = definition(style, "Ds", head);
            assert_rows(&lower(&source), expected, &source);
        }
        for option in [false, true] {
            let prefix = if option { ".Fl q\n" } else { "" };
            let source = definition(
                style,
                "Ds",
                &format!("{prefix}{}.Pp\n", table("l", "--fake")),
            );
            let query = lower(&source);
            let expected = match (run_in, option) {
                (_, false) => &["--fake", "", "ENDING"][..],
                (true, true) => &["-q --fake", "", "ENDING"],
                (false, true) => &["-q", "--fake", "", "ENDING"],
            };
            assert_rows(&query, expected, &source);
            let document = query.document.as_ref().unwrap();
            let [Block::DefinitionList { items, .. }] = document.sections[1].blocks.as_slice()
            else {
                panic!("expected one definition list");
            };
            let [item] = items.as_slice() else {
                panic!("expected one item");
            };
            let names = EntryOwner::Definition(item)
                .validated_names()
                .unwrap_or_default();
            assert_eq!(names, if option { &["-q"][..] } else { &[] }, "{source}");
            assert_eq!(item.terms.len(), 1);
            assert!(inline_plain_text(&item.terms[0]).contains("--fake"));
            if let Some(forms) = EntryOwner::Definition(item).forms() {
                assert!(
                    forms
                        .iter()
                        .all(|form| !inline_plain_text(form).contains("--fake"))
                );
            }
            assert_eq!(whole_word(&query, "--fake").matches.len(), 1);
            assert_direct(&query, "--fake", false);
            assert_direct(&query, "-q --fake", false);
            assert_direct(&query, "-q", option);
        }
    }
}

#[test]
fn paragraph_alternatives_keep_distinct_name_ranges_on_both_sides_of_display() {
    for style in ["tag", "hang"] {
        let cell = table("l", "--fake");
        for (head, q, r, expected) in [
            (
                format!(".Fl q\n.Pp\n{cell}"),
                true,
                false,
                &["-q", "--fake", "ENDING"][..],
            ),
            (
                format!("{cell}.Pp\n.Fl q\n"),
                true,
                false,
                &["--fake", "", "-q ENDING"],
            ),
            (
                format!(".Fl q\n.Pp\n{cell}.Pp\n.Fl r\n"),
                true,
                true,
                &["-q", "--fake", "", "-r ENDING"],
            ),
        ] {
            // Fixed CVS executes a real Pp between these accepted terms.
            // Source declarations can be found independently of the cell.
            let source = definition(style, "Ds", &head);
            let query = lower(&source);
            assert_rows(&query, expected, &source);
            assert_direct(&query, "-q", q);
            assert_direct(&query, "-r", r);
            assert_direct(&query, "--fake", false);
        }
    }
}

fn assert_direct(query: &ResolvedContent, name: &str, present: bool) {
    let result = mant_query::explain_query(
        query,
        &ExplanationQuery {
            entry: name.into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(
        result.counts.direct_entry.total,
        u32::from(present),
        "{name}"
    );
    if present {
        let evidence = result
            .evidence
            .iter()
            .find(|hit| hit.class == EvidenceClass::DirectEntry)
            .unwrap();
        assert!(
            evidence
                .bases
                .iter()
                .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
        );
        let selected = mant_query::select_excerpt(
            query,
            &[mant_protocol::ContentSelector::path(
                evidence.outline.path(),
            )],
        )
        .unwrap();
        assert!(mant_render::render_excerpt_text(&selected).contains("ENDING"));
    }
}

#[test]
fn a_table_and_an_unprinted_head_post_close_rows_independently_of_spacing() {
    // With preceding .sp, tag HEAD post closes the buffered NBRZW field.
    // An empty No word costs no row; the same NBRZW without the prior .sp
    // does not necessarily close a fitting field (term.c:250-253).
    for (before, gap) in [("", 0), (".sp 1\n", 1)] {
        for (content, table_rows) in [("\\&", 1), ("\\&\n\\&", 2)] {
            for (after, row) in [
                ("", 0),
                (".No \\&\n", gap),
                (".No \"\"\n", 0),
                (".br\n", 0),
                (".Pp\n", 1),
            ] {
                let source = definition(
                    "tag",
                    "Ds",
                    &format!("{before}{}{after}", table("l", content)),
                );
                let mut expected = vec![""; gap + table_rows + row];
                expected.push("ENDING");
                let query = lower(&source);
                assert_eq!(
                    reading_rows(&mant_render::render_query_text(&query)),
                    expected,
                    "{source}"
                );
                let markdown = mant_codec::encode::render_markdown(&query);
                let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
                if gap > 0 && after.starts_with(".Pp") {
                    // The anonymous HEAD's positive layout distance can
                    // simplify in Markdown, unlike either hard-row source.
                    expected.remove(0);
                }
                assert_eq!(
                    reading_rows(&mant_render::render_query_text(&readback)),
                    expected,
                    "{source}\n{markdown}"
                );
            }
        }
    }
}
