//! Independent complete-source witnesses shared by actual consumer tests.
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Case {
    pub id: String,
    pub source: String,
    pub expected_label: String,
}

pub fn cases() -> Vec<Case> {
    #[derive(Deserialize)]
    struct Fixture {
        cases: Vec<Case>,
    }
    serde_json::from_str::<Fixture>(include_str!(
        "../roff_lowering/table_execution/consumers.json"
    ))
    .unwrap()
    .cases
}

pub fn positions(text: &str, word: &str) -> Vec<usize> {
    let part = |character: char| character.is_alphanumeric() || matches!(character, '_' | '-');
    text.match_indices(word)
        .filter_map(|(start, _)| {
            let end = start + word.len();
            (!text[..start].chars().next_back().is_some_and(part)
                && !text[end..].chars().next().is_some_and(part))
            .then_some(start)
        })
        .collect()
}

pub fn assert_follower(text: &str, case: &Case) {
    let label = positions(text, &case.expected_label);
    let after = positions(text, "AFTER");
    assert_eq!((label.len(), after.len()), (1, 1), "{}: {text}", case.id);
    assert!(label[0] < after[0], "{}: source order", case.id);
    if case.source.contains("BETA") {
        let beta = positions(text, "BETA");
        assert_eq!(beta.len(), 1, "{}: {text}", case.id);
        assert!(
            label[0] < beta[0] && beta[0] < after[0],
            "{}: source order",
            case.id
        );
    }
}
