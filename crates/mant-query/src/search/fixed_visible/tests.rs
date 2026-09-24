use super::*;
use mant_ir::{
    DisplayLabel, DisplayRow, DisplayRun, DisplayStyle, DisplaySurface, Document, DocumentBody,
    DocumentMeta, RegionKind, RegionMark, SourceCoordinates, SourceFormat, SourceIdentity,
    SourceKey, SourceRecord, TldrDocument, TldrOrigin,
};
use mant_protocol::{SearchCase, SearchSyntax};

fn key(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

fn fixture() -> ResolvedContent {
    // IR-only join fixture: final visible bytes, not a roff input or a
    // claim that a particular formatter width creates these four rows.
    let surface = DisplaySurface {
        text: "alpha alpha".to_owned(),
        rows: (1..=4)
            .map(|number| DisplayRow {
                key: key(number),
                first_run: key(number),
                run_count: 1,
                column_count: [3, 2, 4, 2][(number - 1) as usize],
                break_after: number != 4,
            })
            .collect(),
        runs: ["alp", "ha", " alp", "ha"]
            .into_iter()
            .enumerate()
            .map(|(index, text)| DisplayRun {
                key: key(u32::try_from(index + 1).expect("small fixture")),
                row: key(u32::try_from(index + 1).expect("small fixture")),
                column: 0,
                width: u32::try_from(text.len()).expect("small fixture"),
                byte_start: [0, 3, 5, 9][index],
                byte_count: text.len() as u64,
                label: DisplayLabel {
                    owner: None,
                    link: None,
                    source: None,
                    style: DisplayStyle {
                        bold: false,
                        underline: false,
                    },
                    role: DisplayRole::Body,
                },
            })
            .collect(),
    };
    let region = RegionMark {
        key: key(1),
        parent: None,
        owner: None,
        section: None,
        kind: RegionKind::Literal,
        selection: TextSelection {
            parts: (1..=4)
                .map(|number| OutputSlice {
                    run: key(number),
                    start_byte: 0,
                    end_byte: [3, 2, 4, 2][(number - 1) as usize],
                })
                .collect(),
            joins: vec![
                TextJoin::DirectContact,
                TextJoin::HardBoundary,
                TextJoin::DirectContact,
            ],
        },
        empty_point: None,
        source: None,
    };
    let document = Document {
        parser: None,
        sources: vec![SourceRecord {
            key: SourceKey::FIRST,
            identity: SourceIdentity::Anonymous {
                name: "fixed-search-fixture".into(),
            },
            format: SourceFormat::Man,
            decoded_byte_length: 0,
            content_sha256: None,
            coordinates: SourceCoordinates::NativeNormalizedBytes,
        }],
        root_source: SourceKey::FIRST,
        body: DocumentBody::Fixed(FixedBody {
            surface,
            headings: Vec::new(),
            owners: Vec::new(),
            links: Vec::new(),
            anchors: Vec::new(),
            regions: vec![region],
        }),
        meta: DocumentMeta::default(),
        fragment_aliases: Vec::new(),
        diagnostics: Vec::new(),
    };
    ResolvedContent {
        label: "Fixture".into(),
        address: None,
        document: Some(document),
        tldr: None,
    }
}

fn request(pattern: &str, scope: SearchScope, offset: u32, limit: u32) -> SearchQuery {
    SearchQuery {
        pattern: pattern.into(),
        syntax: SearchSyntax::Literal,
        case: SearchCase::Sensitive,
        scope,
        word: false,
        context_lines: 0,
        offset,
        limit,
    }
}

#[test]
fn soft_join_has_one_occurrence_and_two_display_slices() {
    let query = fixture();
    let result =
        super::super::search_query(&query, &request("alpha", SearchScope::Visible, 0, 1)).unwrap();
    assert_eq!(result.total, 2);
    assert_eq!(result.returned, 1);
    assert_eq!(result.next_offset, Some(1));
    assert_eq!(result.matches[0].matched_text, "alpha");
    assert_eq!(result.matches[0].display_slices.len(), 2);
    assert!(matches!(
        result.matches[0].location,
        SearchLocation::VisibleFixed { .. }
    ));
    let projection = result.content_projection.as_ref().unwrap();
    projection.validate_match(&result.matches[0]).unwrap();
    assert_eq!(projection.fragments[0].text, "alp");
    assert_eq!(projection.fragments[1].text, "ha");
}

#[test]
fn fixed_occurrence_paging_and_hidden_bytes_are_exact() {
    let query = fixture();
    let page =
        super::super::search_query(&query, &request("alpha", SearchScope::Visible, 1, 1)).unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(page.matches[0].ordinal, 2);
    assert_eq!(page.matches[0].display_slices.len(), 2);
    let absent =
        super::super::search_query(&query, &request("covered-name", SearchScope::Visible, 0, 1))
            .unwrap();
    assert_eq!(absent.total, 0);
    assert!(absent.content_projection.is_none());
}

#[test]
fn markdown_fence_is_artifact_only() {
    let query = fixture();
    let result =
        super::super::search_query(&query, &request("```", SearchScope::Markdown, 0, 10)).unwrap();
    assert_eq!(result.total, 2);
    assert!(result.content_projection.is_none());
    assert!(result.matches.iter().all(|matched| matches!(
        matched.location,
        SearchLocation::MarkdownArtifact { .. }
    ) && matched.display_slices.is_empty()));
}

#[test]
fn mixed_tldr_and_fixed_have_one_global_occurrence_cursor() {
    // Pinned CVS `man_term.c::print_man_node` confirms the paired minimal
    // `.TH MIX 1 / .SH DESCRIPTION / alpha` body emits alpha once. The
    // TLDR is separate query data, not a roff-created body row.
    let mut query = fixture();
    query.tldr = Some(TldrDocument {
        title: "Fixture".into(),
        description: vec!["alpha".into()],
        more_information: None,
        examples: Vec::new(),
        platform: "common".into(),
        language: "en".into(),
        source_path: "/tldr/fixture.md".into(),
        origin: TldrOrigin::TldrPages,
    });
    let result =
        super::super::search_query(&query, &request("alpha", SearchScope::Visible, 0, 2)).unwrap();
    assert_eq!(result.total, 3);
    assert_eq!(result.returned, 2);
    assert_eq!(result.next_offset, Some(2));
    assert!(matches!(
        result.matches[0].location,
        SearchLocation::VisibleFlow { .. }
    ));
    assert!(matches!(
        result.matches[1].location,
        SearchLocation::VisibleFixed { .. }
    ));
    let projection = result.content_projection.as_ref().unwrap();
    projection.validate_match(&result.matches[0]).unwrap();
    projection.validate_match(&result.matches[1]).unwrap();
    assert_eq!(result.matches[0].ordinal, 1);
    assert_eq!(result.matches[1].ordinal, 2);
    let page =
        super::super::search_query(&query, &request("alpha", SearchScope::Visible, 2, 1)).unwrap();
    assert_eq!(page.total, 3);
    assert_eq!(page.matches[0].ordinal, 3);
    assert!(matches!(
        page.matches[0].location,
        SearchLocation::VisibleFixed { .. }
    ));
    let blocked =
        super::super::search_query(&query, &request("alphaalpha", SearchScope::Visible, 0, 1))
            .unwrap();
    assert_eq!(blocked.total, 0);

    let artifact =
        super::super::search_query(&query, &request("alpha", SearchScope::Markdown, 0, 1)).unwrap();
    assert!(matches!(
        artifact.matches[0].outline.node,
        OutlineNodeReference::Tldr { .. }
    ));
}
