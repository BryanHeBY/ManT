//! Batch `CommonMark` projection profiler for local roff audits.
//!
//! This development-only example accepts the same JSON-lines request as
//! `roff_structure_profile`. It lowers a native page into `ManT` IR, renders the
//! public `CommonMark` projection, reparses that projection, and compares the
//! topology that must survive the serialization boundary. A deterministic
//! first/middle/last section sample also exercises the public node-excerpt
//! renderer so full-document success cannot hide a broken `--node` path.

use std::{
    collections::BTreeMap,
    io::{self, BufRead, BufWriter, Write},
    path::PathBuf,
};

use mant_engine::{
    ManualPage, parse_manual_page, parse_markdown, render_excerpt_markdown, render_markdown,
    select_excerpt,
};
use mant_ir::{Block, Document, Inline, ListKind, ResolvedContent, Section};
use pulldown_cmark::{Event, Parser};
use serde::Serialize;
use serde_json::{Value, json};

const PROFILE_SCHEMA: &str = "mant.roff-projection-profile/v2";

#[derive(Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectionTopology {
    sections: Vec<SectionTopology>,
    list_items: Vec<ListItemTopology>,
    fences: Vec<FenceTopology>,
    entity_spellings: Vec<String>,
}

#[derive(Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SectionTopology {
    path: Vec<usize>,
    depth: usize,
    title: String,
}

#[derive(Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListItemTopology {
    section: Vec<usize>,
    owner_depth: usize,
    kind: ProjectedListKind,
}

#[derive(Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ProjectedListKind {
    Bullet,
    Ordered,
}

#[derive(Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct FenceTopology {
    section: Vec<usize>,
    owner_depth: usize,
    language: Option<String>,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectionCounts {
    sections: usize,
    list_items: usize,
    fences: usize,
    entity_spellings: usize,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("roff_projection_profile: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let stdin = io::stdin();
    let mut stdout = BufWriter::new(io::stdout().lock());
    for (index, line) in stdin.lock().lines().enumerate() {
        let line = line.map_err(|error| format!("read request {}: {error}", index + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        let response = profile_request(&line).unwrap_or_else(|error| {
            json!({
                "schema": PROFILE_SCHEMA,
                "id": request_id(&line),
                "error": error,
            })
        });
        serde_json::to_writer(&mut stdout, &response)
            .map_err(|error| format!("encode response {}: {error}", index + 1))?;
        stdout
            .write_all(b"\n")
            .map_err(|error| format!("write response {}: {error}", index + 1))?;
    }
    stdout.flush().map_err(|error| error.to_string())
}

fn request_id(line: &str) -> Value {
    serde_json::from_str::<Value>(line)
        .ok()
        .and_then(|request| request.get("id").cloned())
        .unwrap_or(Value::Null)
}

fn profile_request(line: &str) -> Result<Value, String> {
    let request: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
    let id = request
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "request.id must be a string".to_owned())?;
    let path = path_field(&request, "path")?;
    let root = path_field(&request, "root")?;
    let document = parse_manual_page(&ManualPage {
        name: "audit".to_owned(),
        section: "1".to_owned(),
        path,
        manual_root: root,
    })
    .map_err(|error| error.to_string())?;
    let query = ResolvedContent {
        label: "audit".to_owned(),
        address: None,
        document: Some(document.clone()),
        tldr: None,
    };

    let expected = projection_topology(&document);
    let markdown = render_markdown(&query);
    let reparsed = parse_markdown(&markdown, None).map_err(|error| error.to_string())?;
    let observed = projection_topology(&reparsed.document);
    let mut violations = compare_topology("full", &expected, &observed);
    let excerpt_checks = check_section_excerpts(&query, &document, &mut violations)?;

    Ok(json!({
        "schema": PROFILE_SCHEMA,
        "id": id,
        "expected": counts(&expected),
        "observed": counts(&observed),
        "topology": {
            "expected": expected,
            "observed": observed,
        },
        "excerptChecks": excerpt_checks,
        "diagnostics": {
            "ir": document.diagnostics.len(),
            "reparsed": reparsed.document.diagnostics.len(),
        },
        "violations": violations,
    }))
}

fn path_field(request: &Value, field: &str) -> Result<PathBuf, String> {
    request
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| format!("request.{field} must be a non-empty string"))
}

fn projection_topology(document: &Document) -> ProjectionTopology {
    let mut topology = ProjectionTopology::default();
    collect_blocks(&document.blocks, &[], &mut Vec::new(), &mut topology);
    collect_sections(&document.sections, &[], &mut topology);
    collect_entity_blocks(&document.blocks, &mut topology.entity_spellings);
    collect_entity_sections(&document.sections, &mut topology.entity_spellings);
    topology
}

fn collect_entity_sections(sections: &[Section], output: &mut Vec<String>) {
    for section in sections {
        extend_entity_spellings(&section.title, output);
        collect_entity_blocks(&section.blocks, output);
        collect_entity_sections(&section.children, output);
    }
}

fn collect_entity_blocks(blocks: &[Block], output: &mut Vec<String>) {
    for block in blocks {
        match block {
            Block::Paragraph { children, .. } => {
                collect_entity_inlines(children, output);
            }
            Block::List { items, .. } => {
                for item in items {
                    collect_entity_blocks(&item.blocks, output);
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    for term in &item.terms {
                        collect_entity_inlines(term, output);
                    }
                    collect_entity_blocks(&item.description, output);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        collect_entity_blocks(&cell.blocks, output);
                    }
                }
            }
            Block::Unsupported { text, .. } => extend_entity_spellings(text, output),
            Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. } => {}
        }
    }
}

