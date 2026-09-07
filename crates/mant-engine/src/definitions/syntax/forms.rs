//! Keep alias separators and parameter styling distinct until name extraction.
use mant_ir::Inline;

/// Split explicit alias separators without flattening argument spans. A generic
/// strong/code run can contain several names, whereas punctuation inside an
/// emphasized argument is not evidence for another alias.
pub(super) fn alias_groups(term: &[Inline]) -> Vec<Vec<Inline>> {
    let mut groups = vec![Vec::new()];
    for inline in term {
        let parts = match inline {
            Inline::Text { value } => value
                .split([',', '|'])
                .map(|value| {
                    vec![Inline::Text {
                        value: value.into(),
                    }]
                })
                .collect(),
            Inline::Code { value } => value
                .split([',', '|'])
                .map(|value| {
                    vec![Inline::Code {
                        value: value.into(),
                    }]
                })
                .collect(),
            Inline::Strong { children } => alias_groups(children)
                .into_iter()
                .map(|children| vec![Inline::Strong { children }])
                .collect(),
            _ => vec![vec![inline.clone()]],
        };
        for (index, part) in parts.into_iter().enumerate() {
            if index > 0 {
                groups.push(Vec::new());
            }
            groups.last_mut().expect("at least one group").extend(part);
        }
    }
    groups
}

pub(super) fn starts_with_parameter(term: &[Inline]) -> bool {
    term.iter()
        .find(|inline| match inline {
            Inline::Anchor { .. } => false,
            Inline::Text { value } => !value.trim().is_empty(),
            _ => true,
        })
        .is_some_and(|inline| matches!(inline, Inline::Emphasis { .. }))
}
