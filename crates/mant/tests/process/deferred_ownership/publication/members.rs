use super::{Case, descendants, inline_text};
use serde_json::{Value, json};

const ROWS: &str = include_str!(
    "../../../../../mant-engine/tests/roff_lowering/annotation_publication/rows.jsonl"
);

fn expected_target(wrapper: &str) -> Value {
    match wrapper {
        "MT" => json!({"kind":"email","address":"x@example.test"}),
        _ => json!({"kind":"external","uri":"https://one.test"}),
    }
}

pub(super) fn prefix(case: &Case<'_>, links: &[&Value]) {
    // CVS term.c term_word executes invisible atoms and horizontal movement
    // separately. Only an accepted author member opens the clickable label.
    let expected = if case.family == "X-prefix-gap" {
        "LABEL".to_owned()
    } else {
        let prefix = match case.axes[2] {
            "space" => " ",
            "nbsp" => "\u{a0}",
            "visible" => "A",
            "recovery" => r"\[recovered]",
            "none" | "zero" | "break" | "font" => "",
            _ => unreachable!(),
        };
        if prefix.is_empty() {
            "LABEL".to_owned()
        } else {
            // The reading projection retains one internal motion cell and,
            // for Lk split operands with actual content, its word separator.
            let gap = if case.axes[4] == "split" { "  " } else { " " };
            format!("{prefix}{gap}LABEL")
        }
    };
    assert_eq!(links.len(), 1, "{} link occurrences", case.id);
    assert_eq!(
        links[0]["target"],
        expected_target(case.axes[0]),
        "{} target",
        case.id
    );
    assert_eq!(
        inline_text(&links[0]["children"]),
        expected,
        "{} label",
        case.id
    );
}

pub(super) fn gap(case: &Case<'_>, binary: &str, blocks: &[Value], links: &[&Value]) {
    let cross = case.family == "X-prefix-gap";
    let label = if cross { "LABEL" } else { "LASTMARK" };
    if case.axes[0] == "plain" {
        assert!(links.is_empty(), "{} plain input", case.id);
    } else {
        assert_eq!(links.len(), 1, "{} link occurrences", case.id);
        assert_eq!(
            links[0]["target"],
            expected_target(case.axes[0]),
            "{} target",
            case.id
        );
        assert_eq!(
            inline_text(&links[0]["children"]),
            label,
            "{} label",
            case.id
        );
    }
    physical_rows(case, binary);
    spacing_sources(case, blocks);
}

pub(super) fn spacing_sources(case: &Case<'_>, blocks: &[Value]) {
    let cross = case.family == "X-prefix-gap";
    let label = if cross { "LABEL" } else { "LASTMARK" };
    let first = cross || case.axes[1] == "first";
    let gap = case.axes[if cross { 1 } else { 2 }];
    let requested = match gap {
        "one" => 1,
        "two" => 2,
        "twice" => 3,
        _ => 0,
    };
    if first && requested > 0 {
        // roff_term_pre_sp calls term_vspace before any physical content
        // exists. A not-yet-populated link has no line or source to lend it.
        let mut spacing = blocks
            .iter()
            .take_while(|block| block["type"] == "vertical-space");
        let request = case
            .source
            .lines()
            .position(|line| line.starts_with(".sp"))
            .unwrap()
            + 1;
        let first_spacing = spacing
            .next()
            .unwrap_or_else(|| panic!("{} external gap", case.id));
        assert_eq!(
            first_spacing["source"],
            json!({"line":request,"column":2}),
            "{}",
            case.id
        );
        let rows: u64 = std::iter::once(first_spacing)
            .chain(spacing)
            .map(|block| block["lines"].as_u64().unwrap())
            .sum();
        assert_eq!(rows, requested, "{} explicit vertical space", case.id);
        let actual = blocks
            .iter()
            .find(|block| block["type"] == "preformatted")
            .unwrap();
        let source = case
            .source
            .lines()
            .position(|line| line.ends_with(label))
            .unwrap()
            + 1;
        assert_eq!(
            actual["source"],
            json!({"line":source,"column":1}),
            "{} literal source",
            case.id
        );
    }
    if !cross && case.axes[1] == "real" {
        assert_eq!(
            blocks[0]["type"], "preformatted",
            "{} internal gap",
            case.id
        );
        assert!(
            inline_text(&blocks[0]["children"]).starts_with("REALLINE\n"),
            "{}",
            case.id
        );
    }
}

