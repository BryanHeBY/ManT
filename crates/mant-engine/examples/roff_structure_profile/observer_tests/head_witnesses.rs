//! Mutate the observed owner while retaining the native source witness.

use super::*;

fn violations(native: &AstTopology, doc: &Document) -> Vec<String> {
    let mut issues = Vec::new();
    native.head_payloads.compare(doc, &mut issues);
    issues
}

#[test]
fn generated_list_markers_cannot_substitute_for_equal_author_words() {
    // mdoc_term.c::termp_it_pre writes a marker before the BODY. The
    // fixed devices show both occurrences even when they look alike.
    for (kind, marker) in [
        ("dash", "-"),
        ("hyphen", "-"),
        ("bullet", "•"),
        ("enum", "1."),
    ] {
        let (_, native, document) = parsed(&structured_term(&format!(
            ".Bl -{kind}\n.It\n{marker}\n.El\n"
        )));
        assert_term_observation(&native, &document);
        for generated in [false, true] {
            let mut broken = document.clone();
            let nodes = first_term(&mut broken);
            let marker_index = nodes
                .iter()
                .position(|node| mant_ir::inline_plain_text(std::slice::from_ref(node)) == marker)
                .unwrap();
            let index = if generated {
                marker_index
            } else {
                nodes
                    .iter()
                    .rposition(|node| {
                        mant_ir::inline_plain_text(std::slice::from_ref(node)) == marker
                    })
                    .filter(|&index| index > marker_index)
                    .unwrap()
            };
            nodes.remove(index);
            assert_eq!(
                violations(&native, &broken).len(),
                1,
                "{kind}, generated={generated}"
            );
        }
    }
}

#[test]
fn short_display_delimiters_and_continued_words_keep_their_own_rows() {
    // blk_part_imp retains closing punctuation outside BODY; term_word
    // joins it to the preceding word. print_mdoc_node honors NONEWLINE.
    for (source, remove) in [(".Dl ALPHA ,\n", ","), (".D1 ( ALPHA )\n", ")")] {
        let (_, native, mut document) = parsed(&structured_term(source));
        assert_term_observation(&native, &document);
        let nodes = first_term(&mut document);
        let end = nodes
            .iter_mut()
            .rev()
            .find_map(|node| {
                if let Inline::Text { value } = node {
                    value.ends_with(remove).then_some(value)
                } else {
                    None
                }
            })
            .unwrap();
        end.truncate(end.len() - remove.len());
        assert_eq!(violations(&native, &document).len(), 1, "{source}");
    }
    let (_, native, mut document) = parsed(&structured_term(".Bd -literal\nA\\c\n.No B\n.Ed\n"));
    assert_term_observation(&native, &document);
    let nodes = first_term(&mut document);
    let at = nodes
        .iter()
        .position(|node| matches!(node, Inline::Text { value } if value == "B"))
        .unwrap();
    nodes.insert(at, Inline::line_break());
    assert_eq!(violations(&native, &document).len(), 1);
}