fn collect_entity_inlines(inlines: &[Inline], output: &mut Vec<String>) {
    for inline in inlines {
        match inline {
            Inline::Text { value } => extend_entity_spellings(value, output),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => collect_entity_inlines(children, output),
            Inline::Code { .. } | Inline::Anchor { .. } | Inline::LineBreak => {}
        }
    }
}

fn extend_entity_spellings(value: &str, output: &mut Vec<String>) {
    let bytes = value.as_bytes();
    let mut start = 0;
    while let Some(relative) = bytes[start..].iter().position(|byte| *byte == b'&') {
        let ampersand = start + relative;
        let search_end = ampersand.saturating_add(64).min(bytes.len());
        let Some(relative_end) = bytes[ampersand + 1..search_end]
            .iter()
            .position(|byte| *byte == b';')
        else {
            start = ampersand + 1;
            continue;
        };
        let end = ampersand + relative_end + 2;
        let body = &bytes[ampersand + 1..end - 1];
        let named = !body.is_empty() && body.iter().all(u8::is_ascii_alphanumeric);
        let decimal = body
            .strip_prefix(b"#")
            .is_some_and(|digits| !digits.is_empty() && digits.iter().all(u8::is_ascii_digit));
        let hexadecimal = body
            .strip_prefix(b"#x")
            .or_else(|| body.strip_prefix(b"#X"))
            .is_some_and(|digits| !digits.is_empty() && digits.iter().all(u8::is_ascii_hexdigit));
        if (named || decimal || hexadecimal) && value.is_char_boundary(end) {
            let spelling = &value[ampersand..end];
            if commonmark_decodes_entity(spelling) {
                output.push(spelling.to_owned());
            }
        }
        start = end;
    }
}

fn commonmark_decodes_entity(spelling: &str) -> bool {
    let visible = Parser::new(spelling)
        .filter_map(|event| match event {
            Event::Text(value) => Some(value.into_string()),
            _ => None,
        })
        .collect::<String>();
    visible != spelling
}

fn collect_sections(sections: &[Section], parent: &[usize], topology: &mut ProjectionTopology) {
    for (index, section) in sections.iter().enumerate() {
        let mut path = parent.to_vec();
        path.push(index + 1);
        topology.sections.push(SectionTopology {
            path: path.clone(),
            depth: path.len() + 1,
            title: section.title.clone(),
        });
        collect_blocks(&section.blocks, &path, &mut Vec::new(), topology);
        collect_sections(&section.children, &path, topology);
    }
}

