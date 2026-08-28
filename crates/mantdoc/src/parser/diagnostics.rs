use super::{
    Diagnostic, DiagnosticCode, DiagnosticProfile, DocumentBuilder,
    LEGACY_SYNTAX_TREE_DEPTH_MESSAGE, Limits, ParseState, Severity, push_diagnostic,
};

/// Project findings after all parser stages have established their source
/// order. This is deliberately presentation-only: the parser's AST and
/// execution outcome remain identical for every [`DiagnosticProfile`].
pub(super) fn apply_diagnostic_profile(
    diagnostics: &mut Vec<Diagnostic>,
    profile: DiagnosticProfile,
) {
    if profile != DiagnosticProfile::LibmandocRsV0_9 {
        return;
    }
    diagnostics
        .retain(|diagnostic| diagnostic.code.as_str() != DiagnosticCode::MDOC_MDOCDATE_MISSING);
    for diagnostic in diagnostics {
        diagnostic.message = diagnostic
            .message
            .trim_end_matches([' ', '\t'])
            .to_owned()
            .into();
    }
}

pub(super) fn apply_tree_depth_limit(
    outcome: &mut ParseState,
    builder: &mut DocumentBuilder,
    limits: &Limits,
) {
    if !builder.truncate_descendants_at_depth(limits.max_tree_depth) {
        return;
    }

    outcome.truncated = true;
    let (code, message) = if limits.max_tree_depth == 256 {
        (
            DiagnosticCode::LEGACY_SYNTAX_TREE_DEPTH_LIMIT,
            LEGACY_SYNTAX_TREE_DEPTH_MESSAGE,
        )
    } else {
        (
            DiagnosticCode::LIMIT_TREE_DEPTH,
            "syntax-tree assembly exceeds max_tree_depth and retained a finite AST prefix",
        )
    };
    push_diagnostic(
        &mut outcome.diagnostics,
        limits,
        Diagnostic::new(
            DiagnosticCode::new(code).expect("static syntax-tree diagnostic code is valid"),
            Severity::Warning,
            message,
        ),
        &mut outcome.truncated,
    );
}

pub(super) fn apply_man_structure_outcome(
    outcome: &mut ParseState,
    structure: crate::man::StructureOutcome,
    limits: &Limits,
) {
    if let Some(location) = structure.node_limit_location {
        outcome.truncated = true;
        push_diagnostic(
            &mut outcome.diagnostics,
            limits,
            Diagnostic::new(
                DiagnosticCode::new(DiagnosticCode::LIMIT_NODES)
                    .expect("static diagnostic code is valid"),
                Severity::Warning,
                "man semantic restructuring exceeds max_nodes and retained the raw event",
            )
            .with_primary(location),
            &mut outcome.truncated,
        );
    }
    for recovery in structure.recoveries {
        push_diagnostic(
            &mut outcome.diagnostics,
            limits,
            diagnostic_from_man_recovery(recovery),
            &mut outcome.truncated,
        );
    }
}

