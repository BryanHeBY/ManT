//! Deterministic resource limits for untrusted manual sources.

use std::fmt;

/// Every externally influenced parser budget.
///
/// Limits are independent so a document cannot trade an inexpensive dimension
/// for unbounded work elsewhere.  The defaults preserve the observable legacy
/// ceilings where they existed and add explicit core-input and AST budgets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Limits {
    /// Maximum bytes in the top-level uncompressed source.
    pub max_root_source_bytes: usize,
    /// Maximum bytes across the resolved source graph.
    pub max_total_source_bytes: usize,
    /// Maximum sources in one parse session.
    pub max_sources: usize,
    /// Maximum physical source lines retained by the document source map.
    pub max_source_lines: usize,
    /// Maximum include nesting depth.
    pub max_include_depth: usize,
    /// Maximum source-line byte length before expansion.
    pub max_line_bytes: usize,
    /// Maximum expanded output bytes from one physical line.
    pub max_expanded_line_bytes: usize,
    /// Maximum expansion/reparse steps for one line.
    pub max_line_expansion_steps: usize,
    /// Maximum expansion/reparse steps in the parse session.
    pub max_expansion_steps: usize,
    /// Maximum nested macro invocations.
    pub max_macro_depth: usize,
    /// Maximum arguments accepted by one request or macro.
    pub max_arguments: usize,
    /// Maximum aggregate bytes retained for one argument list.
    pub max_argument_bytes: usize,
    /// Maximum loop iterations for one `.while` request.
    pub max_loop_iterations: usize,
    /// Maximum loop iterations aggregated over one parse.
    pub max_total_loop_iterations: usize,
    /// Maximum user-defined strings, registers, and macros combined.
    pub max_definitions: usize,
    /// Maximum bytes retained by those definitions.
    pub max_definition_bytes: usize,
    /// Maximum AST nodes.
    pub max_nodes: usize,
    /// Maximum AST child edges.
    pub max_child_edges: usize,
    /// Maximum stored public text bytes.
    pub max_text_bytes: usize,
    /// Maximum AST nesting depth.
    pub max_tree_depth: usize,
    /// Maximum tbl rows.
    pub max_table_rows: usize,
    /// Maximum tbl columns.
    pub max_table_columns: usize,
    /// Maximum tbl cells.
    pub max_table_cells: usize,
    /// Maximum tbl span value.
    pub max_table_span: usize,
    /// Maximum bytes retained by tbl text blocks.
    pub max_table_text_bytes: usize,
    /// Maximum eqn tokens.
    pub max_equation_tokens: usize,
    /// Maximum eqn nesting depth.
    pub max_equation_depth: usize,
    /// Maximum eqn definitions.
    pub max_equation_definitions: usize,
    /// Maximum eqn expansion steps.
    pub max_equation_expansion_steps: usize,
    /// Maximum primary and related diagnostics retained in a report.
    pub max_diagnostics: usize,
    /// Maximum reference-renderer output bytes when the optional renderer is enabled.
    pub max_render_output_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_root_source_bytes: 16 * 1024 * 1024,
            max_total_source_bytes: 64 * 1024 * 1024,
            max_sources: 4_096,
            max_source_lines: 16 * 1024 * 1024,
            max_include_depth: 64,
            max_line_bytes: 1024 * 1024,
            max_expanded_line_bytes: 1024 * 1024,
            max_line_expansion_steps: 1_000,
            max_expansion_steps: 100_000,
            max_macro_depth: 64,
            max_arguments: 1_024,
            max_argument_bytes: 1024 * 1024,
            max_loop_iterations: 10_000,
            max_total_loop_iterations: 10_000,
            max_definitions: 4_096,
            max_definition_bytes: 16 * 1024 * 1024,
            max_nodes: 1_000_000,
            max_child_edges: 2_000_000,
            max_text_bytes: 64 * 1024 * 1024,
            max_tree_depth: 256,
            max_table_rows: 16_384,
            max_table_columns: 1_024,
            max_table_cells: 1_000_000,
            max_table_span: 1_024,
            max_table_text_bytes: 16 * 1024 * 1024,
            max_equation_tokens: 1_000_000,
            max_equation_depth: 256,
            max_equation_definitions: 4_096,
            max_equation_expansion_steps: 100_000,
            max_diagnostics: 16_384,
            max_render_output_bytes: 64 * 1024 * 1024,
        }
    }
}

