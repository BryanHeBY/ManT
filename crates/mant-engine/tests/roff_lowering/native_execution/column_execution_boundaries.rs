//! Native column fields remain live across words and transparent scopes.

use libmandoc_rs::{Node, NodeKind, Parser};
use mant_ir::{Block, Inline, ResolvedContent};

const HEADER: &str =
    ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
const FOOTER: &str = ".Sh NEXT\n.No END\n";

fn first_column_item(node: &Node) -> Option<&Node> {
    if node.kind == NodeKind::Block && node.macro_name.as_deref() == Some("It") {
        return Some(node);
    }
    node.children.iter().find_map(first_column_item)
}

fn round_trip(source: &str) -> ResolvedContent {
    let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let json = mant_render::render_query_json(&loaded, false).unwrap();
    assert!(!json.contains("\\u0000mant:"), "private owner leaked");
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    restored.into()
}

fn description_table(query: &ResolvedContent) -> &[mant_ir::TableRow] {
    let section = query
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.id.as_str() == "description")
        .unwrap();
    let [Block::Table { rows, .. }] = section.blocks.as_slice() else {
        panic!("one source column table expected: {:#?}", section.blocks);
    };
    rows
}

fn assert_empty_label(inlines: &[Inline], count: &mut usize) {
    for inline in inlines {
        match inline {
            Inline::Link { children, .. } => {
                *count += 1;
                assert!(mant_ir::inline_plain_text(children).is_empty());
                assert_empty_label(children, count);
            }
            Inline::Strong { children } | Inline::Emphasis { children } => {
                assert_empty_label(children, count);
            }
            _ => {}
        }
    }
}

#[test]
fn column_body_post_rejects_the_same_field_in_both_legal_syntaxes() {
    // All 18 exact sources ran pristine ASCII/UTF-8/HTML/tree/lint first;
    // lint is clean. mdoc_term.c::termp_it_pre() configures NOBREAK on the
    // first BODY; only its It post calls term_flushln(). No/Em/Lk are words,
    // and Xo/Xc have NULL pre/post: neither can restart a rejected field.
    for width in [4, 8, 20] {
        for carrier in ["No", "Em", "Lk"] {
            let operand = if carrier == "Lk" {
                "Lk https://ex.org"
            } else {
                carrier
            };
            for extended in [false, true] {
                let cells = if extended {
                    format!(".It Xo\n.{operand} \"\\p D\"\n.No AFTER\n.Xc Ta RightWord\n")
                } else {
                    format!(".It {operand} \"\\p D\" No AFTER Ta RightWord\n")
                };
                let source = format!(
                    "{HEADER}.Bl -column \"{}\" \"xxxx\"\n{cells}.El\n{FOOTER}",
                    "x".repeat(width)
                );
                let parsed = Parser::default()
                    .parse_bytes("column-execution.1", source.as_bytes())
                    .unwrap();
                assert!(parsed.diagnostics.is_empty(), "{source}");
                let item = first_column_item(&parsed.document.root).unwrap();
                let bodies = item
                    .children
                    .iter()
                    .filter(|node| node.kind == NodeKind::Body)
                    .collect::<Vec<_>>();
                assert_eq!(bodies.len(), 2, "both Ta-separated BODY owners: {source}");
                assert_eq!(
                    bodies[0]
                        .children
                        .iter()
                        .any(|node| node.macro_name.as_deref() == Some("Xo")),
                    extended
                );

                let query = round_trip(&source);
                let rows = description_table(&query);
                let [row] = rows else {
                    panic!("one source It row: {source}")
                };
                assert_eq!(row.cells.len(), 2, "{source}");
                let mut identities = 0;
                for block in &row.cells[0].blocks {
                    match block {
                        Block::Paragraph { children, .. }
                        | Block::Preformatted { children, .. } => {
                            assert!(
                                mant_ir::inline_plain_text(children).trim().is_empty(),
                                "rejected glyph revived: {source}: {children:#?}"
                            );
                            assert_empty_label(children, &mut identities);
                        }
                        Block::VerticalSpace { .. } => {}
                        _ => panic!("rejected field acquired structure: {source}: {block:#?}"),
                    }
                }
                assert_eq!(
                    identities,
                    usize::from(carrier == "Lk"),
                    "authored URI identity survives without a hit range: {source}"
                );
                let text = mant_render::render_query_text(&query);
                assert_eq!(text.matches("RightWord").count(), 1, "{source}: {text:?}");
                assert!(!text.contains("AFTER"), "{source}: {text:?}");
                let markdown = mant_codec::encode::render_markdown(&query);
                assert!(!markdown.contains("AFTER"), "{source}: {markdown:?}");
            }
        }
    }
}