#[test]
fn exact_hard_rows_distinguish_blanks_separate_displays_and_separate_terms() {
    // Fixed CVS keeps an authored blank between the same display's text
    // rows, and both short and long display handlers close their fields.
    for source in [
        ".Bd -literal\nALPHA\n\nBRAVO\n.Ed\n",
        ".Bd -literal -compact\nALPHA\n.Ed\n.Bd -literal -compact\nBRAVO\n.Ed\n",
        ".Dl ALPHA\n.Dl BRAVO\n",
        ".D1 ALPHA\n.D1 BRAVO\n",
        ".Dl ALPHA\n.Bd -literal -compact\nBRAVO\n.Ed\n",
        ".Bd -literal -compact\nALPHA\n.Ed\n.Dl BRAVO\n",
    ] {
        let (_, native, mut document) = parsed(&structured_term(source));
        assert_term_observation(&native, &document);
        let nodes = first_term(&mut document);
        let last = nodes
            .iter()
            .rposition(|node| matches!(node, Inline::LineBreak {}))
            .unwrap();
        nodes[last] = Inline::Text { value: " ".into() };
        let issue = violations(&native, &document);
        assert!(
            issue
                .iter()
                .any(|message| message.contains("missing hard line")),
            "{source} {issue:?}"
        );
    }
    let (_, native, mut document) =
        parsed(&structured_term(".Bd -literal\n.No ALPHA BRAVO\n.Ed\n"));
    assert_term_observation(&native, &document);
    let nodes = first_term(&mut document);
    let at = nodes
        .iter()
        .position(|node| matches!(node, Inline::Text { value } if value == "ALPHA BRAVO"))
        .unwrap();
    nodes.splice(
        at..=at,
        [
            Inline::Text {
                value: "ALPHA".into(),
            },
            Inline::line_break(),
            Inline::Text {
                value: "BRAVO".into(),
            },
        ],
    );
    assert!(violations(&native, &document)[0].contains("unexpected hard line"));

    let (_, native, mut document) = parsed(&structured_term(".Bd -literal\nALPHA\nBRAVO\n.Ed\n"));
    assert_term_observation(&native, &document);
    let Block::DefinitionList { items, .. } = &mut document.sections[1].blocks[0] else {
        unreachable!()
    };
    let mut later = items[0].terms[0].clone();
    let term = &mut items[0].terms[0];
    let split = term
        .content
        .iter()
        .rposition(|node| matches!(node, Inline::LineBreak {}))
        .unwrap();
    later.content = term.content.split_off(split + 1);
    term.content.pop();
    items[0].terms.push(later);
    assert!(violations(&native, &document)[0].contains("separate term"));
}

#[test]
fn tagged_display_rows_include_empty_field_retirement() {
    // CVS termp_d1_pre and print_mdoc_node can retire an empty field
    // with the previous tag's displayed columns and trailing spacing.
    for (width, body, breaks) in [
        ("a", ".Bd -literal\nA\n\nB\n.Ed\n", 2),
        ("6n", ".D1 A\n.D1 B\n", 1),
        ("8n", ".D1 A\n.D1 BRAVO\n", 1),
        ("10n", ".D1 A\n.D1 BRAVO\n", 0),
    ] {
        let source = structured_term(body).replace("-width a", &format!("-width {width}"));
        let (_, native, document) = parsed(&source);
        assert!(
            violations(&native, &document).is_empty(),
            "{width}: {:?}",
            violations(&native, &document)
        );
        let mut edited = document.clone();
        let nodes = first_term(&mut edited);
        let right = if body.contains("BRAVO") { "BRAVO" } else { "B" };
        let index = nodes
            .iter()
            .position(|node| mant_ir::inline_plain_text(std::slice::from_ref(node)).trim() == right)
            .unwrap();
        nodes.insert(index, Inline::line_break());
        assert!(
            violations(&native, &edited)[0].contains("unexpected hard line"),
            "{width}"
        );
        if breaks > 0 {
            let mut edited = document.clone();
            let nodes = first_term(&mut edited);
            let before = nodes[..index]
                .iter()
                .rposition(|node| matches!(node, Inline::LineBreak {}))
                .unwrap();
            nodes[before] = Inline::Text { value: " ".into() };
            assert!(
                violations(&native, &edited)[0].contains("missing hard line"),
                "{width}"
            );
        }
    }
}

#[test]
fn function_opening_can_join_two_required_authored_words() {
    // CVS termp_fo_pre writes '(' with NOSPACE on either side.
    fn erase(nodes: &mut [Inline], word: &str) {
        for node in nodes {
            match node {
                Inline::Text { value } | Inline::Code { value } => *value = value.replace(word, ""),
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => erase(children, word),
                _ => {}
            }
        }
    }
    for (body, argument) in [
        (".Fo out\n.No ALPHA\n.Dl PROBE\n.Fc\n", "ALPHA"),
        (".Fo out\n.Oo\n.Dl PROBE\n.Oc\n.Fc\n", "PROBE"),
        (".Oo\n.Fo out\n.No ALPHA\n.Dl PROBE\n.Fc\n.Oc\n", "ALPHA"),
    ] {
        let (_, native, document) = parsed(&structured_term(body));
        assert!(
            violations(&native, &document).is_empty(),
            "{body}: {:?}",
            violations(&native, &document)
        );
        for word in ["out", argument] {
            let mut edited = document.clone();
            erase(first_term(&mut edited), word);
            assert!(!violations(&native, &edited).is_empty(), "{body}: {word}");
        }
    }
}

