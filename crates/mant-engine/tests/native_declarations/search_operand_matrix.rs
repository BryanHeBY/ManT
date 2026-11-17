//! Search operands across source spellings, font wrappers, and overstrikes.
use super::operands::explain;
use super::search_templates::{check_three, source};
use mant_ir::{EntryInlineRoot, EntryOwner, Inline};
use mant_protocol::{EntryProjection, EvidenceBasis, OutlineNode, QueryOutline};
use pulldown_cmark::{Event, Parser};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GlyphStyle {
    Plain,
    Strong,
    Emphasis,
    Code,
}

const CARRIERS: [(&str, GlyphStyle); 5] = [
    ("No", GlyphStyle::Plain),
    ("Pf", GlyphStyle::Plain),
    ("Sy", GlyphStyle::Strong),
    ("Em", GlyphStyle::Emphasis),
    ("Li", GlyphStyle::Code),
];
const SIGN_SPELLINGS: [(&str, char); 9] = [
    ("/", '/'),
    ("?", '?'),
    (r"\(sl", '/'),
    (r"\[sl]", '/'),
    (r"\[u002F]", '/'),
    (r"\[u003F]", '?'),
    (r"\N'47'", '/'),
    (r"\N'63'", '?'),
    (r"\C'sl'", '/'),
];
const ARGUMENT: [(char, GlyphStyle); 2] =
    [('R', GlyphStyle::Emphasis), ('E', GlyphStyle::Emphasis)];

