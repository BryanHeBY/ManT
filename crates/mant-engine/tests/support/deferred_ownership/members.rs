//! Independent original glyphs, accepted sibling/tail words and native source
//! diagnostics. Empty recovery vectors do not exempt a native control.
use super::{Case, Snapshot, Style, positions};
use mant_ir::ResolvedContent;
use serde_json::Value;
use std::collections::BTreeMap;

fn member(at: usize, value: &Value, actual: &Snapshot, id: &str) {
    let style: Style = serde_json::from_value(value["style"].clone()).unwrap();
    let link = value["link"]
        .as_u64()
        .map(|link| usize::try_from(link).unwrap());
    for cell in &actual.cells[at..at + value["text"].as_str().unwrap().chars().count()] {
        assert_eq!(cell.style, style, "{id} native member {value}");
        assert_eq!(cell.link, link, "{id} original annotation {value}");
        if let Some(owner) = value["owner"].as_str() {
            assert!(cell.owner.contains(owner), "{id} source receiver {value}");
        }
    }
}

pub(super) fn check(content: &ResolvedContent, case: &Case, actual: &Snapshot) {
    let Some(facts) = &case.facts else {
        return;
    };
    let text = actual.text();
    let mut members = BTreeMap::new();
    for value in facts["members"].as_array().unwrap() {
        let link = value["link"]
            .as_u64()
            .map(|link| usize::try_from(link).unwrap());
        let spans = positions(&text, value["text"].as_str().unwrap())
            .into_iter()
            .filter(|at| actual.cells[*at].link == link)
            .collect::<Vec<_>>();
        let expected = usize::try_from(value["occurrences"].as_u64().unwrap()).unwrap();
        assert_eq!(
            spans.len(),
            expected,
            "{} original native member {value}",
            case.id
        );
        if value["survives"] == true {
            assert_eq!(spans.len(), 1);
            member(spans[0], value, actual, &case.id);
            members.insert(value["id"].as_str().unwrap().to_owned(), spans[0]);
        } else {
            assert_eq!(expected, 0);
        }
    }
    for value in facts["tail_members"].as_array().unwrap() {
        let spans = positions(&text, value["text"].as_str().unwrap());
        assert_eq!(
            spans.len(),
            usize::try_from(value["occurrences"].as_u64().unwrap()).unwrap()
        );
        member(spans[0], value, actual, &case.id);
        for link in &actual.links {
            if value["before_all_right_cell_members"] == true {
                assert!(spans[0] < link.start);
            }
            if value["after_all_term_members"] == true
                || value["after_all_left_cell_members"] == true
            {
                // A following ordinary AFTERLABEL is outside the completed
                // field/table; only preceding owners participate in this rule.
                if link.owner.contains("/term/") || link.owner.contains("/column/0/") {
                    assert!(link.end <= spans[0]);
                }
            }
        }
        if let Some(code) = value["after_code_value"].as_str() {
            assert!(positions(&text, code)[0] < spans[0]);
        }
    }
    let mut previous = None;
    for value in facts["source_member_order"].as_array().unwrap() {
        let at = if value["kind"] == "glyph" {
            members.get(value["id"].as_str().unwrap()).copied()
        } else {
            let ordinal = value["source_ordinal"].as_u64().unwrap();
            let recovery = &case.recoveries[usize::try_from(ordinal).unwrap()];
            Some(positions(&text, &recovery.text)[recovery.ordinal])
        };
        if let Some(at) = at {
            if let Some(previous) = previous {
                assert!(previous < at, "{} original member order", case.id);
            }
            previous = Some(at);
        }
    }
    hard_rows(case, actual, facts);
    let diagnostics =
        serde_json::to_value(&content.document.as_ref().unwrap().diagnostics).unwrap();
    for native in facts["required_native_diagnostics"].as_array().unwrap() {
        assert!(
            diagnostics.as_array().unwrap().contains(native),
            "{} missing native source diagnostic {native}",
            case.id
        );
    }
}

fn hard_rows(case: &Case, actual: &Snapshot, facts: &Value) {
    let text = actual.text();
    for witness in facts["hardline_witnesses"].as_array().unwrap() {
        let right = usize::try_from(
            witness
                .get("right_link_ordinal")
                .or_else(|| witness.get("following_link_ordinal"))
                .unwrap()
                .as_u64()
                .unwrap(),
        )
        .unwrap();
        let right = actual.links[right].start;
        let ordinals = witness["recovery_source_ordinals"].as_array().map_or_else(
            || vec![witness["left_recovery_source_ordinal"].clone()],
            Clone::clone,
        );
        for ordinal in ordinals {
            let recovery = &case.recoveries[usize::try_from(ordinal.as_u64().unwrap()).unwrap()];
            let left = positions(&text, &recovery.text)[recovery.ordinal];
            let rows = actual.cells[left..right]
                .iter()
                .filter(|cell| cell.value == '\n')
                .count();
            let expected = if witness["type"] == "same-physical-row" {
                0
            } else {
                usize::try_from(witness["row_difference"].as_u64().unwrap()).unwrap()
            };
            assert_eq!(
                rows, expected,
                "{} exact native physical boundary {witness}",
                case.id
            );
        }
    }
}

