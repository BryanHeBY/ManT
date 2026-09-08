//! Restore effective SGR at physical row boundaries, never insert source newlines.
use std::borrow::Cow;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct SgrState(BTreeMap<u16, String>);

impl SgrState {
    pub(super) fn logical_line<'a>(&mut self, line: &'a str) -> Cow<'a, str> {
        let prefix = (!self.0.is_empty()).then(|| self.prefix());
        self.scan(line);
        prefix.map_or(Cow::Borrowed(line), |prefix| Cow::Owned(prefix + line))
    }
    pub(super) fn scan(&mut self, text: &str) {
        let bytes = text.as_bytes();
        let mut offset = 0;
        while offset + 2 < bytes.len() {
            if bytes[offset..].starts_with(b"\x1b[") {
                let start = offset + 2;
                let mut end = start;
                while end < bytes.len() && matches!(bytes[end], b'0'..=b'9' | b';' | b':') {
                    end += 1;
                }
                if bytes.get(end) == Some(&b'm') {
                    self.apply(&text[start..end]);
                    offset = end;
                }
            }
            offset += 1;
        }
    }

    pub(super) fn reversed(&self) -> bool {
        self.0.contains_key(&7)
    }

    fn apply(&mut self, sequence: &str) {
        let mut fields = sequence.split(';').peekable();
        while let Some(field) = fields.next() {
            let Ok(code) = field
                .split(':')
                .next()
                .unwrap_or("")
                .parse::<u16>()
                .or_else(|error| if field.is_empty() { Ok(0) } else { Err(error) })
            else {
                continue;
            };
            match code {
                0 => self.0.clear(),
                22 => {
                    self.0.remove(&1);
                    self.0.remove(&2);
                }
                23 => {
                    self.0.remove(&3);
                    self.0.remove(&20);
                }
                24 => {
                    self.0.remove(&4);
                    self.0.remove(&21);
                }
                25 => {
                    self.0.remove(&5);
                    self.0.remove(&6);
                }
                27..=29 => {
                    self.0.remove(&(code - 20));
                }
                39 => {
                    self.0.remove(&30);
                }
                49 => {
                    self.0.remove(&40);
                }
                54 => {
                    self.0.remove(&51);
                    self.0.remove(&52);
                }
                55 => {
                    self.0.remove(&53);
                }
                59 => {
                    self.0.remove(&58);
                }
                38 | 48 | 58 => {
                    let mut value = field.to_owned();
                    if !field.contains(':') {
                        let Some(mode) = fields.next() else { continue };
                        value.push(';');
                        value.push_str(mode);
                        let count = match mode {
                            "2" => 3,
                            "5" => 1,
                            _ => continue,
                        };
                        let mut valid = true;
                        for _ in 0..count {
                            let Some(component) = fields.next().filter(|v| v.parse::<u8>().is_ok())
                            else {
                                valid = false;
                                break;
                            };
                            value.push(';');
                            value.push_str(component);
                        }
                        if !valid {
                            continue;
                        }
                    }
                    if value.len() <= 64 {
                        self.0.insert(
                            if code == 38 {
                                30
                            } else if code == 48 {
                                40
                            } else {
                                58
                            },
                            value,
                        );
                    }
                }
                30..=37 | 90..=97 => {
                    self.0.insert(30, field.to_owned());
                }
                40..=47 | 100..=107 => {
                    self.0.insert(40, field.to_owned());
                }
                1..=9 | 20..=21 | 51..=53 if field.len() <= 64 => {
                    self.0.insert(code, field.to_owned());
                }
                _ => {}
            }
        }
    }

    fn prefix(&self) -> String {
        let mut output = String::from("\x1b[0m");
        for field in self.0.values() {
            output.push_str("\x1b[");
            output.push_str(field);
            output.push('m');
        }
        output
    }
}