#[allow(clippy::too_many_lines)] // One exhaustive recovery-to-diagnostic mapping keeps compatibility prose auditable.
fn diagnostic_from_man_recovery(recovery: crate::man::Recovery) -> Diagnostic {
    let (code, message, location, severity) = match recovery {
        crate::man::Recovery::TitleNotUppercase { title, location } => (
            DiagnosticCode::MAN_TITLE_NOT_UPPERCASE,
            format!("lower case character in document title: TH {title}"),
            location,
            Severity::Style,
        ),
        crate::man::Recovery::TitleDateUnparseable { date, location } => (
            DiagnosticCode::MAN_TITLE_DATE_UNPARSEABLE,
            format!("cannot parse date, using it verbatim: TH {date}"),
            location,
            Severity::Warning,
        ),
        crate::man::Recovery::TitleDateMissing { location } => (
            DiagnosticCode::MAN_TITLE_DATE_MISSING,
            "missing date, using \"\": TH".into(),
            location,
            Severity::Warning,
        ),
        crate::man::Recovery::TitleArgumentMissing { location } => (
            DiagnosticCode::MAN_TITLE_MISSING,
            "missing manual title, using \"\": TH".into(),
            location,
            Severity::Warning,
        ),
        crate::man::Recovery::TitleSectionMissing { title, location } => (
            DiagnosticCode::MAN_TITLE_SECTION_MISSING,
            title.map_or_else(
                || "missing manual section, using \"\": TH ".into(),
                |title| format!("missing manual section, using \"\": TH {title}"),
            ),
            location,
            Severity::Warning,
        ),
        crate::man::Recovery::MissingManualTitle => (
            DiagnosticCode::MAN_TITLE_MISSING,
            "missing manual title, using \"\"".into(),
            None,
            Severity::Warning,
        ),
        crate::man::Recovery::MissingManualDate => (
            DiagnosticCode::MAN_TITLE_DATE_MISSING,
            "missing date, using \"\"".into(),
            None,
            Severity::Warning,
        ),
        crate::man::Recovery::NoDocumentBody => (
            DiagnosticCode::MAN_NO_DOCUMENT_BODY,
            "no document body".into(),
            None,
            Severity::Warning,
        ),
        crate::man::Recovery::UnmatchedClose {
            macro_name,
            location,
        } => (
            DiagnosticCode::MAN_UNMATCHED_CLOSE,
            format!("skipping end of block that is not open: {macro_name}"),
            location,
            Severity::Error,
        ),
        crate::man::Recovery::UnclosedBlock {
            macro_name,
            location,
        } => (
            DiagnosticCode::MAN_UNCLOSED_BLOCK,
            format!("appending missing end of block: {macro_name}"),
            location,
            Severity::Error,
        ),
        crate::man::Recovery::LineScopeBroken {
            macro_name,
            location,
        } => (
            DiagnosticCode::MAN_LINE_SCOPE_BROKEN,
            format!("line scope broken: EOF breaks {macro_name}"),
            location,
            Severity::Warning,
        ),
        crate::man::Recovery::LineScopeInterrupted {
            scope,
            breaker,
            location,
        } => (
            DiagnosticCode::MAN_LINE_SCOPE_BROKEN,
            format!("line scope broken: {breaker} breaks {scope}"),
            location,
            Severity::Warning,
        ),
        crate::man::Recovery::BlankLineInScope { location } => (
            DiagnosticCode::MAN_BLANK_LINE_SCOPE,
            "skipping blank line in line scope".to_owned(),
            location,
            Severity::Warning,
        ),
        crate::man::Recovery::EmptyParagraph {
            macro_name,
            location,
        } => (
            DiagnosticCode::MAN_EMPTY_PARAGRAPH,
            format!("skipping paragraph macro: {macro_name} empty"),
            location,
            Severity::Warning,
        ),
        crate::man::Recovery::ParagraphAfterSection {
            macro_name,
            section_name,
            location,
        } => (
            DiagnosticCode::MAN_EMPTY_PARAGRAPH,
            format!("skipping paragraph macro: {macro_name} after {section_name}"),
            location,
            Severity::Warning,
        ),
        crate::man::Recovery::ParagraphSkip {
            macro_name,
            relation,
            context,
            location,
        } => (
            DiagnosticCode::MAN_EMPTY_PARAGRAPH,
            format!("skipping paragraph macro: {macro_name} {relation} {context}"),
            location,
            Severity::Warning,
        ),
        crate::man::Recovery::ParagraphAtSectionEnd {
            macro_name,
            section_name,
            location,
        } => (
            DiagnosticCode::MAN_EMPTY_PARAGRAPH,
            format!("skipping paragraph macro: {macro_name} at the end of {section_name}"),
            location,
            Severity::Warning,
        ),
        crate::man::Recovery::ExcessArguments {
            macro_name,
            argument,
            location,
        } => (
            DiagnosticCode::MAN_EXCESS_ARGUMENTS,
            format!("skipping excess arguments: {macro_name} ... {argument}"),
            location,
            Severity::Error,
        ),
        crate::man::Recovery::MissingResource {
            macro_name,
            location,
        } => (
            DiagnosticCode::MAN_MISSING_RESOURCE,
            format!("missing resource identifier, using \"\": {macro_name}"),
            location,
            Severity::Warning,
        ),
        crate::man::Recovery::MissingOption {
            macro_name,
            location,
        } => (
            DiagnosticCode::MAN_MISSING_OPTION,
            format!("missing option string, using \"\": {macro_name}"),
            location,
            Severity::Warning,
        ),
        crate::man::Recovery::AllArguments {
            macro_name,
            first_argument,
            has_more,
            location,
        } => (
            DiagnosticCode::MAN_ALL_ARGUMENTS,
            format!(
                "skipping all arguments: {macro_name} {first_argument}{}",
                if has_more { " ..." } else { "" }
            ),
            location,
            Severity::Error,
        ),
        crate::man::Recovery::IgnoredArguments {
            macro_name,
            arguments,
            location,
        } => (
            DiagnosticCode::MAN_ALL_ARGUMENTS,
            format!("skipping all arguments: {macro_name} {arguments}"),
            location,
            Severity::Error,
        ),
        crate::man::Recovery::FewerIndents { target, location } => (
            DiagnosticCode::MAN_FEWER_INDENTS,
            format!("fewer RS blocks open, skipping: RE {target}"),
            location,
            Severity::Error,
        ),
        crate::man::Recovery::RedundantFillMode { message, location } => (
            DiagnosticCode::MAN_REDUNDANT_FILL_MODE,
            message.to_owned(),
            location,
            Severity::Style,
        ),
        crate::man::Recovery::EmptyBlock {
            macro_name,
            location,
        } => (
            DiagnosticCode::MAN_EMPTY_BLOCK,
            format!("empty block: {macro_name}"),
            location,
            Severity::Warning,
        ),
    };
    let diagnostic = Diagnostic::new(
        DiagnosticCode::new(code).expect("static diagnostic code is valid"),
        severity,
        message,
    );
    match location {
        Some(location) => diagnostic.with_primary(location),
        None => diagnostic,
    }
}

