//! Source formatter positions, IR parent origins and man macro scope restoration.
use libmandoc_rs::Node;

use super::{Distance, first_part_argument};

/// Independent source formatter position, IR parent origin and man macro base.
/// Entering an owned description changes the IR parent, not the macro base;
/// only an RS scope establishes a new base for argument-less `in` restoration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::mandoc) struct SourceIndent {
    source: Distance,
    parent: Distance,
    // man_term's mt->offset is not the current output column or IR owner.
    // Entering a definition body must not change where argument-less in
    // restores; only an RS scope establishes a new macro base.
    macro_base: Distance,
}

impl SourceIndent {
    pub(in crate::mandoc) fn absolute(self, distance: Distance) -> Self {
        Self {
            source: distance.add(Distance::cells(-5)).0.at_page_floor(),
            parent: self.parent,
            macro_base: self.macro_base,
        }
    }
    pub(in crate::mandoc) fn relative_columns(self) -> i32 {
        self.source
            .position_columns()
            .saturating_sub(self.parent.position_columns())
    }

    pub(in crate::mandoc) fn content_origin(self) -> Self {
        Self {
            source: self.source,
            parent: self.source,
            macro_base: self.macro_base,
        }
    }

    pub(in crate::mandoc) fn macro_origin(self) -> Self {
        Self {
            source: self.macro_base,
            ..self
        }
    }

    pub(in crate::mandoc) fn offset_from(self, other: Self) -> i32 {
        self.source
            .position_columns()
            .saturating_sub(other.source.position_columns())
    }
}

#[cfg(test)]
impl From<i32> for SourceIndent {
    fn from(columns: i32) -> Self {
        Self {
            source: Distance::cells(columns),
            parent: Distance::default(),
            macro_base: Distance::cells(columns),
        }
    }
}

impl crate::mandoc::LoweringContext<'_> {
    pub(in crate::mandoc) fn offset_indent(
        &self,
        node: &Node,
        parent: SourceIndent,
        extra: Distance,
    ) -> SourceIndent {
        let (sum, bounded) = parent.source.add(extra);
        if bounded {
            self.warn_indent(node);
        }
        SourceIndent {
            source: sum.at_page_floor(),
            parent: parent.parent,
            macro_base: parent.macro_base,
        }
    }

    pub(in crate::mandoc) fn distance_or(
        &self,
        node: &Node,
        argument: &str,
        fallback: Distance,
    ) -> Distance {
        self.checked_distance(node, argument).unwrap_or(fallback)
    }

    pub(in crate::mandoc) fn checked_distance(
        &self,
        node: &Node,
        argument: &str,
    ) -> Option<Distance> {
        let distance = Distance::parse(argument);
        if distance.is_none() {
            self.warn_indent(node);
        }
        distance
    }

    pub(in crate::mandoc) fn man_relative_indent(
        &self,
        node: &Node,
        parent: SourceIndent,
        prevailing: Distance,
    ) -> SourceIndent {
        let distance = first_part_argument(node).map_or(prevailing, |argument| {
            self.distance_or(node, argument, prevailing)
        });
        let mut scope = self.offset_indent(node, parent.macro_origin(), distance);
        scope.macro_base = scope.source;
        scope
    }

    fn warn_indent(&self, node: &Node) {
        let mut diagnostics = self.diagnostics.borrow_mut();
        if diagnostics
            .iter()
            .any(|item| item.code.as_deref() == Some("manual.indentation-limit"))
        {
            return;
        }
        diagnostics.push(mant_ir::Diagnostic {
            level: mant_ir::DiagnosticLevel::Warning,
            code: Some("manual.indentation-limit".into()),
            message: "unsupported or excessive indentation was bounded; invalid offsets use the default and cumulative indentation is limited to 4096 columns".into(),
            source: crate::mandoc::source_span(node),
        });
    }
}