pub(super) fn independent_rows(rows: Vec<Cow<'_, str>>) -> Vec<Cow<'_, str>> {
    if !rows.iter().any(|row| row.contains('\x1b')) {
        return rows;
    }
    let mut state = SgrState::default();
    rows.into_iter()
        .map(|row| {
            let mut output = state.prefix();
            state.scan(&row);
            output.push_str(&row);
            output.push_str("\x1b[0m");
            Cow::Owned(output)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_indexes_visible_physical_rows_not_sgr_parameters() {
        use super::super::native::{
            LineNumbers,
            screen::{format_line, format_search_rows},
        };
        for width in [20, 40, 80] {
            let original = format!("\x1b[92m{}\x1b[0m", "z".repeat(160));
            for (pattern, expected) in [("92", false), ("^z+$", true), ("NEVER_PRESENT", false)] {
                let query = regex::Regex::new(pattern).unwrap();
                let rows = format_line(&original, 1, 0, LineNumbers::Disabled, width, true);
                for (_, found) in format_search_rows(rows, Some(&query)) {
                    assert_eq!(found, expected, "{pattern} at {width}");
                }
            }
        }
        let digits = regex::Regex::new("92").unwrap();
        let body = regex::Regex::new("^BODY$").unwrap();
        for styled in ["\x1b[38:2::92:0:0mBODY\x1b[0m", "\x1b[92mBODY\x1b[0m"] {
            assert!(!super::super::native::search::line_matches_query(
                styled, &digits
            ));
            assert!(super::super::native::search::line_matches_query(
                styled, &body
            ));
        }
    }
    #[test]
    fn continuation_restores_attributes_colors_and_selective_resets() {
        let rows = independent_rows(vec![
            Cow::Borrowed("\x1b[1;3;4;92;48;2;1;2;3mfirst"),
            Cow::Borrowed("second\x1b[22;39;49m"),
            Cow::Borrowed("third\x1b[0m"),
        ]);
        assert!(rows[1].starts_with("\x1b[0m\x1b[1m\x1b[3m\x1b[4m\x1b[92m\x1b[48;2;1;2;3m"));
        assert_eq!(rows[2], "\x1b[0m\x1b[3m\x1b[4mthird\x1b[0m\x1b[0m");
    }
    #[test]
    fn wrapping_keeps_original_source_and_recomputes_on_every_width() {
        use super::super::native::{LineNumbers, screen::format_line};
        let original = format!("\x1b[92m--{}\x1b[0m", "日本e\u{301}😀x".repeat(30));
        for width in [20, 40, 80, 120, 20] {
            // Negative control: upstream wrapping leaves continuation glyphs
            // without the foreground needed for independent redraw.
            let bare = textwrap::wrap(&original, width);
            assert!(!bare[1].starts_with("\x1b[92m"));
            let rows: Vec<_> = format_line(&original, 1, 0, LineNumbers::Disabled, width, true)
                .map(|r| r.to_string())
                .collect();
            assert!(rows.len() > 1);
            assert!(
                rows.iter()
                    .skip(1)
                    .all(|r| r.starts_with("\x1b[0m\x1b[92m")),
                "{width}: {rows:?}"
            );
            assert_eq!(strip(&rows.join("")), strip(&original));
        }
    }
    #[test]
    fn original_newlines_tabs_and_color_resets_survive_relayout() {
        let mut state = SgrState::default();
        let first = state.logical_line("\x1b[35mA\t日本e\u{301}😀");
        assert_eq!(strip(&first), "A\t日本e\u{301}😀");
        let second = state.logical_line("B\x1b[39m plain");
        assert!(second.starts_with("\x1b[0m\x1b[35mB"));
        assert_eq!(state.logical_line("default"), "default");
        let mut colors = SgrState::default();
        colors.scan("\x1b[38:2::10:20:30;48;5;42;4:3m");
        assert!(colors.prefix().contains("38:2::10:20:30m"));
        colors.scan("\x1b[24;39;49m");
        assert_eq!(colors.prefix(), "\x1b[0m");
    }
    fn strip(text: &str) -> String {
        let mut chars = text.chars();
        let mut output = String::new();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                assert_eq!(chars.next(), Some('['));
                for c in chars.by_ref() {
                    if c == 'm' {
                        break;
                    }
                }
            } else {
                output.push(ch);
            }
        }
        output
    }
}
