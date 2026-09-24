//! Report furniture is generated; source remains unframed in plain text and
//! uses standard blockquotes in Markdown. Neither surface is a wire protocol.
use super::{definition::DefinitionDisplay, metadata, spans};
use crate::presentation::{InlinePresentation, TextPresentation, TextRole};
use mant_protocol::{
    EvidenceClass, EvidenceCounts, ExplanationContent, ExplanationEvidence,
    ExplanationIdentityField,
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
        let value = crate::presentation::sanitize_terminal_text(value);
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
    #[allow(clippy::too_many_lines)]
    pub(super) fn records<'a>(
        &self,
        output: &mut String,
        records: impl Iterator<
            Item = (
                &'a ExplanationEvidence,
                Option<&'a str>,
                &'a [mant_protocol::ExplanationSupport],
                Option<mant_ir::ContentContext<'a>>,
            ),
        >,
    ) {
        let records = records.collect::<Vec<_>>();
        let mut previous = None;
        let mut displayed = std::collections::HashMap::new();
        for &(e, address, supports, content) in &records {
            self.record_separator(output, e.class, previous == Some(e.class));
            previous = Some(e.class);
            let retained =
                || content.expect("retained explanation support has a content projection");
            let reference = (!supports.is_empty() || e.support.is_some())
                .then(|| e.source_reference(retained(), supports))
                .flatten();
            let covered = reference.is_some();
            self.owner(output, e, address, covered, content);
            if let Some(support) = reference.and_then(|index| supports.get(index)) {
                let Some(block) = support.materialized(retained(), supports) else {
                    continue;
                };
                let block_key = std::ptr::from_ref(block) as usize;
                let grouped = e.covered_by_support(retained(), supports);
                let context_label = if grouped {
                    "Declaration-group context"
                } else {
                    "Original entry context"
                };
                if let std::collections::hash_map::Entry::Vacant(slot) = displayed.entry(block_key)
                {
                    slot.insert(reference.expect("validated support"));
                    self.line(
                        output,
                        TextRole::Metadata,
                        &format!(
                            "{context_label} [support {}]:{}",
                            reference.expect("resolved reference"),
                            if grouped {
                                " recovered from consecutive declarations"
                            } else {
                                ""
                            }
                        ),
                    );
                    let locations = spans::LocatedStyles::for_support(
                        content.expect("materialized explanation support has a content projection"),
                        block,
                        records
                            .iter()
                            .filter_map(|&(other, _, pool, other_content)| {
                                let other_content = other_content?;
                                let context = other
                                    .source_reference(other_content, pool)
                                    .and_then(|index| pool.get(index))?
                                    .materialized(other_content, pool)?;
                                std::ptr::eq(context, block).then_some((other, pool, other_content))
                            }),
                    );
                    let text = if self.markdown {
                        mant_codec::encode::render_located_blocks_fragment(
                            locations
                                .content()
                                .expect("retained explanation support has a content projection"),
                            std::slice::from_ref(block),
                            mant_codec::encode::MarkdownFragmentOptions::default(),
                            Some(&locations),
                        )
                        .join("\n\n")
                    } else {
                        super::super::text::render_located_blocks(
                            std::slice::from_ref(block),
                            &locations,
                            &|p, t| (self.decorate)(p, &metadata::safe(t)),
                        )
                    };
                    self.quote(output, &text);
                } else {
                    self.line(
                        output,
                        TextRole::Metadata,
                        &format!(
                            "{context_label}: see support {} displayed above.",
                            displayed[&block_key]
                        ),
                    );
                }
                if let Some(provider) = grouped.then(|| support.members().last()).flatten() {
                    self.line(
                        output,
                        TextRole::Path,
                        &format!(
                            "Source of description: {}; node {}",
                            provider.title(),
                            provider.path()
                        ),
                    );
                }
            }
            if e.support_omitted {
                self.line(
                    output,
                    TextRole::Notice,
                    "Declaration-group context exists but was omitted by the content budget.",
                );
            }
        }
    }
    fn record_separator(&self, output: &mut String, class: EvidenceClass, same_class: bool) {
        let separator = match (same_class, self.markdown) {
            (true, true) => "---".into(),
            (true, false) => self.meta(TextRole::Guide, "----------"),
            (false, true) => format!("## {}", metadata::escape(class.title())),
            (false, false) => self.meta(
                TextRole::EvidenceClass(class),
                &format!("========== {} ==========", class.title()),
            ),
        };
        write!(output, "\n\n{separator}").expect("String writer");
    }
    fn owner<'a>(
        &self,
        output: &mut String,
        e: &'a ExplanationEvidence,
        address: Option<&str>,
        covered: bool,
        content: Option<mant_ir::ContentContext<'a>>,
    ) {
        let locations = spans::LocatedStyles::new(content, e);
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
        self.details(output, e, &locations, covered);
        if !covered {
            self.body(output, e, &locations);
        }
        output.push('\n');
        self.line(
            output,
            TextRole::Path,
            &format!(
                "Original location: {}; node {}",
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
        covered: bool,
    ) {
        let Some(entry) = &evidence.entry else { return };
        if !covered
            && (!entry.forms.is_empty() || !entry.fixed_forms.is_empty())
            && !DefinitionDisplay::new(locations.content(), evidence)
                .is_some_and(|body| body.includes_forms())
        {
            self.line(output, TextRole::Metadata, "Forms:");
            for form in &entry.forms {
                let text = if self.markdown {
                    locations.markdown_inline(
                        form,
                        mant_codec::encode::MarkdownFragmentOptions::default(),
                    )
                } else {
                    locations.inline(form, TextRole::Body, &|p, t| {
                        (self.decorate)(p, &metadata::safe(t))
                    })
                };
                self.quote(output, &text);
            }
            for form in &entry.fixed_forms {
                if let Some(text) = form.complete_text() {
                    let text = metadata::safe(&text);
                    self.quote(
                        output,
                        &if self.markdown {
                            metadata::escape(&text)
                        } else {
                            (self.decorate)(TextRole::Body.into(), &text)
                        },
                    );
                }
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
            }
            output.push_str(line);
        }
        if self.markdown {
            output.push('\n');
        }
    }
    fn fixed_body(&self, output: &mut String, body: &mant_protocol::ExplanationFixedSelection) {
        self.line(output, TextRole::Metadata, "Definition (Fixed):");
        if body.validate().is_err() {
            self.line(
                output,
                TextRole::Notice,
                "Returned Fixed content is invalid.",
            );
            return;
        }
        if body.parts.is_empty() {
            self.line(
                output,
                TextRole::Notice,
                "Declaration located; no independent description was provided for this owner.",
            );
            return;
        }
        // TextJoin proves logical search continuity, not physical line layout.
        // The final native row/column and style are the only display authority.
        let mut row = body.parts[0].row.get();
        let mut column = 0;
        self.fixed_line_start(output);
        for part in &body.parts {
            while row < part.row.get() {
                row += 1;
                column = 0;
                self.fixed_line_start(output);
            }
            if part.column < column {
                self.line(
                    output,
                    TextRole::Notice,
                    "Returned Fixed geometry overlaps.",
                );
                return;
            }
            output.extend(std::iter::repeat_n(' ', (part.column - column) as usize));
            let text = metadata::safe(&part.text);
            if self.markdown {
                output.push_str(&text);
            } else {
                output.push_str(&(self.decorate)(
                    TextPresentation {
                        role: TextRole::Body,
                        inline: InlinePresentation {
                            strong: part.style.bold,
                            emphasis: part.style.underline,
                            ..InlinePresentation::default()
                        },
                        matched: false,
                    },
                    &text,
                ));
            }
            column = part.column + part.width;
        }
    }

    fn fixed_line_start(&self, output: &mut String) {
        output.push('\n');
        if self.markdown {
            // Indented code inside the report's block quote retains native
            // spaces, physical blank rows, and punctuation without reflow.
            output.push_str(">     ");
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
            if let Some(ExplanationContent::FixedOwner { reading_body, .. }) = &e.content {
                self.fixed_body(output, reading_body);
                return;
            }
            if let Some(display) = DefinitionDisplay::new(locations.content(), e) {
                let block = display.block;
                self.line(output, TextRole::Metadata, "Definition:");
                let body = if self.markdown {
                    mant_codec::encode::render_located_blocks_fragment(
                        locations
                            .content()
                            .expect("retained explanation body has a content projection"),
                        std::slice::from_ref(block),
                        mant_codec::encode::MarkdownFragmentOptions::default(),
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
                if display.empty_description() && e.support.is_none() && !e.support_omitted {
                    self.line(output, TextRole::Notice, "Declaration located; no independent description was provided for this owner.");
                } else if e.content_omitted {
                    self.line(
                        output,
                        TextRole::Notice,
                        "Definition content is incomplete or omitted in this response.",
                    );
                }
            } else {
                self.line(
                    output,
                    TextRole::Notice,
                    "Definition content was not returned in this response.",
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

#[cfg(test)]
mod tests {
    use super::Report;
    use mant_ir::{DisplayStyle, OutputSlice, TextJoin};
    use mant_protocol::{ExplanationFixedPart, ExplanationFixedSelection};
    use std::num::NonZeroU32;

    fn part(run: u32, text: &str) -> ExplanationFixedPart {
        ExplanationFixedPart {
            slice: OutputSlice {
                run: NonZeroU32::new(run).unwrap(),
                start_byte: 0,
                end_byte: text.len() as u64,
            },
            row: NonZeroU32::new(1).unwrap(),
            run_column: (run - 1) * 4,
            column: (run - 1) * 4,
            width: u32::try_from(text.len()).unwrap(),
            style: DisplayStyle {
                bold: false,
                underline: false,
            },
            text: text.to_owned(),
            source: None,
        }
    }

    #[test]
    fn fixed_body_uses_native_geometry_not_logical_join_for_display() {
        let body = ExplanationFixedSelection {
            parts: vec![part(1, "foo"), part(2, "bar"), part(3, "baz")],
            joins: vec![
                TextJoin::AuthoredSeparator(" ".to_owned()),
                TextJoin::Unknown,
            ],
        };
        let report = Report {
            markdown: false,
            decorate: &|_, text| text.to_owned(),
        };
        let mut output = String::new();
        report.fixed_body(&mut output, &body);
        assert!(output.contains("foo bar baz"), "{output}");
        assert!(!output.contains("Native join"), "{output}");
    }
}
