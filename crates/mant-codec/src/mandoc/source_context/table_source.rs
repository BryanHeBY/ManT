//! Optional source evidence for native tbl cells.

use super::LoweringContext;

#[derive(Debug)]
pub(in crate::mandoc) struct TableTextBlock {
    pub(in crate::mandoc) source: String,
    pub(in crate::mandoc) escape: Option<u8>,
}

/// Apply the lexical comment rule that roff executes before handing text to
/// tbl.
///
/// This intentionally is not a second roff interpreter.  Source-backed tbl
/// recovery only needs the one lexical transformation that both CVS mandoc
/// and groff perform before parsing a table cell: `\\\"` discards the rest
/// of the physical line and `\\#` discards it while continuing with the next
/// line.  Keeping it at the source-recovery boundary prevents comment prose
/// from becoming a synthetic table cell when libmandoc has correctly omitted
/// that cell from its owned table snapshot.
fn table_execution_source(source: &str, initial_escape: Option<u8>) -> String {
    let mut output = String::with_capacity(source.len());
    let escape = initial_escape;
    for physical_line in source.split_inclusive('\n') {
        let (line, had_newline) = physical_line.strip_suffix("\r\n").map_or_else(
            || {
                physical_line
                    .strip_suffix('\n')
                    .map_or((physical_line, false), |line| (line, true))
            },
            |line| (line, true),
        );
        let (visible, continues) = roff_line_without_comment(line, escape);
        output.push_str(visible);
        if had_newline && !continues {
            output.push('\n');
        }
    }
    output
}

/// Return the printable prefix of one roff input line and whether `\\#`
/// suppresses its trailing newline.
///
/// CVS mandoc's `roff_parse_comment()` scans a physical line before
/// `tbl_read()`: an escaped escape skips both bytes, while `\\\"` and `\\#`
/// start a comment.  This mirrors only that lexical contract; callers supply
/// the already-executed `.ec`/`.eo` state for their source coordinate.
fn roff_line_without_comment(line: &str, escape: Option<u8>) -> (&str, bool) {
    let Some(escape) = escape else {
        return (line, false);
    };
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != escape {
            index += 1;
            continue;
        }
        match bytes.get(index.saturating_add(1)).copied() {
            Some(next) if next == escape => index = index.saturating_add(2),
            Some(b'"' | b'#') => {
                let mut end = index;
                // Match mandoc's deliberate whitespace rule: remove only
                // literal spaces immediately preceding a comment, unless
                // that space was itself escaped.
                while end > 0 && bytes[end - 1] == b' ' && (end == 1 || bytes[end - 2] != escape) {
                    end -= 1;
                }
                return (&line[..end], bytes[index + 1] == b'#');
            }
            _ => index += 1,
        }
    }
    (line, false)
}

