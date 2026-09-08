//! Keep resolved gaps separate from literal line breaks until the entire
//! content flow is assembled. Transparent containers must not create a new
//! gap budget, and literal blank lines must not be mistaken for requests.
use mant_protocol::geometry::GapPlan;

#[derive(Default)]
pub(super) struct Flow {
    parts: Vec<Part>,
}

enum Part {
    Text(String),
    Gap(u16),
}

impl Flow {
    pub(super) fn text(value: String) -> Self {
        let mut result = Self::default();
        result.push_text(value);
        result
    }

    pub(super) fn push_text(&mut self, value: String) {
        if !value.is_empty() {
            self.parts.push(Part::Text(value));
        }
    }

    pub(super) fn gap(&mut self, rows: u16) {
        self.parts.push(Part::Gap(rows));
    }

    pub(super) fn extend(&mut self, other: Self) {
        self.parts.extend(other.parts);
    }

    pub(super) fn finish(self, preceding_content: bool) -> String {
        let mut output = String::new();
        let mut gap = GapPlan::default();
        let mut has_content = preceding_content;
        for part in self.parts {
            match part {
                Part::Gap(rows) => gap.append_resolved(rows),
                Part::Text(text) => {
                    if has_content {
                        output.push('\n');
                    }
                    output.push_str(&"\n".repeat(usize::from(gap.rows(0))));
                    output.push_str(&text);
                    has_content = true;
                    gap = GapPlan::default();
                }
            }
        }
        output.push_str(&"\n".repeat(usize::from(gap.rows(0))));
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_children_share_a_budget_but_literal_rows_do_not() {
        let mut parent = Flow::text("BEFORE".into());
        parent.gap(3000);
        let mut child = Flow::default();
        child.gap(3000);
        child.push_text("AFTER\n\nLITERAL".into());
        parent.extend(child);
        assert_eq!(
            parent.finish(false),
            format!("BEFORE{}AFTER\n\nLITERAL", "\n".repeat(4097))
        );
    }
}
