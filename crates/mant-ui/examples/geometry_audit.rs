//! Development-only, single-input geometry evidence. Not a product protocol.
//! Run: `cargo run -p mant-ui --example geometry_audit -- --input FILE
//!      --input-format roff --widths 20,40,80,120`
//! `--input -` reads bounded stdin; its format must be explicit.
//! Plain/decorated bodies are complete. Row/cell/decoration evidence is bounded
//! and reports omissions explicitly. Coordinates are zero-based terminal cells.

use std::{cell::RefCell, collections::BTreeSet, error::Error, io::Read, path::Path};

use mant_ir::{DocumentIndex, ResolvedContent};
use mant_ui::DocumentView;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::Line,
    widgets::{Paragraph, Widget},
};
use serde_json::{Value, json};
use unicode_width::UnicodeWidthStr;

const SCHEMA: &str = "mant-dev-geometry-audit-v1";
const MAX_ROWS: usize = 20_000;
const MAX_CELLS: usize = 1_000_000;
const MAX_DECORATIONS: usize = 20_000;

struct Args {
    input: String,
    format: String,
    widths: Vec<u16>,
}

impl Args {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut input = None;
        let mut format = "auto".to_owned();
        let mut widths = vec![20, 40, 80, 120];
        let mut args = args.iter();
        while let Some(key) = args.next() {
            let value = args
                .next()
                .ok_or_else(|| format!("missing value for {key}"))?;
            match key.as_str() {
                "--input" if input.is_none() => input = Some(value.clone()),
                "--input-format" if matches!(value.as_str(), "auto" | "roff" | "markdown") => {
                    format.clone_from(value);
                }
                "--widths" => {
                    widths = value
                        .split(',')
                        .map(str::parse::<u16>)
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|error| error.to_string())?;
                    if widths.is_empty()
                        || widths.len() > 8
                        || widths.iter().any(|&w| w == 0 || w > 512)
                    {
                        return Err("widths require 1..8 integers, each within 1..512".into());
                    }
                    if widths.iter().collect::<BTreeSet<_>>().len() != widths.len() {
                        return Err("duplicate widths are not allowed".into());
                    }
                }
                _ => return Err(format!("unknown or duplicate option: {key}")),
            }
        }
        let input = input.ok_or(
            "required: --input FILE|- [--input-format roff|markdown|auto] [--widths 20,40,80,120]",
        )?;
        if input == "-" && format == "auto" {
            return Err("stdin requires an explicit --input-format".into());
        }
        Ok(Self {
            input,
            format,
            widths,
        })
    }
}

fn bounded_read(reader: impl Read, limit: u64) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len())? > limit {
        return Err(format!("input exceeds {limit}-byte limit").into());
    }
    Ok(bytes)
}

fn load(args: &Args) -> Result<ResolvedContent, Box<dyn Error>> {
    let markdown = args.format == "markdown"
        || (args.format == "auto"
            && Path::new(&args.input)
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| {
                    s.eq_ignore_ascii_case("md") || s.eq_ignore_ascii_case("markdown")
                }));
    if args.input != "-" && !markdown {
        // This existing file loader enforces compressed/decompressed limits and
        // does not discover MANPATH or invoke an external renderer.
        let document = mant_loader::parse_manual_source(Path::new(&args.input))?;
        return Ok(ResolvedContent {
            label: document
                .meta
                .names
                .first()
                .cloned()
                .or_else(|| document.meta.title.clone())
                .unwrap_or_else(|| "input".to_owned()),
            address: None,
            document: Some(document),
            tldr: None,
        });
    }
    let limit = if markdown {
        mant_loader::MAX_MARKDOWN_BYTES
    } else {
        mant_loader::MAX_MANUAL_BYTES
    };
    let bytes = if args.input == "-" {
        bounded_read(std::io::stdin().lock(), limit)?
    } else {
        bounded_read(std::fs::File::open(&args.input)?, limit)?
    };
    if markdown {
        Ok(mant_loader::load_markdown_text(
            std::str::from_utf8(&bytes)?,
            None,
        )?)
    } else {
        Ok(mant_loader::load_roff_bytes(&bytes)?)
    }
}