impl LoweringContext<'_> {
    pub(in crate::mandoc) fn table_execution_source(source: &str, escape: Option<u8>) -> String {
        table_execution_source(source, escape)
    }

    /// Query libmandoc's pinned roff registry once per distinct table request.
    ///
    /// `tbl_read()` receives raw operands only for unknown or high-level
    /// man/mdoc macros. Native roff requests have already been handled by the
    /// parser and must not leak their operands into recovered cell text.
    pub(in crate::mandoc) fn is_native_table_request(&self, name: &str) -> bool {
        if let Some(native) = self.native_table_requests.borrow().get(name) {
            return *native;
        }
        let native = libmandoc_rs::is_native_roff_request(name);
        self.native_table_requests
            .borrow_mut()
            .insert(name.to_owned(), native);
        native
    }

    pub(in crate::mandoc) fn table_text_blocks(
        &self,
        line: u32,
        eligible_cells: impl Iterator<Item = bool>,
        escape: Option<u8>,
    ) -> Vec<Option<TableTextBlock>> {
        let mut eligible_cells = eligible_cells.peekable();
        // Ordinary tbl rows must not scan forward for `T{` markers.  Besides
        // wasting work, that used to let a commented-out multiline-cell
        // marker claim later real rows as its embedded semantic children.
        if eligible_cells.peek().is_none() {
            return Vec::new();
        }
        let Some(source_lines) = self.source_lines.as_ref() else {
            return Vec::new();
        };
        let mut blocks = Vec::new();
        // Outer None means no active T{}; inner None preserves the ordinal
        // of an unsafe or locally oversized cell without copying its source.
        let mut current = None::<Option<String>>;
        for (_, line) in source_lines.lines_from(line) {
            // Charge even an empty physical row before lexical scanning.
            // Only complete blocks survive an allowance refusal; native
            // finalized cells never depend on this optional source scan.
            if !self
                .table_recovery_budget
                .borrow_mut()
                .charge_scan(line.len().saturating_add(1))
            {
                break;
            }
            // Interpret tbl's `T{` / `T}` sentinels after roff has removed
            // inline comments.  A comment can follow a real sentinel, while
            // a comment-only request can mention a disabled sentinel without
            // claiming a later text block.
            let (visible_line, _) = roff_line_without_comment(line, escape);
            let trimmed = visible_line.trim_start();
            if let Some(content) = current.as_mut() {
                if let Some(remainder) = trimmed.strip_prefix("T}") {
                    blocks.push(
                        content
                            .take()
                            .map(|source| TableTextBlock { source, escape }),
                    );
                    current = None;
                    if eligible_cells.peek().is_none() {
                        break;
                    }
                    // tbl serializes adjacent multiline cells as `T}\tT{`.
                    // Closing the first cell must not hide the next opening
                    // marker carried by the same physical source line.
                    if remainder.trim_end().ends_with("T{") {
                        current = eligible_cells
                            .next()
                            .map(|eligible| eligible.then(String::new));
                    }
                } else if let Some(source) = content {
                    let copied = line.len().saturating_add(usize::from(!source.is_empty()));
                    if source.len().saturating_add(copied)
                        > crate::mandoc::table_recovery_budget::MAX_FRAGMENT_BYTES
                    {
                        self.table_recovery_budget.borrow_mut().reject_fragment();
                        *content = None;
                        continue;
                    }
                    if !self.table_recovery_budget.borrow_mut().charge_input(copied) {
                        break;
                    }
                    if !source.is_empty() {
                        source.push('\n');
                    }
                    source.push_str(line);
                }
            } else if trimmed.trim_end().ends_with("T{") {
                // Preserve the native text-block ordinal for unsafe cells,
                // but never copy their source into an enhancement candidate.
                current = eligible_cells
                    .next()
                    .map(|eligible| eligible.then(String::new));
            }
        }
        blocks
    }
}

#[cfg(test)]
mod tests {
    use super::{LoweringContext, table_execution_source};

    #[test]
    fn table_execution_source_matches_roff_inline_comment_and_continuation_rules() {
        assert_eq!(
            table_execution_source("visible \\\" ignored\nnext", Some(b'\\')),
            "visible\nnext"
        );
        assert_eq!(
            table_execution_source("joined \\# ignored\nnext", Some(b'\\')),
            "joinednext"
        );
        assert_eq!(
            table_execution_source(r#"literal \\" remains"#, Some(b'\\')),
            r#"literal \\" remains"#
        );
        assert_eq!(
            table_execution_source(r#"escaped\ \" comment"#, Some(b'\\')),
            r"escaped\ "
        );
    }
    #[test]
    fn table_execution_source_uses_the_native_escape_state_at_the_cell() {
        assert_eq!(
            LoweringContext::table_execution_source("visible @\" ignored", Some(b'@')),
            "visible"
        );
        assert_eq!(
            LoweringContext::table_execution_source("visible \\\" literal", Some(b'@')),
            "visible \\\" literal"
        );
    }
}