#[test]
fn equations_remain_in_their_proved_outer_owners() {
    // roff_eqndelim emits a local equation node between source text
    // fragments. Sh and Pp supply distinct enclosing owners.
    for (body, inline) in [
        ("BEFORE\n.EQ\nx\n.EN\n.Sh NEXT\nLATER\n.EQ\ny\n.EN\n", false),
        (".EQ\ndelim $$\n.EN\nA $x$\n.Pp\nB $y$\n", true),
    ] {
        let source = format!("{DISPATCH_HEADER}{body}.Sh ENDPROBE\nDone.\n");
        let (expected, native, mut document) = parsed(&source);
        let check = |observed: &Document| {
            let (structure, topology) = ir_profile(observed);
            compare_structure(&expected, &structure, &native, &topology)
        };
        assert!(check(&document).is_empty(), "{:?}", check(&document));
        if inline {
            let [first, _, last] = document.sections[1].blocks.as_mut_slice() else {
                panic!("paragraphs and Pp")
            };
            let (
                Block::Paragraph {
                    children: source, ..
                },
                Block::Paragraph {
                    children: target, ..
                },
            ) = (first, last)
            else {
                panic!("two paragraphs")
            };
            let index = source
                .iter()
                .position(|node| matches!(node, Inline::Equation { .. }))
                .unwrap();
            target.insert(0, source.remove(index));
        } else {
            let block = document.sections[1].blocks.pop().unwrap();
            document.sections[2].blocks.insert(0, block);
        }
        let issues = check(&document);
        let expected = if inline { "in paragraph" } else { "in section" };
        assert!(
            issues.iter().any(|issue| issue.contains(expected)),
            "{issues:?}"
        );
    }
}

#[test]
fn partially_proved_wrappers_still_owe_their_table_words_and_equations() {
    // Oo and Bk still descend into native child blocks. A Review on the
    // wrapper's boundary is independent from the proof of its descendants.
    for (start, end) in [("Oo", "Oc"), ("Bk -words", "Ek")] {
        let (_, native, document) = parsed(&structured_term(&format!(
            ".{start}\n.Bl -column a b\n.It LEFT Ta RIGHT\n.El\n.{end}\n"
        )));
        let mut issues = Vec::new();
        assert_eq!(
            native.head_payloads.compare(&document, &mut issues).len(),
            1
        );
        assert!(issues.is_empty(), "{issues:?}");
        let mut broken = document.clone();
        first_term(&mut broken)
            .retain(|node| !matches!(node, Inline::Text { value } if value == "RIGHT"));
        assert!(violations(&native, &broken)[0].contains("RIGHT"));

        let (_, native, mut document) = parsed(&structured_term(&format!(
            ".Bl -column a\n.It ALPHA\n.El\n.{start}\n.No OPAQUE\n.{end}\n.Bl -column a\n.It OMEGA\n.El\n"
        )));
        let nodes = first_term(&mut document);
        let at = nodes
            .iter()
            .position(|node| matches!(node, Inline::Text { value } if value == "ALPHA"))
            .unwrap();
        nodes.splice(at..at, [nodes[at].clone(), Inline::line_break()]);
        assert!(!violations(&native, &document).is_empty());
    }
}