fn physical_rows(query: &ResolvedContent) -> Vec<String> {
    let text = mant_render::render_query_text(query);
    let rows = text.lines().collect::<Vec<_>>();
    let start = rows.iter().position(|row| *row == "DESCRIPTION").unwrap() + 1;
    let end = rows.iter().position(|row| *row == "NEXT").unwrap();
    assert_eq!(rows[end - 1], "", "section separator: {text:?}");
    rows[start..end - 1]
        .iter()
        .map(|row| row.trim_start().to_owned())
        .collect()
}

#[test]
fn real_column_handlers_consume_the_field_before_later_words() {
    // These seven exact sources ran pristine CVS in five profiles first.
    // The first column's leading marker rejects its old buffer. A genuine
    // pre-handler/newline request consumes it, so AFTER (and structural
    // Marker text) belongs to a new buffer. Bl/D1/Dl pre call term_newln;
    // Bd pre calls print_bvspace -> term_newln; Pp calls term_vspace;
    // br uses roff_term_pre_br. It BODY post calls term_flushln, not a
    // no-fill source-row approximation. Only common outer margins are
    // omitted here; internal separators and empty physical rows stay exact.
    for (control, expected) in [
        ("", vec!["RightWord"]),
        (".br\n", vec!["AFTER       RightWord"]),
        (".Pp\n", vec!["", "AFTER       RightWord"]),
        (".D1 Marker\n", vec!["Marker", "AFTER       RightWord"]),
        (".Dl Marker\n", vec!["Marker", "AFTER       RightWord"]),
        (
            ".Bl -item -compact\n.It\n.No Marker\n.El\n",
            vec!["Marker", "AFTER", "RightWord"],
        ),
        (
            ".Bd -literal -compact\nMarker\n.Ed\n",
            vec!["Marker AFTER", "RightWord"],
        ),
    ] {
        let source = format!(
            "{HEADER}.Bl -column \"xxxxxxxx\" \"xxxx\"\n.It Xo\n.No \"\\p D\"\n{control}.No AFTER\n.Xc Ta RightWord\n.El\n{FOOTER}"
        );
        let query = round_trip(&source);
        assert_eq!(physical_rows(&query), expected, "{source}");
    }
}

#[test]
fn no_fill_column_source_flush_can_leave_the_device_row_open() {
    // Exact filled/no-fill sources ran pristine first. NODE_LINE calls
    // term_newln in no-fill; NOBREAK/trailspace can leave viscol active
    // after that field flush. Xo return and source flush are neither
    // an unconditional physical line end nor an IR owner drain.
    for no_fill in [false, true] {
        let source = format!(
            "{HEADER}{}.Bl -column \"xxxxxxxx\" \"xxxx\"\n.It Xo\n.No LEFT\n.No AFTER\n.Xc Ta RightWord\n.El\n{}{FOOTER}",
            if no_fill { ".nf\n" } else { "" },
            if no_fill { ".fi\n" } else { "" }
        );
        let query = round_trip(&source);
        let expected = if no_fill {
            vec!["", "LEFT AFTER  RightWord"]
        } else {
            vec!["LEFT AFTER  RightWord"]
        };
        assert_eq!(physical_rows(&query), expected, "{source}");
    }
}

#[test]
fn last_column_post_is_owned_once_when_followed_by_source_text() {
    // Both complete sources ran pristine ASCII/UTF-8/HTML/tree/lint first.
    // Each It BODY post consumes pending glyphs (mdoc_term.c:953), but the
    // final field's endline is also the table output owner's row close.
    // Projecting it twice would add a blank row before the following Y.
    for (row, expected) in [
        (".It \\zX\\c Ta Z", vec!["X    Z", "Y"]),
        (".It Q Ta \\zX\\c", vec!["Q    X", "Y"]),
    ] {
        let source = format!("{HEADER}.nf\n.Bl -column A B -compact\n{row}\n.El\nY\n.fi\n{FOOTER}");
        assert_eq!(physical_rows(&round_trip(&source)), expected, "{source}");
    }
}

