//! Authored annotation membership is independent of native buffer settlement.
use super::*;
mod native_members;

#[derive(Default)]
struct Links(Vec<(mant_ir::LinkTarget, Vec<Inline>)>);
impl<'a> Visit<'a> for Links {
    fn visit_inline(&mut self, inline: &'a Inline) {
        if let Inline::Link {
            target, children, ..
        } = inline
        {
            self.0.push((target.clone(), children.clone()));
        }
        visit::walk_inline(self, inline);
    }
}

fn styled_scalars(nodes: &[Inline], inherited: u8, output: &mut Vec<(char, u8)>) {
    for node in nodes {
        match node {
            Inline::Text { value } | Inline::Equation { value, .. } => {
                output.extend(value.chars().map(|ch| (ch, inherited)));
            }
            Inline::Code { value } => output.extend(value.chars().map(|ch| (ch, inherited | 4))),
            Inline::Strong { children } => styled_scalars(children, inherited | 1, output),
            Inline::Emphasis { children } => styled_scalars(children, inherited | 2, output),
            Inline::Link { children, .. } => styled_scalars(children, inherited, output),
            Inline::LineBreak { .. } => output.push(('\n', inherited)),
            Inline::Anchor { .. } => {}
        }
    }
}

#[test]
fn annotations_preserve_initial_root_authored_prefix_and_birth_style() {
    // Each complete source ran pristine before this fixture was written.
    // term.c:573-589 writes the entry separator before source cells;
    // encode()/encode1():901-964 retains all but the last held blank.
    // man_html.c::man_UR_pre + html_fillmode close the initial label root
    // on nf/IP, while roff_html_pre_br does not close that annotation.
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("annotation_ownership/cases.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let source = case["source"].as_str().unwrap();
        let document =
            parse_manual_bytes(std::path::Path::new("annotation.1"), source.as_bytes()).unwrap();
        let json = serde_json::to_string(&document).unwrap();
        assert_public_wire(&json, &case["id"]);
        assert!(!json.contains(r"\u0000mant:"), "{}: {json}", case["id"]);
        let decoded: mant_ir::Document = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, document);
        let section = document
            .sections
            .iter()
            .find(|section| mant_ir::inline_plain_text(&section.heading.content) == "DESCRIPTION")
            .unwrap();
        let mut links = Links::default();
        links.visit_section(section);
        if let Some(label) = case["label"].as_str() {
            assert_eq!(
                links.0.len(),
                usize::try_from(case["occurrences"].as_u64().unwrap()).unwrap(),
                "{}: {json}",
                case["id"]
            );
            assert_eq!(
                mant_ir::inline_plain_text(&links.0[0].1),
                label,
                "{}: {json}",
                case["id"]
            );
            if case.get("target").is_some() {
                let target: mant_ir::LinkTarget =
                    serde_json::from_value(case["target"].clone()).unwrap();
                assert_eq!(links.0[0].0, target, "{}", case["id"]);
            }
        }
        if let Some(styles) = case["scalar_styles"].as_array() {
            let mut expected = Vec::new();
            for span in styles {
                let style = match span["style"].as_str().unwrap() {
                    "strong" => 1,
                    "emphasis" => 2,
                    "code" => 4,
                    "plain" => 0,
                    other => panic!("unknown independent style {other}"),
                };
                expected.extend(span["text"].as_str().unwrap().chars().map(|ch| (ch, style)));
            }
            let mut actual = Vec::new();
            if case.get("label").is_some() {
                styled_scalars(&links.0[0].1, 0, &mut actual);
            } else {
                let mut collector = InlineRoots::default();
                collector.visit_section(section);
                for root in collector.0 {
                    styled_scalars(root, 0, &mut actual);
                }
            }
            assert_eq!(actual, expected, "{}: {json}", case["id"]);
        }
    }
}