fn physical_rows(case: &Case<'_>, binary: &str) {
    let rendered = String::from_utf8(case.output(binary, "man", &[])).unwrap();
    assert_rows(case, &rendered);
}

pub(super) fn assert_rows(case: &Case<'_>, rendered: &str) {
    let gold = ROWS
        .lines()
        .map(|row| serde_json::from_str::<Value>(row).unwrap())
        .find(|row| row["caseId"] == case.id)
        .unwrap();
    if gold["status"] == "Review" {
        return;
    }
    let region = rendered.split_once("DESCRIPTION\n").unwrap().1;
    let rows: Vec<_> = region.lines().collect();
    let last = rows.iter().rposition(|row| row.contains("TAIL")).unwrap();
    assert_eq!(
        json!(rows[..=last]),
        gold["expectation"],
        "{} physical rows",
        case.id
    );
}

pub(super) fn row(case: &Case<'_>, binary: &str, links: &[&Value]) {
    // man_term.c pre_literal ends the current terminal row for both EX
    // and EE. Only a real fill transition ends the existing label root.
    physical_rows(case, binary);
    label_rows(case, links);
}

pub(super) fn label_rows(case: &Case<'_>, links: &[&Value]) {
    if case.axes[0] == "plain" {
        assert!(links.is_empty(), "{}", case.id);
        return;
    }
    assert_eq!(links.len(), 1, "{} link occurrences", case.id);
    assert_eq!(
        links[0]["target"],
        expected_target(case.axes[0]),
        "{} target",
        case.id
    );
    let changed = matches!(
        (case.axes[1], case.axes[2]),
        ("filled", "EX" | "nf") | ("nofill", "EE" | "fi")
    );
    let actual = inline_text(&links[0]["children"]);
    if case.axes[3] == "zero" {
        assert_eq!(
            actual.contains("LASTMARK"),
            !changed,
            "{} zero label member",
            case.id
        );
    } else {
        let expected = if changed {
            "FIRSTMARK"
        } else {
            "FIRSTMARK\nLASTMARK"
        };
        assert_eq!(actual, expected, "{} label and internal hard row", case.id);
    }
}

fn references_with_suffix(case: &Case<'_>, section: &Value) -> Vec<(&'static str, String)> {
    let suffix = match case.axes[0] {
        "exact" | "raw" => " <>",
        "near" => " <X>",
        "none" => "",
        _ => unreachable!(),
    };
    let scope = case.axes[2];
    let traditional = case.axes[1] != "word";
    let generated = case.axes[0] == "exact" && matches!(scope, "plain" | "outside");
    let protected = matches!(scope, "UR" | "MT");
    let mut expected = Vec::new();
    if protected {
        let kind = if scope == "MT" { "email" } else { "external" };
        expected.push((kind, format!("printf(3){suffix}")));
    } else if scope == "outside" {
        expected.push(("external", "FIRST".to_owned()));
    }
    if !protected && (traditional || generated) {
        expected.push(("manual", "printf(3)".to_owned()));
    }
    let blocks = &section["blocks"];
    let mut body = descendants(blocks, "paragraph");
    body.extend(descendants(blocks, "preformatted"));
    let text: String = body
        .iter()
        .map(|block| inline_text(&block["children"]))
        .collect();
    assert_eq!(
        text.contains(&format!("printf(3){suffix}")),
        !generated || suffix.is_empty(),
        "{} visible suffix",
        case.id
    );
    expected
}