#[test]
fn structural_equation_payloads_stay_at_their_term_and_column_addresses() {
    // A text expression can carry an empty value; configuration alone
    // carries no payload. roff_eqndelim yields an owned Equation in the
    // second It BODY, which termp_it_pre treats as the second column.
    for body in ["\"\"", "\\&", "delim $$"] {
        let (_, native, document) = parsed(&structured_term(&format!(".EQ\n{body}\n.EN\n")));
        assert_term_observation(&native, &document);
    }
    for (start, end) in [("Oo", "Oc"), ("Bk -words", "Ek")] {
        let (_, native, mut document) = parsed(&structured_term(&format!(
            ".{start}\n.EQ\nx\n.EN\n.{end}\n"
        )));
        assert!(violations(&native, &document).is_empty());
        let Block::DefinitionList { items, .. } = &mut document.sections[1].blocks[0] else {
            unreachable!()
        };
        let item = &mut items[0];
        let nodes = &mut item.terms[0].content;
        let at = nodes
            .iter()
            .position(|n| matches!(n, Inline::Equation { .. }))
            .unwrap();
        let equation = nodes.remove(at);
        let Block::Paragraph { children, .. } = &mut item.description[0] else {
            unreachable!()
        };
        children.push(equation);
        assert!(violations(&native, &document)[0].contains("definition-head-equation"));
    }
    let source = format!(
        "{DISPATCH_HEADER}.EQ\ndelim $$\n.EN\n.Bl -column a b\n.It ALPHA Ta BRAVO\ntext $x$ tail\n.It NEXT Ta LAST\n.El\n.Sh ENDPROBE\nDone.\n"
    );
    let (expected, native, document) = parsed(&source);
    let check = |observed: &Document| {
        let (ir, topology) = ir_profile(observed);
        compare_structure(&expected, &ir, &native, &topology)
    };
    assert!(check(&document).is_empty());
    for (row, column) in [(0, 0), (1, 0), (1, 1)] {
        let mut edited = document.clone();
        let Block::Table { rows, .. } = &mut edited.sections[1].blocks[0] else {
            unreachable!()
        };
        let Block::Paragraph { children, .. } = &mut rows[0].cells[1].blocks[0] else {
            unreachable!()
        };
        let at = children
            .iter()
            .position(|n| matches!(n, Inline::Equation { .. }))
            .unwrap();
        let equation = children.remove(at);
        let Block::Paragraph { children, .. } = &mut rows[row].cells[column].blocks[0] else {
            unreachable!()
        };
        children.push(equation);
        let issues = check(&edited);
        assert!(
            issues.iter().any(|issue| issue.contains("row 1, cell 2")),
            "{issues:?}"
        );
    }
}

#[test]
fn structured_head_row_obligations_survive_a_mixed_literal() {
    // CVS termp_bd_pre and print_mdoc_node keep the first two rows
    // separate, including when the third field changes font or uses .br.
    for body in [
        ".Bd -literal\nALPHA\nBRAVO\n.Em CHARLIE\n.Ed\n",
        ".Bd -literal\n.Em ALPHA\n.Em BRAVO\n.Ed\n",
        ".Bd -literal\nALPHA\nBRAVO\n.br\nCHARLIE\n.Ed\n",
    ] {
        let (_, native, mut document) = parsed(&structured_term(body));
        assert!(violations(&native, &document).is_empty(), "{body}");
        let nodes = first_term(&mut document);
        let bravo = nodes
            .iter()
            .position(|node| mant_ir::inline_plain_text(std::slice::from_ref(node)) == "BRAVO")
            .unwrap();
        let at = nodes[..bravo]
            .iter()
            .rposition(|node| matches!(node, Inline::LineBreak {}))
            .unwrap();
        nodes[at] = Inline::Text { value: " ".into() };
        let issues = violations(&native, &document);
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("missing hard line")),
            "{body}: {issues:?}"
        );
    }
}

