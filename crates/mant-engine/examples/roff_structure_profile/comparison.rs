//! Conservative structural obligations and topology comparison.
use super::{
    AstEquationTopology, AstListTopology, AstStructure, AstTableRowTopology, AstTopology, BTreeMap,
    EquationContext, IrEquationTopology, IrListTopology, IrStructure, IrTableRowTopology,
    IrTopology,
};

pub(super) fn compare_structure(
    expected: &AstStructure,
    observed: &IrStructure,
    expected_topology: &AstTopology,
    observed_topology: &IrTopology,
) -> Vec<String> {
    let mut violations = Vec::new();
    underflow(
        &mut violations,
        "no-fill-lines",
        expected.no_fill_lines,
        observed.preformatted_lines,
    );
    exact(
        &mut violations,
        "display-equations",
        expected.display_equations,
        observed.display_equations,
    );
    underflow(
        &mut violations,
        "literal-displays",
        expected.literal_displays,
        observed.preformatted_lines,
    );
    underflow(
        &mut violations,
        "generic-list-items",
        expected.generic_list_items,
        observed.generic_list_items,
    );
    underflow(
        &mut violations,
        "table-rows",
        expected.table_rows,
        observed.table_rows,
    );
    underflow(
        &mut violations,
        "table-spanning-cells",
        expected.table_spanning_cells,
        observed.table_spanning_cells,
    );
    underflow(
        &mut violations,
        "manual-links",
        expected.manual_links,
        observed.manual_links,
    );
    underflow(
        &mut violations,
        "external-links",
        expected.external_links,
        observed.external_links,
    );
    underflow(
        &mut violations,
        "email-links",
        expected.email_links,
        observed.email_links,
    );
    underflow(
        &mut violations,
        "section-links-or-diagnostics",
        expected.section_links,
        observed
            .section_links
            .saturating_add(observed.unresolved_section_references),
    );
    if expected.positive_relative_indent_scopes > 0 && observed.max_indent_columns == 0 {
        violations.push(format!(
            "relative-indent: expected positive indentation from {} RS scopes (nested depth {}), observed no indented IR block",
            expected.positive_relative_indent_scopes, expected.max_relative_indent_depth
        ));
    }
    compare_list_topology(
        &mut violations,
        &expected_topology.lists,
        &observed_topology.lists,
    );
    compare_table_topology(
        &mut violations,
        &expected_topology.table_rows,
        &observed_topology.table_rows,
    );
    compare_equation_topology(
        &mut violations,
        &expected_topology.equations,
        &observed_topology.equations,
    );
    violations
}

fn compare_equation_topology(
    violations: &mut Vec<String>,
    expected: &[AstEquationTopology],
    observed: &[IrEquationTopology],
) {
    let mut used = vec![false; observed.len()];
    for equation in expected {
        let candidate = observed.iter().enumerate().position(|(index, candidate)| {
            !used[index]
                && candidate.context == equation.context
                && candidate.value == equation.value
                && (equation.context != EquationContext::Display
                    || candidate.source_line == equation.source_line)
        });
        if let Some(index) = candidate {
            used[index] = true;
            continue;
        }
        violations.push(format!(
            "{}-equation at line {}: expected normalized value {:?}, observed no matching IR value",
            equation.context.as_str(),
            equation.source_line,
            equation.value,
        ));
    }
}

fn exact(violations: &mut Vec<String>, label: &str, expected: usize, observed: usize) {
    if expected != observed {
        violations.push(format!("{label}: expected {expected}, observed {observed}"));
    }
}

fn compare_list_topology(
    violations: &mut Vec<String>,
    expected: &[AstListTopology],
    observed: &[IrListTopology],
) {
    for expected_list in expected {
        let observed_list = observed.iter().find(|candidate| {
            candidate.source_line == expected_list.source_line
                && candidate.kind == expected_list.kind
        });
        match observed_list {
            Some(observed_list) if observed_list.items == expected_list.items => {}
            Some(observed_list) => violations.push(format!(
                "list-topology at line {}: expected {} {} items, observed {}",
                expected_list.source_line,
                expected_list.items,
                expected_list.kind.as_str(),
                observed_list.items,
            )),
            None => violations.push(format!(
                "list-topology at line {}: expected {} list with {} items, observed none",
                expected_list.source_line,
                expected_list.kind.as_str(),
                expected_list.items,
            )),
        }
    }
}