pub(super) fn manual(case: &Case<'_>, section: &Value, links: &[&Value]) {
    let target = json!({"kind":"manual","name":"printf","manualSection":"3"});
    // Traditional BR/IR recognition is independent of the exact-source
    // Sphinx marker. Only an eligible, independently accepted marker vanishes.
    let manual = ("manual", "printf(3)");
    let expected = match case.family {
        "E12" => references_with_suffix(case, section),
        "E12extra" => match case.axes[0] {
            "twice" => vec![("external", "printf(3) <> and open(2) <>".into())],
            "around" => vec![
                (manual.0, manual.1.into()),
                ("external", "printf(3) <>".into()),
                (manual.0, manual.1.into()),
            ],
            "lk" | "tty" => vec![("external", "printf(3) <>".into())],
            _ => unreachable!(),
        },
        "X-head-marker" => {
            let blocks = super::array(&section["blocks"]);
            super::entries::group(case, blocks);
            let provider = &blocks[0]["items"][1]["description"];
            let text: String = descendants(provider, "paragraph")
                .iter()
                .map(|block| inline_text(&block["children"]))
                .collect();
            assert!(text.starts_with("SHAREDBODY"), "{}", case.id);
            match (case.axes[0], case.axes[1]) {
                (_, "head") => vec![("external", "printf(3) <>".into())],
                ("exact", "body") => {
                    assert!(!text.contains(" <>"), "{} promoted BODY marker", case.id);
                    vec![(manual.0, manual.1.into())]
                }
                ("raw", "body") => {
                    assert!(
                        text.contains("printf(3) <>"),
                        "{} literal BODY marker",
                        case.id
                    );
                    Vec::new()
                }
                _ => unreachable!(),
            }
        }
        _ => unreachable!(),
    };
    assert_eq!(links.len(), expected.len(), "{} occurrence count", case.id);
    for (link, (kind, label)) in links.iter().zip(expected) {
        assert_eq!(link["target"]["kind"], kind, "{} target kind", case.id);
        if kind == "manual" {
            assert_eq!(link["target"], target, "{} manual target", case.id);
        } else {
            assert_eq!(
                link["target"],
                expected_target(if kind == "email" { "MT" } else { "UR" }),
                "{} author target",
                case.id
            );
        }
        assert_eq!(
            inline_text(&link["children"]),
            label,
            "{} exact label",
            case.id
        );
        assert!(
            descendants(&link["children"], "link").is_empty(),
            "{} nested link",
            case.id
        );
    }
}

fn loses_equation_member(case: &Case<'_>) -> bool {
    if case.family == "X-line-eqn" {
        matches!(case.axes[1], "EE" | "fi")
    } else {
        matches!(
            case.axes[1],
            "eq" | "first-eq" | "eq-last" | "first-eq-last"
        )
    }
}

pub(super) fn explanation_coverage(case: &Case<'_>, explanation: &mant_protocol::QueryExplanation) {
    assert_eq!(
        explanation.semantics_complete,
        !loses_equation_member(case),
        "{} explicit explain completeness",
        case.id
    );
    assert!(
        explanation.content_complete,
        "{} visible content coverage",
        case.id
    );
}

pub(super) fn equation(case: &Case<'_>, document: &Value, section: &Value, links: &[&Value]) {
    // man_html.c prints ROFFT_EQN inside the UR/MT BODY while it is the
    // active root. The terminal and structured equation retain the formula.
    let has_equation = !matches!(case.axes[1], "config" | "config-word");
    let equations = descendants(&section["blocks"], "equation");
    assert_eq!(
        equations.len(),
        usize::from(has_equation),
        "{} structured equations",
        case.id
    );
    if let Some(eq) = equations.first() {
        assert_eq!(eq["value"], "x ^ 2", "{} readable equation", case.id);
        assert_eq!(
            eq["expression"]["children"][0]["operator"], "superscript",
            "{} structure",
            case.id
        );
        assert_eq!(
            eq["expression"]["children"][0]["children"][0]["text"], "x",
            "{}",
            case.id
        );
        assert_eq!(
            eq["expression"]["children"][0]["children"][1]["text"], "2",
            "{}",
            case.id
        );
    }
    assert_eq!(links.len(), 1, "{} author link", case.id);
    assert_eq!(
        links[0]["target"],
        expected_target(case.axes[0]),
        "{} target",
        case.id
    );
    let coverage = document["diagnostics"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|diagnostic| diagnostic["code"] == "manual.annotation-coverage")
        .collect::<Vec<_>>();
    assert_eq!(
        coverage.len(),
        usize::from(loses_equation_member(case)),
        "{} semantic diagnostic",
        case.id
    );
    if let Some(diagnostic) = coverage.first() {
        assert_eq!(diagnostic["impact"], "semantic-coverage", "{}", case.id);
    }
}