impl Limits {
    /// Validate relationships that must hold before parsing starts.
    ///
    /// # Errors
    ///
    /// Returns the first invalid field or cross-field relationship.
    pub fn validate(&self) -> Result<(), LimitViolation> {
        for (field, value) in [
            ("max_root_source_bytes", self.max_root_source_bytes),
            ("max_total_source_bytes", self.max_total_source_bytes),
            ("max_sources", self.max_sources),
            ("max_source_lines", self.max_source_lines),
            ("max_include_depth", self.max_include_depth),
            ("max_line_bytes", self.max_line_bytes),
            ("max_expanded_line_bytes", self.max_expanded_line_bytes),
            ("max_line_expansion_steps", self.max_line_expansion_steps),
            ("max_expansion_steps", self.max_expansion_steps),
            ("max_macro_depth", self.max_macro_depth),
            ("max_arguments", self.max_arguments),
            ("max_argument_bytes", self.max_argument_bytes),
            ("max_loop_iterations", self.max_loop_iterations),
            ("max_total_loop_iterations", self.max_total_loop_iterations),
            ("max_definitions", self.max_definitions),
            ("max_definition_bytes", self.max_definition_bytes),
            ("max_nodes", self.max_nodes),
            ("max_child_edges", self.max_child_edges),
            ("max_text_bytes", self.max_text_bytes),
            ("max_tree_depth", self.max_tree_depth),
            ("max_table_rows", self.max_table_rows),
            ("max_table_columns", self.max_table_columns),
            ("max_table_cells", self.max_table_cells),
            ("max_table_span", self.max_table_span),
            ("max_table_text_bytes", self.max_table_text_bytes),
            ("max_equation_tokens", self.max_equation_tokens),
            ("max_equation_depth", self.max_equation_depth),
            ("max_equation_definitions", self.max_equation_definitions),
            (
                "max_equation_expansion_steps",
                self.max_equation_expansion_steps,
            ),
            ("max_diagnostics", self.max_diagnostics),
            ("max_render_output_bytes", self.max_render_output_bytes),
        ] {
            if value == 0 {
                return Err(LimitViolation::Zero { field });
            }
        }
        if self.max_root_source_bytes > self.max_total_source_bytes {
            return Err(LimitViolation::Relationship {
                smaller: "max_root_source_bytes",
                larger: "max_total_source_bytes",
            });
        }
        if self.max_loop_iterations > self.max_total_loop_iterations {
            return Err(LimitViolation::Relationship {
                smaller: "max_loop_iterations",
                larger: "max_total_loop_iterations",
            });
        }
        Ok(())
    }
}

/// Invalid resource-limit configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitViolation {
    /// A budget that must be positive was zero.
    Zero {
        /// Name of the invalid public field.
        field: &'static str,
    },
    /// One limit exceeded another limit that bounds it.
    Relationship {
        /// Field which must be no greater than `larger`.
        smaller: &'static str,
        /// Field which bounds `smaller`.
        larger: &'static str,
    },
}

impl fmt::Display for LimitViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero { field } => write!(formatter, "{field} must be greater than zero"),
            Self::Relationship { smaller, larger } => {
                write!(formatter, "{smaller} must not exceed {larger}")
            }
        }
    }
}

impl std::error::Error for LimitViolation {}

#[cfg(test)]
mod tests {
    use super::{LimitViolation, Limits};

    #[test]
    fn defaults_preserve_legacy_resource_boundaries() {
        let limits = Limits::default();
        assert_eq!(limits.max_root_source_bytes, 16 * 1024 * 1024);
        assert_eq!(limits.max_total_source_bytes, 64 * 1024 * 1024);
        assert_eq!(limits.max_sources, 4_096);
        assert_eq!(limits.max_source_lines, 16 * 1024 * 1024);
        assert_eq!(limits.max_tree_depth, 256);
        assert_eq!(limits.max_equation_depth, 256);
        assert_eq!(limits.max_loop_iterations, 10_000);
        limits.validate().expect("defaults must be valid");
    }

    #[test]
    fn invalid_relationships_are_rejected_before_work_starts() {
        let mut limits = Limits::default();
        limits.max_root_source_bytes = limits.max_total_source_bytes + 1;
        assert_eq!(
            limits.validate(),
            Err(LimitViolation::Relationship {
                smaller: "max_root_source_bytes",
                larger: "max_total_source_bytes",
            })
        );
    }
}
