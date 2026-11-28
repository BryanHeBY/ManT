//! Authored annotation membership is independent of native buffer settlement.
use super::*;
mod native_members;
mod publication_boundaries;

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

#[test]
fn reduced_equation_label_does_not_admit_a_generated_reference() {
    // man_html.c::man_UR_pre traverses the BODY in one anchor, including
    // print_man_node's MathML and the text after it. Splitting a public
    // owner at the display block cannot turn those glyphs into a candidate.
    let source = b".TH PROBE 1 \"2026-11-07\"\n.SH DESCRIPTION\n.UR https://one.test\nFIRST\n.EQ\nx sup 2\n.EN\nprintf(3) \\%<>\n.UE\nTAIL\n";
    let document = parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
    let section = &document.sections[0];
    let mut links = Links::default();
    links.visit_section(section);
    assert_eq!(links.0.len(), 1);
    assert!(
        matches!(&links.0[0].0, mant_ir::LinkTarget::External { uri } if uri == "https://one.test")
    );
    assert_eq!(mant_ir::inline_plain_text(&links.0[0].1), "FIRST");
    assert!(projected_document_text(&document).contains("printf(3) <>"));
    assert!(section.blocks.iter().any(|block| matches!(
        block,
        Block::Equation {
            expression: Some(_),
            display: true,
            ..
        }
    )));
    assert_eq!(
        document
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.impact == mant_ir::DiagnosticImpact::SemanticCoverage
                    && diagnostic.code.as_deref() == Some("manual.annotation-coverage")
            })
            .count(),
        1
    );
}