/// CVS `tbl_data.c` retains whole-rule and empty spans in the same table chain.
/// Compare that complete chain within its actual first-span source identity;
/// dropping rules before a global zip shifts every subsequent cell obligation.
pub(super) fn compare_table_topology(
    violations: &mut Vec<String>,
    expected: &[AstTableRowTopology],
    observed: &[IrTableRowTopology],
) {
    let mut native = BTreeMap::<_, Vec<_>>::new();
    let mut lowered = BTreeMap::<_, Vec<_>>::new();
    for row in expected {
        native
            .entry((row.table_source_line, row.table_source_column))
            .or_default()
            .push(row);
    }
    for row in observed {
        lowered
            .entry((row.table_source_line, row.table_source_column))
            .or_default()
            .push(row);
    }
    for (origin, native_rows) in native {
        if origin.0 == 0 || origin.1 == 0 {
            violations.push("table-topology: unknown native table source identity".to_owned());
            lowered.remove(&origin);
            continue;
        }
        let Some(lowered_rows) = lowered.remove(&origin) else {
            violations.push(format!(
                "table-topology at {}:{}: expected table, observed none",
                origin.0, origin.1
            ));
            continue;
        };
        if native_rows.iter().skip(1).any(|row| row.row_index == 0)
            || lowered_rows.iter().skip(1).any(|row| row.row_index == 0)
        {
            violations.push(format!(
                "table-topology at {}:{}: ambiguous repeated table source identity",
                origin.0, origin.1
            ));
            continue;
        }
        if native_rows.len() != lowered_rows.len() {
            violations.push(format!(
                "table-topology at {}:{}: expected {} rows, observed {}",
                origin.0,
                origin.1,
                native_rows.len(),
                lowered_rows.len()
            ));
        }
        for (expected_row, observed_row) in native_rows.iter().zip(lowered_rows) {
            compare_table_row(violations, expected_row, observed_row);
        }
    }
    for (origin, rows) in lowered {
        violations.push(format!(
            "table-topology at {}:{}: unexpected table with {} rows",
            origin.0,
            origin.1,
            rows.len()
        ));
    }
}

fn compare_table_row(
    violations: &mut Vec<String>,
    expected_row: &AstTableRowTopology,
    observed_row: &IrTableRowTopology,
) {
    if expected_row.kind != observed_row.kind {
        violations.push(format!(
            "table-topology at {}:{}, row {}: expected kind {:?}, observed {:?}",
            expected_row.table_source_line,
            expected_row.table_source_column,
            expected_row.row_index + 1,
            expected_row.kind,
            observed_row.kind
        ));
    }
    if observed_row.cells.len() != expected_row.cells.len() {
        violations.push(format!(
            "table-topology at {}:{}, row {}: expected {} cells, observed {}",
            expected_row.table_source_line,
            expected_row.table_source_column,
            expected_row.row_index + 1,
            expected_row.cells.len(),
            observed_row.cells.len()
        ));
    }
    for (cell_index, (expected_cell, observed_cell)) in expected_row
        .cells
        .iter()
        .zip(&observed_row.cells)
        .enumerate()
    {
        if expected_cell.column_span != observed_cell.column_span
            || expected_cell.row_span != observed_cell.row_span
        {
            violations.push(format!(
                "table-topology at row {}, cell {}: expected span {}x{}, observed {}x{}",
                expected_row.row_index + 1,
                cell_index + 1,
                expected_cell.column_span,
                expected_cell.row_span,
                observed_cell.column_span,
                observed_cell.row_span,
            ));
        }
        if expected_cell.vertical_continuation && !observed_cell.empty {
            violations.push(format!(
                "table-topology at row {}, cell {}: vertical continuation retained visible content",
                expected_row.row_index + 1,
                cell_index + 1,
            ));
        }
    }
}

pub(super) fn underflow(
    violations: &mut Vec<String>,
    property: &str,
    expected: usize,
    observed: usize,
) {
    if expected > observed {
        violations.push(format!(
            "{property}: expected at least {expected}, observed {observed}"
        ));
    }
}
