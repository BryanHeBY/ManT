//! Inputs recorded from the fixed CVS execution profiles before adding expectations.
use mant_ir::{
    Inline, LinkTarget, Section,
    visit::{self, Visit},
};
use mant_protocol::{ContentSelector, EntryProjection};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Matrix {
    sx: Vec<Case>,
    references: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    source: String,
    source_sha256: String,
    native_exit: BTreeMap<String, i32>,
    label: String,
    expected_heading: Option<String>,
    font: Option<String>,
    second: Option<bool>,
    visible_after: Option<String>,
    placement: Option<String>,
}

fn matrix() -> Matrix {
    serde_json::from_str(include_str!("contracts.json")).unwrap()
}

impl Case {
    fn load(&self) -> mant_ir::ResolvedContent {
        assert_eq!(self.source_sha256.len(), 64, "{}", self.id);
        for profile in ["ascii", "utf8", "html", "tree"] {
            assert_eq!(self.native_exit[profile], 0, "{} {profile}", self.id);
        }
        let query = mant_loader::load_roff_bytes(self.source.as_bytes()).unwrap();
        let roundtrip = crate::json_roundtrip::json_string_roundtrip(&query);
        assert_eq!(query, roundtrip, "{}", self.id);
        assert!(
            mant_ir::validate_document(roundtrip.document.as_ref().unwrap()).is_empty(),
            "{}: {:?}",
            self.id,
            roundtrip.document.as_ref().unwrap().diagnostics,
        );
        roundtrip
    }
}

#[derive(Default)]
struct Links<'a>(Vec<(&'a LinkTarget, &'a [Inline])>);

impl<'a> Visit<'a> for Links<'a> {
    fn visit_inline(&mut self, node: &'a Inline) {
        if let Inline::Link {
            target, children, ..
        } = node
        {
            self.0.push((target, children));
        }
        visit::walk_inline(self, node);
    }
}

fn find_heading<'a>(sections: &'a [Section], title: &str) -> &'a Section {
    sections
        .iter()
        .flat_map(|section| std::iter::once(section).chain(&section.children))
        .find(|section| section.heading.plain_text() == title)
        .expect("source heading reached the expected section owner")
}

#[test]
fn qualified_section_references_require_one_authored_candidate() {
    // CVS mdoc_html.c::mdoc_sx_pre uses the authored fragment. ManT's
    // qualified lookup extension may bind only a unique authored heading.
    let cases = matrix().sx;
    assert_eq!(cases.len(), 112);
    for case in cases {
        let query = case.load();
        let document = query.document.as_ref().unwrap();
        let mut links = Links::default();
        links.visit_section(&document.sections[1]);
        let unresolved = document.diagnostics.iter().filter(|diagnostic| {
            diagnostic.code.as_deref() == Some("unresolved-section-reference")
        });
        let markdown = mant_codec::encode::render_addressable_markdown(&query);
        let (label, id) = (&case.label, &case.id);
        if let Some(title) = &case.expected_heading {
            let owner = find_heading(&document.sections, title);
            let [(LinkTarget::Section { id: target }, children)] = links.0.as_slice() else {
                panic!("{id}: exactly one resolved Sx: {:?}", links.0);
            };
            assert_eq!(target, &owner.id, "{id}");
            assert_eq!(mant_ir::inline_plain_text(children), *label, "{id}");
            assert_eq!(unresolved.count(), 0, "{id}");
            let destination = LinkTarget::Section { id: target.clone() }.to_uri().unwrap();
            assert!(
                markdown.text().contains(&format!("]({destination})")),
                "{id}: {}",
                markdown.text()
            );
            let selector = ContentSelector::id(target.clone());
            let outline = mant_query::build_outline_projection(
                &query,
                EntryProjection::All,
                Some(selector.clone()),
            )
            .unwrap();
            let excerpt = mant_query::select_excerpt(&query, &[selector]).unwrap();
            assert_eq!(outline.nodes[0].id(), target.as_str(), "{id}");
            let [mant_protocol::ExcerptSelection::DocumentSection { section, .. }] =
                excerpt.selections.as_slice()
            else {
                panic!("{id}: must read one original section")
            };
            assert_eq!(section, owner, "{id}");
        } else {
            assert!(links.0.is_empty(), "{id}: {:?}", links.0);
            assert_eq!(unresolved.count(), 1, "{id}");
            assert!(!markdown.text().contains(&format!("[{label}](#")), "{id}");
            assert!(
                mant_render::render_query_text(&query).contains(label),
                "{id}"
            );
        }
    }
}