#[test]
fn column_post_consumes_the_live_display_owner_and_preserves_link_identity() {
    // All three complete sources ran pristine ASCII/UTF-8/HTML/tree/lint
    // first, lint=0. Bd BODY post flushes Marker but NOBREAK keeps its
    // device row open (mdoc_term.c:1482, term.c:250-253). The later It
    // post consumes the same native field and rejects its leading marker
    // suffix. Returning a display's IR owner cannot accept that suffix.
    for carrier in ["No", "Em", "Lk"] {
        let operand = if carrier == "Lk" {
            "Lk https://ex.org"
        } else {
            carrier
        };
        let source = format!(
            "{HEADER}.Bl -column \"xxxxxxxx\" \"xxxx\"\n.It Xo\n.Bd -literal -compact\nMarker\n.Ed\n.{operand} \"\\p D\"\n.No AFTER\n.Xc Ta RightWord\n.El\n{FOOTER}"
        );
        let query = round_trip(&source);
        assert_eq!(physical_rows(&query), ["Marker      RightWord"], "{source}");
        let rows = description_table(&query);
        let mut identities = 0;
        for block in &rows[0].cells[0].blocks {
            if let Block::Paragraph { children, .. } | Block::Preformatted { children, .. } = block
            {
                assert_empty_label(children, &mut identities);
            }
        }
        assert_eq!(identities, usize::from(carrier == "Lk"), "{source}");
    }
}

#[test]
fn native_column_row_closes_survive_json_and_ansi_cell_layout() {
    // Both exact complete sources ran pristine in five profiles, lint=0.
    // It post closes a real filled row after a nested item; Bd post instead
    // leaves Marker/AFTER on one device row. Column layout must distinguish
    // that open-row delimiter from an additional term_vspace() receipt.
    for (control, expected, paragraph_tail) in [
        (
            ".Bl -item -compact\n.It\n.No Marker\n.El\n",
            vec!["Marker", "AFTER", "RightWord: https://e.example/x"],
            true,
        ),
        (
            ".Bd -literal -compact\nMarker\n.Ed\n",
            vec!["Marker AFTER", "RightWord: https://e.example/x"],
            false,
        ),
    ] {
        let source = format!(
            "{HEADER}.Bl -column \"xxxxxxxx\" \"xxxx\"\n.It Xo\n.No \"\\p D\"\n{control}.No AFTER\n.Xc Ta Lk https://e.example/x RightWord\n.El\n{FOOTER}"
        );
        let query = round_trip(&source);
        assert_eq!(physical_rows(&query), expected, "{source}");
        let rows = description_table(&query);
        assert_eq!(
            matches!(
                rows[0].cells[0].blocks.last(),
                Some(Block::Paragraph { .. })
            ),
            paragraph_tail,
            "the filled paragraph retains its block role: {source}"
        );
        let plain = mant_render::render_query_text(&query);
        let styled =
            mant_render::render_query_text_with(&query, |_, text| format!("\x1b[1m{text}\x1b[0m"));
        assert_eq!(styled.replace("\x1b[1m", "").replace("\x1b[0m", ""), plain);
    }
}

#[test]
fn nested_list_head_and_body_posts_clear_their_own_field_flags() {
    // Four exact sources ran pristine ASCII/UTF-8/HTML/tree/lint first,
    // all lint=0. mdoc_term.c::termp_it_post clears the native field flags
    // for every non-BLOCK part, including the skipped ordinary-list HEAD;
    // BODY additionally executes term_newln. Outer column flags cannot be
    // restored merely because its Rust output owner is still active.
    for (style, marker) in [
        ("item", "Marker"),
        ("bullet", "•   Marker"),
        ("dash", "-   Marker"),
        ("enum", "1.   Marker"),
    ] {
        let source = format!(
            "{HEADER}.Bl -column \"xxxxxxxx\" \"xxxx\"\n.It Xo\n.No \"\\p D\"\n.Bl -{style} -compact\n.It\n.No Marker\n.El\n.No AFTER\n.Xc Ta RightWord\n.El\n{FOOTER}"
        );
        let query = round_trip(&source);
        assert_eq!(physical_rows(&query), [marker, "AFTER", "RightWord"]);
    }
}

