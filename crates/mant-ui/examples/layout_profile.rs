//! Local source/layout acceptance probe. No host-dependent inputs in CI.
//! Run with an explicit manual path and optional source tokens to locate.
use std::{error::Error, hint::black_box, path::Path, sync::Arc, time::Instant};

use mant_ir::{DocumentIndex, ResolvedContent, SemanticIndex};
use mant_ui::{App, DocumentView, ReaderOptions};

fn main() -> Result<(), Box<dyn Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let options = ProbeArgs::parse(&args)?;
    let path = options.path;
    let start = Instant::now();
    let document = load_document(path)?;
    println!("parse_ms\t{:.3}", start.elapsed().as_secs_f64() * 1000.0);
    let start = Instant::now();
    black_box(DocumentIndex::build(&document));
    black_box(SemanticIndex::build(&document));
    println!("indexes_ms\t{:.3}", start.elapsed().as_secs_f64() * 1000.0);
    let bundle = ResolvedContent {
        label: path.to_owned(),
        address: None,
        document: Some(document),
        tldr: None,
    };
    profile_discovery(&bundle)?;
    // Move the current document into the caller-owned scope instead of cloning
    // it to prepare the measurement. Additional sources are parsed before the
    // application timer and never change the primary document's query results.
    let start = Instant::now();
    let mut scope = vec![bundle];
    for path in options.scope_paths {
        scope.push(ResolvedContent {
            label: path.to_owned(),
            address: None,
            document: Some(load_document(path)?),
            tldr: None,
        });
    }
    println!(
        "scope_prepare_ms\t{:.3}\tscope_documents={}",
        start.elapsed().as_secs_f64() * 1000.0,
        scope.len()
    );
    if options.shared {
        // Transfer ownership before timing: this path never clones source IR.
        let scope = scope.into_iter().map(Arc::new).collect::<Vec<_>>();
        profile_shared_application(&scope);
        profile_view(&scope[0], &options.tokens);
    } else {
        profile_application(&scope[0], &scope);
        profile_view(&scope[0], &options.tokens);
    }
    Ok(())
}

fn profile_view(bundle: &ResolvedContent, tokens: &[&str]) {
    let start = Instant::now();
    let view = DocumentView::new(bundle);
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
        for &token in tokens {
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
}

struct ProbeArgs<'a> {
    path: &'a str,
    tokens: Vec<&'a str>,
    scope_paths: Vec<&'a str>,
    shared: bool,
}

impl<'a> ProbeArgs<'a> {
    fn parse(args: &'a [String]) -> Result<Self, &'static str> {
        let Some(path) = args.first() else {
            return Err("usage: layout_profile MANUAL [--shared] [--scope=PATH ...] [TOKEN ...]");
        };
        let mut options = Self {
            path,
            tokens: Vec::new(),
            scope_paths: Vec::new(),
            shared: false,
        };
        let mut literal_tokens = false;
        for argument in &args[1..] {
            if !literal_tokens && argument == "--" {
                literal_tokens = true;
            } else if !literal_tokens && argument == "--shared" {
                options.shared = true;
            } else if let Some(path) = argument
                .strip_prefix("--scope=")
                .filter(|_| !literal_tokens)
            {
                if path.is_empty() {
                    return Err("--scope requires a nonempty path");
                }
                options.scope_paths.push(path);
            } else {
                options.tokens.push(argument);
            }
        }
        Ok(options)
    }
}

fn load_document(path: &str) -> Result<mant_ir::Document, Box<dyn Error>> {
    if Path::new(path)
        .extension()
        .is_some_and(|extension| extension == "md")
    {
        Ok(mant_codec::parse_markdown(&std::fs::read_to_string(path)?, None)?.document)
    } else {
        Ok(mant_loader::parse_manual_source(Path::new(path))?)
    }
}

/// Keep caller-owned scope preparation outside the application timing. This
/// exposes the cost of the reader's owning convenience constructor separately
/// from parsing and from the derived document/viewport measurements below.
fn profile_application(bundle: &ResolvedContent, scope: &[ResolvedContent]) {
    let start = Instant::now();
    let app = App::with_catalog_and_scope(bundle, mant_protocol::DocumentCatalog::default(), scope);
    println!(
        "app_current_in_scope_ms\t{:.3}\tscope_documents={}",
        start.elapsed().as_secs_f64() * 1000.0,
        scope.len()
    );
    black_box(app);
}

/// Host-owned snapshots enter the reader by handle; view construction still
/// does all normal work. Keep this an alternate probe, not additional work in
/// the legacy default process/RSS measurement.
fn profile_shared_application(scope: &[Arc<ResolvedContent>]) {
    let start = Instant::now();
    let app = App::from_shared(ReaderOptions {
        current: Arc::clone(&scope[0]),
        scope: scope.to_vec(),
        catalog: mant_protocol::DocumentCatalog::default(),
    });
    println!(
        "app_shared_scope_ms\t{:.3}\tscope_documents={}",
        start.elapsed().as_secs_f64() * 1000.0,
        scope.len()
    );
    black_box(app);
}

fn profile_discovery(bundle: &ResolvedContent) -> Result<(), Box<dyn Error>> {
    for mode in [
        mant_protocol::ReferenceProjectionMode::None,
        mant_protocol::ReferenceProjectionMode::Summary,
        mant_protocol::ReferenceProjectionMode::All,
    ] {
        let start = Instant::now();
        let outline = mant_query::build_outline_with_references(
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
    let explanation = mant_query::explain_query(
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

#[cfg(test)]
mod tests {
    use super::ProbeArgs;

    #[test]
    fn scope_options_keep_primary_path_and_source_tokens_separate() {
        let args = [
            "primary.1.gz",
            "first token",
            "--scope=other.1.gz",
            "--scope=space path.md",
            "--",
            "--scope=literal token",
        ]
        .map(str::to_owned);
        let options = ProbeArgs::parse(&args).expect("probe arguments");
        assert_eq!(options.path, "primary.1.gz");
        assert_eq!(options.scope_paths, ["other.1.gz", "space path.md"]);
        assert_eq!(options.tokens, ["first token", "--scope=literal token"]);
    }

    #[test]
    fn missing_primary_or_empty_scope_path_is_rejected() {
        assert!(ProbeArgs::parse(&[]).is_err());
        assert!(ProbeArgs::parse(&["primary.md".into(), "--scope=".into()]).is_err());
    }

    #[test]
    fn shared_startup_is_explicit_and_double_dash_keeps_literal_tokens() {
        let args = ["primary.md", "--shared", "--", "--shared"].map(str::to_owned);
        let options = ProbeArgs::parse(&args).expect("shared probe");
        assert!(options.shared);
        assert_eq!(options.tokens, ["--shared"]);
        let ordinary = ProbeArgs::parse(&args[..1]).expect("ordinary probe");
        assert!(!ordinary.shared, "legacy measurement remains unchanged");
    }
}