#[test]
fn styled_operands_use_executed_spacing_and_preserve_typed_macros() {
    // termp_bold_pre and termp_under_pre change the font. Sm and Ns
    // determine whether term_word inserts an actual separator.
    for (head, form, group) in [
        (".It Sy / Ns Ar RE", "/RE", Some(1)),
        (".It Sy / Ar RE", "/ RE", None),
        (r".It Bq Sy \z/ Ar RE", "[/RE]", None),
        (r".It Em \[u002F] Ns Ar RE", "/RE", Some(1)),
        (r".It Em \[u002F] Ar RE", "/ RE", None),
    ] {
        let input = source("", ".Sh COMMANDS\n.Sm on\n", &format!("{head}\n"));
        check_three(&input, form, &[], group);
    }

    // Equal terminal glyphs under a semantic macro do not prove the same
    // search expression. The empty Ar executes without an argument glyph.
    for (head, form) in [
        (".It Cm / Ar RE", "/RE"),
        (".It Dv / Ar RE", "/RE"),
        (".It Va / Ar RE", "/RE"),
        (r#".It Em \[u002F] Ar """#, "/"),
        (r".It Bq Em \z\[u002F] Ar Ré", "[Ré]"),
    ] {
        let input = source("", ".Sh COMMANDS\n.Sm off\n", &format!("{head}\n"));
        let content = check_three(&input, form, &[], None);
        assert!(explain(&content, "RE").supports.is_empty(), "{input}");
    }
}

#[test]
fn closing_signs_use_the_same_native_word_shapes_as_openers() {
    // In mdoc_term.c, Pf executes its word and sets NOSPACE for a same-line
    // argument. Dv keeps its own role even when it prints exactly that sign.
    for (head, form, group) in [
        (".It Pf / Ar RE Pf / Ar OFFSET", "/RE/OFFSET", Some(1)),
        (r".It Pf / Ar RE Pf \(sl Ar OFFSET", "/RE/OFFSET", Some(1)),
        (r".It Pf / Ar RE Em \(sl Ar OFFSET", "/RE/OFFSET", Some(1)),
        (".It Pf / Ar RE Dv / Ar OFFSET", "/RE/OFFSET", None),
        (".It Pf / Ar RE Pf ? Ar OFFSET", "/RE?OFFSET", None),
        (r".It Pf / Ar RE Pf \z/ Ar OFFSET", "/REOFFSET", None),
    ] {
        let input = source("", ".Sh COMMANDS\n.Sm off\n", &format!("{head}\n"));
        check_three(&input, form, &[], group);
    }
}

#[test]
fn unicode_search_arguments_leave_independent_declared_names_addressable() {
    // The upstream Ar scope writes its entire quoted operand. A literal
    // comma within it is part of the parameter, while a comma after that
    // scope can delimit the independent Cm spelling.
    for (head, form, names, group) in [
        (
            r#".It Bq Em \z\[u003F] Ar "查找" , Cm suffix"#,
            "[查找,suffix]",
            &["suffix"][..],
            Some(0),
        ),
        (
            r#".It Bq Em \z\[u003F] Ar "查找,suffix""#,
            "[查找,suffix]",
            &[][..],
            None,
        ),
        (
            r#".It Bq Em \z\[u003F] Ar "查找" No "!unknown" , Cm suffix"#,
            "[查找!unknown,suffix]",
            &[][..],
            None,
        ),
        (".It Bq Ar 查找 , Cm suffix", "[查找,suffix]", &[][..], None),
    ] {
        let input = source("", ".Sh COMMANDS\n.Sm off\n", &format!("{head}\n"));
        let content = check_three(&input, form, names, group);
        assert_eq!(
            explain(&content, "查找").counts.direct_entry.total,
            0,
            "{input}"
        );
        assert_eq!(
            explain(&content, "suffix").counts.direct_entry.total,
            u32::from(!names.is_empty()),
            "{input}"
        );
    }
}

fn styled_glyphs(nodes: &[Inline], style: GlyphStyle, glyphs: &mut Vec<(char, GlyphStyle)>) {
    for node in nodes {
        match node {
            Inline::Text { value } => glyphs.extend(value.chars().map(|ch| (ch, style))),
            Inline::Code { value } => glyphs.extend(value.chars().map(|ch| (ch, GlyphStyle::Code))),
            Inline::Strong { children } => styled_glyphs(children, GlyphStyle::Strong, glyphs),
            Inline::Emphasis { children } => styled_glyphs(children, GlyphStyle::Emphasis, glyphs),
            Inline::Anchor { .. } => {}
            other => panic!("unexpected inline in a search form: {other:?}"),
        }
    }
}

fn check_form(head: &str, expected: &[(char, GlyphStyle)], shared: bool) {
    let input = source("", ".Sh COMMANDS\n.Sm off\n", &format!("{head}\n"));
    let form: String = expected.iter().map(|&(ch, _)| ch).collect();
    let content = check_three(&input, &form, &[], shared.then_some(1));
    let document = content.document.as_ref().unwrap();
    let items = super::definitions(document);
    let mut visible = Vec::new();
    styled_glyphs(&items[0].terms[0].content, GlyphStyle::Plain, &mut visible);
    assert_eq!(visible, expected, "{head}");

    let parameter = explain(&content, "RE");
    // An unwrapped overstrike can leave the complete displayed form RE.
    // Explain may match that form directly while leaving the name unbound.
    assert_eq!(
        parameter.counts.direct_entry.total,
        u32::from(form == "RE"),
        "{head}"
    );
    assert!(parameter.supports.is_empty(), "{head}");
    assert!(
        parameter.evidence.iter().all(|hit| {
            hit.bases
                .iter()
                .all(|basis| !matches!(basis, EvidenceBasis::Name { .. }))
        }),
        "{head}"
    );

    let template = EntryOwner::Definition(items[0]);
    assert!(template.facts().unwrap().name_bindings.is_empty(), "{head}");
    for (index, name) in [(1, "n"), (2, "N")] {
        let facts = EntryOwner::Definition(items[index]).facts().unwrap();
        let [binding] = facts.name_bindings.as_slice() else {
            panic!("expected exactly one binding for {name}: {head}");
        };
        assert_eq!(binding.name, 0, "{head}");
        let [occurrence] = binding.occurrences.as_slice() else {
            panic!("expected exactly one occurrence for {name}: {head}");
        };
        let [part] = occurrence.parts.as_slice() else {
            panic!("expected exactly one slice for {name}: {head}");
        };
        assert_eq!(part.root, EntryInlineRoot::Term { index: 0 }, "{head}");

        let result = explain(&content, name);
        let markdown = mant_render::render_explanation_markdown(&result);
        let visible: String = Parser::new(&markdown)
            .filter_map(|event| match event {
                Event::Text(text) | Event::Code(text) => Some(text.into_string()),
                _ => None,
            })
            .collect();
        assert_eq!(
            visible.matches("GROUP_BODY").count(),
            usize::from(name == "N" || shared),
            "{head}\n{markdown}"
        );
    }

    let outline =
        mant_query::build_outline_projection(&content, EntryProjection::All, None).unwrap();
    let outline: QueryOutline =
        serde_json::from_str(&serde_json::to_string(&outline).unwrap()).unwrap();
    let names: Vec<&[String]> = outline
        .nodes
        .last()
        .unwrap()
        .children()
        .iter()
        .filter_map(|node| {
            if let OutlineNode::DocumentEntry { names, .. } = node {
                Some(names.as_slice())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(names, [&[][..], &["n"][..], &["N"][..]], "{head}");
}

fn check_operand(carrier: &str, spelling: &str, sign: char, style: GlyphStyle, overwritten: bool) {
    let [r, e] = ARGUMENT;
    if overwritten {
        check_form(
            &format!(".It Bq {carrier} \\z{spelling} Ar RE"),
            &[('[', GlyphStyle::Plain), r, e, (']', GlyphStyle::Plain)],
            false,
        );
    } else {
        check_form(
            &format!(".It {carrier} {spelling} Ar RE"),
            &[(sign, style), r, e],
            true,
        );
    }
}

#[test]
fn source_spellings_and_wrappers_preserve_search_operands() {
    // Fixed CVS term.c::term_word() decodes each equivalent spelling;
    // encode1() overstrikes the pending sign with the first Ar glyph.
    // mdoc_term.c dispatches Sy to termp_bold_pre(), Em to termp_under_pre(),
    // and Li/No to termp_li_pre(). mdoc_html.c dispatches Li to mdoc_code_pre().
    // Of the 90 literal cases, 82 are lint-clean. For No/Sy/Em/Li, the
    // trailing '?' skips the empty macro if bare and reports delimiter
    // spacing if overstruck. The bare recovery has a plain sign in CVS.
    for (carrier, style) in CARRIERS {
        for (spelling, sign) in SIGN_SPELLINGS {
            let style = if spelling == "?" && carrier != "Pf" {
                GlyphStyle::Plain
            } else {
                style
            };
            for overwritten in [false, true] {
                check_operand(carrier, spelling, sign, style, overwritten);
            }
        }
    }
}

#[test]
fn protected_question_marks_keep_their_source_wrappers() {
    // Fixed CVS tree and lint confirm a real wrapper after protecting the
    // delimiter with ESCAPE_IGNORE in the same word (mdoc_macro.c::in_line()).
    // The sibling Ar word still overwrites the sign in the enclosed inputs.
    for (carrier, style) in CARRIERS.into_iter().filter(|(carrier, _)| *carrier != "Pf") {
        for overwritten in [false, true] {
            check_operand(carrier, r"?\&", '?', style, overwritten);
        }
    }
}

#[test]
fn font_and_keep_blocks_preserve_the_executed_role_and_punctuation() {
    // All 20 fixed CVS inputs are lint-clean. mdoc_term.c::termp_bf_pre()
    // sets the BODY font, while termp_bk_pre()/post() scope word keeping.
    // No resets the slash font; direct TEXT and Bq punctuation inherit it.
    // The opening delimiter on Bf is a block child before HEAD and BODY
    // (mdoc_macro.c::blk_full()). Bk has ordinary flanking Xo BODY words.
    // Entering or leaving either scope preserves a pending overstrike.
    let [r, e] = ARGUMENT;
    for (open, open_with_punctuation, close, scope_style) in [
        (
            ".Bf -symbolic",
            ".Bf -symbolic (",
            ".Ef",
            GlyphStyle::Strong,
        ),
        (".Bk -words", "(\n.Bk -words", ".Ek", GlyphStyle::Plain),
    ] {
        let plain = [('/', GlyphStyle::Plain), r, e];
        let scoped = [('/', scope_style), r, e];
        let covered = [('[', scope_style), r, e, (']', scope_style)];
        for (words, glyphs, shared) in [
            (".No / Ar RE", &plain[..], true),
            ("/\n.Ar RE", &scoped[..], true),
            (r".Bq No \z/ Ar RE", &covered[..], false),
            (r".Bq \z/ Ar RE", &covered[..], false),
        ] {
            check_form(
                &format!(".It Xo\n{open}\n{words}\n{close}\n.Xc"),
                glyphs,
                shared,
            );
        }

        for (words, glyphs) in [
            (".No / Ar RE", &plain[..]),
            (r".Bq No \z/ Ar RE", &covered[..]),
        ] {
            let mut expected = vec![('(', GlyphStyle::Plain)];
            expected.extend_from_slice(glyphs);
            expected.push((')', GlyphStyle::Plain));
            check_form(
                &format!(".It Xo\n{open_with_punctuation}\n{words}\n{close}\n)\n.Xc"),
                &expected,
                false,
            );
        }

        for (operator, shared) in [("/", true), (r"\z/", false)] {
            check_form(
                &format!(".It Xo\n.No {operator}\n{open}\n.Ar RE\n{close}\n.Xc"),
                if shared { &plain } else { &ARGUMENT },
                shared,
            );
            check_form(
                &format!(".It Xo\n{open}\n{operator}\n{close}\n.Ar RE\n.Xc"),
                if shared { &scoped } else { &ARGUMENT },
                shared,
            );
        }
    }
}
