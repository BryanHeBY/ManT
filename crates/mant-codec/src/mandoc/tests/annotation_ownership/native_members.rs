//! Hidden native cells have source positions and accepted-field receipts.
use super::{Inline, Links};

pub(super) fn check(
    case: &serde_json::Value,
    links: &Links,
    text: &str,
    scalars: &[(char, u8)],
    failures: &mut Vec<String>,
) {
    for member in case["unknown_members"].as_array().unwrap() {
        let literal = member["literal"].as_str().unwrap();
        let Some(expected) = member["expected_description_occurrences"].as_u64() else {
            assert_eq!(member["source_role"], "authored-final-Sh");
            continue;
        };
        let expected = usize::try_from(expected).unwrap();
        if text.matches(literal).count() != expected {
            failures.push(format!(
                "{} native member {literal:?} count {} != {expected}: {text:?}",
                case["id"],
                text.matches(literal).count()
            ));
        }
        let style = &member["source_style"];
        let wanted = u8::from(style["strong"] == true)
            | (u8::from(style["emphasis"] == true) << 1)
            | (u8::from(style["code"] == true) << 2);
        for (byte, _) in text.match_indices(literal) {
            let start = text[..byte].chars().count();
            for (_, actual) in &scalars[start..start + literal.chars().count()] {
                if *actual != wanted {
                    failures.push(format!(
                        "{} member {literal:?} style {actual} != {wanted}",
                        case["id"]
                    ));
                    break;
                }
            }
        }
        if member["reference_ordinal"].as_u64().is_some() {
            for (index, (_, children)) in links.0.iter().enumerate() {
                // Two true sources can use the same literal. Count this
                // label's source records, not that spelling globally.
                let expected = case["unknown_members"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|other| {
                        other["literal"] == literal
                            && other["reference_ordinal"].as_u64()
                                == Some(u64::try_from(index).unwrap())
                    })
                    .count();
                check_count(
                    case,
                    children,
                    literal,
                    u64::try_from(expected).unwrap(),
                    failures,
                );
            }
        } else {
            if let Some(expected) = member["expected_primary_label_occurrences"].as_u64() {
                let primary = case["expected_references"][0]["source_role"]
                    .as_str()
                    .unwrap()
                    .starts_with("primary-");
                for (index, (_, children)) in links.0.iter().enumerate() {
                    check_count(
                        case,
                        children,
                        literal,
                        if primary && index == 0 { expected } else { 0 },
                        failures,
                    );
                }
            }
            if let Some((_, children)) = links.0.last() {
                check_count(
                    case,
                    children,
                    literal,
                    member["expected_LAST_label_occurrences"].as_u64().unwrap(),
                    failures,
                );
            }
        }
    }
}

fn check_count(
    case: &serde_json::Value,
    nodes: &[Inline],
    literal: &str,
    expected: u64,
    failures: &mut Vec<String>,
) {
    let text = mant_ir::inline_plain_text(nodes);
    let actual = text.matches(literal).count();
    if actual != usize::try_from(expected).unwrap() {
        failures.push(format!(
            "{} label native member {literal:?} count {actual} != {expected}: {text:?}",
            case["id"]
        ));
    }
}

pub(super) fn check_heading(
    case: &serde_json::Value,
    document: &mant_ir::Document,
    failures: &mut Vec<String>,
) {
    for member in case["unknown_members"].as_array().unwrap() {
        if member["source_role"] != "authored-final-Sh" {
            continue;
        }
        let text = mant_ir::inline_plain_text(&document.sections.last().unwrap().heading.content);
        let literal = member["literal"].as_str().unwrap();
        if text.matches(literal).count() != 1 {
            failures.push(format!(
                "{} final heading lost own native member {literal:?}: {text:?}",
                case["id"]
            ));
        }
    }
}
