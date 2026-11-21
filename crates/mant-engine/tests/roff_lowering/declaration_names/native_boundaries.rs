//! Exact names and queryable signatures follow the native operand boundaries.

use super::{ExplanationOptions, ExplanationQuery, assert_name_positions, owners};
use crate::json_roundtrip::json_string_roundtrip;
use mant_ir::inline_plain_text;

#[path = "native_boundaries/alias_boundaries.rs"]
mod alias_boundaries;
#[path = "native_boundaries/assignment_groups.rs"]
mod assignment_groups;
#[path = "native_boundaries/assignment_values.rs"]
mod assignment_values;
#[path = "native_boundaries/path_values.rs"]
mod path_values;

const HEADER: &str = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n";

fn check(section: &str, head: &str, names: &[&str], label: &str, full: bool, hidden: &[&str]) {
    let source =
        format!("{HEADER}.Sh {section}\n.Bl -tag -width Ds\n.It {head}\nOwner proof.\n.El\n");
    let native = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let content = json_string_roundtrip(&native);
    let entries = owners(content.document.as_ref().unwrap());
    let [owner] = entries.as_slice() else {
        panic!("expected one native It owner: {source}");
    };
    let facts = owner.facts().unwrap();
    assert_eq!(facts.names, names, "{head}");
    let actual = owner
        .forms()
        .unwrap()
        .iter()
        .map(inline_plain_text)
        .collect::<Vec<_>>();
    let forms = if full {
        std::slice::from_ref(&label)
    } else {
        names
    };
    assert_eq!(actual, forms, "{head}");
    for name in names {
        assert_name_positions(&content, *owner, name);
    }
    for (query, expected) in names
        .iter()
        .map(|name| (*name, true))
        .chain(std::iter::once((label, full)))
        .chain(hidden.iter().map(|name| (*name, false)))
    {
        let explanation = mant_query::explain_query(
            &content,
            &ExplanationQuery {
                entry: query.into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(
            explanation.counts.direct_entry.total,
            u32::from(expected),
            "{head}: {query}"
        );
        if expected {
            assert!(mant_render::render_explanation_text(&explanation).contains("Owner proof."));
        }
    }
}
