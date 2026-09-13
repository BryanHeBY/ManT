//! First private consumer of the owned native execution report.
//!
//! This is deliberately not the production switch. It establishes the
//! execution-to-projection boundary used by the staged migration without
//! reconstructing formatter state in Rust.

use libmandoc_rs::{
    AtomRole, BoundaryEffect, ExecutionFont, ExecutionNodeKey, FragmentRole, GeometryKind,
    GeometryOriginKind, NativeExecutionReport,
};
use std::{collections::BTreeMap, path::PathBuf};

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeTextRun {
    pub(super) node: ExecutionNodeKey,
    pub(super) source: PathBuf,
    pub(super) font: ExecutionFont,
    pub(super) text: String,
    pub(super) device_line: u32,
    pub(super) start_bu: i64,
    pub(super) end_bu: i64,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeProjection {
    pub(super) runs: Vec<NativeTextRun>,
    pub(super) visible_lines: Vec<String>,
    pub(super) implicit_spaces: usize,
    pub(super) hard_boundaries: usize,
    pub(super) glyph_geometries: usize,
}

#[allow(dead_code)]
pub(super) fn project(report: &NativeExecutionReport) -> NativeProjection {
    let mut runs: Vec<NativeTextRun> = Vec::new();
    let mut visible_cells: BTreeMap<u32, BTreeMap<i64, char>> = BTreeMap::new();
    for fragment in &report.fragments {
        let Some(node) = fragment.node else {
            continue;
        };
        if fragment.role != FragmentRole::Content {
            continue;
        }
        for key in &fragment.atoms {
            let atom = &report.atoms[key.0 as usize];
            let source = report.sources[report.nodes[node.0 as usize].source as usize]
                .path
                .clone();
            let Some(character) = char::from_u32(atom.display_scalar) else {
                continue;
            };
            if character != '\u{8}' {
                visible_cells
                    .entry(fragment.device_line)
                    .or_default()
                    .insert(fragment.start_bu, character);
            }
            if let Some(previous) = runs.last_mut()
                && previous.node == node
                && previous.font == atom.font
                && previous.device_line == fragment.device_line
                && previous.end_bu == fragment.start_bu
            {
                previous.text.push(character);
                previous.end_bu = fragment.end_bu;
            } else {
                runs.push(NativeTextRun {
                    node,
                    source,
                    font: atom.font,
                    text: character.to_string(),
                    device_line: fragment.device_line,
                    start_bu: fragment.start_bu,
                    end_bu: fragment.end_bu,
                });
            }
        }
    }
    NativeProjection {
        runs,
        visible_lines: visible_cells
            .into_values()
            .map(|cells| {
                let Some((&start, _)) = cells.first_key_value() else {
                    return String::new();
                };
                let Some((&end, _)) = cells.last_key_value() else {
                    return String::new();
                };
                (start..=end)
                    .step_by(24)
                    .map(|column| cells.get(&column).copied().unwrap_or(' '))
                    .collect()
            })
            .collect(),
        implicit_spaces: report
            .atoms
            .iter()
            .filter(|atom| atom.role == AtomRole::ImplicitSpace)
            .count(),
        hard_boundaries: report
            .boundaries
            .iter()
            .filter(|boundary| boundary.effect == BoundaryEffect::EndedLine)
            .count(),
        glyph_geometries: report
            .geometry
            .iter()
            .filter(|fact| {
                fact.kind == GeometryKind::Glyph && fact.origin_kind == GeometryOriginKind::Atom
            })
            .count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libmandoc_rs::{ExecutionLimits, InputFormat, ParseOptions, Parser};

    #[test]
    fn consumes_owned_man_and_mdoc_execution_facts() {
        let cases = [
            (
                "native-projection-man.1",
                InputFormat::Man,
                include_bytes!("../../../libmandoc-rs/tests/fixtures/execution/plain-man.1")
                    .as_slice(),
            ),
            (
                "native-projection-mdoc.1",
                InputFormat::Mdoc,
                include_bytes!("../../../libmandoc-rs/tests/fixtures/execution/plain-mdoc.1")
                    .as_slice(),
            ),
        ];
        for (path, format, source) in cases {
            let report = Parser::new(ParseOptions::default())
                .with_input_format(format)
                .with_mdoc_operating_system("ManT")
                .unwrap()
                .execute_bytes(path, source, ExecutionLimits::default())
                .unwrap();
            let projection = project(&report.execution);
            assert!(!projection.runs.is_empty());
            assert!(
                projection
                    .runs
                    .iter()
                    .all(|run| run.source == std::path::Path::new(path))
            );
            assert_eq!(
                projection.glyph_geometries,
                report.execution.fragments.len()
            );
            assert!(projection.runs.iter().all(|run| run.end_bu >= run.start_bu));
            if format == InputFormat::Man {
                assert_eq!(projection.implicit_spaces, 1);
                assert_eq!(projection.hard_boundaries, 21);
                assert_eq!(
                    projection.visible_lines,
                    [
                        "NAME",
                        "probe - execution report",
                        "DESCRIPTION",
                        "Ordinary words preserve source order and deterministic wrapping across",
                        "the native report boundary.",
                    ]
                );
            } else {
                assert_eq!(projection.implicit_spaces, 4);
                assert_eq!(projection.hard_boundaries, 19);
                assert_eq!(
                    projection.visible_lines,
                    [
                        "NAME",
                        "probe – execution report",
                        "DESCRIPTION",
                        "Plain emphasized text.",
                    ]
                );
                let emphasized = projection
                    .runs
                    .iter()
                    .find(|run| run.text == "emphasized")
                    .expect("native emphasized run");
                assert_eq!(emphasized.font, ExecutionFont::Underline);
                assert_eq!(report.execution.nodes[emphasized.node.0 as usize].line, 9);
            }
        }
    }
}