#[test]
fn head_rows_distinguish_a_tag_run_in_invisible_text_and_continued_blank() {
    // term_flushln() keeps the two one-column D1 fields on one row
    // inside the wide tag. A NBRZW field and an empty TEXT after
    // NONEWLINE also have no independent physical line in these tags.
    for (width, body) in [
        ("10n", ".D1 A\n.D1 B\n"),
        ("a", ".Bd -literal\nALPHA\n\\&\nBRAVO\n.Ed\n"),
        ("a", ".Bd -literal\nA\\c\n\nB\n.Ed\n"),
    ] {
        let source = structured_term(body).replacen("-width a", &format!("-width {width}"), 1);
        let (_, native, document) = parsed(&source);
        let issues = violations(&native, &document);
        assert!(issues.is_empty(), "{width} {body}: {issues:?}");
    }
}

#[test]
fn generated_wrappers_do_not_hide_authored_words_or_supply_missing_words() {
    // Pinned termp_quote_pre() and termp_fo_pre() place [ALPHA and out(
    // respectively on the same row. Only the generated affix is optional.
    for (body, author) in [
        (".Oo\n.No ALPHA\n.Dl PROBE\n.Oc\n", "ALPHA"),
        (".Fo out\n.EQ\nalpha\n.EN\n.Fc\n", "out"),
    ] {
        let (_, native, mut document) = parsed(&structured_term(body));
        let issues = violations(&native, &document);
        assert!(issues.is_empty(), "{body}: {issues:?}");
        fn erase(nodes: &mut [Inline], author: &str) {
            for node in nodes {
                match node {
                    Inline::Text { value } | Inline::Code { value } => {
                        *value = value.replace(author, "");
                    }
                    Inline::Link { children, .. }
                    | Inline::Strong { children }
                    | Inline::Emphasis { children } => erase(children, author),
                    _ => {}
                }
            }
        }
        erase(first_term(&mut document), author);
        assert!(
            violations(&native, &document)
                .iter()
                .any(|issue| issue.contains(author))
        );
    }
}

#[test]
fn equation_owners_follow_execution_when_macro_nodes_share_source_lines() {
    for (source, name) in [
        (
            r".EQ
delim $$
.EN
$x$
world $y$
",
            "eq-start",
        ),
        (
            r".de XX
.Sh FIRST
BEFORE
.EQ
x
.EN
.Sh SECOND
AFTER
..
.XX
",
            "macro-section-display",
        ),
        (
            r".de XX
.Sh FIRST
BEFORE $x$
.Sh SECOND
AFTER
..
.EQ
delim $$
.EN
.XX
",
            "macro-section-inline",
        ),
        (
            r".de XX
.Sh FIRST
BEFORE $x$
.Pp
AFTER
..
.EQ
delim $$
.EN
.XX
",
            "macro-paragraph-inline",
        ),
        (
            r".de XX
.Sh FIRST
.No BEFORE
plain $x$
.Pp
AFTER $x$
..
.EQ
delim $$
.EN
.XX
",
            "same-coordinate-identical",
        ),
    ] {
        let input = format!("{DISPATCH_HEADER}{source}.Sh ENDPROBE\nDone.\n");
        let (expected, native, mut document) = parsed(&input);
        let check = |document: &Document| {
            let (observed, topology) = ir_profile(document);
            compare_structure(&expected, &observed, &native, &topology)
        };
        let issues = check(&document);
        assert!(issues.is_empty(), "{name}: {issues:?}");
        match name {
            "eq-start" | "same-coordinate-identical" => continue,
            "macro-section-display" => {
                let equation = document.sections[2].blocks.pop().unwrap();
                document.sections[3].blocks.insert(0, equation);
            }
            _ => {
                let Block::Paragraph { children, .. } = &mut document.sections[2].blocks[0] else {
                    panic!("first paragraph");
                };
                let index = children
                    .iter()
                    .position(|node| matches!(node, Inline::Equation { .. }))
                    .unwrap();
                let equation = children.remove(index);
                let section = if name == "macro-section-inline" { 3 } else { 2 };
                let Some(Block::Paragraph { children, .. }) =
                    document.sections[section].blocks.last_mut()
                else {
                    panic!("destination paragraph");
                };
                children.push(equation);
            }
        }
        let issues = check(&document);
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("observed no matching IR value")
                    && issue.contains("occurrence 1")),
            "{name}: {issues:?}"
        );
    }
}