fn buffer_row(line: &Line<'_>, width: u16) -> Value {
    let area = Rect::new(0, 0, width, 1);
    let mut buffer = Buffer::empty(area);
    // DocumentView already wraps; adding Paragraph::wrap would change geometry.
    Paragraph::new(line.clone()).render(area, &mut buffer);
    let cells = (0..width)
        .map(|column| {
            let cell = &buffer[(column, 0)];
            json!({"column": column, "symbol": cell.symbol(),
            "symbol_columns": UnicodeWidthStr::width(cell.symbol()),
            "fg": format!("{:?}", cell.fg), "bg": format!("{:?}", cell.bg),
            "modifiers": cell.modifier.bits(), "diff_option": format!("{:?}", cell.diff_option)})
        })
        .collect::<Vec<_>>();
    // Continuation cells of wide glyphs contain spaces in Ratatui's buffer;
    // preserve those cells rather than incorrectly concatenating them as text.
    json!({"complete": true, "cells": cells})
}

fn terminal_columns(text: &str) -> usize {
    mant_render::cells::graphemes(text)
        .map(|grapheme| grapheme.columns())
        .sum()
}

fn audit(bundle: &ResolvedContent, widths: &[u16]) -> Value {
    audit_with_limits(bundle, widths, MAX_ROWS, MAX_CELLS, MAX_DECORATIONS)
}

fn audit_with_limits(
    bundle: &ResolvedContent,
    widths: &[u16],
    max_rows: usize,
    max_cells: usize,
    max_decorations: usize,
) -> Value {
    let plain = mant_render::render_query_text(bundle);
    let decorations = RefCell::new(Vec::new());
    let decoration_count = std::cell::Cell::new(0_usize);
    let decorated = mant_render::render_query_text_with(bundle, |presentation, text| {
        decoration_count.set(decoration_count.get() + 1);
        if decorations.borrow().len() < max_decorations {
            decorations.borrow_mut().push(json!({"text": text,
                "role": format!("{:?}", presentation.role),
                "strong": presentation.inline.strong, "emphasis": presentation.inline.emphasis,
                "code": presentation.inline.code, "link": presentation.inline.link,
                "entry_kind": presentation.inline.entry_kind.map(|kind| format!("{kind:?}")),
                "matched": presentation.matched}));
        }
        // Identity decorator deliberately exercises the decorated renderer path
        // without injecting ANSI into content or changing boundary whitespace.
        text.to_owned()
    });
    let view = DocumentView::new(bundle);
    let mut ids = BTreeSet::new();
    if let Some(document) = &bundle.document {
        ids.extend(
            DocumentIndex::build(document)
                .iter()
                .map(|(id, _)| id.to_string()),
        );
    }
    ids.extend(view.navigation().iter().map(|node| node.target_id.clone()));
    let mut remaining_cells = max_cells;
    let mut remaining_rows = max_rows;
    let renders = widths.iter().map(|&width| {
        let rendered = view.render(width);
        let captured = rendered.text.lines.len().min(remaining_rows);
        remaining_rows -= captured;
        let rows = rendered.text.lines.iter().take(captured).enumerate().map(|(row, line)| {
            let text = line.to_string();
            let columns = line.spans.iter().map(|span| terminal_columns(&span.content)).sum::<usize>();
            let buffer = if remaining_cells >= usize::from(width) {
                remaining_cells -= usize::from(width);
                buffer_row(line, width)
            } else { json!({"complete": false, "cells": [], "reason": "cell-budget"}) };
            json!({"row": row, "text": text, "columns": columns,
                "unicode_string_columns": UnicodeWidthStr::width(text.as_str()),
                "over_width": columns > usize::from(width), "buffer": buffer,
                "spans": line.spans.iter().map(|span| json!({"text": span.content,
                    "columns": terminal_columns(&span.content),
                    "fg": span.style.fg.map(|v| format!("{v:?}")),
                    "bg": span.style.bg.map(|v| format!("{v:?}")),
                    "add_modifiers": span.style.add_modifier.bits(),
                    "sub_modifiers": span.style.sub_modifier.bits()})).collect::<Vec<_>>()})
        }).collect::<Vec<_>>();
        let anchors = ids.iter().map(|id| {
            let row = rendered.anchor_row(id);
            json!({"id": id, "row": row,
                "in_row_range": row.map(|row| row < rendered.row_count),
                "line_text": row.and_then(|row| rendered.text.lines.get(row)).map(ToString::to_string)})
        }).collect::<Vec<_>>();
        let cells_complete = rows.iter().all(|row| row["buffer"]["complete"] == true);
        json!({"width": width, "row_count": rendered.row_count,
            "rows_complete": captured == rendered.text.lines.len(),
            "cells_complete": captured == rendered.text.lines.len() && cells_complete,
            "captured_rows": captured, "rows": rows, "anchors": anchors})
    }).collect::<Vec<_>>();
    let complete = renders
        .iter()
        .all(|render| render["rows_complete"] == true && render["cells_complete"] == true)
        && decoration_count.get() <= max_decorations;
    json!({"schema": SCHEMA, "status": "ok", "complete": complete,
        "coordinate_unit": "zero-based-terminal-cell", "anchor_coordinate_unit": "row-only",
        "anchor_scope": "DocumentIndex identities plus public navigation destinations; absent rows are explicit null",
        "limits": {"rows_total": max_rows, "cells_total": max_cells, "decoration_runs": max_decorations},
        "ir": {"present": bundle.document.is_some(), "identity_count": ids.len(),
            "diagnostics": bundle.document.as_ref().map(|document| &document.diagnostics)},
        "body": {"complete": true, "plain_text": plain, "decorated_plain_text": decorated,
            "decoration_mode": "identity callback with separate presentation evidence",
            "decoration_runs_total": decoration_count.get(),
            "decoration_runs_complete": decoration_count.get() <= max_decorations,
            "decoration_runs": decorations.into_inner()},
        "renders": renders})
}