#[derive(Default)]
struct InlineRoots<'a>(Vec<&'a [Inline]>);
impl<'a> Visit<'a> for InlineRoots<'a> {
    fn visit_block(&mut self, block: &'a Block) {
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                self.0.push(children);
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    self.0
                        .extend(item.terms.iter().map(|term| term.content.as_slice()));
                    for block in &item.description {
                        self.visit_block(block);
                    }
                }
            }
            _ => visit::walk_block(self, block),
        }
    }
}

#[test]
fn annotation_payload_and_run_costs_are_linear_in_members_and_destination_size() {
    // All 52 exact N×U sources ran pristine UTF8/HTML/lint before this
    // fixture and these assertions. term.c:901-964 removes only the final
    // held byte; unknown NBRZW recovery retains the existing ManT spelling.
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("annotation_ownership/costs.json")).unwrap();
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 52);
    for case in fixture["cases"].as_array().unwrap() {
        let _ = super::super::inline::ownership::metrics::take();
        let source = case["source"].as_str().unwrap();
        let document =
            parse_manual_bytes(std::path::Path::new("annotation-cost.1"), source.as_bytes())
                .unwrap();
        let counts = super::super::inline::ownership::metrics::take();
        let mut links = Links::default();
        links.visit_document(&document);
        assert_eq!(links.0.len(), 1, "{}: {:?}", case["id"], links.0);
        assert_eq!(
            mant_ir::inline_plain_text(&links.0[0].1),
            case["label"].as_str().unwrap(),
            "{}",
            case["id"]
        );
        let uri = case["uri"].as_str().unwrap();
        assert_eq!(
            links.0[0].0,
            mant_ir::LinkTarget::External { uri: uri.into() }
        );
        assert_eq!(
            (counts.registered, counts.published),
            (1, 1),
            "{}: {counts:?}",
            case["id"]
        );
        assert_eq!(
            (counts.registered_bytes, counts.published_bytes),
            (uri.len(), uri.len()),
            "{}: {counts:?}",
            case["id"]
        );
        let n = usize::try_from(case["n"].as_u64().unwrap()).unwrap();
        assert!(counts.bound <= 4 * (n + 1), "{}: {counts:?}", case["id"]);
        assert!(counts.released <= 4 * (n + 1), "{}: {counts:?}", case["id"]);
        assert!(counts.decorations <= 8, "{}: {counts:?}", case["id"]);
        assert!(counts.runs <= 8, "{}: {counts:?}", case["id"]);
        assert!(counts.held <= n, "{}: {counts:?}", case["id"]);
        assert!(
            counts.normalization <= 32 * (n + 32),
            "{}: {counts:?}",
            case["id"]
        );
        eprintln!("annotation-cost {} {counts:?}", case["id"]);
    }
}

#[test]
fn annotation_source_instances_targets_and_accepted_members_cover_all_families() {
    // The source/target and native lexical masks are independently derived
    // from the sealed pristine profiles and the cited native handlers. Null
    // labels explicitly mean a lexical/ownership check, not a full text gold.
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("annotation_ownership/families.json")).unwrap();
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 1246);
    let mut failures = Vec::new();
    for case in fixture["cases"].as_array().unwrap() {
        let source = case["source"].as_str().unwrap();
        let document = parse_manual_bytes(
            std::path::Path::new("annotation-family.1"),
            source.as_bytes(),
        )
        .unwrap();
        let mut links = Links::default();
        links.visit_document(&document);
        let expected = case["expected_references"].as_array().unwrap();
        if links.0.len() != expected.len() {
            failures.push(format!(
                "{} count: {:?}; expected {expected:?}",
                case["id"], links.0
            ));
            continue;
        }
        for ((target, children), expected) in links.0.iter().zip(expected) {
            let wanted: mant_ir::LinkTarget =
                serde_json::from_value(expected["target"].clone()).unwrap();
            if target != &wanted {
                failures.push(format!("{} target {target:?} != {wanted:?}", case["id"]));
            }
            if let Some(label) = expected["label"].as_str() {
                let actual = mant_ir::inline_plain_text(children);
                if actual != label {
                    failures.push(format!("{} label {actual:?} != {label:?}", case["id"]));
                }
            }
        }
        let mut roots = InlineRoots::default();
        for section in &document.sections {
            if mant_ir::inline_plain_text(&section.heading.content) == "DESCRIPTION" {
                roots.visit_section(section);
            }
        }
        let mut scalars = Vec::new();
        for root in roots.0 {
            styled_scalars(root, 0, &mut scalars);
        }
        let text: String = scalars.iter().map(|(ch, _)| ch).collect();
        for member in case["accepted_lexical_members"].as_array().unwrap() {
            let spelling = member["text"].as_str().unwrap();
            let count =
                usize::try_from(member["native_accepted_occurrences"].as_u64().unwrap()).unwrap();
            if text.matches(spelling).count() != count {
                failures.push(format!(
                    "{} member {spelling:?} count {} != {count}: {text:?}",
                    case["id"],
                    text.matches(spelling).count()
                ));
            }
        }
        native_members::check(case, &links, &text, &scalars, &mut failures);
        native_members::check_heading(case, &document, &mut failures);
        let json = serde_json::to_string(&document).unwrap();
        assert_public_wire(&json, &case["id"]);
        assert!(!json.contains(r"\u0000mant:"), "{}: {json}", case["id"]);
        let decoded: mant_ir::Document = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, document);
    }
    assert!(
        failures.is_empty(),
        "{} contract failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn annotation_boundary_matrix_preserves_complete_labels_and_scalar_styles() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("annotation_ownership/boundaries.json")).unwrap();
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 162);
    check_boundary_cases(&fixture);
}