fn collect_blocks(
    blocks: &[Block],
    section: &[usize],
    owner_items: &mut Vec<usize>,
    topology: &mut ProjectionTopology,
) {
    for block in blocks {
        match block {
            Block::Preformatted { language, .. } => topology.fences.push(FenceTopology {
                section: section.to_vec(),
                owner_depth: owner_items.len(),
                language: language.clone(),
            }),
            Block::Table { .. } => topology.fences.push(FenceTopology {
                section: section.to_vec(),
                owner_depth: owner_items.len(),
                language: None,
            }),
            Block::Equation { display: true, .. } => topology.fences.push(FenceTopology {
                section: section.to_vec(),
                owner_depth: owner_items.len(),
                language: Some("math".to_owned()),
            }),
            Block::List { kind, items, .. } => {
                let kind = match kind {
                    ListKind::Ordered => ProjectedListKind::Ordered,
                    ListKind::Bullet | ListKind::Plain => ProjectedListKind::Bullet,
                };
                for (index, item) in items
                    .iter()
                    .filter(|item| blocks_have_projection(&item.blocks))
                    .enumerate()
                {
                    topology.list_items.push(ListItemTopology {
                        section: section.to_vec(),
                        owner_depth: owner_items.len(),
                        kind,
                    });
                    owner_items.push(index + 1);
                    collect_blocks(&item.blocks, section, owner_items, topology);
                    owner_items.pop();
                }
            }
            Block::DefinitionList { items, .. } => {
                for (index, item) in items
                    .iter()
                    .filter(|item| {
                        item.terms.iter().any(|term| has_visible_inline(term))
                            || blocks_have_projection(&item.description)
                    })
                    .enumerate()
                {
                    topology.list_items.push(ListItemTopology {
                        section: section.to_vec(),
                        owner_depth: owner_items.len(),
                        kind: ProjectedListKind::Bullet,
                    });
                    owner_items.push(index + 1);
                    collect_blocks(&item.description, section, owner_items, topology);
                    owner_items.pop();
                }
            }
            Block::Paragraph { .. }
            | Block::Equation { display: false, .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn counts(topology: &ProjectionTopology) -> ProjectionCounts {
    ProjectionCounts {
        sections: topology.sections.len(),
        list_items: topology.list_items.len(),
        fences: topology.fences.len(),
        entity_spellings: topology.entity_spellings.len(),
    }
}

fn compare_topology(
    scope: &str,
    expected: &ProjectionTopology,
    observed: &ProjectionTopology,
) -> Vec<String> {
    let mut violations = Vec::new();
    if expected.sections != observed.sections {
        violations.push(format!(
            "{scope} sections: expected {}, observed {}",
            expected.sections.len(),
            observed.sections.len()
        ));
    }
    compare_list_items(
        scope,
        &expected.list_items,
        &observed.list_items,
        &mut violations,
    );
    compare_fences(scope, &expected.fences, &observed.fences, &mut violations);
    compare_entity_spellings(
        scope,
        &expected.entity_spellings,
        &observed.entity_spellings,
        &mut violations,
    );
    violations
}

fn compare_entity_spellings(
    scope: &str,
    expected: &[String],
    observed: &[String],
    violations: &mut Vec<String>,
) {
    if expected == observed {
        return;
    }
    violations.push(format!(
        "{scope} entity spellings: expected {expected:?}, observed {observed:?}"
    ));
}

fn compare_list_items(
    scope: &str,
    expected: &[ListItemTopology],
    observed: &[ListItemTopology],
    violations: &mut Vec<String>,
) {
    let expected = list_items_by_section(expected);
    let observed = list_items_by_section(observed);
    for section in expected
        .keys()
        .chain(observed.keys())
        .collect::<std::collections::BTreeSet<_>>()
    {
        let expected_items = expected.get(section).map_or(&[][..], Vec::as_slice);
        let observed_items = observed.get(section).map_or(&[][..], Vec::as_slice);
        if expected_items != observed_items {
            violations.push(format!(
                "{scope} list items in section {:?}: expected {:?}, observed {:?}",
                section,
                summarize_list_items(expected_items),
                summarize_list_items(observed_items),
            ));
        }
    }
}

fn list_items_by_section(items: &[ListItemTopology]) -> BTreeMap<Vec<usize>, Vec<(u8, usize)>> {
    let mut grouped = BTreeMap::<Vec<usize>, Vec<(u8, usize)>>::new();
    for item in items {
        grouped.entry(item.section.clone()).or_default().push((
            match item.kind {
                ProjectedListKind::Bullet => 0,
                ProjectedListKind::Ordered => 1,
            },
            item.owner_depth,
        ));
    }
    grouped
}

fn summarize_list_items(items: &[(u8, usize)]) -> String {
    let preview = items
        .iter()
        .take(8)
        .map(|(kind, depth)| format!("{}@{depth}", if *kind == 0 { "bullet" } else { "ordered" }))
        .collect::<Vec<_>>()
        .join(",");
    if items.len() > 8 {
        format!("{} items [{preview},…]", items.len())
    } else {
        format!("{} items [{preview}]", items.len())
    }
}

fn compare_fences(
    scope: &str,
    expected: &[FenceTopology],
    observed: &[FenceTopology],
    violations: &mut Vec<String>,
) {
    let expected = fences_by_section(expected);
    let observed = fences_by_section(observed);
    for section in expected
        .keys()
        .chain(observed.keys())
        .collect::<std::collections::BTreeSet<_>>()
    {
        let expected_fences = expected.get(section).map_or(&[][..], Vec::as_slice);
        let observed_fences = observed.get(section).map_or(&[][..], Vec::as_slice);
        if expected_fences != observed_fences {
            violations.push(format!(
                "{scope} fences in section {section:?}: expected {expected_fences:?}, observed {observed_fences:?}",
            ));
        }
    }
}

fn fences_by_section(fences: &[FenceTopology]) -> BTreeMap<Vec<usize>, Vec<(Option<&str>, usize)>> {
    let mut grouped = BTreeMap::<Vec<usize>, Vec<(Option<&str>, usize)>>::new();
    for fence in fences {
        grouped
            .entry(fence.section.clone())
            .or_default()
            .push((fence.language.as_deref(), fence.owner_depth));
    }
    grouped
}

fn blocks_have_projection(blocks: &[Block]) -> bool {
    blocks.iter().any(block_has_projection)
}

fn block_has_projection(block: &Block) -> bool {
    match block {
        Block::Paragraph { children, .. } => has_visible_inline(children),
        Block::Preformatted { .. } | Block::ThematicBreak { .. } => true,
        Block::List { items, .. } => items
            .iter()
            .any(|item| blocks_have_projection(&item.blocks)),
        Block::DefinitionList { items, .. } => items.iter().any(|item| {
            item.terms.iter().any(|term| has_visible_inline(term))
                || blocks_have_projection(&item.description)
        }),
        Block::Table { rows, .. } => rows.iter().any(|row| {
            row.cells
                .iter()
                .any(|cell| blocks_have_projection(&cell.blocks))
        }),
        Block::Equation { value, display, .. } => *display || !value.is_empty(),
        Block::Unsupported { text, .. } => !text.trim().is_empty(),
        Block::VerticalSpace { .. } => false,
    }
}

fn has_visible_inline(inlines: &[mant_ir::Inline]) -> bool {
    inlines.iter().any(|inline| match inline {
        mant_ir::Inline::Text { value } | mant_ir::Inline::Code { value } => !value.is_empty(),
        mant_ir::Inline::Strong { children }
        | mant_ir::Inline::Emphasis { children }
        | mant_ir::Inline::Link { children, .. } => has_visible_inline(children),
        mant_ir::Inline::Anchor { .. } | mant_ir::Inline::LineBreak => false,
    })
}

fn check_section_excerpts(
    query: &ResolvedContent,
    document: &Document,
    violations: &mut Vec<String>,
) -> Result<usize, String> {
    let mut sections = Vec::new();
    flatten_sections(&document.sections, &[], &mut sections);
    let indexes = sample_indexes(sections.len());
    for index in &indexes {
        let (coordinates, section) = &sections[*index];
        let selector = coordinates
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(".");
        let excerpt =
            select_excerpt(query, &[selector.as_str()]).map_err(|error| error.to_string())?;
        let markdown = render_excerpt_markdown(&excerpt);
        let reparsed = parse_markdown(&markdown, None).map_err(|error| error.to_string())?;
        let expected_document = Document {
            source: document.source.clone(),
            meta: document.meta.clone(),
            parser: document.parser.clone(),
            blocks: Vec::new(),
            sections: vec![(*section).clone()],
            diagnostics: Vec::new(),
        };
        let expected = projection_topology(&expected_document);
        let observed = projection_topology(&reparsed.document);
        violations.extend(compare_topology(
            &format!("excerpt {selector}"),
            &expected,
            &observed,
        ));
    }
    Ok(indexes.len())
}

fn flatten_sections<'a>(
    sections: &'a [Section],
    parent: &[usize],
    output: &mut Vec<(Vec<usize>, &'a Section)>,
) {
    for (index, section) in sections.iter().enumerate() {
        let mut path = parent.to_vec();
        path.push(index + 1);
        output.push((path.clone(), section));
        flatten_sections(&section.children, &path, output);
    }
}

fn sample_indexes(length: usize) -> Vec<usize> {
    if length == 0 {
        return Vec::new();
    }
    let mut indexes = vec![0, length / 2, length - 1];
    indexes.sort_unstable();
    indexes.dedup();
    indexes
}
