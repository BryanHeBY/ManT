//! Bounded native equation normalization and source delimiter state.
use super::{
    LoweringContext, MAX_INLINE_EQUATION_NORMALIZATIONS, Node, Parser, Path, visible_text,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct EquationDelimiterChange {
    line: u32,
    delimiters: Option<(char, char)>,
}

#[derive(Clone, Copy, Debug)]
enum EquationDelimiterDirective {
    Enable(char, char),
    Disable,
}

impl EquationDelimiterDirective {
    const fn delimiters(self) -> Option<(char, char)> {
        match self {
            Self::Enable(opening, closing) => Some((opening, closing)),
            Self::Disable => None,
        }
    }
}

fn first_equation(node: &Node) -> Option<&str> {
    node.equation
        .as_deref()
        .or_else(|| node.children.iter().find_map(first_equation))
}

/// Track active inline eqn delimiters at each source line.
///
/// eqn configures delimiters inside an `.EQ`/`.EN` block; they take effect on
/// following prose and tbl cells. Keeping the change points makes lookup
/// logarithm-free and deterministic without replaying the whole source for
/// every table cell.
pub(super) fn equation_delimiter_changes(source: &str) -> Vec<EquationDelimiterChange> {
    let mut changes = Vec::new();
    let mut in_equation = false;
    let mut pending = None;
    for (index, source_line) in source.lines().enumerate() {
        let line = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let trimmed = source_line.trim();
        if trimmed.starts_with(".\\\"") || trimmed.starts_with("'\\\"") {
            continue;
        }
        if let Some(rest) = trimmed
            .strip_prefix(".EQ")
            .or_else(|| trimmed.strip_prefix("'EQ"))
            .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
        {
            in_equation = true;
            pending = parse_equation_delimiters(rest.trim()).or(pending);
            continue;
        }
        if in_equation {
            if trimmed == ".EN" || trimmed == "'EN" {
                if let Some(delimiters) = pending.take() {
                    changes.push(EquationDelimiterChange {
                        line: line.saturating_add(1),
                        delimiters: delimiters.delimiters(),
                    });
                }
                in_equation = false;
            } else if let Some(delimiters) = parse_equation_delimiters(trimmed) {
                pending = Some(delimiters);
            }
        }
    }
    changes
}

fn parse_equation_delimiters(value: &str) -> Option<EquationDelimiterDirective> {
    let value = value.strip_prefix("delim")?.trim_start();
    if value == "off" {
        return Some(EquationDelimiterDirective::Disable);
    }
    let mut delimiters = value.chars();
    let opening = delimiters.next()?;
    let closing = delimiters.next()?;
    Some(EquationDelimiterDirective::Enable(opening, closing))
}

impl LoweringContext<'_> {
    pub(super) fn equation_delimiters_at(&self, line: u32) -> Option<(char, char)> {
        self.equation_delimiters
            .iter()
            .rev()
            .find(|change| change.line <= line)
            .and_then(|change| change.delimiters)
    }
    /// Normalize an eqn fragment through the same pinned parser used for
    /// display equations. tbl retains delimiter-wrapped cell text as an
    /// opaque string, so reparsing only that bounded fragment is the sole way
    /// to avoid a second, incomplete eqn grammar in the lowering layer.
    pub(super) fn normalize_equation(&self, source: &str, line: u32) -> String {
        {
            let normalized = self.normalized_equations.borrow();
            if let Some(value) = normalized.get(source) {
                return value.clone();
            }
            if normalized.len() >= MAX_INLINE_EQUATION_NORMALIZATIONS {
                drop(normalized);
                self.warn_inline_equation_budget(line);
                return visible_text(source);
            }
        }
        let synthetic = format!(".TH MANT-EQN 7\n.EQ\n{source}\n.EN\n");
        let normalized = Parser::default()
            .parse_bytes(Path::new("mant-inline-eqn.7"), synthetic.as_bytes())
            .ok()
            .and_then(|report| first_equation(&report.document.root).map(visible_text))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| visible_text(source));
        self.normalized_equations
            .borrow_mut()
            .insert(source.to_owned(), normalized.clone());
        normalized
    }
}
