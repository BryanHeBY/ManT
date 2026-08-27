#![no_main]

//! M2 parser boundary: arbitrary byte input must produce only bounded,
//! source-addressable nodes and diagnostics.

use libfuzzer_sys::fuzz_target;
use mantdoc::{Parser, Source, SourceName};

const MAX_INPUT_BYTES: usize = 128 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }
    let name = SourceName::new("fuzz.roff").expect("literal source name is valid");
    let report = Parser::default()
        .parse(Source::new(&name, data))
        .expect("fuzz input stays inside the parser's root-byte limit");

    assert!(report.document.node_count() <= report.statistics.emitted_nodes);
    assert!(report.statistics.emitted_nodes <= Parser::default().config().limits.max_nodes);
    for node in report.document.preorder() {
        if let Some(span) = node.location() {
            assert!(span.start <= span.end);
            assert!(
                usize::try_from(span.end).expect("u32 always fits usize") <= data.len(),
                "node {:?} at {:?} exceeds {} input bytes",
                node.macro_name(),
                span,
                data.len()
            );
            assert!(report.document.source_position(span).is_some());
        }
        // Drive all iterative public traversal paths while parser data is live.
        let _ = node.children().count();
        let _ = node.ancestors().count();
        let _ = node.macro_name();
        let _ = node.text();
    }
    for finding in &report.diagnostics {
        for span in finding
            .primary
            .iter()
            .chain(finding.related.iter().map(|related| &related.span))
        {
            assert!(span.start <= span.end);
            assert!(
                usize::try_from(span.end).expect("u32 always fits usize") <= data.len(),
                "diagnostic {} at {:?} exceeds {} input bytes: {}",
                finding.code,
                span,
                data.len(),
                finding.message
            );
            assert!(report.document.source_position(span).is_some());
        }
    }
});