#[test]
fn display_pre_and_source_line_flushes_remain_distinct_in_columns() {
    // Eight exact complete sources ran pristine in five profiles before
    // these rows were written, all lint=0. print_mdoc_node's NODE_LINE
    // gate precedes Bd's print_bvspace and the BODY post's term_newln.
    // A compact display removes only its vertical request; it cannot skip
    // the source gate or invent an extra output-owner-return newline.
    for (no_fill, compact, accepted, expected) in [
        (false, false, false, vec!["", "Marker AFTER", "RightWord"]),
        (
            false,
            false,
            true,
            vec!["LEFT", "Marker AFTER", "RightWord"],
        ),
        (false, true, false, vec!["Marker AFTER", "RightWord"]),
        (false, true, true, vec!["LEFT Marker AFTER", "RightWord"]),
        (
            true,
            false,
            false,
            vec!["", "", "Marker AFTER", "RightWord"],
        ),
        (
            true,
            false,
            true,
            vec!["", "LEFT", "Marker AFTER", "RightWord"],
        ),
        (true, true, false, vec!["", "Marker AFTER", "RightWord"]),
        (
            true,
            true,
            true,
            vec!["", "LEFT Marker", "AFTER       RightWord"],
        ),
    ] {
        let source = format!(
            "{HEADER}{}.Bl -column \"xxxxxxxx\" \"xxxx\"\n.It Xo\n.No {}\n.Bd -literal{}\nMarker\n.Ed\n.No AFTER\n.Xc Ta RightWord\n.El\n{}{FOOTER}",
            if no_fill { ".nf\n" } else { "" },
            if accepted { "LEFT" } else { "\"\\p D\"" },
            if compact { " -compact" } else { "" },
            if no_fill { ".fi\n" } else { "" },
        );
        let query = round_trip(&source);
        assert_eq!(physical_rows(&query), expected, "{source}");
    }
}

#[test]
fn single_line_displays_apply_native_origin_until_their_real_post() {
    // All 36 exact sources ran pristine in five profiles, lint=0, before
    // these expectations. termp_d1_pre adds defindent+1 to the BLOCK offset;
    // print_mdoc_node restores it only after termp_bl_post's term_newln.
    // This offset affects field overrun, not merely displayed indentation.
    for macro_name in ["D1", "Dl"] {
        for width in [8, 12, 20] {
            for no_fill in [false, true] {
                for operand in ["Marker", r"\zX", r"\&"] {
                    let source = format!(
                        "{HEADER}{}.Bl -column \"{}\" \"xxxx\"\n.It Xo\n.No \"\\p D\"\n.{macro_name} {operand}\n.No AFTER\n.Xc Ta RightWord\n.El\n{}{FOOTER}",
                        if no_fill { ".nf\n" } else { "" },
                        "x".repeat(width),
                        if no_fill { ".fi\n" } else { "" }
                    );
                    let mut expected = match (width, operand) {
                        (8, "Marker") => vec!["Marker", "AFTER       RightWord"],
                        (8, r"\zX") => vec!["X AFTER", "RightWord"],
                        (8, r"\&") => vec!["AFTER       RightWord"],
                        (12, "Marker") => vec!["Marker AFTER", "RightWord"],
                        (12, r"\zX") => vec!["X AFTER   RightWord"],
                        (12, r"\&") => vec!["AFTER           RightWord"],
                        (20, "Marker") => vec!["Marker AFTER      RightWord"],
                        (20, r"\zX") => vec!["X AFTER           RightWord"],
                        (20, r"\&") => vec!["AFTER                   RightWord"],
                        _ => unreachable!("three frozen widths and native cell forms"),
                    };
                    if no_fill {
                        expected.insert(0, "");
                    }
                    assert_eq!(physical_rows(&round_trip(&source)), expected, "{source}");
                }
            }
        }
    }
}

#[test]
fn ordinary_column_overruns_remain_geometry_without_polluting_cell_words() {
    // Four complete No/Dv × short/overrun sources ran pristine first,
    // five profiles and lint=0. Column BODY post's NOBREAK overrun is
    // responsive placement, rather than an authored inline break. The
    // nested-It clear-NOBREAK counterparts above do preserve a hard row.
    for macro_name in ["No", "Dv"] {
        for first in ["X", "CLSET_TIMEOUT"] {
            let source = format!(
                "{HEADER}.Bl -column \"name\" \"type\" \"description\"\n.It {macro_name} {first} Ta \"struct timeval *\" Ta \"set total timeout\"\n.El\n{FOOTER}"
            );
            let query = round_trip(&source);
            let [row] = description_table(&query) else {
                panic!("one actual column It expected: {source}")
            };
            let words = row
                .cells
                .iter()
                .map(|cell| {
                    let [Block::Paragraph { children, .. }] = cell.blocks.as_slice() else {
                        panic!("one ordinary cell paragraph: {source}: {cell:?}")
                    };
                    mant_ir::inline_plain_text(children)
                })
                .collect::<Vec<_>>();
            assert_eq!(words, [first, "struct timeval *", "set total timeout"]);
        }
    }
}

