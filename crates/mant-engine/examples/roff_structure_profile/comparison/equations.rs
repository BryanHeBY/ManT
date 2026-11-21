//! Compare exact equation occurrences within their proven source owners.

use std::collections::{HashMap, VecDeque};

use super::super::{
    AstEquationTopology, EquationContext, EquationOwner, FlowOccurrence, IrEquationTopology,
};

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
        let any_literal = matches!(equation.owner, Some(EquationOwner::Literal(Some(_))))
            .then_some(Some(EquationOwner::Literal(None)));
        for owner in std::iter::once(None)
            .chain(equation.owner.map(Some))
            .chain(any_literal)
        {
            known
                .entry((equation.cell, equation.section, owner, key))
                .or_default()
                .push_back(index);
        }
    }
    let mut used = vec![false; observed.len()];
    let mut matched = vec![None; expected.len()];
    // Weaker proofs must not consume an occurrence with an exact owner.
    let ordered = [0, 1, 2, 3].into_iter().flat_map(|priority| {
        expected.iter().enumerate().filter(move |(_, eq)| {
            let rank = if eq.unproved_cell {
                3
            } else if eq.cell.is_some() {
                0
            } else {
                match eq.owner {
                    Some(EquationOwner::Paragraph(_) | EquationOwner::Literal(Some(_))) => 0,
                    Some(EquationOwner::Literal(None)) => 1,
                    None => 2,
                }
            };
            rank == priority
        })
    });
    let mut uncertain = None;
    for (expected_index, equation) in ordered {
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
                                candidate.section,
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
                .get_mut(&(equation.section, key))
                .and_then(VecDeque::pop_front)
        } else {
            known
                .get_mut(&(equation.cell, equation.section, equation.owner, key))
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
                .entry((equation.cell, equation.section))
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
        || match equation.owner {
            Some(EquationOwner::Paragraph(address)) => describe_flow_owner("paragraph", address),
            Some(EquationOwner::Literal(Some(address))) => {
                describe_flow_owner("literal owner", address)
            }
            Some(EquationOwner::Literal(None)) => format!(
                " in literal owner within section at line {}, occurrence {}",
                equation.section.line, equation.section.occurrence
            ),
            None => format!(
                " in section at line {}, occurrence {}",
                equation.section.line, equation.section.occurrence
            ),
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

fn describe_flow_owner(kind: &str, address: FlowOccurrence) -> String {
    use std::fmt::Write;
    let mut owner = format!(
        " in {kind} at line {}, occurrence {}",
        address.source.line, address.source.occurrence
    );
    if let Some(definition) = address.definition {
        let _ = write!(
            owner,
            " in definition description at {}:{}, occurrence {}",
            definition.line, definition.column, definition.occurrence
        );
    }
    owner
}