#[test]
fn annotation_factories_and_repeated_fill_controls_share_native_root_lifetimes() {
    // All exact sources ran five pristine profiles before these assertions.
    // roff_html_pre_fi/nf leaves the root open when the mode is unchanged;
    // man_UR_pre and tbl_html close it only at their real output boundaries.
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("annotation_ownership/misc.json")).unwrap();
    check_boundary_cases(&fixture);
}

fn check_boundary_cases(fixture: &serde_json::Value) {
    let mut failures = Vec::new();
    for case in fixture["cases"].as_array().unwrap() {
        let source = case["source"].as_str().unwrap();
        let document = parse_manual_bytes(
            std::path::Path::new("annotation-boundary.1"),
            source.as_bytes(),
        )
        .unwrap();
        let mut links = Links::default();
        links.visit_document(&document);
        let expected = &case["expected"];
        let wanted = expected["links"].as_array().unwrap();
        if links.0.len() != wanted.len() {
            failures.push(format!("{} count {:?} != {wanted:?}", case["id"], links.0));
            continue;
        }
        for ((target, children), wanted) in links.0.iter().zip(wanted) {
            let target_gold: mant_ir::LinkTarget =
                serde_json::from_value(wanted["target"].clone()).unwrap();
            if target != &target_gold {
                failures.push(format!(
                    "{} target {target:?} != {target_gold:?}",
                    case["id"]
                ));
            }
            let text = mant_ir::inline_plain_text(children);
            if text != wanted["label"].as_str().unwrap() {
                failures.push(format!(
                    "{} label {text:?} != {}",
                    case["id"], wanted["label"]
                ));
            }
            check_scalar_styles(case, children, &wanted["scalar_styles"], &mut failures);
        }
        let mut roots = InlineRoots::default();
        for section in &document.sections {
            if mant_ir::inline_plain_text(&section.heading.content) == "DESCRIPTION" {
                roots.visit_section(section);
            }
        }
        let mut body = Vec::new();
        for root in roots.0 {
            body.extend_from_slice(root);
        }
        let body_text = mant_ir::inline_plain_text(&body);
        check_accepted_cell_styles(case, &body, &body_text);
        if let Some(wanted) = expected["body_text"].as_str() {
            if body_text != wanted {
                failures.push(format!("{} body {body_text:?} != {wanted:?}", case["id"]));
            }
            check_scalar_styles(case, &body, &expected["body_scalar_styles"], &mut failures);
        }
        let mut all_roots = InlineRoots::default();
        all_roots.visit_document(&document);
        let all_text: String = all_roots
            .0
            .into_iter()
            .map(mant_ir::inline_plain_text)
            .collect();
        for word in expected["outside_link_literals"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let word = word.as_str().unwrap();
            if !all_text.contains(word)
                || links
                    .0
                    .iter()
                    .any(|(_, children)| mant_ir::inline_plain_text(children).contains(word))
            {
                failures.push(format!(
                    "{} outside {word:?} missing or linked: {body_text:?}",
                    case["id"]
                ));
            }
        }
        check_boundary_wire(&document, case);
    }
    assert!(
        failures.is_empty(),
        "{} boundary failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn check_scalar_styles(
    case: &serde_json::Value,
    children: &[Inline],
    spans: &serde_json::Value,
    failures: &mut Vec<String>,
) {
    let Some(spans) = spans.as_array() else {
        return;
    };
    let mut wanted = Vec::new();
    for span in spans {
        let style = match span["style"].as_str().unwrap() {
            "plain" | "unpainted" => 0,
            "strong" => 1,
            "emphasis" => 2,
            "code" => 4,
            other => panic!("unknown source style {other}"),
        };
        wanted.extend(span["text"].as_str().unwrap().chars().map(|ch| (ch, style)));
    }
    let mut actual = Vec::new();
    styled_scalars(children, 0, &mut actual);
    for (ch, style) in &mut actual {
        if *ch == '\n' {
            *style = 0;
        }
    }
    if actual != wanted {
        failures.push(format!(
            "{} scalar styles {actual:?} != {wanted:?}",
            case["id"]
        ));
    }
}

fn assert_public_wire(wire: &str, id: &serde_json::Value) {
    for marker in [
        r"\u0000mant:",
        "mant-annotation-entry",
        "mant-link-scope",
        "mant-annotation-end",
    ] {
        assert!(!wire.contains(marker), "{id}: leaked private {marker}");
    }
}

fn check_boundary_wire(document: &mant_ir::Document, case: &serde_json::Value) {
    let expected = &case["expected"];
    if let Some(partial) = expected["semantic_partial"].as_bool() {
        assert_eq!(
            document
                .diagnostics
                .iter()
                .any(|d| d.code.as_deref() == Some("manual.annotation-coverage")),
            partial,
            "{}",
            case["id"]
        );
    }
    if let Some(diagnostics) = expected["diagnostics"].as_array() {
        for wanted in diagnostics {
            let impact: mant_ir::DiagnosticImpact =
                serde_json::from_value(wanted["impact"].clone()).unwrap();
            assert_eq!(
                document
                    .diagnostics
                    .iter()
                    .filter(|d| d.code.as_deref() == wanted["code"].as_str() && d.impact == impact)
                    .count(),
                usize::try_from(wanted["count"].as_u64().unwrap()).unwrap(),
                "{}",
                case["id"]
            );
        }
    }
    let wire = serde_json::to_string(document).unwrap();
    assert_public_wire(&wire, &case["id"]);
    assert!(!wire.contains(r"\u0000mant:"), "{} {wire}", case["id"]);
    assert_eq!(
        serde_json::from_str::<mant_ir::Document>(&wire).unwrap(),
        *document
    );
}

fn check_accepted_cell_styles(case: &serde_json::Value, body: &[Inline], body_text: &str) {
    let expected = &case["expected"];
    if let Some(members) = expected["cell_accepted_members"].as_array() {
        let mut scalars = Vec::new();
        styled_scalars(body, 0, &mut scalars);
        for member in members {
            let text = member["text"].as_str().unwrap();
            assert_eq!(
                body_text.matches(text).count(),
                usize::try_from(member["occurrences"].as_u64().unwrap()).unwrap()
            );
            let at = body_text.find(text).unwrap();
            let start = body_text[..at].chars().count();
            let style = u8::from(member["style"] == "strong");
            assert!(
                scalars[start..start + text.chars().count()]
                    .iter()
                    .all(|(_, actual)| *actual == style),
                "{} accepted cell style",
                case["id"]
            );
        }
    }
}