#[test]
fn display_vertical_pre_observes_the_post_flush_device_row() {
    // These four complete sources ran pristine in five profiles, lint=0.
    // print_bvspace has its own term_newln, then term_vspace invokes a
    // second one. A nearly full NOBREAK row can therefore end before the
    // requested backend endline, unlike the short LEFT counterpart above.
    for (no_fill, compact, expected) in [
        (
            false,
            false,
            vec!["LEFT1234567", "", "Marker AFTER", "RightWord"],
        ),
        (
            false,
            true,
            vec!["LEFT1234567", "Marker AFTER", "RightWord"],
        ),
        (
            true,
            false,
            vec!["", "LEFT1234567", "", "Marker AFTER", "RightWord"],
        ),
        (
            true,
            true,
            vec!["", "LEFT1234567", "Marker AFTER", "RightWord"],
        ),
    ] {
        let source = format!(
            "{HEADER}{}.Bl -column \"xxxxxxxx\" \"xxxx\"\n.It Xo\n.No LEFT1234567\n.Bd -literal{}\nMarker\n.Ed\n.No AFTER\n.Xc Ta RightWord\n.El\n{}{FOOTER}",
            if no_fill { ".nf\n" } else { "" },
            if compact { " -compact" } else { "" },
            if no_fill { ".fi\n" } else { "" },
        );
        assert_eq!(physical_rows(&round_trip(&source)), expected, "{source}");
    }
}

#[test]
fn generated_container_words_reuse_the_live_column_display_destination() {
    // Six exact sources ran pristine CVS ASCII/UTF-8/HTML/tree/lint first,
    // all lint-clean. mdoc_term.c runs termp_bd_post's term_newln in the
    // column buffer; enclosure/Fo/Fa generated term_word calls then consume
    // that same buffer. A filled generated word does not drain its literal
    // IR destination. Only the It BODY post executes the column field tail.
    for no_fill in [false, true] {
        for (body, filled, literal) in [
            (
                ".Ao\n.No ARG\n.Ac\n",
                vec!["Marker ⟨ARG⟩            RightWord"],
                vec!["Marker ⟨ ARG⟩           RightWord"],
            ),
            (
                ".Fo call\n.Fa first\n.Fa second\n.Fc\n",
                vec!["Marker call(first, second)", "RightWord"],
                vec!["Marker call( first, second)", "RightWord"],
            ),
            (
                ".Eo OPEN\n.No ARG\n.Ec CLOSE\n",
                vec!["Marker OPENARGCLOSE     RightWord"],
                vec!["Marker OPEN ARG CLOSE   RightWord"],
            ),
        ] {
            let source = format!(
                "{HEADER}{}.Bl -column \"xxxxxxxxxxxxxxxxxxxx\" \"xxxx\"\n.It Xo\n.Bd -literal -compact\nMarker\n.Ed\n{body}.Xc Ta RightWord\n.El\n{}{FOOTER}",
                if no_fill { ".nf\n" } else { "" },
                if no_fill { ".fi\n" } else { "" },
            );
            let parsed = Parser::default()
                .parse_bytes("column-generated-words.1", source.as_bytes())
                .unwrap();
            assert!(parsed.diagnostics.is_empty(), "{source}");
            let item = first_column_item(&parsed.document.root).unwrap();
            assert_eq!(
                item.children
                    .iter()
                    .filter(|child| child.kind == NodeKind::Body)
                    .count(),
                2,
                "two actual column BODY scopes: {source}"
            );
            let mut expected = if no_fill { literal } else { filled };
            if no_fill {
                expected.insert(0, "");
            }
            assert_eq!(physical_rows(&round_trip(&source)), expected, "{source}");
        }
    }
}

