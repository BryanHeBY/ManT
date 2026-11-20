//! Compare exact equation occurrences within their proven source owners.

use std::collections::{HashMap, VecDeque};

use super::super::{AstEquationTopology, EquationContext, IrEquationTopology};

pub(super) fn compare_equation_topology(
    violations: &mut Vec<String>,
    expected: &[AstEquationTopology],
    observed: &[IrEquationTopology],
) {
    fn signature(
        context: EquationContext,
        value: &str,
        line: u32,
    ) -> (EquationContext, &str, Option<u32>) {
        (
            context,
            value,
            (context == EquationContext::Display).then_some(line),
        )
    }
    let mut known = HashMap::<_, VecDeque<_>>::new();
    for (index, equation) in observed.iter().enumerate() {
        let key = signature(equation.context, &equation.value, equation.source_line);
        for paragraph in [None, Some(equation.source_line)] {
            known
                .entry((equation.cell, equation.section_line, paragraph, key))
                .or_default()
                .push_back(index);
        }
    }
    let mut used = vec![false; observed.len()];
    let mut matched = vec![None; expected.len()];
    // Reserve exact owner occurrences before comparing source equations
    // whose AST proves they belong to a table without an exact address.
    let known_equations = |exact| {
        expected
            .iter()
            .enumerate()
            .filter(move |(_, eq)| !eq.unproved_cell && eq.paragraph_line.is_some() == exact)
    };
    let uncertain_equations = expected
        .iter()
        .enumerate()
        .filter(|(_, eq)| eq.unproved_cell);
    let mut uncertain = None;
    for (expected_index, equation) in known_equations(true)
        .chain(known_equations(false))
        .chain(uncertain_equations)
    {
        let key = signature(
            equation.context,
            equation.value.as_str(),
            equation.source_line,
        );
        let candidate = if equation.unproved_cell {
            let unmatched = uncertain.get_or_insert_with(|| {
                let mut unmatched = HashMap::<_, VecDeque<_>>::new();
                for (index, candidate) in observed.iter().enumerate() {
                    if !used[index] && candidate.cell.is_some() {
                        unmatched
                            .entry((
                                candidate.section_line,
                                signature(
                                    candidate.context,
                                    candidate.value.as_str(),
                                    candidate.source_line,
                                ),
                            ))
                            .or_default()
                            .push_back(index);
                    }
                }
                unmatched
            });
            unmatched
                .get_mut(&(equation.section_line, key))
                .and_then(VecDeque::pop_front)
        } else {
            known
                .get_mut(&(
                    equation.cell,
                    equation.section_line,
                    equation.paragraph_line,
                    key,
                ))
                .and_then(|indices| {
                    std::iter::from_fn(|| indices.pop_front()).find(|&index| !used[index])
                })
        };
        if let Some(index) = candidate {
            used[index] = true;
            matched[expected_index] = Some(index);
        } else {
            report_missing_equation(violations, equation);
        }
    }
    check_equation_order(violations, expected, matched);
    for (index, equation) in observed.iter().enumerate() {
        if !used[index] {
            violations.push(format!(
                "{}-equation at line {}: unexpected occurrence of {:?} in {:?}",
                equation.context.as_str(),
                equation.source_line,
                equation.value,
                equation.cell,
            ));
        }
    }
}

fn check_equation_order(
    violations: &mut Vec<String>,
    expected: &[AstEquationTopology],
    matched: Vec<Option<usize>>,
) {
    let mut last = HashMap::new();
    for (equation, index) in expected.iter().zip(matched) {
        if let Some(index) = index
            && !equation.unproved_cell
        {
            let previous = last
                .entry((equation.cell, equation.section_line))
                .or_insert(index);
            if index < *previous {
                violations.push(format!(
                    "{}-equation at line {}: {:?} is out of order in its owner",
                    equation.context.as_str(),
                    equation.source_line,
                    equation.value,
                ));
            }
            *previous = (*previous).max(index);
        }
    }
}

fn report_missing_equation(violations: &mut Vec<String>, equation: &AstEquationTopology) {
    let owner = equation.cell.map_or_else(
        || {
            equation.paragraph_line.map_or_else(
                || format!(" in section at line {}", equation.section_line),
                |line| format!(" in paragraph at line {line}"),
            )
        },
        |cell| {
            format!(
                " in table {}:{}, occurrence {}, row {}, cell {}",
                cell.table_source_line,
                cell.table_source_column,
                cell.table_occurrence,
                cell.row_index + 1,
                cell.cell_index + 1,
            )
        },
    );
    violations.push(format!(
        "{}-equation at line {}: expected normalized value {:?}{owner}, observed no matching IR value",
        equation.context.as_str(),
        equation.source_line,
        equation.value,
    ));
}
