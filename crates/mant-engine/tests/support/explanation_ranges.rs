//! Resolve real response-local previews and name/form members through support pools.
pub fn assert_ranges(explanation: &mant_protocol::QueryExplanation, label: &str) -> usize {
    let mut selected = 0;
    for evidence in &explanation.evidence {
        let Some(content) = &evidence.content else {
            continue;
        };
        for range in evidence
            .previews
            .iter()
            .flat_map(|preview| &preview.content_ranges)
        {
            let actual = content
                .resolve_range(&explanation.supports, range)
                .unwrap()
                .safe_text()
                .chars()
                .skip(range.char_range().start)
                .take(range.char_range().len())
                .collect::<String>();
            assert_eq!(actual, label);
            selected += 1;
        }
    }
    if selected == 0 {
        selected = assert_name_occurrences(explanation, label);
    }
    selected
}

fn assert_name_occurrences(explanation: &mant_protocol::QueryExplanation, label: &str) -> usize {
    let mut selected = 0;
    for evidence in &explanation.evidence {
        let has_name = evidence
            .bases
            .iter()
            .any(|basis| matches!(basis, mant_protocol::EvidenceBasis::Name { .. }));
        for basis in &evidence.bases {
            // Name and complete-form are two bases for the same physical
            // occurrence, not two source owners. Prefer the exact name domain.
            if has_name && matches!(basis, mant_protocol::EvidenceBasis::Form { .. }) {
                continue;
            }

            let occurrences = match basis {
                mant_protocol::EvidenceBasis::Name { matches } => matches
                    .iter()
                    .flat_map(|m| &m.occurrences)
                    .collect::<Vec<_>>(),
                mant_protocol::EvidenceBasis::Form { matches } => matches
                    .iter()
                    .flat_map(|m| &m.occurrences)
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            };
            for occurrence in occurrences {
                if !occurrence.content.is_empty() {
                    let body = evidence.content.as_ref().expect("returned name content");
                    let text = occurrence
                        .content
                        .iter()
                        .map(|range| {
                            body.resolve_range(&explanation.supports, range)
                                .unwrap()
                                .safe_text()
                                .chars()
                                .skip(range.char_range().start)
                                .take(range.char_range().len())
                                .collect::<String>()
                        })
                        .collect::<String>();
                    assert_eq!(text, label);
                }
                if !occurrence.forms.is_empty() {
                    let forms = &evidence.entry.as_ref().expect("returned name forms").forms;
                    let text = occurrence
                        .forms
                        .iter()
                        .map(|range| {
                            mant_ir::inline_plain_text(range.resolve(forms).unwrap())
                                .chars()
                                .skip(usize::try_from(range.start_char).unwrap())
                                .take(usize::try_from(range.end_char - range.start_char).unwrap())
                                .collect::<String>()
                        })
                        .collect::<String>();
                    assert_eq!(text, label);
                }
                if !occurrence.content.is_empty() || !occurrence.forms.is_empty() {
                    selected += 1;
                }
            }
        }
    }
    selected
}