/// Some scanner-detected conditions are validated by mandoc after the normal
/// request stream. Keep their scanner budget reservation, then reproduce that
/// observable diagnostic phase just before publishing the report.
pub(super) fn reorder_deferred_post_validation_diagnostics(outcome: &mut ParseState) {
    publish_deferred_diagnostics(
        &mut outcome.diagnostics,
        &mut outcome.deferred_post_validation_diagnostics,
    );
    publish_deferred_diagnostics(
        &mut outcome.diagnostics,
        &mut outcome.deferred_filled_text_tab_diagnostics,
    );
}

/// Publish scanner-recorded ordinary-text tab findings before a later visible
/// man-macro argument finding.  This is a distinct phase boundary from the
/// generic post-validation queue: it preserves source ordering between the
/// two kinds of tab diagnostics without moving either ahead of scanner styles.
pub(super) fn publish_deferred_filled_text_tabs(
    diagnostics: &mut Vec<Diagnostic>,
    deferred: &mut Vec<Diagnostic>,
) {
    publish_deferred_diagnostics(diagnostics, deferred);
}

fn publish_deferred_diagnostics(diagnostics: &mut Vec<Diagnostic>, deferred: &mut Vec<Diagnostic>) {
    for diagnostic in deferred.drain(..) {
        if let Some(index) = diagnostics
            .iter()
            .position(|candidate| candidate == &diagnostic)
        {
            diagnostics.remove(index);
            diagnostics.push(diagnostic);
        }
    }
}

pub(super) fn apply_preprocess_outcome(
    outcome: &mut ParseState,
    structure: crate::preprocess::PreprocessOutcome,
    limits: &Limits,
) {
    if let Some(limit) = structure.limit {
        outcome.truncated = true;
        let diagnostic = Diagnostic::new(
            DiagnosticCode::new(limit.code).expect("static preprocessing diagnostic code is valid"),
            Severity::Warning,
            limit.message,
        );
        // The old wrapper synthesized its equation-depth finding only after
        // copying the completed document, so it had no source location. Keep
        // that observable shape for the default 256-level compatibility cap;
        // caller-selected native equation limits retain their useful opener
        // location.
        let location = (limit.code != DiagnosticCode::LEGACY_EQUATION_TREE_DEPTH_LIMIT)
            .then_some(limit.location)
            .flatten();
        let diagnostic = match location {
            Some(location) => diagnostic.with_primary(location),
            None => diagnostic,
        };
        push_diagnostic(
            &mut outcome.diagnostics,
            limits,
            diagnostic,
            &mut outcome.truncated,
        );
    }
    for recovery in structure.recoveries {
        let diagnostic = Diagnostic::new(
            DiagnosticCode::new(recovery.code)
                .expect("static preprocessing recovery diagnostic code is valid"),
            Severity::Error,
            recovery.message,
        );
        let diagnostic = match recovery.location {
            Some(location) => diagnostic.with_primary(location),
            None => diagnostic,
        };
        push_diagnostic(
            &mut outcome.diagnostics,
            limits,
            diagnostic,
            &mut outcome.truncated,
        );
    }
    for recovery in structure.dynamic_recoveries {
        let diagnostic = Diagnostic::new(
            DiagnosticCode::new(recovery.code)
                .expect("static preprocessing recovery diagnostic code is valid"),
            recovery.severity,
            recovery.message,
        );
        let diagnostic = match recovery.location {
            Some(location) => diagnostic.with_primary(location),
            None => diagnostic,
        };
        if recovery.code == DiagnosticCode::TBL_MACRO {
            // tbl macros are diagnosed by `roff_parseln()` while the input
            // stream is being scanned.  Table normalization necessarily runs
            // later, so splice these recovered findings back into that phase
            // instead of appending them behind unrelated later-line styles.
            // This is observable for real manuals whose table is followed by
            // an alternating-font macro with an input comment.
            let insertion = diagnostic.primary.as_ref().and_then(|primary| {
                outcome.diagnostics.iter().position(|candidate| {
                    candidate.primary.as_ref().is_some_and(|location| {
                        location.source == primary.source && location.start > primary.start
                    })
                })
            });
            if outcome.diagnostics.len() >= limits.max_diagnostics {
                outcome.truncated = true;
            } else if let Some(index) = insertion {
                outcome.diagnostics.insert(index, diagnostic);
            } else {
                outcome.diagnostics.push(diagnostic);
            }
        } else {
            push_diagnostic(
                &mut outcome.diagnostics,
                limits,
                diagnostic,
                &mut outcome.truncated,
            );
        }
    }
}

