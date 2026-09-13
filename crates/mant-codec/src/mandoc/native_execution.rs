//! First private consumer of the owned native execution report.
//!
//! This is deliberately not the production switch. It establishes the
//! execution-to-projection boundary used by the staged migration without
//! reconstructing formatter state in Rust.

use libmandoc_rs::{
    AtomRole, BoundaryEffect, Document as NativeDocument, ExecutionAffinity, ExecutionFlush,
    ExecutionFont, ExecutionNodeKey, ExecutionReferenceKind, FragmentRole, GeometryKind,
    GeometryOriginKind, NativeExecutionReport, Node as NativeNode, NodeKind, NormalizedListKind,
};
use std::{collections::BTreeMap, ops::Range, path::PathBuf};

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeTextRun {
    pub(super) node: ExecutionNodeKey,
    pub(super) source: PathBuf,
    pub(super) line: u32,
    pub(super) column: u32,
    pub(super) font: ExecutionFont,
    pub(super) text: String,
    pub(super) device_line: u32,
    pub(super) start_bu: i64,
    pub(super) end_bu: i64,
    pub(super) reference: Option<u32>,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeAnchor {
    pub(super) key: u32,
    pub(super) node: ExecutionNodeKey,
    pub(super) target: Vec<u8>,
    pub(super) device_line: u32,
    pub(super) atom_cursor: u32,
    pub(super) fragment_cursor: u32,
    pub(super) affinity: ExecutionAffinity,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeReference {
    pub(super) key: u32,
    pub(super) parent: Option<u32>,
    pub(super) owner_node: ExecutionNodeKey,
    pub(super) target_node: ExecutionNodeKey,
    pub(super) kind: ExecutionReferenceKind,
    pub(super) primary: Vec<u8>,
    pub(super) secondary: Option<Vec<u8>>,
    pub(super) execution_atoms: Range<u32>,
    pub(super) atoms: Range<u32>,
    pub(super) affinity: ExecutionAffinity,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeOrigin {
    pub(super) key: ExecutionNodeKey,
    pub(super) parent: Option<ExecutionNodeKey>,
    pub(super) source: PathBuf,
    pub(super) line: u32,
    pub(super) column: u32,
    pub(super) kind: u32,
    pub(super) macro_name: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeDefinitionFact {
    pub(super) owner: ExecutionNodeKey,
    pub(super) macro_name: String,
    pub(super) head: ExecutionNodeKey,
    pub(super) body: ExecutionNodeKey,
    pub(super) head_flushes: Vec<ExecutionFlush>,
    pub(super) body_flushes: Vec<ExecutionFlush>,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeProjection {
    pub(super) origins: Vec<NativeOrigin>,
    pub(super) runs: Vec<NativeTextRun>,
    pub(super) visible_lines: Vec<String>,
    pub(super) implicit_spaces: usize,
    pub(super) hard_boundaries: usize,
    pub(super) glyph_geometries: usize,
    pub(super) references: Vec<NativeReference>,
    pub(super) anchors: Vec<NativeAnchor>,
    pub(super) definitions: Vec<NativeDefinitionFact>,
}

fn mark_definition_items(
    node: &NativeNode,
    inherited_list_kind: Option<NormalizedListKind>,
    definitions: &mut [bool],
) {
    let list_kind = if node.kind == NodeKind::Block && node.macro_name.as_deref() == Some("Bl") {
        node.list_kind
    } else {
        inherited_list_kind
    };
    if node.kind == NodeKind::Block
        && node.macro_name.as_deref() == Some("It")
        && list_kind == Some(NormalizedListKind::Definition)
        && let Some(key) = node.execution_node_key
    {
        definitions[key as usize] = true;
    }
    for child in &node.children {
        mark_definition_items(child, list_kind, definitions);
    }
}

fn definition_facts(
    document: &NativeDocument,
    report: &NativeExecutionReport,
) -> Vec<NativeDefinitionFact> {
    #[derive(Default)]
    struct PendingDefinition {
        owner: Option<ExecutionNodeKey>,
        macro_name: Option<String>,
        head: Option<ExecutionNodeKey>,
        body: Option<ExecutionNodeKey>,
        head_flushes: Vec<ExecutionFlush>,
        body_flushes: Vec<ExecutionFlush>,
    }

    let mut definition_items = vec![false; report.nodes.len()];
    mark_definition_items(&document.root, None, &mut definition_items);
    let mut owner_definition = vec![None; report.nodes.len()];
    let mut definitions = Vec::new();
    for node in &report.nodes {
        let is_man_definition =
            matches!(node.macro_name.as_deref(), Some("IP" | "TP" | "TQ" | "HP"));
        let is_mdoc_definition =
            node.macro_name.as_deref() == Some("It") && definition_items[node.key.0 as usize];
        if node.kind == 1 && (is_man_definition || is_mdoc_definition) {
            owner_definition[node.key.0 as usize] = Some(definitions.len());
            definitions.push(PendingDefinition {
                owner: Some(node.key),
                macro_name: node.macro_name.clone(),
                ..PendingDefinition::default()
            });
        }
    }

    let mut direct_content_definition = vec![None; report.nodes.len()];
    for node in &report.nodes {
        let Some(parent) = node.parent else {
            continue;
        };
        let Some(definition) = owner_definition[parent.0 as usize] else {
            continue;
        };
        match node.kind {
            2 => {
                definitions[definition].head = Some(node.key);
                direct_content_definition[node.key.0 as usize] = Some((definition, true));
            }
            3 => {
                definitions[definition].body = Some(node.key);
                direct_content_definition[node.key.0 as usize] = Some((definition, false));
            }
            _ => {}
        }
    }

    let mut content_definition = vec![None; report.nodes.len()];
    for node in &report.nodes {
        content_definition[node.key.0 as usize] = direct_content_definition[node.key.0 as usize]
            .or_else(|| {
                node.parent
                    .and_then(|parent| content_definition[parent.0 as usize])
            });
    }

    for flush in &report.flushes {
        let Some(node) = flush.node else {
            continue;
        };
        if let Some((definition, is_head)) = content_definition[node.0 as usize] {
            if is_head {
                definitions[definition].head_flushes.push(flush.clone());
            } else {
                definitions[definition].body_flushes.push(flush.clone());
            }
        }
    }

    definitions
        .into_iter()
        .filter_map(|definition| {
            Some(NativeDefinitionFact {
                owner: definition.owner?,
                macro_name: definition.macro_name?,
                head: definition.head?,
                body: definition.body?,
                head_flushes: definition.head_flushes,
                body_flushes: definition.body_flushes,
            })
        })
        .collect()
}

fn atom_reference_owners(report: &NativeExecutionReport) -> Vec<Option<u32>> {
    let mut atom_references = vec![None; report.atoms.len()];
    let mut reference_events = report
        .references
        .iter()
        .filter(|reference| !reference.atoms.is_empty())
        .flat_map(|reference| {
            [
                (reference.atoms.start, true, reference.key),
                (reference.atoms.end, false, reference.key),
            ]
        })
        .collect::<Vec<_>>();
    reference_events.sort_unstable_by(|left, right| {
        left.0.cmp(&right.0).then_with(|| match (left.1, right.1) {
            (false, true) => std::cmp::Ordering::Less,
            (true, false) => std::cmp::Ordering::Greater,
            (true, true) => left.2.cmp(&right.2),
            (false, false) => right.2.cmp(&left.2),
        })
    });
    let mut references = Vec::new();
    let mut event = 0;
    for (atom, reference) in atom_references.iter_mut().enumerate() {
        while event < reference_events.len() && reference_events[event].0 as usize == atom {
            let (_, entering, key) = reference_events[event];
            if entering {
                references.push(key);
            } else {
                assert_eq!(references.pop(), Some(key), "validated reference nesting");
            }
            event += 1;
        }
        *reference = references.last().copied();
    }
    while event < reference_events.len()
        && reference_events[event].0 as usize == atom_references.len()
    {
        let (_, entering, key) = reference_events[event];
        assert!(!entering, "validated reference interval end");
        assert_eq!(references.pop(), Some(key), "validated reference nesting");
        event += 1;
    }
    assert_eq!(event, reference_events.len(), "validated reference cursor");
    assert!(references.is_empty(), "validated reference closure");
    atom_references
}

fn text_projection(report: &NativeExecutionReport) -> (Vec<NativeTextRun>, Vec<String>) {
    let mut runs: Vec<NativeTextRun> = Vec::new();
    let atom_references = atom_reference_owners(report);
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
            let reference = atom_references[key.0 as usize];
            let origin = &report.nodes[node.0 as usize];
            let source = report.sources[origin.source as usize].path.clone();
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
                && previous.reference == reference
            {
                previous.text.push(character);
                previous.end_bu = fragment.end_bu;
            } else {
                runs.push(NativeTextRun {
                    node,
                    source,
                    line: origin.line,
                    column: origin.column,
                    font: atom.font,
                    text: character.to_string(),
                    device_line: fragment.device_line,
                    start_bu: fragment.start_bu,
                    end_bu: fragment.end_bu,
                    reference,
                });
            }
        }
    }
    let visible_lines = visible_cells
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
        .collect();
    (runs, visible_lines)
}

#[allow(dead_code)]
pub(super) fn project(
    document: &NativeDocument,
    report: &NativeExecutionReport,
) -> NativeProjection {
    let (runs, visible_lines) = text_projection(report);
    NativeProjection {
        origins: report
            .nodes
            .iter()
            .map(|node| NativeOrigin {
                key: node.key,
                parent: node.parent,
                source: report.sources[node.source as usize].path.clone(),
                line: node.line,
                column: node.column,
                kind: node.kind,
                macro_name: node.macro_name.clone(),
            })
            .collect(),
        runs,
        visible_lines,
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
        references: report
            .references
            .iter()
            .map(|reference| NativeReference {
                key: reference.key,
                parent: reference.parent,
                owner_node: reference.owner_node,
                target_node: reference.target_node,
                kind: reference.kind,
                primary: report
                    .pool_bytes(reference.primary)
                    .expect("validated native reference target")
                    .to_vec(),
                secondary: reference.secondary.map(|range| {
                    report
                        .pool_bytes(range)
                        .expect("validated native reference component")
                        .to_vec()
                }),
                execution_atoms: reference.execution_atoms.clone(),
                atoms: reference.atoms.clone(),
                affinity: reference.affinity,
            })
            .collect(),
        anchors: report
            .anchors
            .iter()
            .map(|anchor| NativeAnchor {
                key: anchor.key,
                node: anchor.node,
                target: report
                    .pool_bytes(anchor.target)
                    .expect("validated native anchor pool range")
                    .to_vec(),
                device_line: anchor.device_line,
                atom_cursor: anchor.atom_cursor,
                fragment_cursor: anchor.fragment_cursor,
                affinity: anchor.affinity,
            })
            .collect(),
        definitions: definition_facts(document, report),
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
            let projection = project(&report.document, &report.execution);
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

    #[test]
    fn projects_definition_roles_and_semantic_annotations_without_c_borrows() {
        for (path, format, source, definition_macro) in [
            (
                "annotated-man.1",
                InputFormat::Man,
                include_bytes!("../../../libmandoc-rs/tests/fixtures/execution/annotated-man.1")
                    .as_slice(),
                "TP",
            ),
            (
                "annotated-mdoc.1",
                InputFormat::Mdoc,
                include_bytes!("../../../libmandoc-rs/tests/fixtures/execution/annotated-mdoc.1")
                    .as_slice(),
                "It",
            ),
        ] {
            let report = Parser::new(ParseOptions::default())
                .with_input_format(format)
                .with_mdoc_operating_system("ManT")
                .unwrap()
                .execute_bytes(path, source, ExecutionLimits::default())
                .unwrap();
            let projection = project(&report.document, &report.execution);
            assert!(projection.runs.iter().any(|run| {
                run.reference.is_some()
                    && (run.text.contains("linked") || run.text.contains("label"))
            }));
            assert!(!projection.runs.iter().any(|run| {
                run.reference.is_some() && run.text.contains("https://example.org/manual")
            }));
            let definition = projection
                .definitions
                .iter()
                .find(|definition| definition.macro_name == definition_macro)
                .expect("native definition structure");
            assert!(!definition.head_flushes.is_empty());
            assert!(!definition.body_flushes.is_empty());
            assert!(
                definition
                    .head_flushes
                    .iter()
                    .chain(&definition.body_flushes)
                    .all(|flush| flush.accepted.end <= flush.scanned.end)
            );
            let reference = projection
                .references
                .iter()
                .find(|reference| reference.kind == ExecutionReferenceKind::ExternalUri)
                .expect("owned URI reference");
            assert_eq!(reference.primary, b"https://example.org/manual");
            assert!(!reference.atoms.is_empty());
            if format == InputFormat::Mdoc {
                let anchor = projection
                    .anchors
                    .iter()
                    .find(|anchor| anchor.target == b"custom-target")
                    .expect("owned native anchor");
                assert!(anchor.device_line > 0);
                assert!(
                    usize::try_from(anchor.atom_cursor)
                        .is_ok_and(|cursor| cursor <= report.execution.atoms.len())
                );
                assert!(
                    usize::try_from(anchor.fragment_cursor)
                        .is_ok_and(|cursor| cursor <= report.execution.fragments.len())
                );
            }
            drop(report);
            assert!(
                !projection.runs.is_empty(),
                "projection must be fully owned"
            );
            assert_eq!(
                projection.references[0].primary,
                b"https://example.org/manual"
            );
            assert_eq!(
                projection.origins[projection.references[0].owner_node.0 as usize]
                    .macro_name
                    .as_deref(),
                Some(if format == InputFormat::Man {
                    "UR"
                } else {
                    "Lk"
                })
            );
        }
    }

    #[test]
    fn mdoc_definition_projection_uses_the_native_list_subtype() {
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "list-roles-mdoc.1",
                include_bytes!("../../../libmandoc-rs/tests/fixtures/execution/list-roles-mdoc.1"),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        assert_eq!(projection.definitions.len(), 1);
        let definition = &projection.definitions[0];
        assert_eq!(definition.macro_name, "It");
        assert!(!definition.head_flushes.is_empty());
        assert!(!definition.body_flushes.is_empty());
        assert!(
            projection
                .visible_lines
                .iter()
                .any(|line| line.contains("Bullet body."))
        );
    }

    #[test]
    fn reference_projection_owns_wrapped_labels_and_nested_relationships() {
        let wrapped = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "wrapped-reference-mdoc.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/wrapped-reference-mdoc.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let wrapped_projection = project(&wrapped.document, &wrapped.execution);
        let wrapped_reference = wrapped_projection
            .references
            .iter()
            .find(|reference| reference.kind == ExecutionReferenceKind::ExternalUri)
            .expect("native wrapped URI reference");
        let wrapped_key = wrapped_reference.key;
        assert_eq!(wrapped_reference.primary, b"https://example.org/wrapped");
        let label_runs = wrapped_projection
            .runs
            .iter()
            .filter(|run| run.reference == Some(wrapped_key))
            .collect::<Vec<_>>();
        assert!(
            label_runs
                .iter()
                .map(|run| run.device_line)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                >= 2,
            "the fixed native width must wrap the semantic label"
        );
        assert!(
            label_runs
                .iter()
                .all(|run| run.font == ExecutionFont::Underline)
        );
        assert!(
            wrapped_projection.runs.iter().any(|run| {
                run.reference.is_none() && run.text == "https://example.org/wrapped"
            })
        );
        drop(wrapped);
        assert_eq!(
            wrapped_projection
                .references
                .iter()
                .find(|reference| reference.key == wrapped_key)
                .expect("owned wrapped reference")
                .primary,
            b"https://example.org/wrapped"
        );

        let nested = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Man)
            .execute_bytes(
                "nested-reference-man.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/nested-reference-man.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let nested_projection = project(&nested.document, &nested.execution);
        let outer = nested_projection
            .references
            .iter()
            .find(|reference| reference.kind == ExecutionReferenceKind::ExternalUri)
            .expect("outer URI reference");
        let inner = nested_projection
            .references
            .iter()
            .find(|reference| reference.kind == ExecutionReferenceKind::Manual)
            .expect("nested manual reference");
        assert_eq!(inner.parent, Some(outer.key));
        assert!(outer.execution_atoms.start <= inner.execution_atoms.start);
        assert!(inner.execution_atoms.end <= outer.execution_atoms.end);
        assert!(
            nested_projection
                .runs
                .iter()
                .filter(|run| run.reference == Some(inner.key))
                .any(|run| run.text.contains("printf") || run.text.contains('3'))
        );
        drop(nested);
        assert_eq!(inner.primary, b"printf");
        assert_eq!(inner.secondary.as_deref(), Some(b"3".as_slice()));
    }

    #[test]
    fn definition_projection_owns_each_partial_native_flush() {
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Man)
            .execute_bytes(
                "partial-definition-man.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/partial-definition-man.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        let definition = projection
            .definitions
            .first()
            .expect("native TP definition");
        assert!(!definition.head_flushes.is_empty());
        assert!(
            definition
                .body_flushes
                .iter()
                .any(|flush| flush.outcome == libmandoc_rs::FlushOutcome::Wrapped)
        );
        assert!(
            definition
                .head_flushes
                .iter()
                .chain(&definition.body_flushes)
                .all(|flush| {
                    flush.accepted.start == flush.scanned.start
                        && flush.accepted.end == flush.consumed.end
                        && flush.remaining.end == flush.scanned.end
                        && flush.fragments.end >= flush.fragments.start
                })
        );
    }
}