#[test]
fn printed_temporary_origins_preserve_column_hard_rows_and_word_boundaries() {
    // All 24 complete sources ran pristine in five profiles, lint=0, before
    // these assertions. term_field writes an origin advance only before an
    // actual printed graph (term.c:397-434); restoring the node offset does
    // not rewind viscol (mdoc_term.c:437). Its later column post must retain
    // the resulting row close. The responsive reading contract does not copy
    // this temporary device padding inside a cell. These sources contain no
    // authored blank strings: compare accepted scalars on EACH physical row,
    // preserving empty rows, and independently require ordinary word gaps.
    // The separate 36-source test above retains its exact full-row assertions.
    for macro_name in ["D1", "Dl", "Bd"] {
        for prefix in [false, true] {
            for operand in ["X", r"\zX", r"\&", r"\z"] {
                let display = if macro_name == "Bd" {
                    format!(".Bd -literal -compact -offset 6n\n{operand}\n.Ed\n")
                } else {
                    format!(".{macro_name} {operand}\n")
                };
                let source = format!(
                    "{HEADER}.Bl -column \"xxxxxxxx\" \"xxxx\"\n.It Xo\n{}{display}.No AFTER\n.Xc Ta RightWord\n.El\n{FOOTER}",
                    if prefix { ".No LEFT\n" } else { "" }
                );
                let expected = match (prefix, operand) {
                    (false, "X" | r"\zX") => vec!["X AFTER", "RightWord"],
                    (false, r"\&") => vec!["AFTER       RightWord"],
                    (false, r"\z") => vec!["FTER        RightWord"],
                    (true, "X" | r"\zX") => vec!["LEFT  X AFTER", "RightWord"],
                    (true, r"\&" | r"\z") => vec!["LEFT AFTER  RightWord"],
                    _ => unreachable!("four native cell forms"),
                };
                let query = round_trip(&source);
                let actual = physical_rows(&query);
                let accepted = |row: &str| row.chars().filter(|ch| *ch != ' ').collect::<String>();
                assert_eq!(
                    actual.iter().map(|row| accepted(row)).collect::<Vec<_>>(),
                    expected.iter().map(|row| accepted(row)).collect::<Vec<_>>(),
                    "accepted scalars and physical rows: {source}: {actual:?}"
                );
                for row in &actual {
                    if row.contains("LEFT") {
                        assert!(row.contains("LEFT "), "prefix word gap: {source}: {row:?}");
                    }
                    if row.contains("XAFTER") || row.contains("X AFTER") {
                        assert!(
                            row.contains("X AFTER"),
                            "display word gap: {source}: {row:?}"
                        );
                    }
                    assert!(
                        !row.contains("AFTERRightWord"),
                        "column word gap: {source}: {row:?}"
                    );
                    assert!(
                        !row.contains("FTERRightWord"),
                        "column word gap: {source}: {row:?}"
                    );
                }
                let plain = mant_render::render_query_text(&query);
                let styled = mant_render::render_query_text_with(&query, |_, text| {
                    format!("\x1b[1m{text}\x1b[0m")
                });
                assert_eq!(styled.replace("\x1b[1m", "").replace("\x1b[0m", ""), plain);
            }
        }
    }
}

#[test]
fn completed_literal_rows_survive_large_owner_transfers() {
    // These exact 256/1024/4096-row sources ran pristine in all five
    // profiles before the assertion. Each empty TEXT takes term_vspace(),
    // and fi takes term_newln() (term.c:489, roff_term.c:48). Transferring
    // the literal owner must preserve every completed empty row once.
    for rows in [256_usize, 1024, 4096] {
        let source = format!(
            ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\n.nf\nBEFORE\n{}.fi\n.SH NEXT\nAFTER\n",
            "\n".repeat(rows)
        );
        let query = round_trip(&source);
        let mut expected = vec!["BEFORE".to_owned()];
        expected.extend(std::iter::repeat_n(String::new(), rows));
        assert_eq!(physical_rows(&query), expected, "{rows} native empty rows");
        let section = query
            .document
            .as_ref()
            .unwrap()
            .sections
            .iter()
            .find(|section| section.id.as_str() == "description")
            .unwrap();
        assert!(
            section
                .blocks
                .iter()
                .all(|block| !matches!(block, Block::VerticalSpace { .. })),
            "raw literal rows cannot enter the bounded spacing plan: {rows}"
        );
        let literal_rows = section
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Preformatted { children, .. } => Some(mant_ir::inline_plain_text(children)),
                _ => None,
            })
            .map(|text| text.matches('\n').count())
            .sum::<usize>();
        assert_eq!(literal_rows, rows, "all raw rows remain content in IR");
    }
}