fn run() -> Result<Value, Box<dyn Error>> {
    let args = Args::parse(&std::env::args().skip(1).collect::<Vec<_>>())?;
    Ok(audit(&load(&args)?, &args.widths))
}

fn main() {
    let (value, code) = match run() {
        Ok(value) => (value, 0),
        Err(error) => (
            json!({"schema": SCHEMA, "status": "error", "complete": false,
            "error": error.to_string()}),
            1,
        ),
    };
    if let Err(error) = serde_json::to_writer(std::io::stdout().lock(), &value) {
        eprintln!("geometry audit output: {error}");
        std::process::exit(1);
    }
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;
    use mant_ir::{Block, Inline, LayoutHint, ListItem, ListKind};

    fn text(value: &str) -> Inline {
        Inline::Text {
            value: value.into(),
        }
    }
    fn paragraph(children: Vec<Inline>, indent: i32, gap: u16) -> Block {
        Block::Paragraph {
            children,
            layout: LayoutHint {
                indent_columns: indent,
                spacing_before_lines: gap,
                ..Default::default()
            },
            source: None,
        }
    }
    fn bundle(blocks: Vec<Block>) -> ResolvedContent {
        let mut bundle = mant_loader::load_markdown_text("# Probe\n\nseed", None).unwrap();
        let document = bundle.document.as_mut().unwrap();
        document.sections.clear();
        document.blocks = blocks;
        bundle
    }
    fn row<'a>(render: &'a Value, token: &str) -> &'a Value {
        render["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["text"].as_str().unwrap().contains(token))
            .unwrap()
    }

    #[test]
    fn hard_rows_gaps_and_coincident_targets_survive_all_widths() {
        let bundle = bundle(vec![
            paragraph(
                vec![
                    Inline::anchor("a"),
                    Inline::anchor("b"),
                    text("FIRST"),
                    Inline::LineBreak,
                    text("SECOND"),
                ],
                0,
                0,
            ),
            paragraph(vec![text("THIRD")], 0, 2),
        ]);
        let report = audit(&bundle, &[20, 40, 80, 120]);
        assert_eq!(report["complete"], true);
        assert_eq!(
            report["body"]["plain_text"],
            report["body"]["decorated_plain_text"]
        );
        assert!(
            report["body"]["plain_text"]
                .as_str()
                .unwrap()
                .contains("FIRST\nSECOND\n\n\nTHIRD")
        );
        for render in report["renders"].as_array().unwrap() {
            let first = row(render, "FIRST")["row"].as_u64().unwrap();
            assert_eq!(row(render, "SECOND")["row"], first + 1);
            assert_eq!(row(render, "THIRD")["row"], first + 4);
            for id in ["a", "b"] {
                let anchor = render["anchors"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|v| v["id"] == id)
                    .unwrap();
                assert_eq!(anchor["row"], first);
                assert_eq!(anchor["line_text"], "FIRST");
            }
        }
    }

    #[test]
    fn signed_child_indent_composes_with_actual_parent_once() {
        let bundle = bundle(vec![Block::List {
            kind: ListKind::Plain,
            compact: true,
            items: vec![ListItem {
                blocks: vec![paragraph(vec![text("CHILD")], -3, 0)],
                entry: None,
                source: None,
                layout: Default::default(),
            }],
            layout: LayoutHint {
                indent_columns: 8,
                ..Default::default()
            },
            source: None,
        }]);
        let report = audit(&bundle, &[20, 40, 80, 120]);
        for render in report["renders"].as_array().unwrap() {
            assert_eq!(row(render, "CHILD")["text"], "     CHILD");
        }
        assert!(
            report["body"]["plain_text"]
                .as_str()
                .unwrap()
                .contains("\n     CHILD")
        );
    }

    #[test]
    fn real_buffer_preserves_graphemes_and_records_clipping() {
        let line = Line::from("界👩‍💻Z");
        let wide = buffer_row(&line, 5);
        assert_eq!(wide["cells"][0]["symbol"], "界");
        assert_eq!(wide["cells"][1]["symbol"], " ");
        assert_eq!(wide["cells"][2]["symbol"], "👩‍💻");
        assert_eq!(wide["cells"][4]["symbol"], "Z");
        let narrow = buffer_row(&line, 3);
        assert_eq!(narrow["cells"][2]["symbol"], " ");
        let bundle = bundle(vec![Block::Preformatted {
            children: vec![text("界👩‍💻Z")],
            language: None,
            layout: LayoutHint::default(),
            source: None,
        }]);
        let report = audit(&bundle, &[20]);
        let render = &report["renders"][0];
        let literal = row(render, "界");
        assert!(literal["text"].as_str().unwrap().starts_with("界👩‍💻Z"));
        assert_eq!(literal["buffer"]["cells"][2]["symbol"], "👩‍💻");
        assert_eq!(literal["buffer"]["cells"][4]["symbol"], "Z");
        assert_eq!(literal["over_width"], false);
    }

    #[test]
    fn evidence_limits_never_claim_complete_or_truncate_body() {
        let bundle = bundle(vec![paragraph(vec![text("UNTRUNCATED_BODY")], 0, 0)]);
        let report = audit_with_limits(&bundle, &[20, 40], 1, 0, 0);
        assert_eq!(report["complete"], false);
        assert_eq!(report["body"]["complete"], true);
        assert!(
            report["body"]["plain_text"]
                .as_str()
                .unwrap()
                .contains("UNTRUNCATED_BODY")
        );
        assert_eq!(
            report["body"]["plain_text"],
            report["body"]["decorated_plain_text"]
        );
        assert_eq!(report["body"]["decoration_runs_complete"], false);
        assert_eq!(report["renders"][0]["captured_rows"], 1);
        assert_eq!(report["renders"][0]["rows_complete"], false);
        assert_eq!(report["renders"][0]["cells_complete"], false);
        assert_eq!(report["renders"][1]["captured_rows"], 0);
    }

    #[test]
    fn native_and_markdown_loader_paths_feed_the_same_probe() {
        let native =
            mant_loader::load_roff_bytes(b".TH PROBE 1 2026-09-11\n.SH BODY\nFIRST\n.br\nSECOND\n")
                .unwrap();
        let markdown =
            mant_loader::load_markdown_text("# Probe\n\nFIRST  \nSECOND\n", None).unwrap();
        for bundle in [native, markdown] {
            let report = audit(&bundle, &[20]);
            let render = &report["renders"][0];
            assert_eq!(
                row(render, "SECOND")["row"].as_u64().unwrap(),
                row(render, "FIRST")["row"].as_u64().unwrap() + 1
            );
        }
    }

    #[test]
    fn argument_and_stream_limits_are_enforced() {
        for args in [
            vec![],
            vec!["--input", "-"],
            vec!["--input", "x", "--widths", "0"],
            vec!["--input", "x", "--widths", "20,20"],
            vec!["--input", "x", "--widths", "513"],
        ] {
            assert!(Args::parse(&args.into_iter().map(str::to_owned).collect::<Vec<_>>()).is_err());
        }
        assert!(bounded_read(&b"12345"[..], 4).is_err());
        assert_eq!(bounded_read(&b"1234"[..], 4).unwrap(), b"1234");
    }
}
