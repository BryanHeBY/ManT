//! Observe actual SGR runs, not only text after stripping decoration.
use serde_json::Value;

pub(super) fn assert_recovery_styles(ansi: &str, case: &Value) {
    let mut chars = ansi.chars();
    let mut cells = Vec::new();
    let mut bold = false;
    let mut italic = false;
    while let Some(value) = chars.next() {
        if value != '\u{1b}' {
            cells.push((value, bold, italic));
            continue;
        }
        assert_eq!(chars.next(), Some('['));
        let sequence = chars.by_ref().take_while(|c| *c != 'm').collect::<String>();
        let mut codes = sequence.split(';').map(|part| part.parse::<u16>().unwrap());
        while let Some(code) = codes.next() {
            match code {
                0 => {
                    bold = false;
                    italic = false;
                }
                1 => bold = true,
                3 => italic = true,
                22 => bold = false,
                23 => italic = false,
                38 | 48 | 58 => match codes.next() {
                    Some(5) => {
                        codes.next().unwrap();
                    }
                    Some(2) => {
                        for _ in 0..3 {
                            codes.next().unwrap();
                        }
                    }
                    other => panic!("invalid extended color {other:?}"),
                },
                _ => {}
            }
        }
    }
    let text = cells.iter().map(|(value, _, _)| value).collect::<String>();
    for recovery in case["recoveries"].as_array().unwrap() {
        let word = recovery["text"].as_str().unwrap();
        let ordinal = usize::try_from(recovery["ordinal"].as_u64().unwrap()).unwrap();
        let at = text.match_indices(word).nth(ordinal).unwrap().0;
        let start = text[..at].chars().count();
        for (_, bold, italic) in &cells[start..start + word.chars().count()] {
            assert_eq!(
                (*bold, *italic),
                (
                    recovery["style"]["strong"].as_bool().unwrap(),
                    recovery["style"]["emphasis"].as_bool().unwrap()
                ),
                "{} actual SGR source member",
                case["id"]
            );
        }
    }
}