#[test]
fn accepted_display_rows_restore_the_next_physical_origin() {
    // These four exact sources ran pristine in five profiles, lint=0.
    // term_field() prints the saved offset only at accepted graph; its
    // next actual endline resets viscol, and print_mdoc_node restores the
    // enclosing offset after post (term.c:397-434; mdoc_term.c:437-439).
    // A split into existing layout blocks must neither add a second close
    // nor keep the old six-cell offset on the later physical row.
    for (body, expected) in [
        (
            ".D1 \"X\\p Y\"\n.No AFTER\n",
            vec!["X", "Y AFTER   RightWord"],
        ),
        (".D1 X\n.br\n.No AFTER\n", vec!["X AFTER   RightWord"]),
        (
            ".D1 Marker\n.No AFTER\n.br\n.No NEXTWORD\n",
            vec!["Marker AFTER", "NEXTWORD        RightWord"],
        ),
        (".D1 \"  X\"\n.No AFTER\n", vec!["X AFTER RightWord"]),
    ] {
        let source = format!(
            "{HEADER}.Bl -column \"xxxxxxxxxxxx\" \"xxxx\"\n.It Xo\n{body}.Xc Ta RightWord\n.El\n{FOOTER}"
        );
        let query = round_trip(&source);
        assert_eq!(physical_rows(&query), expected, "{source}");
        let text = mant_render::render_query_text(&query);
        let styled = mant_render::render_query_text_with(&query, |_, value| {
            format!("\x1b[1m{value}\x1b[0m")
        });
        assert_eq!(styled.replace("\x1b[1m", "").replace("\x1b[0m", ""), text);
    }
}