#[test]
fn empty_destination_marker_uses_the_same_overstrike_as_other_glyphs() {
    // term.c::term_word buffers the ignored \% as a zero-width graph.
    // encode1 then writes '<', backspace and '>'; the later A is intact.
    let source = b".TH PROBE 1 \"2026-11-07\"\n.SH DESCRIPTION\n.UR https://one.test\n\\z\\%<> AFTER\n.UE\nTAIL\n";
    let document = parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
    let mut links = Links::default();
    links.visit_section(&document.sections[0]);
    assert_eq!(links.0.len(), 1);
    assert_eq!(mant_ir::inline_plain_text(&links.0[0].1), "> AFTER");
    assert!(projected_document_text(&document).contains("> AFTER <https://one.test> TAIL"));
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

const LIVE_TARGET_HEAD: &str = ".Dd November 27, 2026\n.Dt GLYPHS 1\n.Os\n.Sh NAME\n.Nm glyphs\n.Nd small roff inputs\n.Sh DESCRIPTION\n";
const LIVE_TARGET_END: &str = ".Sh ENDTEST\nDone.\n";

fn target_probe_witness(
    source: &str,
) -> (
    super::super::inline::ownership::metrics::Counts,
    super::super::inline::ownership::metrics::Counts,
) {
    use super::super::inline::ownership::metrics;
    let parse =
        || parse_manual_bytes(std::path::Path::new("live-target.1"), source.as_bytes()).unwrap();
    metrics::take();
    let gated = parse();
    let gated_counts = metrics::take();
    let eager = metrics::with_eager_navigation_probes(parse);
    let eager_counts = metrics::take();
    assert_eq!(
        serde_json::to_value(&gated).unwrap(),
        serde_json::to_value(&eager).unwrap(),
        "{source}"
    );
    assert_eq!(
        gated_counts.navigation_relocated_word_starts,
        eager_counts.navigation_relocated_word_starts
    );
    (gated_counts, eager_counts)
}

#[test]
fn absolute_link_rewrites_without_targets_do_not_search_the_output() {
    // AT repeated-16.1 and plain/inset scales ran pinned ASCII/UTF-8/HTML.
    // CVS termp_lk_pre calls term_word for the colon; term.c's HORIZ reuses
    // earlier physical slots. This gate retains that native work and removes
    // only unsuccessful navigation traversal; it does not time the parser.
    const UNIT: &str = ".Lk https://a.test Y\n.No \"\\h'|0n'X\"\n";
    for n in [4, 8, 16, 32] {
        for (start, end) in [
            ("", ""),
            (".ft B\n.Bl -inset\n.It No Q\\z Ap No A\n", ".El\n"),
        ] {
            let source = format!(
                "{LIVE_TARGET_HEAD}{start}{}.No END\n{end}{LIVE_TARGET_END}",
                UNIT.repeat(n)
            );
            let (fast, eager) = target_probe_witness(&source);
            assert!(fast.navigation_relocated_word_starts >= n, "{n}: {fast:?}");
            assert_eq!(fast.navigation_target_probe_visits, 0, "{n}: {fast:?}");
            assert!(eager.navigation_target_probe_visits > n, "{n}: {eager:?}");
            eprintln!(
                "live target {n} inset={} starts={} visits {}→{}",
                !start.is_empty(),
                fast.navigation_relocated_word_starts,
                eager.navigation_target_probe_visits,
                fast.navigation_target_probe_visits
            );
        }
    }
}

#[test]
fn active_targets_and_true_row_retirement_preserve_navigation_projection() {
    // Pinned mdoc_validate.c::post_tg tags a single source owner. Native
    // term_word and the actual .br boundaries independently distinguish
    // surviving, covered, duplicated and flushed owners; navigation's
    // exact canonical IDs/source spans must stay as in the eager path.
    let source = |body: &str| format!("{LIVE_TARGET_HEAD}{body}{LIVE_TARGET_END}");
    let head = ".No HEAD\n";
    let rewind = ".No \"\\h'|0n'R\"\n.Lk https://z.test Z\nEND\n";
    let mut active = 0;
    for before in [
        ".Tg Keep.Target\n.Sy Y\n",
        ".Tg Keep.Target\n.Em Y\n",
        ".Tg Earlier\n.Sy HEAD2\n.Sy Y\n",
        ".Tg Previous\n.Sy Y\n.br\n.No HEAD\nY\n",
        "Y\n",
    ] {
        let body = if before == "Y\n" {
            format!("{head}{before}.No R\n.Lk https://z.test Z\nEND\n")
        } else {
            format!("{head}{before}{rewind}")
        };
        let (fast, eager) = target_probe_witness(&source(&body));
        assert!(fast.navigation_target_probe_visits <= eager.navigation_target_probe_visits);
        active += fast.navigation_target_probe_visits;
        if before.contains("Previous") || before == "Y\n" {
            assert_eq!(fast.navigation_target_probe_visits, 0, "{body}");
        }
    }
    let duplicate = format!(
        "{head}.Tg Keep.Target\n.Sy Y\n{}\
        .br\n.Tg Keep.Target\n.Sy B\nEND\n",
        rewind.trim_end_matches("END\n")
    );
    let (fast, eager) = target_probe_witness(&source(&duplicate));
    assert!(fast.navigation_target_probe_visits <= eager.navigation_target_probe_visits);
    assert!(
        active + fast.navigation_target_probe_visits > 0,
        "must exercise registered moves"
    );
}

#[test]
fn contiguous_bare_uri_excludes_relocated_old_suffix_without_changing_disjoint_labels() {
    let head = ".Dd November 27, 2026\n.Dt GLYPHS 1\n.Os\n.Sh NAME\n.Nm glyphs\n.Nd small roff inputs\n.Sh DESCRIPTION\n.No HEAD\n";
    for (source, expected, reading) in [
        (
            "Y\n.No \"\\h'|1n'R\"\n.Lk https://z.test Z U\nEND\n",
            &["ZYU"][..],
            "HRAZYU:https://z.testEND",
        ),
        (
            "YABCDEFGHIJKLMNOPQRSTUVW\n.No \"\\h'|0n'R\"\n.Lk https://z.test\nEND\n",
            &["https://z.test"][..],
            "REhttps://z.testJENDNOPQRSTUVW",
        ),
        (
            "Y\n.Lk https://z.test Z U\nEND\n",
            &["Z U"][..],
            "HEADYZU:https://z.testEND",
        ),
        (
            "Y\n.Lk https://z.test Z U\n.br\n.Lk https://z.test V W\nEND\n",
            &["Z U", "V W"][..],
            "HEADYZU:https://z.testVW:https://z.testEND",
        ),
    ] {
        let page = format!("{head}{source}.Sh ENDTEST\nDone.\n");
        let document =
            parse_manual_bytes(std::path::Path::new("glyphs.1"), page.as_bytes()).unwrap();
        let section = document
            .sections
            .iter()
            .find(|section| mant_ir::inline_plain_text(&section.heading.content) == "DESCRIPTION")
            .unwrap();
        let mut links = Links::default();
        links.visit_section(section);
        let labels: Vec<_> = links.0.iter().map(|(target, children)| {
            assert!(matches!(target, mant_ir::LinkTarget::External { uri } if uri == "https://z.test"));
            mant_ir::inline_plain_text(children)
        }).collect();
        assert_eq!(labels, expected, "{source}: {section:?}");
        let document_text = projected_document_text(&document);
        let visible: String = document_text
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        assert!(visible.contains(reading), "{source}: {document_text}");
        let wire = serde_json::to_string(&document).unwrap();
        assert_public_wire(&wire, &serde_json::Value::from(source));
        assert_eq!(
            serde_json::from_str::<mant_ir::Document>(&wire).unwrap(),
            document
        );
    }
}
