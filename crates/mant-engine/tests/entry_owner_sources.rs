//! Structural conversions preserve the original head, not the surviving body.
use mant_ir::{
    DefinitionItem, Document, SourceSpan,
    visit::{self, Visit},
};
use mant_protocol::{EvidenceBasis, EvidenceClass, ExplanationOptions, ExplanationQuery};

fn owner_sources(document: &Document) -> Vec<Option<SourceSpan>> {
    struct Owners(Vec<Option<SourceSpan>>);
    impl<'a> Visit<'a> for Owners {
        fn visit_definition_item(&mut self, item: &'a DefinitionItem) {
            if item.entry.is_some() {
                self.0.push(item.source);
            }
            visit::walk_definition_item(self, item);
        }
    }
    let mut owners = Owners(Vec::new());
    owners.visit_document(document);
    owners.0
}

#[test]
fn transformed_native_owners_keep_the_first_head_for_explain_and_search() {
    for (source, names, line) in [
        (
            ".TH PROBE 1\n.SH ENVIRONMENT\n.PP\n.B FOO\n.RS\nPAYLOAD\n.RE\n",
            vec!["FOO"],
            4,
        ),
        (
            ".TH PROBE 1\n.SH OPTIONS\n.TP\n.B -a\n.TQ\n.B --all\nPAYLOAD\n",
            vec!["-a", "--all"],
            3,
        ),
        (
            ".TH PROBE 1\n.SH OPTIONS\n.TP\n.B -a\n.TQ\n.B --all\n.TQ\n.B --everything\nPAYLOAD\n",
            vec!["-a", "--all", "--everything"],
            3,
        ),
        (
            ".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It Fl a\n.It Fl all\nPAYLOAD\n.El\n",
            vec!["-a", "-all"],
            6,
        ),
        (
            ".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It Fl a\n.It Fl all\n.It Fl everything\nPAYLOAD\n.El\n",
            vec!["-a", "-all", "-everything"],
            6,
        ),
        (
            ".TH PROBE 1\n.SH OPTIONS\n.TP\n.B -a\nPAYLOAD\n",
            vec!["-a"],
            3,
        ),
        (
            ".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It Fl a\nPAYLOAD\n.El\n",
            vec!["-a"],
            6,
        ),
    ] {
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        let owners = owner_sources(query.document.as_ref().unwrap());
        assert_eq!(owners.len(), 1, "{source}");
        let original = owners[0].unwrap();
        assert_eq!(original.line, line, "{source}");
        assert!(original.byte_range.is_none());
        for name in names {
            let result = mant_engine::explain_query(
                &query,
                &ExplanationQuery {
                    entry: name.into(),
                    options: ExplanationOptions::default(),
                },
            )
            .unwrap();
            let direct = result
                .evidence
                .iter()
                .find(|e| {
                    e.class == EvidenceClass::DirectEntry && e.bases.contains(&EvidenceBasis::Name)
                })
                .unwrap();
            assert_eq!(direct.source, Some(original), "{name}");
        }
        let search = mant_engine::search_query(
            &query,
            &serde_json::from_value(serde_json::json!({"pattern":"PAYLOAD"})).unwrap(),
        )
        .unwrap();
        assert_eq!(search.matches.len(), 1);
        assert_eq!(search.matches[0].node_source, Some(original));
        assert!(mant_ir::validate_document(query.document.as_ref().unwrap()).is_empty());
    }
}