#[test]
fn no_break_flush_keeps_the_origins_of_accepted_prefixes() {
    // Both exact sources ran pristine in ASCII/UTF-8/HTML/tree/lint first,
    // lint=0. roff_term_pre_mc holds NOBREAK while term_flushln consumes the
    // accepted prefix, then resets the same buffer (roff_term.c:147-150;
    // term.c:143-146,233-237). Rejected suffix projection must not advance
    // the consumed owner's start before its accepted row origins are read.
    for (operand, expected) in [
        (r"X\p \p DROP\c", vec!["X", "AFTER", "RightWord"]),
        (r"X\p Y\c", vec!["X", "Y", "AFTER", "RightWord"]),
    ] {
        let source = format!(
            "{HEADER}.Bl -column \"xxxxxxxx\" \"xxxx\"\n.It Xo\n.Bd -literal -compact -offset 6n\n.No \"{operand}\"\n.mc\n.Ed\n.No AFTER\n.Xc Ta RightWord\n.El\n{FOOTER}"
        );
        let query = round_trip(&source);
        assert_eq!(physical_rows(&query), expected, "{source}");
        let rows = description_table(&query);
        let literals = rows[0].cells[0]
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Preformatted {
                    children, layout, ..
                } => Some((mant_ir::inline_plain_text(children), layout.indent_columns)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(
            literals
                .iter()
                .any(|(text, origin)| text.starts_with('X') && *origin == 6),
            "accepted X keeps its native source origin: {source}\n{literals:?}"
        );
        assert!(
            literals
                .iter()
                .any(|(text, origin)| text.contains("AFTER") && *origin == 0),
            "later source rows restore the enclosing origin: {source}\n{literals:?}"
        );
        let text = mant_render::render_query_text(&query);
        assert!(text.lines().any(|row| row == "      X"), "{text:?}");
        assert!(!text.contains("DROP"), "rejected glyphs remain rejected");
        let styled = mant_render::render_query_text_with(&query, |_, value| {
            format!("\x1b[1m{value}\x1b[0m")
        });
        assert_eq!(styled.replace("\x1b[1m", "").replace("\x1b[0m", ""), text);
    }
}

#[test]
fn resumed_native_fields_preserve_only_their_unprinted_separator() {
    // All eight exact sources ran pristine in five profiles with lint=0.
    // roff_term_pre_mc() clears only NOBREAK/NOSPACE: a Column must retain
    // its actual flags rather than acquire TAG's BRIND (roff_term.c:147-150).
    // term_field() prints deferred positioning only with this field's next
    // graph (term.c:389-427); accepted prefix, raw suffix, and identities
    // cannot be trimmed by a pending-only future separator's cell count.
    for (body, expected, indented) in [
        (
            ".No \"X\\p Y\\c\"\n.mc\n.No Z\n.mc\n",
            vec!["X", "Y  Z", "AFTER", "RightWord"],
            vec!["      X", "      Y  Z"],
        ),
        (
            ".No \"X\\p \\p DROP\\c\"\n.mc\n.No Z\n.mc\n",
            vec!["X", "Z", "AFTER", "RightWord"],
            vec!["      X", "       Z"],
        ),
        (
            ".Em \"X\\p Y\\c\"\n.mc\n.Em Z\n",
            vec!["X", "Y  Z", "AFTER", "RightWord"],
            vec!["      X", "      Y  Z"],
        ),
        (
            ".Em \"X\\p \\p DROP\\c\"\n.mc\n.Em Z\n",
            vec!["X", "Z", "AFTER", "RightWord"],
            vec!["      X", "       Z"],
        ),
        (
            ".No \"X\\p Y\\c\"\n.mc\n.No \\&\n",
            vec!["X", "Y", "AFTER", "RightWord"],
            vec!["      X", "      Y"],
        ),
        (
            ".No \\&\n.mc\n",
            vec!["AFTER       RightWord"],
            vec!["AFTER       RightWord"],
        ),
        (
            ".No \"X\\p Y\\c\"\n.mc\n.No \"Z\\p Q\\c\"\n",
            vec!["X", "Y  Z", "Q", "AFTER", "RightWord"],
            vec!["      X", "      Y  Z", "      Q"],
        ),
        (
            ".No \"X\\p \\p DROP\\c\"\n.mc\n.No \"Z\\p Q\\c\"\n",
            vec!["X", "Z", "Q", "AFTER", "RightWord"],
            vec!["      X", "       Z", "      Q"],
        ),
    ] {
        let source = format!(
            "{HEADER}.Bl -column \"xxxxxxxx\" \"xxxx\"\n.It Xo\n.Bd -literal -compact -offset 6n\n{body}.Ed\n.No AFTER\n.Xc Ta RightWord\n.El\n{FOOTER}"
        );
        let query = round_trip(&source);
        assert_eq!(physical_rows(&query), expected, "{source}");
        let text = mant_render::render_query_text(&query);
        for row in indented {
            assert!(text.lines().any(|line| line == row), "{row:?}: {text:?}");
        }
        assert!(!text.contains("DROP"), "rejected glyphs remain rejected");
        let styled = mant_render::render_query_text_with(&query, |_, value| {
            format!("\x1b[1m{value}\x1b[0m")
        });
        assert_eq!(styled.replace("\x1b[1m", "").replace("\x1b[0m", ""), text);
    }
}

#[test]
fn literal_content_and_spacing_rows_keep_their_execution_order() {
    // These exact full sources ran pristine in ASCII/UTF-8/HTML/tree/lint,
    // all lint=0, before the assertions. Empty NODE_NOFILL TEXT executes
    // term_vspace() as content; roff_term_pre_sp() executes bounded layout.
    // Both resolve skipvsp in term.c:489-497 at their own source positions.
    for (body, empty_rows, layout_rows) in [
        ("\n.sp 1\n\n".to_owned(), 3_usize, 1_u16),
        (".sp 1\n\n.sp 1\n".to_owned(), 3, 2),
        ("\n.sp 2\n\n".to_owned(), 4, 2),
        (".sp -1\n\n.sp 1\n".to_owned(), 1, 1),
        ("\n.sp -1\n\n".to_owned(), 1, 0),
        (format!(".sp 1\n{}", "\n".repeat(2)), 3, 1),
        (format!(".sp 1\n{}", "\n".repeat(4095)), 4096, 1),
        (format!(".sp 1\n{}", "\n".repeat(4096)), 4097, 1),
        (format!(".sp 1\n{}", "\n".repeat(4097)), 4098, 1),
        (format!("{}.sp 1\n", "\n".repeat(2)), 3, 1),
        (format!("{}.sp 1\n", "\n".repeat(4095)), 4096, 1),
        (format!("{}.sp 1\n", "\n".repeat(4096)), 4097, 1),
        (format!("{}.sp 1\n", "\n".repeat(4097)), 4098, 1),
    ] {
        let source = format!(
            ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\n.nf\nBEFORE\n{body}.fi\n.SH NEXT\nAFTER\n"
        );
        let query = round_trip(&source);
        let mut expected = vec!["BEFORE".to_owned()];
        expected.extend(std::iter::repeat_n(String::new(), empty_rows));
        assert_eq!(physical_rows(&query), expected, "{source}");
        let section = query
            .document
            .as_ref()
            .unwrap()
            .sections
            .iter()
            .find(|section| section.id.as_str() == "description")
            .unwrap();
        let layout = section
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::VerticalSpace { lines, .. } => Some(*lines),
                _ => None,
            })
            .sum::<u16>();
        assert_eq!(
            layout, layout_rows,
            "only spacing requests are layout: {source}"
        );
        let text = mant_render::render_query_text(&query);
        let styled = mant_render::render_query_text_with(&query, |_, value| {
            format!("\x1b[1m{value}\x1b[0m")
        });
        assert_eq!(styled.replace("\x1b[1m", "").replace("\x1b[0m", ""), text);
    }
}
