//! Report furniture is generated; every line of source stays in a quote frame.
use super::{metadata, spans};
use mant_protocol::{
    EvidenceClass, EvidenceCounts, ExplanationContent, ExplanationEvidence,
    ExplanationIdentityField, TextPresentation, TextRole,
};
use std::fmt::Write;

pub(super) struct Report<'a> {
    pub(super) markdown: bool,
    pub(super) decorate: &'a dyn Fn(TextPresentation, &str) -> String,
}

impl Report<'_> {
    pub(super) fn meta(&self, role: TextRole, value: &str) -> String {
        self.field(role, value, false)
    }
    fn field(&self, role: TextRole, value: &str, matched: bool) -> String {
        let value = mant_protocol::sanitize_terminal_text(value);
        if self.markdown {
            metadata::escape(&value)
        } else {
            (self.decorate)(
                TextPresentation {
                    matched,
                    ..role.into()
                },
                &value,
            )
        }
    }
    pub(super) fn counts(&self, output: &mut String, counts: &EvidenceCounts) {
        for class in EvidenceClass::ALL {
            let count = counts.get(class);
            write!(
                output,
                "\n{}",
                self.meta(
                    TextRole::EvidenceClass(class),
                    &format!(
                        "{}: total={}, returned={}",
                        class.title(),
                        count.total,
                        count.returned
                    )
                )
            )
            .expect("String writer");
        }
        if counts.direct_entry.total == 0 {
            write!(output,"\n{}",self.meta(TextRole::Notice,"No direct entry collected; any following records are original mentions or declared relationships, not proof that an option is absent.")).expect("String writer");
        } else if counts.direct_entry.returned == 0 {
            write!(
                output,
                "\n{}",
                self.meta(
                    TextRole::Notice,
                    "Direct entries exist but are not included on this page."
                )
            )
            .expect("String writer");
        }
    }
    pub(super) fn records<'a>(
        &self,
        output: &mut String,
        records: impl Iterator<Item = (&'a ExplanationEvidence, Option<&'a str>)>,
    ) {
        let mut previous = None;
        for (e, address) in records {
            if previous == Some(e.class) {
                write!(
                    output,
                    "\n\n{}",
                    if self.markdown {
                        "---".into()
                    } else {
                        self.meta(TextRole::Guide, "----------")
                    }
                )
                .expect("String writer");
            } else {
                write!(
                    output,
                    "\n\n{}",
                    if self.markdown {
                        format!("## {}", metadata::escape(e.class.title()))
                    } else {
                        self.meta(
                            TextRole::EvidenceClass(e.class),
                            &format!("========== {} ==========", e.class.title()),
                        )
                    }
                )
                .expect("String writer");
            }
            previous = Some(e.class);
            self.owner(output, e, address);
        }
    }
    fn owner(&self, output: &mut String, e: &ExplanationEvidence, address: Option<&str>) {
        let locations = spans::LocatedStyles::new(e);
        write!(
            output,
            "\n\n{}{} [{}]",
            if self.markdown { "### " } else { "" },
            self.field(
                TextRole::Path,
                e.outline.path(),
                metadata::identity_match(e, ExplanationIdentityField::Path)
            ),
            self.field(
                TextRole::Coordinate,
                e.outline.node.id(),
                metadata::identity_match(e, ExplanationIdentityField::Id)
            )
        )
        .expect("String writer");
        output.push('\n');
        for ancestor in &e.outline.ancestors {
            output.push_str(&self.meta(TextRole::Heading, &ancestor.title));
            output.push_str(&self.meta(TextRole::Guide, " > "));
        }
        let kind = metadata::kind(e);
        output.push_str(&self.meta(
            kind.map_or(TextRole::Heading, TextRole::EntryLabel),
            e.outline.title(),
        ));
        if let Some(path) = &e.block_path {
            self.line(
                output,
                TextRole::Coordinate,
                &format!("Source block: {path}"),
            );
        }
        if let Some(kind) = kind {
            self.line(
                output,
                TextRole::EntryLabel(kind),
                &format!("Kind: {}", mant_protocol::entry_kind_label(kind)),
            );
        }
        self.line(
            output,
            TextRole::Metadata,
            &format!("Matched by: {}", metadata::bases(e)),
        );
        self.details(output, e, &locations);
        self.body(output, e, &locations);
        output.push('\n');
        self.line(
            output,
            TextRole::Path,
            &format!(
                "Read original: {}; node {}",
                address.unwrap_or("current input"),
                e.outline.path()
            ),
        );
        if e.has_omitted_content() {
            self.line(output,TextRole::Notice,&format!("[content budget: bodyOmitted={}, detailsOmitted={}, previewsOmitted={}, matchDetailsOmitted={}, nameBindingsOmitted={}]",e.content_omitted,e.details_omitted,e.previews_omitted,e.match_details_omitted,e.name_bindings_omitted));
        }
    }
    fn details(
        &self,
        output: &mut String,
        evidence: &ExplanationEvidence,
        locations: &spans::LocatedStyles<'_>,
    ) {
        let Some(entry) = &evidence.entry else { return };
        if !entry.forms.is_empty() {
            self.line(output, TextRole::Metadata, "Forms:");
            for form in &entry.forms {
                let text = if self.markdown {
                    locations.markdown_inline(form, super::super::MarkdownOptions::default())
                } else {
                    locations.inline(form, TextRole::Body, &|p, t| {
                        (self.decorate)(p, &metadata::safe(t))
                    })
                };
                self.quote(output, &text);
            }
        }
        if !entry.alias_groups.is_empty() {
            self.line(
                output,
                TextRole::Metadata,
                &format!("Declared alias groups: {:?}", entry.alias_groups),
            );
        }
        if let Some(domain) = &entry.value_domain {
            self.line(
                output,
                TextRole::Metadata,
                &format!(
                    "Value domain (not followed): {}",
                    metadata::domain_label(domain)
                ),
            );
        }
    }
    fn line(&self, output: &mut String, role: TextRole, text: &str) {
        write!(output, "\n{}", self.meta(role, text)).expect("String writer");
    }
    fn quote(&self, output: &mut String, text: &str) {
        if self.markdown {
            output.push('\n');
        }
        for line in text.split('\n') {
            output.push('\n');
            if self.markdown {
                output.push_str("> ");
            } else {
                output.push_str(&self.meta(TextRole::Guide, "| "));
            }
            output.push_str(line);
        }
        if self.markdown {
            output.push('\n');
        }
    }
    fn body(
        &self,
        output: &mut String,
        e: &ExplanationEvidence,
        locations: &spans::LocatedStyles<'_>,
    ) {
        if matches!(
            e.class,
            EvidenceClass::DirectEntry | EvidenceClass::RelatedEntry
        ) {
            if let Some(ExplanationContent::Entry { block } | ExplanationContent::Block { block }) =
                &e.content
            {
                self.line(output, TextRole::Metadata, "Definition:");
                let body = if self.markdown {
                    super::super::markdown::blocks::render_located_blocks(
                        std::slice::from_ref(block),
                        super::super::MarkdownOptions::default(),
                        Some(locations),
                    )
                    .join("\n\n")
                } else {
                    super::super::text::render_located_blocks(
                        std::slice::from_ref(block),
                        locations,
                        &|p, t| (self.decorate)(p, &metadata::safe(t)),
                    )
                };
                self.quote(
                    output,
                    &if self.markdown {
                        metadata::safe(&body)
                    } else {
                        body
                    },
                );
            }
        } else {
            for preview in &e.previews {
                self.line(
                    output,
                    TextRole::Coordinate,
                    &format!(
                        "At {}; match chars={}..{}; clippedBefore={}, clippedAfter={}",
                        preview.block_path,
                        preview.match_start_char,
                        preview.match_end_char,
                        preview.clipped_before,
                        preview.clipped_after
                    ),
                );
                self.line(output, TextRole::Metadata, "Preview:");
                let body = spans::preview(
                    &preview.text,
                    preview.match_start_char,
                    preview.match_end_char,
                    &|style, text| {
                        let text = metadata::safe(text);
                        if self.markdown {
                            metadata::marked(&text, style.matched)
                        } else {
                            (self.decorate)(style, &text)
                        }
                    },
                );
                self.quote(output, &body);
            }
        }
    }
}