#[test]
fn structured_heads_keep_literal_rows_and_words_joined_by_field_execution() {
    // CVS mdoc_term.c closes an extra line for indented TEXT. HANG
    // resets BRIND after br/sp; the next literal words can share a field.
    // Fo's nested wrappers may join authored words with generated punctuation.
    for (index, source) in [
        r".Bl -tag -width a
.It Xo
.Bd -literal
A
 B
C
.Ed
.Xc
BODY
.El
",
        r".Bl -hang -width a
.It Xo
.Bd -literal
A
.br
B
C
.Ed
.Xc
BODY
.El
",
        r".Bl -hang -width a
.It Xo
.Bd -literal
A
.sp
B
C
.Ed
.Xc
BODY
.El
",
        r".Bl -tag -width a
.It Xo
.Fo out
.Oo
.EQ
alpha
.EN
.Oc
.Fc
.Xc
BODY
.El
",
        r".Bl -tag -width a
.It Xo
.Fo out
.Fo inside
.No ALPHA
.Dl PROBE
.Fc
.Fc
.Xc
BODY
.El
",
    ]
    .into_iter()
    .enumerate()
    {
        let input = format!("{DISPATCH_HEADER}{source}.Sh ENDPROBE\nDone.\n");
        let (_, native, mut document) = parsed(&input);
        let issues = violations(&native, &document);
        assert!(issues.is_empty(), "{index}: {issues:?}");
        let nodes = first_term(&mut document);
        if index == 0 {
            let next = nodes
                .iter()
                .position(|node| matches!(node, Inline::Text { value } if value == " B"))
                .unwrap();
            let at = nodes[..next]
                .iter()
                .rposition(|node| matches!(node, Inline::LineBreak {}))
                .unwrap();
            nodes.remove(at);
        } else {
            nodes.clear();
        }
        assert!(!violations(&native, &document).is_empty(), "{index}");
    }
}

#[test]
fn relocated_literal_tag_keeps_the_source_row_and_the_destination() {
    // mdoc_term.c::print_mdoc_node() closes the no-fill source row before
    // NODE_NOPRT skips Tg. A preceding \c suppresses that particular close.
    // mdoc_validate.c::post_tg() puts the target on the selected successor.
    for next in [".Em B", ".Sy B", ".No B", ".Li B", "B"] {
        for (continuation, want) in [("", &["A", "B  C", "D"][..]), ("\\c", &["AB", "C  D"][..])] {
            let source = format!(
                "{DISPATCH_HEADER}.Bl -tag -width a\n.It Xo\n.Bd -literal\nA{continuation}\n.Tg dest\n{next}\n.No C\nD\n.Ed\n.Xc\nBODY\n.El\n"
            );
            let (expected, native, mut document) = parsed(&source);
            let (observed, topology) = ir_profile(&document);
            assert!(
                compare_structure(&expected, &observed, &native, &topology).is_empty(),
                "{source}"
            );
            let term = first_term(&mut document);
            let text = mant_ir::inline_plain_text(term);
            let rows = text
                .lines()
                .map(str::trim_start)
                .filter(|row| !row.is_empty())
                .collect::<Vec<_>>();
            assert_eq!(rows, want, "{source}");
            let anchors = term
                .iter()
                .enumerate()
                .filter(|(_, node)| matches!(node, Inline::Anchor { id, .. } if id == "dest"))
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            let [anchor] = anchors.as_slice() else {
                panic!("expected one relocated anchor: {term:?}");
            };
            assert_eq!(mant_ir::inline_plain_text(&term[..*anchor]).trim(), "A");
            assert!(
                mant_ir::inline_plain_text(&term[*anchor + 1..])
                    .trim_start()
                    .starts_with('B')
            );
        }
    }
}
