//! Local source/layout acceptance probe. No host-dependent inputs in CI.
//! Run with an explicit manual path and optional source tokens to locate.
use std::{error::Error, hint::black_box, path::Path, time::Instant};

use mant_ir::{DocumentIndex, ResolvedContent, SemanticIndex};
use mant_ui::DocumentView;

fn main() -> Result<(), Box<dyn Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(path) = args.first() else {
        return Err("usage: layout_profile MANUAL [TOKEN ...]".into());
    };
    let start = Instant::now();
    let document = if Path::new(path)
        .extension()
        .is_some_and(|extension| extension == "md")
    {
        mant_engine::parse_markdown(&std::fs::read_to_string(path)?, None)?.document
    } else {
        mant_engine::parse_manual_source(Path::new(path))?
    };
    println!("parse_ms\t{:.3}", start.elapsed().as_secs_f64() * 1000.0);
    let start = Instant::now();
    black_box(DocumentIndex::build(&document));
    black_box(SemanticIndex::build(&document));
    println!("indexes_ms\t{:.3}", start.elapsed().as_secs_f64() * 1000.0);
    let bundle = ResolvedContent {
        label: path.clone(),
        address: None,
        document: Some(document),
        tldr: None,
    };
    profile_discovery(&bundle)?;
    let start = Instant::now();
    let view = DocumentView::new(&bundle);
    println!(
        "logical_view_ms\t{:.3}",
        start.elapsed().as_secs_f64() * 1000.0
    );
    for width in [40, 80, 120] {
        let start = Instant::now();
        let rendered = view.render(width);
        println!(
            "resize\t{width}\t{}\t{:.3}",
            rendered.row_count,
            start.elapsed().as_secs_f64() * 1000.0
        );
        for token in &args[1..] {
            if let Some((row, column)) =
                rendered
                    .text
                    .lines
                    .iter()
                    .enumerate()
                    .find_map(|(row, line)| {
                        let text = line
                            .spans
                            .iter()
                            .map(|span| span.content.as_ref())
                            .collect::<String>();
                        text.find(token).map(|offset| {
                            (row, unicode_width::UnicodeWidthStr::width(&text[..offset]))
                        })
                    })
            {
                println!("token\t{width}\t{row}\t{column}\t{token}");
            } else {
                println!("wrapped_or_absent\t{width}\t{token}");
            }
        }
        black_box(rendered);
    }
    Ok(())
}

fn profile_discovery(bundle: &ResolvedContent) -> Result<(), Box<dyn Error>> {
    for mode in [
        mant_protocol::ReferenceProjectionMode::None,
        mant_protocol::ReferenceProjectionMode::Summary,
        mant_protocol::ReferenceProjectionMode::All,
    ] {
        let start = Instant::now();
        let outline = mant_engine::build_outline_with_references(
            bundle,
            mant_protocol::EntryProjection::Summary,
            None,
            &mant_protocol::ReferenceProjection {
                mode,
                ..mant_protocol::ReferenceProjection::default()
            },
        )?;
        println!(
            "outline\t{mode:?}\t{:.3}\t{:?}\t{:?}\tsteps={}\tbytes={}\tretained={}",
            start.elapsed().as_secs_f64() * 1000.0,
            outline.references.occurrences,
            outline.references.targets,
            outline.references.coverage.steps,
            outline.references.coverage.bytes,
            outline.references.records.len()
        );
        black_box(outline);
    }
    let start = Instant::now();
    let explanation = mant_engine::explain_query(
        bundle,
        &mant_protocol::ExplanationQuery {
            entry: "--help".into(),
            options: mant_protocol::ExplanationOptions::default(),
        },
    )?;
    println!(
        "explain_ms\t{:.3}\towners={}",
        start.elapsed().as_secs_f64() * 1000.0,
        explanation.total
    );
    black_box(explanation);
    Ok(())
}