#[allow(clippy::too_many_lines)] // One recovery-to-diagnostic mapping preserves upstream ordering.
pub(super) fn apply_mdoc_structure_outcome(
    outcome: &mut ParseState,
    structure: crate::mdoc::StructureOutcome,
    limits: &Limits,
) {
    if let Some(location) = structure.node_limit_location {
        outcome.truncated = true;
        push_diagnostic(
            &mut outcome.diagnostics,
            limits,
            Diagnostic::new(
                DiagnosticCode::new(DiagnosticCode::LIMIT_NODES)
                    .expect("static diagnostic code is valid"),
                Severity::Warning,
                "mdoc semantic restructuring exceeds max_nodes and retained the raw event",
            )
            .with_primary(location),
            &mut outcome.truncated,
        );
    }
    // libmandoc validates the accepted `.Dt` title after it has handled the
    // document's ordinary macro diagnostics.  Keep its style finding behind
    // those source-order recoveries rather than exposing structure-pass order
    // as a public report difference.
    let mut recoveries = Vec::with_capacity(structure.recoveries.len());
    let mut deferred_title_case_recoveries = Vec::new();
    for recovery in structure.recoveries {
        if matches!(&recovery, crate::mdoc::Recovery::TitleNotUppercase { .. }) {
            deferred_title_case_recoveries.push(recovery);
        } else {
            recoveries.push(recovery);
        }
    }
    recoveries.extend(deferred_title_case_recoveries);
    for recovery in recoveries {
        let empty_no_macro = matches!(
            &recovery,
            crate::mdoc::Recovery::EmptyMacro {
                macro_name: "No",
                ..
            }
        );
        let (code, severity, message, location) = match recovery {
            crate::mdoc::Recovery::UnmatchedClose {
                macro_name,
                location,
            } => (
                DiagnosticCode::MDOC_UNMATCHED_CLOSE,
                Severity::Error,
                format!("skipping end of block that is not open: {macro_name}"),
                location,
            ),
            crate::mdoc::Recovery::UnclosedBlock {
                macro_name,
                location,
            } => (
                DiagnosticCode::MDOC_UNCLOSED_BLOCK,
                Severity::Error,
                format!("appending missing end of block: {macro_name}"),
                location,
            ),
            crate::mdoc::Recovery::EmptyBlock {
                macro_name,
                location,
            } => (
                DiagnosticCode::MDOC_EMPTY_BLOCK,
                Severity::Warning,
                format!("empty block: {macro_name}"),
                location,
            ),
            crate::mdoc::Recovery::EmptyListItem {
                list_type,
                location,
            } => (
                DiagnosticCode::MDOC_EMPTY_LIST_ITEM,
                Severity::Warning,
                format!("empty list item: Bl -{list_type} It"),
                location,
            ),
            crate::mdoc::Recovery::EmptyListItemHead {
                list_type,
                location,
            } => (
                DiagnosticCode::MDOC_EMPTY_LIST_ITEM,
                Severity::Warning,
                format!("empty head in list item: Bl -{list_type} It"),
                location,
            ),
            crate::mdoc::Recovery::ContentOutsideList { content, location } => (
                DiagnosticCode::MDOC_CONTENT_OUTSIDE_LIST,
                Severity::Warning,
                format!("moving content out of list: {content}"),
                location,
            ),
            crate::mdoc::Recovery::EmptyMacro {
                macro_name,
                location,
            } => (
                DiagnosticCode::MDOC_EMPTY_MACRO,
                Severity::Warning,
                format!("skipping empty macro: {macro_name}"),
                location,
            ),
            crate::mdoc::Recovery::MissingReferenceSection { name, location } => (
                DiagnosticCode::MDOC_REFERENCE_SECTION_MISSING,
                Severity::Warning,
                format!("missing section argument: Xr {name}"),
                location,
            ),
            crate::mdoc::Recovery::UnusualReferenceOrder {
                name,
                section,
                previous_name,
                previous_section,
                location,
            } => (
                DiagnosticCode::MDOC_REFERENCE_ORDER,
                Severity::Warning,
                if section == previous_section {
                    format!("unusual Xr order: {name} after {previous_name}")
                } else {
                    format!(
                        "unusual Xr order: {name}({section}) after {previous_name}({previous_section})"
                    )
                },
                location,
            ),
            crate::mdoc::Recovery::InvalidTag { tag, location } => (
                DiagnosticCode::MDOC_INVALID_TAG,
                Severity::Error,
                format!("skipping tag containing whitespace: Tg {tag}"),
                location,
            ),
            crate::mdoc::Recovery::NonCallableMacro {
                macro_name,
                location,
            } => (
                DiagnosticCode::MDOC_NON_CALLABLE_MACRO,
                Severity::Warning,
                format!("macro neither callable nor escaped: {macro_name}"),
                location,
            ),
            crate::mdoc::Recovery::NoSpaceMacro { location } => (
                DiagnosticCode::MDOC_NO_SPACE_MACRO,
                Severity::Warning,
                "skipping no-space macro".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::InvalidBooleanArgument {
                macro_name,
                argument,
                location,
            } => (
                DiagnosticCode::MDOC_BOOLEAN_ARGUMENT,
                Severity::Warning,
                format!("invalid Boolean argument: {macro_name} {argument}"),
                location,
            ),
            crate::mdoc::Recovery::ReferenceContent { content, location } => (
                DiagnosticCode::MDOC_REFERENCE_CONTENT,
                Severity::Warning,
                format!("invalid content in Rs block: {content}"),
                location,
            ),
            crate::mdoc::Recovery::EmptyReferenceBlock { location } => (
                DiagnosticCode::MDOC_EMPTY_REFERENCE_BLOCK,
                Severity::Warning,
                "empty reference block: Rs".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::UnknownRoffFont { font, location } => (
                DiagnosticCode::ROFF_UNKNOWN_FONT,
                Severity::Warning,
                format!("unknown font, skipping request: ft {font}"),
                location,
            ),
            crate::mdoc::Recovery::FirstSectionNotName { section, location } => (
                DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME,
                Severity::Warning,
                format!("first section is not \"NAME\": Sh {section}"),
                location,
            ),
            crate::mdoc::Recovery::TrailingDelimiterSpacing {
                macro_name,
                display,
                location,
            } => (
                DiagnosticCode::MDOC_TRAILING_DELIMITER_SPACING,
                Severity::Style,
                format!("no blank before trailing delimiter: {macro_name} {display}"),
                location,
            ),
            crate::mdoc::Recovery::TrailingDelimiter {
                macro_name,
                display,
                location,
            } => (
                DiagnosticCode::MDOC_TRAILING_DELIMITER,
                Severity::Style,
                format!("trailing delimiter: {macro_name} {display}"),
                location,
            ),
            crate::mdoc::Recovery::DescriptionOutsideName { location } => (
                DiagnosticCode::MDOC_DESCRIPTION_OUTSIDE_NAME,
                Severity::Warning,
                "description line outside NAME section: Nd".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::MissingDescription { location } => (
                DiagnosticCode::MDOC_DESCRIPTION_MISSING,
                Severity::Warning,
                "missing description line, using \"\": Nd".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::BadNameSectionContent { content, location } => (
                DiagnosticCode::MDOC_NAME_SECTION_CONTENT,
                Severity::Warning,
                format!("bad NAME section content: {content}"),
                location,
            ),
            crate::mdoc::Recovery::NameSectionMissingComma { name, location } => (
                DiagnosticCode::MDOC_NAME_SECTION_COMMA_MISSING,
                Severity::Warning,
                format!("missing comma before name: Nm {name}"),
                location,
            ),
            crate::mdoc::Recovery::NameSectionMissingName { location } => (
                DiagnosticCode::MDOC_NAME_SECTION_NAME_MISSING,
                Severity::Warning,
                "NAME section without Nm before Nd".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::NameSectionMissingDescription { location } => (
                DiagnosticCode::MDOC_NAME_SECTION_DESCRIPTION_MISSING,
                Severity::Warning,
                "NAME section without description".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::DescriptionNotAtEndOfName { location } => (
                DiagnosticCode::MDOC_NAME_SECTION_DESCRIPTION_NOT_LAST,
                Severity::Warning,
                "description not at the end of NAME".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::AuthorsSectionWithoutAuthor { location } => (
                DiagnosticCode::MDOC_AUTHORS_MISSING,
                Severity::Warning,
                "AUTHORS section without An macro".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::FunctionNameParenthesis { name, location } => (
                DiagnosticCode::MDOC_FUNCTION_NAME_PARENTHESIS,
                Severity::Warning,
                format!("parenthesis in function name: {name}"),
                location,
            ),
            crate::mdoc::Recovery::FunctionArgumentComma { argument, location } => (
                DiagnosticCode::MDOC_FUNCTION_ARGUMENT_COMMA,
                Severity::Warning,
                format!("comma in function argument: {argument}"),
                location,
            ),
            crate::mdoc::Recovery::UnknownLibrary { library, location } => (
                DiagnosticCode::MDOC_UNKNOWN_LIBRARY,
                Severity::Warning,
                format!("unknown library name: Lb {library}"),
                location,
            ),
            crate::mdoc::Recovery::UnknownStandard { standard, location } => (
                DiagnosticCode::MDOC_UNKNOWN_STANDARD,
                Severity::Error,
                format!("unknown standard specifier: St {standard}"),
                location,
            ),
            crate::mdoc::Recovery::DuplicateArgument {
                macro_name,
                argument,
                location,
            } => (
                DiagnosticCode::MDOC_DUPLICATE_ARGUMENT,
                Severity::Warning,
                format!("skipping duplicate argument: {macro_name} {argument}"),
                location,
            ),
            crate::mdoc::Recovery::EmptyListLayoutArgument { option, location } => (
                DiagnosticCode::MDOC_EMPTY_ARGUMENT,
                Severity::Warning,
                format!("empty argument, using 0n: Bl -{option}"),
                location,
            ),
            crate::mdoc::Recovery::DuplicateListType { argument, location } => (
                DiagnosticCode::MDOC_DUPLICATE_ARGUMENT,
                Severity::Warning,
                format!("skipping duplicate list type: Bl {argument}"),
                location,
            ),
            crate::mdoc::Recovery::DuplicateListArgument { argument, location } => (
                DiagnosticCode::MDOC_DUPLICATE_ARGUMENT,
                Severity::Warning,
                format!("duplicate argument: Bl {argument}"),
                location,
            ),
            crate::mdoc::Recovery::ListTypeNotFirst { argument, location } => (
                DiagnosticCode::MDOC_ARGUMENTS,
                Severity::Warning,
                format!("list type is not the first argument: Bl {argument}"),
                location,
            ),
            crate::mdoc::Recovery::MissingListType { location } => (
                DiagnosticCode::MDOC_ARGUMENTS,
                Severity::Error,
                "missing list type, using -item: Bl".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::SkippedListWidth {
                list_type,
                location,
            } => (
                DiagnosticCode::MDOC_ARGUMENTS,
                Severity::Warning,
                format!("skipping -width argument: Bl -{list_type}"),
                location,
            ),
            crate::mdoc::Recovery::MissingTagListWidth { location } => (
                DiagnosticCode::MDOC_ARGUMENTS,
                Severity::Warning,
                "missing -width in -tag list, using 6n: Bl -tag".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::WrongNumberOfColumnCells {
                columns,
                cells,
                location,
            } => (
                DiagnosticCode::MDOC_ARGUMENTS,
                Severity::Warning,
                format!("wrong number of cells: {columns} columns, {cells} cells"),
                location,
            ),
            crate::mdoc::Recovery::ColumnItemUsesNextLine { location } => (
                DiagnosticCode::MDOC_ARGUMENTS,
                Severity::Warning,
                "missing argument, using next line: Bl -column It".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::ColumnFirstMacro { location } => (
                DiagnosticCode::MDOC_ARGUMENTS,
                Severity::Warning,
                "first macro on line: Ta".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::UnknownAtVersion { argument, location } => (
                DiagnosticCode::MDOC_UNKNOWN_AT_VERSION,
                Severity::Warning,
                format!("unknown AT&T UNIX version: At {argument}"),
                location,
            ),
            crate::mdoc::Recovery::EmptyDisplayOffset { location } => (
                DiagnosticCode::MDOC_EMPTY_ARGUMENT,
                Severity::Warning,
                "empty argument, using 0n: Bd -offset".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::DuplicateDisplayArgument { argument, location } => (
                DiagnosticCode::MDOC_DUPLICATE_ARGUMENT,
                Severity::Warning,
                format!("duplicate argument: Bd {argument}"),
                location,
            ),
            crate::mdoc::Recovery::DuplicateDisplayType { argument, location } => (
                DiagnosticCode::MDOC_DUPLICATE_DISPLAY_TYPE,
                Severity::Warning,
                format!("skipping duplicate display type: Bd -{argument}"),
                location,
            ),
            crate::mdoc::Recovery::MissingDisplayType { location } => (
                DiagnosticCode::MDOC_MISSING_DISPLAY_TYPE,
                Severity::Warning,
                "missing display type, using -ragged: Bd".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::UnsupportedDisplayFile { location } => (
                DiagnosticCode::MDOC_UNSUPPORTED_DISPLAY_FILE,
                Severity::Error,
                "NOT IMPLEMENTED: Bd -file".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::DisplayWithoutArguments { location } => (
                DiagnosticCode::MDOC_DISPLAY_WITHOUT_ARGUMENTS,
                Severity::Error,
                "skipping display without arguments: Bd".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::NestedDisplay { location } => (
                DiagnosticCode::MDOC_NESTED_DISPLAY,
                Severity::Warning,
                "nested displays are not portable: Bd in Bd".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::BrokenBlock {
                breaker,
                macro_name,
                location,
            } => (
                DiagnosticCode::MDOC_BROKEN_BLOCK,
                Severity::Error,
                format!("inserting missing end of block: {breaker} breaks {macro_name}"),
                location,
            ),
            crate::mdoc::Recovery::MissingFontType { location } => (
                DiagnosticCode::MDOC_MISSING_FONT_TYPE,
                Severity::Warning,
                "missing font type, using \\fR: Bf".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::UnknownFontType { argument, location } => (
                DiagnosticCode::MDOC_UNKNOWN_FONT_TYPE,
                Severity::Warning,
                format!("unknown font type, using \\fR: Bf {argument}"),
                location,
            ),
            crate::mdoc::Recovery::InvalidArguments { message, location } => (
                DiagnosticCode::MDOC_ARGUMENTS,
                Severity::Error,
                message.into_string(),
                location,
            ),
            crate::mdoc::Recovery::Obsolete {
                macro_name,
                location,
            } => (
                DiagnosticCode::MDOC_OBSOLETE,
                Severity::Warning,
                format!("obsolete macro: {macro_name}"),
                location,
            ),
            crate::mdoc::Recovery::DuplicatePrologue {
                macro_name,
                location,
            } => (
                DiagnosticCode::MDOC_DUPLICATE_PROLOGUE,
                Severity::Error,
                format!("duplicate prologue macro: {macro_name}"),
                location,
            ),
            crate::mdoc::Recovery::OperatingSystemExplicit {
                operating_system,
                flavour,
                location,
            } => (
                DiagnosticCode::MDOC_OPERATING_SYSTEM_EXPLICIT,
                Severity::Style,
                format!("operating system explicitly specified: Os {operating_system} ({flavour})"),
                location,
            ),
            crate::mdoc::Recovery::MdocDateFound { date, location } => (
                DiagnosticCode::MDOC_MDOCDATE_FOUND,
                Severity::Style,
                format!("Mdocdate found: Dd {date} (NetBSD)"),
                location,
            ),
            crate::mdoc::Recovery::MdocDateMissing { date, location } => (
                DiagnosticCode::MDOC_MDOCDATE_MISSING,
                Severity::Style,
                format!("Mdocdate missing: Dd {date} (OpenBSD)"),
                location,
            ),
            crate::mdoc::Recovery::RcsIdMissing { flavour } => (
                DiagnosticCode::MDOC_RCS_ID_MISSING,
                Severity::Style,
                format!("RCS id missing: ({flavour})"),
                None,
            ),
            crate::mdoc::Recovery::LateOperatingSystem { location } => (
                DiagnosticCode::MDOC_LATE_OPERATING_SYSTEM,
                Severity::Warning,
                "late prologue macro: Os".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::MissingOperatingSystem => (
                DiagnosticCode::MDOC_OPERATING_SYSTEM_MISSING,
                Severity::Warning,
                "missing Os macro, using \"\"".to_owned(),
                None,
            ),
            crate::mdoc::Recovery::PrefixWithoutFollowing { display, location } => (
                DiagnosticCode::MDOC_PREFIX_WITHOUT_FOLLOWING,
                Severity::Warning,
                format!("nothing follows prefix: {display}"),
                location,
            ),
            crate::mdoc::Recovery::UselessMacro {
                macro_name,
                location,
            } => (
                DiagnosticCode::MDOC_USELESS_MACRO,
                Severity::Style,
                format!("useless macro: {macro_name}"),
                location,
            ),
            crate::mdoc::Recovery::LateTitle { location } => (
                DiagnosticCode::MDOC_LATE_TITLE,
                Severity::Error,
                "skipping late title macro: Dt".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::TitleNotUppercase { title, location } => (
                DiagnosticCode::MDOC_TITLE_NOT_UPPERCASE,
                Severity::Style,
                format!("lower case character in document title: Dt {title}"),
                location,
            ),
            crate::mdoc::Recovery::UnknownTitleSection { section, location } => (
                DiagnosticCode::MDOC_TITLE_SECTION_UNKNOWN,
                Severity::Warning,
                format!("unknown manual section: Dt ... {section}"),
                location,
            ),
            crate::mdoc::Recovery::MissingTitleArgument { location } => (
                DiagnosticCode::MDOC_TITLE_MISSING,
                Severity::Warning,
                "missing manual title, using UNTITLED: Dt".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::MissingTitleSection { title, location } => (
                DiagnosticCode::MDOC_TITLE_SECTION_MISSING,
                Severity::Warning,
                format!("missing manual section, using \"\": Dt {title}"),
                location,
            ),
            crate::mdoc::Recovery::TitleAfterOperatingSystem { location } => (
                DiagnosticCode::MDOC_PROLOGUE_ORDER,
                Severity::Warning,
                "prologue macros out of order: Dt after Os".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::DateMissing { location } => (
                DiagnosticCode::MDOC_DATE_MISSING,
                Severity::Warning,
                "missing date, using \"\": Dd".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::DateUnparseable { date, location } => (
                DiagnosticCode::MDOC_DATE_UNPARSEABLE,
                Severity::Warning,
                format!("cannot parse date, using it verbatim: Dd {date}"),
                location,
            ),
            crate::mdoc::Recovery::LegacyDate { date, location } => (
                DiagnosticCode::MDOC_DATE_LEGACY,
                Severity::Style,
                format!("legacy man(7) date format: Dd {date}"),
                location,
            ),
            crate::mdoc::Recovery::LateDate { location } => (
                DiagnosticCode::MDOC_LATE_PROLOGUE,
                Severity::Warning,
                "late prologue macro: Dd".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::DateAfterTitle { location } => (
                DiagnosticCode::MDOC_PROLOGUE_ORDER,
                Severity::Warning,
                "prologue macros out of order: Dd after Dt".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::MissingTitle => (
                DiagnosticCode::MDOC_TITLE_MISSING,
                Severity::Warning,
                "missing manual title, using UNTITLED: EOF".to_owned(),
                None,
            ),
            crate::mdoc::Recovery::NoDocumentBody => (
                DiagnosticCode::MDOC_NO_DOCUMENT_BODY,
                Severity::Warning,
                "no document body".to_owned(),
                None,
            ),
            crate::mdoc::Recovery::MissingName { location } => (
                DiagnosticCode::MDOC_NAME_MISSING,
                Severity::Error,
                "missing manual name, using \"\": Nm".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::MissingFunctionName { location } => (
                DiagnosticCode::MDOC_FUNCTION_NAME_MISSING,
                Severity::Warning,
                "missing function name, using \"\": Fo".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::MissingExitName { location } => (
                DiagnosticCode::MDOC_EXIT_NAME_MISSING,
                Severity::Warning,
                "missing utility name, using \"\": Ex".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::MissingStandardSelector {
                macro_name,
                location,
            } => (
                DiagnosticCode::MDOC_STANDARD_SELECTOR_MISSING,
                Severity::Warning,
                format!("missing -std argument, adding it: {macro_name}"),
                location,
            ),
            crate::mdoc::Recovery::ContentBeforeFirstSection { content, location } => (
                DiagnosticCode::MDOC_CONTENT_BEFORE_SECTION,
                Severity::Warning,
                format!("content before first section header: {content}"),
                location,
            ),
            crate::mdoc::Recovery::UnexpectedSection {
                section,
                allowed_sections,
                location,
            } => (
                DiagnosticCode::MDOC_UNEXPECTED_SECTION,
                Severity::Warning,
                format!("unexpected section: Sh {section} for {allowed_sections} only"),
                location,
            ),
            crate::mdoc::Recovery::DuplicateSection { section, location } => (
                DiagnosticCode::MDOC_DUPLICATE_SECTION,
                Severity::Warning,
                format!("duplicate section title: Sh {section}"),
                location,
            ),
            crate::mdoc::Recovery::SectionOutOfOrder { section, location } => (
                DiagnosticCode::MDOC_SECTION_ORDER,
                Severity::Warning,
                format!("sections out of conventional order: Sh {section}"),
                location,
            ),
            crate::mdoc::Recovery::FilledTextTab { location } => (
                DiagnosticCode::INPUT_TAB_IN_FILLED_TEXT,
                Severity::Warning,
                "tab in filled text".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::FilledBlankLine { location } => (
                DiagnosticCode::INPUT_BLANK_LINE_IN_FILLED_TEXT,
                Severity::Warning,
                "blank line in fill mode, using .sp".to_owned(),
                location,
            ),
            crate::mdoc::Recovery::ParagraphBoundary {
                macro_name,
                placement,
                blocker,
                location,
            } => (
                DiagnosticCode::MDOC_PARAGRAPH_BEFORE_BLOCK,
                Severity::Warning,
                format!("skipping paragraph macro: {macro_name} {placement} {blocker}"),
                location,
            ),
            crate::mdoc::Recovery::ParagraphMovedOutOfList {
                macro_name,
                location,
            } => (
                DiagnosticCode::MDOC_PARAGRAPH_MOVED_OUT_OF_LIST,
                Severity::Warning,
                format!("moving paragraph macro out of list: {macro_name}"),
                location,
            ),
            crate::mdoc::Recovery::BadlyNestedBlock {
                breaker,
                interrupted,
                location,
            } => (
                DiagnosticCode::MDOC_BADLY_NESTED_BLOCK,
                Severity::Warning,
                format!("blocks badly nested: {breaker} breaks {interrupted}"),
                location,
            ),
            crate::mdoc::Recovery::ItemOutsideList {
                arguments,
                location,
            } => (
                DiagnosticCode::MDOC_ITEM_OUTSIDE_LIST,
                Severity::Error,
                format!("skipping item outside list: It {arguments}"),
                location,
            ),
            crate::mdoc::Recovery::ColumnOutsideColumnList { location } => (
                DiagnosticCode::MDOC_COLUMN_OUTSIDE_LIST,
                Severity::Error,
                "skipping column outside column list: Ta".to_owned(),
                location,
            ),
        };
        let diagnostic = Diagnostic::new(
            DiagnosticCode::new(code).expect("static mdoc recovery diagnostic code is valid"),
            severity,
            message,
        );
        let diagnostic = match location {
            Some(span) => diagnostic.with_primary(span),
            None => diagnostic,
        };
        let diagnostic_primary = diagnostic.primary.clone();
        let diagnostic_count = outcome.diagnostics.len();
        push_diagnostic(
            &mut outcome.diagnostics,
            limits,
            diagnostic,
            &mut outcome.truncated,
        );
        if empty_no_macro && outcome.diagnostics.len() > diagnostic_count {
            // `push_diagnostic()` appends so regular semantic recoveries keep
            // their established ordering.  This one legacy exception instead
            // belongs immediately before the first later physical finding in
            // the same source, preserving order without globally sorting
            // post-validation diagnostics.
            let diagnostic = outcome
                .diagnostics
                .pop()
                .expect("a newly accepted diagnostic is present");
            let insertion = diagnostic_primary
                .as_ref()
                .and_then(|primary| {
                    outcome.diagnostics.iter().position(|candidate| {
                        candidate.primary.as_ref().is_some_and(|existing| {
                            existing.source == primary.source && existing.start > primary.start
                        })
                    })
                })
                .unwrap_or(outcome.diagnostics.len());
            outcome.diagnostics.insert(insertion, diagnostic);
        }
    }
}