#[test]
fn manual_reference_annotations_keep_both_source_fonts_and_unlinked_suffixes() {
    // CVS man_term.c::pre_alternate applies a font per actual operand.
    // For T{} the native tree contains both operands; the documented local
    // recovery executes the same man macros on that bounded cell fragment.
    let cases = matrix().references;
    assert_eq!(cases.len(), 40);
    for case in cases {
        let query = case.load();
        let document = query.document.as_ref().unwrap();
        let mut links = Links::default();
        links.visit_document(document);
        // man_term.c::pre_SH gives the heading structural bold. ManT
        // erases that structural layer before annotation, leaving no
        // distinct BR lead on this input. IR retains the distinct emphasis.
        let structural_bold =
            case.placement.as_deref() == Some("heading") && case.font.as_deref() == Some("BR");
        let expected = [
            (!structural_bold).then_some(("printf", "3")),
            case.second.unwrap().then_some(("man", "1")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        assert_eq!(
            links.0.len(),
            expected.len(),
            "{}: {:?}",
            case.id,
            document.sections
        );
        for ((target, children), (name, section)) in links.0.iter().zip(expected) {
            assert!(
                matches!(target, LinkTarget::Manual { name: n, manual_section: Some(s) }
                if n == name && s == section),
                "{}",
                case.id
            );
            assert_eq!(
                mant_ir::inline_plain_text(children),
                format!("{name}({section})"),
                "{}",
                case.id
            );
            let [styled, Inline::Text { value }] = children else {
                panic!("{}: styled name plus original section operand", case.id);
            };
            assert!(
                match case.font.as_deref() {
                    Some("BR") => matches!(styled, Inline::Strong { .. }),
                    Some("IR") => matches!(styled, Inline::Emphasis { .. }),
                    _ => false,
                },
                "{}",
                case.id
            );
            assert_eq!(value, &format!("({section})"), "{}", case.id);
        }

        let text = mant_render::render_query_text(&query);
        assert_eq!(text.matches("BodyWord.").count(), 1, "{}", case.id);
        assert!(
            text.contains(&format!(
                "{}{}",
                case.label,
                case.visible_after.as_deref().unwrap()
            )),
            "{}: {text}",
            case.id
        );
        if !structural_bold && case.placement.as_deref() != Some("tbl") {
            // Table fallback is intentionally plain in portable Markdown.
            let markdown = mant_codec::encode::render_markdown(&query);
            let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
            let styled_name = std::cell::Cell::new(false);
            let _text = mant_render::render_query_text_with(&readback, |style, text| {
                if text.contains("printf") {
                    styled_name.set(
                        styled_name.get()
                            || match case.font.as_deref() {
                                Some("BR") => style.inline.strong,
                                Some("IR") => style.inline.emphasis,
                                _ => false,
                            },
                    );
                }
                text.to_owned()
            });
            assert!(styled_name.get(), "{}: {markdown}", case.id);
        }
    }
}

#[test]
fn a_link_in_an_option_head_does_not_move_name_bindings() {
    // The pinned man_term.c::pre_alternate operands of this exact TP
    // include a styled option, the roman suffix and another styled option.
    // Each spelling must bind its accepted glyphs after link publication.
    let source = b".TH NAVPROBE 1 \"November 17, 2026\"\n.SH DESCRIPTION\n.TP\n.BR --color \"(3), \" --surviving\nBodyWord.\n";
    let query = mant_loader::load_roff_bytes(source).unwrap();
    let query = crate::json_roundtrip::json_string_roundtrip(&query);
    let document = query.document.as_ref().unwrap();
    assert!(mant_ir::validate_document(document).is_empty());
    let [mant_ir::Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice()
    else {
        panic!("original TP owner")
    };
    let owner = mant_ir::EntryOwner::Definition(&items[0]);
    let facts = owner.facts().unwrap();
    assert_eq!(facts.names, ["--color", "--surviving"]);
    assert_eq!(facts.name_bindings.len(), facts.names.len());
    assert_eq!(
        mant_ir::inline_plain_text(&owner.form(&facts.forms[0]).unwrap()),
        "--color(3), --surviving"
    );
    for (binding, name) in facts.name_bindings.iter().zip(&facts.names) {
        assert_eq!(binding.occurrences.len(), 1);
        assert_eq!(
            mant_ir::inline_plain_text(&owner.form(&binding.occurrences[0]).unwrap()),
            *name
        );
        let explanation = mant_query::explain_query(
            &query,
            &mant_protocol::ExplanationQuery {
                entry: name.clone(),
                options: mant_protocol::ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(explanation.counts.direct_entry.total, 1);
    }
}

#[test]
fn no_fill_manual_references_keep_native_rows_and_unsupported_labels() {
    // Pinned CVS marks each operand separately (pre_alternate) and keeps all
    // four input rows in NOFILL. ManT's conservative link grammar may retain
    // untouched styled text for non-ASCII names and section 0.
    let source = ".TH NAVPROBE 1 \"November 17, 2026\"\n.SH DESCRIPTION\n.nf\n.BR naïve (3)\n.IR tool (0)\n.BR printf (3),\n.IR man (1)\n.fi\n";
    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let document = query.document.as_ref().unwrap();
    let [mant_ir::Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice()
    else {
        panic!("original literal rows")
    };
    assert_eq!(
        mant_ir::inline_plain_text(children).trim_end_matches('\n'),
        "naïve(3)\ntool(0)\nprintf(3),\nman(1)",
    );
    let mut links = Links::default();
    links.visit_document(document);
    assert_eq!(
        links
            .0
            .iter()
            .map(|(_, label)| mant_ir::inline_plain_text(label))
            .collect::<Vec<_>>(),
        ["printf(3)", "man(1)"],
    );
}
