//! Complete finite M4 domain; independently witnessed before codec admission.
use super::*;
use pulldown_cmark::{Event, Parser, Tag};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fmt::Write;

#[derive(Deserialize)]
struct Catalogue {
    case_count: usize,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    id: String,
    axes: Axes,
    source: String,
    source_sha256: String,
    expected_label: String,
    original_uri: String,
    original_title: String,
    recovered_uri: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Axes {
    form: String,
    entry: String,
}
impl Case {
    fn original_use(&self) -> String {
        match self.axes.form.as_str() {
            "full" => "[X][r\x1bf]",
            "collapsed" => "[r\x1bf][]",
            "shortcut" => "[r\x1bf]",
            other => panic!("unknown matrix form: {other}"),
        }
        .into()
    }
    fn witness_original_identity(&self) {
        // firstpass.rs::parse_refdef_total uses entry(...).or_insert(), and
        // RefDefs::get retains that exact effective LinkDef.span. Equal URI
        // and equal title are independently insufficient identity witnesses.
        // Admission compares both parsers in the same prepared source space;
        // diagnostics below independently retain original CR/CRLF bytes.
        let prepared = self.source.replace("\r\n", "\n").replace('\r', "\n");
        let original = Parser::new_ext(&prepared, super::super::super::markdown_options());
        let definitions = original.reference_definitions();
        let original_definition = definitions.get("r\x1bf").unwrap();
        assert_eq!(original_definition.dest.as_ref(), self.original_uri);
        assert_eq!(
            original_definition.title.as_deref(),
            Some(self.original_title.as_str())
        );
        let spelling = format!(
            "[r\x1bf]: {} \"{}\"",
            self.original_uri, self.original_title
        );
        let start = prepared.find(&spelling).unwrap();
        assert_eq!(original_definition.span, start..start + spelling.len());
        let recovered_source = prepared.replace('\x1b', " ");
        let recovered = Parser::new_ext(&recovered_source, super::super::super::markdown_options());
        let recovered_definition = recovered.reference_definitions().get("r f").unwrap();
        assert_eq!(recovered_definition.dest.as_ref(), self.recovered_uri);
        assert_ne!(original_definition.span, recovered_definition.span);
        let use_ = self.original_use();
        let mut resolved = None;
        let mut callback = |broken: pulldown_cmark::BrokenLink<'_>| {
            let definition = definitions.get(&broken.reference)?;
            resolved = Some(definition.span.clone());
            Some((
                definition.dest.clone(),
                definition.title.clone().unwrap_or_else(|| "".into()),
            ))
        };
        let links = Parser::new_with_broken_link_callback(
            &use_,
            super::super::super::markdown_options(),
            Some(&mut callback),
        )
        .into_offset_iter()
        .filter_map(|(event, range)| match event {
            Event::Start(Tag::Link { dest_url, .. }) => Some((dest_url.into_string(), range)),
            _ => None,
        })
        .collect::<Vec<_>>();
        // The locked parser's collapsed-link offset covers its label,
        // excluding the trailing empty reference brackets. Preserve that
        // actual event range in the independent diagnostic witness.
        let end = use_.len() - usize::from(self.axes.form == "collapsed") * 2;
        assert_eq!(links, [(self.original_uri.clone(), 0..end)]);
        assert_eq!(resolved, Some(original_definition.span.clone()));
    }
}

#[test]
fn all_108_reference_collision_cases_require_the_original_definition_identity() {
    let matrix: Catalogue = serde_json::from_str(include_str!("cases.json")).unwrap();
    assert_eq!(matrix.case_count, 108);
    assert_eq!(matrix.cases.len(), 108);
    for case in matrix.cases {
        let mut source_hash = String::with_capacity(64);
        for byte in &Sha256::digest(case.source.as_bytes()) {
            write!(source_hash, "{byte:02x}").unwrap();
        }
        assert_eq!(source_hash, case.source_sha256, "{}", case.id);
        case.witness_original_identity();
        let document = parse(&case.source);
        let actual = reading(&document);
        assert_eq!(actual.text, case.expected_label, "{}", case.id);
        assert!(actual.links.is_empty(), "{}", case.id);
        assert!(decoded_findings(&document).is_empty(), "{}", case.id);
        let refused = findings(&document, "markdown.recovered-link-target");
        assert_eq!(refused.len(), 1, "{}", case.id);
        assert_eq!(refused[0].impact, mant_ir::DiagnosticImpact::None);
        let use_ = case.original_use();
        let start = case.source.find(&use_).unwrap();
        assert_span(
            &case.source,
            refused[0].source.unwrap(),
            start..start + use_.len() - usize::from(case.axes.form == "collapsed") * 2,
        );
        if case.axes.entry == "canonical-break" {
            assert!(actual.text.starts_with('\n'));
        }
        assert_two_reads(&document, &case.expected_label, 0);
    }
}