/// Portable Markdown may omit a link or flatten a fence. That permission
/// affects style/geometry, never the original surviving glyphs and tail words.
pub fn check_portable(case: &Case, actual: &Snapshot, ordinals: &[Option<usize>]) {
    let mut members = Vec::new();
    if let Some(old) = &case.old_x
        && case.facts.is_none()
    {
        members.push(serde_json::json!({"text":"X", "occurrences":usize::from(old["present"] == true), "link":old.get("link").cloned()}));
    }
    if let Some(facts) = &case.facts {
        members.extend(facts["members"].as_array().unwrap().iter().cloned());
        members.extend(facts["tail_members"].as_array().unwrap().iter().cloned());
    }
    let text = actual.text();
    for member in members {
        let word = member["text"].as_str().unwrap();
        let old_link = member["link"]
            .as_u64()
            .map(|value| usize::try_from(value).unwrap());
        let projected = old_link.and_then(|ordinal| ordinals[ordinal]);
        let count = positions(&text, word)
            .into_iter()
            .filter(|at| actual.cells[*at].link == projected)
            .count();
        let omitted_label_members = if projected.is_none() {
            case.links
                .iter()
                .enumerate()
                .filter(|(ordinal, _)| Some(*ordinal) != old_link && ordinals[*ordinal].is_none())
                .map(|(_, link)| link.text.matches(word).count())
                .sum::<usize>()
        } else {
            0
        };
        assert_eq!(
            count,
            usize::try_from(member["occurrences"].as_u64().unwrap()).unwrap()
                + omitted_label_members,
            "{} portable original graph/tail {member}",
            case.id
        );
    }
    portable_order(case, actual, ordinals);
    for constraint in &case.constraints {
        if constraint["kind"] == "code-root" {
            let code = constraint["text"].as_str().unwrap();
            assert_eq!(actual.codes, [code], "{} portable Code payload", case.id);
            let tail = constraint["trailing_plain_text"].as_str().unwrap();
            assert!(positions(&text, code)[0] < positions(&text, tail)[0]);
        }
    }
}

fn portable_order(case: &Case, actual: &Snapshot, ordinals: &[Option<usize>]) {
    let text = actual.text();
    let mut order = Vec::new();
    let mut record = |value: &Value| {
        let count = value["occurrences"].as_u64().unwrap();
        if count == 0 {
            return;
        }
        let word = value["text"].as_str().unwrap();
        let link = value["link"]
            .as_u64()
            .and_then(|link| ordinals[usize::try_from(link).unwrap()]);
        let at = positions(&text, word)
            .into_iter()
            .find(|at| actual.cells[*at].link == link)
            .unwrap();
        order.push((value["source"]["byteRange"]["start"].as_u64().unwrap(), at));
    };
    if let Some(facts) = &case.facts {
        for value in facts["members"]
            .as_array()
            .unwrap()
            .iter()
            .chain(facts["tail_members"].as_array().unwrap())
        {
            record(value);
        }
    } else if let Some(old) = case.old_x.as_ref().filter(|old| old["present"] == true) {
        record(
            &serde_json::json!({"text":"X", "occurrences":1,"link":null,"source":old["source"]}),
        );
    }
    for recovery in &case.recoveries {
        order.push((
            recovery.source["byteRange"]["start"].as_u64().unwrap(),
            positions(&text, &recovery.text)[recovery.ordinal],
        ));
    }
    for (ordinal, link) in case.links.iter().enumerate() {
        if link.text.is_empty() {
            continue;
        }
        let at = ordinals[ordinal].map_or_else(
            || {
                let occurrence = case.links[..ordinal]
                    .iter()
                    .filter(|other| other.text == link.text)
                    .count();
                positions(&text, &link.text)[occurrence]
            },
            |ordinal| actual.links[ordinal].start,
        );
        order.push((link.source[0]["byteRange"]["start"].as_u64().unwrap(), at));
    }
    for constraint in &case.constraints {
        if constraint["kind"] == "code-root" {
            order.push((
                constraint["source"]["byteRange"]["start"].as_u64().unwrap(),
                positions(&text, constraint["text"].as_str().unwrap())[0],
            ));
        }
    }
    order.sort_unstable();
    assert!(
        order.windows(2).all(|pair| pair[0].1 <= pair[1].1),
        "{} portable source-member order {order:?}",
        case.id
    );
}
