//! Parser configuration and the M1 byte-safe session boundary.

use std::fmt;

use crate::ast::DocumentBuilder;
use crate::{
    Diagnostic, DiagnosticCode, Document, IncludeRequest, LimitViolation, Limits, MacroSet,
    NodeFlags, NodeId, NodeKind, Severity, Source, SourcePosition, SourceResolver, SourceSpan,
};

const LEGACY_SYNTAX_TREE_DEPTH_MESSAGE: &str =
    "owned syntax tree exceeded the 256-level copy limit; deeper descendants were omitted";
use crate::{
    escape::{
        EscapeIssue, EscapeIssueKind, decode_visible_bytes, normalize_ast_escapes,
        normalize_escapes,
    },
    numeric::evaluate_sum,
    roff::{
        Environment, EnvironmentError, PackageFillScope, TranslationRequestIssue,
        translation_request_issue,
    },
    scan::{
        Argument, ArgumentIssue, ScannedLine, Scanner, lex_arguments, lex_user_macro_arguments,
        strip_inline_comment,
    },
};

/// Select the semantic macro package before parsing begins.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Syntax {
    /// Detect mdoc from `.Dd`, man from `.TH`, and retain `None` otherwise.
    #[default]
    Auto,
    /// Execute generic roff without selecting a man or mdoc semantic package.
    ///
    /// This is useful for callers that need roff execution evidence insulated
    /// from macro-package structural lowering.
    Roff,
    /// Parse as traditional man(7) syntax.
    Man,
    /// Parse as semantic mdoc(7) syntax.
    Mdoc,
}

/// Recovery policy for source defects once syntax support is enabled.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RecoveryMode {
    /// Preserve a bounded coherent prefix and report recoverable diagnostics.
    #[default]
    BestEffort,
    /// Stop after the first non-style syntax error when a coherent document is impossible.
    Strict,
}

/// Complete configuration for one independent parse session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParserConfig {
    /// Initial syntax selection.
    pub syntax: Syntax,
    /// Deterministic fallback for otherwise unspecified document OS metadata.
    pub operating_system: Option<Box<str>>,
    /// Validated deterministic work and allocation budgets.
    pub limits: Limits,
    /// Recovery behavior for non-fatal source defects.
    pub recovery: RecoveryMode,
}

impl Default for ParserConfig {
    fn default() -> Self {
        Self {
            syntax: Syntax::Auto,
            operating_system: None,
            limits: Limits::default(),
            recovery: RecoveryMode::BestEffort,
        }
    }
}

/// Reusable, `Send`/`Sync` parser configuration with no global parser state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Parser {
    config: ParserConfig,
}

impl Parser {
    /// Construct a parser with immutable caller-owned configuration.
    #[must_use]
    pub const fn new(config: ParserConfig) -> Self {
        Self { config }
    }

    /// Borrow immutable parser configuration.
    #[must_use]
    pub const fn config(&self) -> &ParserConfig {
        &self.config
    }

    /// Parse one borrowed source without filesystem or resolver access.
    ///
    /// M2 scans byte-oriented physical lines, normalizes the scanner-stage
    /// visible escape subset, and returns a flat syntax prefix. Full roff
    /// expansion and man/mdoc structural validation are later milestones, so
    /// recoverable scanner findings remain explicit diagnostics.
    ///
    /// # Errors
    ///
    /// Returns [`FatalError`] only for invalid configuration or input that
    /// cannot be represented within configured deterministic limits.
    pub fn parse(&self, source: Source<'_>) -> Result<ParseReport, FatalError> {
        let mut resolver = DenyResolver;
        self.parse_session(source, &mut resolver)
    }

    /// Parse with explicit authority to resolve `.so` includes.
    ///
    /// The parser never falls back to a process working directory or other
    /// host state.  Resolver misses and failures become source-addressable
    /// recovery diagnostics, while accepted sources are retained in the
    /// document's source map.
    ///
    /// # Errors
    ///
    /// Returns the same root-boundary failures as [`Self::parse`].
    pub fn parse_with_resolver<R: SourceResolver + ?Sized>(
        &self,
        source: Source<'_>,
        resolver: &mut R,
    ) -> Result<ParseReport, FatalError> {
        self.parse_session(source, resolver)
    }

    fn parse_session<R: SourceResolver + ?Sized>(
        &self,
        source: Source<'_>,
        resolver: &mut R,
    ) -> Result<ParseReport, FatalError> {
        self.config
            .limits
            .validate()
            .map_err(FatalError::invalid_configuration)?;
        if source.bytes.len() > self.config.limits.max_root_source_bytes {
            return Err(FatalError::source_limit(
                source.name.as_str(),
                source.bytes.len(),
                self.config.limits.max_root_source_bytes,
            ));
        }
        if source.bytes.len() > u32::MAX as usize {
            return Err(FatalError {
                kind: FatalErrorKind::SourceTooLargeForSpans,
                message: "source byte offsets exceed the public u32 span range".into(),
            });
        }
        let source_lines = source.bytes.split(|byte| *byte == b'\n').count();
        if source_lines > self.config.limits.max_source_lines {
            return Err(FatalError::source_line_limit(
                source.name.as_str(),
                source_lines,
                self.config.limits.max_source_lines,
            ));
        }

        let macro_set = select_macro_set(self.config.syntax, source.bytes);
        let mut builder = DocumentBuilder::new(macro_set, source);
        let mut environment = Environment::default();
        environment.configure_limits(&self.config.limits);
        let mut active_sources = vec![source.name.clone()];
        let root_source_has_mdoc_os = source_has_mdoc_operating_system_request(source.bytes);
        let mut outcome = scan_source(
            source,
            &self.config,
            DocumentBuilder::root_source(),
            0,
            &mut builder,
            &mut environment,
            Vec::new(),
            Vec::new(),
            0,
            0,
            false,
            1,
            None,
            0,
            source.bytes.len(),
            1,
            root_source_has_mdoc_os,
            &mut active_sources,
            resolver,
        );
        outcome.saw_mdoc_operating_system |= root_source_has_mdoc_os;
        let structure = crate::preprocess::structure(&mut builder, &self.config.limits);
        apply_preprocess_outcome(&mut outcome, structure, &self.config.limits);
        let structure = crate::man::structure(&mut builder, self.config.limits.max_nodes);
        let missing_man_title_control = structure.missing_title_control;
        apply_man_structure_outcome(&mut outcome, structure, &self.config.limits);
        let structure = crate::mdoc::structure(
            &mut builder,
            self.config.limits.max_nodes,
            outcome.saw_mdoc_operating_system,
        );
        apply_mdoc_structure_outcome(&mut outcome, structure, &self.config.limits);
        apply_tree_depth_limit(&mut outcome, &mut builder, &self.config.limits);
        reorder_deferred_post_validation_diagnostics(&mut outcome);
        if builder.metadata_mut().os.is_none()
            && let Some(arguments) = source_mdoc_operating_system_request(source.bytes)
            && !trim_horizontal_space(arguments).is_empty()
        {
            builder.operating_system(visible_bytes(trim_horizontal_space(arguments)));
        }
        if !missing_man_title_control
            && builder.metadata_mut().os.is_none()
            && let Some(operating_system) = self.config.operating_system.as_deref()
        {
            builder.operating_system(operating_system);
        }
        let emitted_nodes = builder.node_count();
        Ok(ParseReport {
            document: builder.finish(),
            diagnostics: outcome.diagnostics,
            statistics: ParseStatistics {
                source_bytes: outcome.source_bytes,
                source_files: outcome.source_files,
                expansion_steps: outcome.expansion_steps,
                emitted_nodes,
                maximum_depth: outcome.maximum_depth,
                truncated: outcome.truncated,
            },
        })
    }
}

fn apply_tree_depth_limit(
    outcome: &mut ScanOutcome,
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

fn apply_man_structure_outcome(
    outcome: &mut ScanOutcome,
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
                || "missing manual section, using \"\": TH".into(),
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
fn reorder_deferred_post_validation_diagnostics(outcome: &mut ScanOutcome) {
    for diagnostic in outcome.deferred_post_validation_diagnostics.drain(..) {
        if let Some(index) = outcome
            .diagnostics
            .iter()
            .position(|candidate| candidate == &diagnostic)
        {
            outcome.diagnostics.remove(index);
            outcome.diagnostics.push(diagnostic);
        }
    }
}

fn apply_preprocess_outcome(
    outcome: &mut ScanOutcome,
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
        let diagnostic = location.map_or(diagnostic.clone(), |location| {
            diagnostic.with_primary(location)
        });
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
        let diagnostic = recovery.location.map_or(diagnostic.clone(), |location| {
            diagnostic.with_primary(location)
        });
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
        let diagnostic = recovery.location.map_or(diagnostic.clone(), |location| {
            diagnostic.with_primary(location)
        });
        push_diagnostic(
            &mut outcome.diagnostics,
            limits,
            diagnostic,
            &mut outcome.truncated,
        );
    }
}

#[allow(clippy::too_many_lines)] // One recovery-to-diagnostic mapping preserves upstream ordering.
fn apply_mdoc_structure_outcome(
    outcome: &mut ScanOutcome,
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
                if arguments.is_empty() {
                    "skipping item outside list: It".to_owned()
                } else {
                    format!("skipping item outside list: It {arguments}")
                },
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
        let diagnostic = location.map_or(diagnostic.clone(), |span| diagnostic.with_primary(span));
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

struct DenyResolver;

impl SourceResolver for DenyResolver {
    fn resolve(
        &mut self,
        _request: IncludeRequest<'_>,
    ) -> Result<Option<crate::ResolvedSource>, crate::ResolveError> {
        Ok(None)
    }
}

struct ScanOutcome {
    diagnostics: Vec<Diagnostic>,
    deferred_post_validation_diagnostics: Vec<Diagnostic>,
    source_bytes: usize,
    source_files: usize,
    text_bytes: usize,
    expansion_steps: usize,
    truncated: bool,
    maximum_depth: usize,
    previous_conditional: Option<bool>,
    total_loop_iterations: usize,
    saw_mdoc_operating_system: bool,
}

/// One physical line retained while a bounded roff `\\{ ... \\}` scope is
/// collected.  Collection intentionally owns its bytes: a loop body may run
/// more than once after the scanner has advanced beyond the physical source.
#[derive(Clone, Debug, Eq, PartialEq)]
enum ScopeLine {
    Text {
        start: u32,
        end: u32,
        bytes: Vec<u8>,
        /// This text starts after a roff conditional-scope closer on the
        /// same physical source line.  Terminal rendering must keep it in
        /// the preceding inline flow even though it is replayed separately.
        terminal_inline: bool,
    },
    Control {
        start: u32,
        end: u32,
        /// Absolute offset of the first raw request argument before any
        /// scope closer is removed.  Scope replay needs this independently
        /// of the request name when an attached `\}` occupies source bytes.
        argument_start: u32,
        name: Vec<u8>,
        arguments: Vec<u8>,
    },
    Loop {
        start: u32,
        end: u32,
        predicate: Vec<u8>,
        lines: Vec<ScopeLine>,
    },
    Conditional {
        start: u32,
        end: u32,
        predicate: Vec<u8>,
        else_eligible: bool,
        lines: Vec<ScopeLine>,
    },
    Else {
        start: u32,
        end: u32,
        lines: Vec<ScopeLine>,
    },
}

fn scope_line_start(line: &ScopeLine) -> u32 {
    match line {
        ScopeLine::Text { start, .. }
        | ScopeLine::Control { start, .. }
        | ScopeLine::Loop { start, .. }
        | ScopeLine::Conditional { start, .. }
        | ScopeLine::Else { start, .. } => *start,
    }
}

fn scope_line_end(line: &ScopeLine) -> u32 {
    match line {
        ScopeLine::Text { end, .. }
        | ScopeLine::Control { end, .. }
        | ScopeLine::Loop { end, .. }
        | ScopeLine::Conditional { end, .. }
        | ScopeLine::Else { end, .. } => *end,
    }
}

/// Logical input position used by repeated executions of a collected scope.
///
/// The physical spans stay on their authored body lines. Mandoc's roff input
/// frame, however, advances to the closing scope line before a subsequent
/// replay becomes visible to its owned AST.
fn scope_replay_logical_start(
    builder: &DocumentBuilder,
    source_id: crate::SourceId,
    scope: &CollectedScope,
) -> Option<SourcePosition> {
    let offset = scope.closer_start?;
    builder.source_position(&SourceSpan::new(source_id, offset, offset).ok()?)
}

/// Apply a logical source position to nodes emitted at the direct scope root.
fn set_new_root_children_logical_start(
    builder: &mut DocumentBuilder,
    root: NodeId,
    first_child: usize,
    position: SourcePosition,
) {
    let nodes = builder
        .children(root)
        .and_then(|children| children.get(first_child..))
        .unwrap_or_default()
        .to_vec();
    for node in nodes {
        let _ = builder.set_node_logical_start(node, position);
    }
}

/// Apply a logical source position to the first direct node emitted by a
/// scope execution.
fn set_first_root_child_logical_start(
    builder: &mut DocumentBuilder,
    root: NodeId,
    first_child: usize,
    position: SourcePosition,
) {
    let Some(node) = builder
        .children(root)
        .and_then(|children| children.get(first_child))
        .copied()
    else {
        return;
    };
    let _ = builder.set_node_logical_start(node, position);
}

/// Retain the input-frame column of a multiline `\{\\` opener while the
/// first emitted node remains physically anchored on its following line.
///
/// Mandoc advances to the next physical input line to read that body, but its
/// owned tree exposes the continuation escape's column for the first node.
/// This differs from repeated-loop provenance, which instead uses the closer.
fn set_first_scope_child_opening_column(
    builder: &mut DocumentBuilder,
    root: NodeId,
    first_child: usize,
    source_id: crate::SourceId,
    opener_start: u32,
) {
    let Some(span) = SourceSpan::new(source_id, opener_start, opener_start).ok() else {
        return;
    };
    let Some(opening) = builder.source_position(&span) else {
        return;
    };
    let Some(node) = builder
        .children(root)
        .and_then(|children| children.get(first_child))
        .copied()
    else {
        return;
    };
    let Some(physical) = builder.node_source_position(node) else {
        return;
    };
    set_first_scope_child_logical_start(
        builder,
        root,
        first_child,
        SourcePosition {
            line: physical.line,
            column: opening.column,
        },
    );
}

/// Apply a known logical source position to the first child emitted by a
/// collected scope while keeping its physical body span unchanged.
fn set_first_scope_child_logical_start(
    builder: &mut DocumentBuilder,
    root: NodeId,
    first_child: usize,
    position: SourcePosition,
) {
    let Some(node) = builder
        .children(root)
        .and_then(|children| children.get(first_child))
        .copied()
    else {
        return;
    };
    let Some(physical) = builder.node_source_position(node) else {
        return;
    };
    let _ = builder.set_node_logical_start(
        node,
        SourcePosition {
            line: physical.line,
            column: position.column,
        },
    );
}

/// One deferred user-macro input line.
///
/// `macro_origin` is the byte cursor inherited by a nested macro reparse.
/// The C reader keeps this cursor when a macro body invokes another macro;
/// it is observable as a logical column even though the public source span
/// remains the outer physical invocation line. `text_origin` is the absolute
/// logical cursor for an inline conditional body requeued as ordinary text.
/// It must remain separate: macro invocation cursors are additive for
/// generated controls, while an inline body starts at its own copied-input
/// cursor. `scope_reparse` identifies an independently retained line of a
/// braced conditional scope: a nested user macro starts a fresh input frame
/// there instead of inheriting that retained line's byte width.
type PendingMacroLine = (Vec<u8>, Vec<Vec<u8>>, usize, u32, Option<u32>, bool);

struct CollectedScope {
    lines: Vec<ScopeLine>,
    terminated: bool,
    /// Start of the physical line whose `\\}` closed this scope, if any.
    closer_start: Option<u32>,
}

struct PendingScope {
    start: u32,
    end: u32,
    kind: Option<ScopeKind>,
    lines: Vec<ScopeLine>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ScopeKind {
    Loop {
        predicate: Vec<u8>,
        /// Same-line body and the first visible source byte after its `\{`
        /// opener.  The body survives collection, so its original physical
        /// offset must survive with it for canonical AST locations.
        initial_body: Option<(Vec<u8>, u32)>,
    },
    Conditional {
        predicate: Vec<u8>,
        /// See [`ScopeKind::Loop::initial_body`].
        initial_body: Option<(Vec<u8>, u32)>,
        else_eligible: bool,
    },
    Else {
        /// See [`ScopeKind::Loop::initial_body`].
        initial_body: Option<(Vec<u8>, u32)>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ScopeFlow {
    Continue,
    Break,
    LoopContinue,
    /// A macro body closed the caller's `.while` scope.  The absolute source
    /// offset is the physical invocation line, mirroring mandoc's reparse
    /// provenance rather than the copied macro definition line.
    CloseLoopInInnerScope {
        invocation_start: u32,
    },
    Halt,
}

enum ScopeExecutionFrame<'a> {
    Lines {
        lines: &'a [ScopeLine],
        next: usize,
        previous_conditional: Option<bool>,
    },
    Loop {
        start: u32,
        end: u32,
        predicate: &'a [u8],
        lines: &'a [ScopeLine],
        iterations: usize,
        /// A nested `.while` is executed in mandoc's active input frame,
        /// then causes its enclosing loop to stop rather than resuming the
        /// outer scope after the inner predicate becomes false.
        break_after: bool,
    },
    /// Apply the copied-input provenance of a nested loop only after its
    /// replayed body has emitted nodes at the direct scope root.
    SetNewRootChildrenLogicalStart {
        first_child: usize,
        position: SourcePosition,
    },
}

#[allow(clippy::too_many_lines)] // M2's explicit scanner-stage dispatch is kept in source order.
#[allow(clippy::needless_borrow)] // Borrowed session fields remain explicit at the recursive boundary.
#[allow(clippy::too_many_arguments)]
fn scan_source<R: SourceResolver + ?Sized>(
    source: Source<'_>,
    config: &ParserConfig,
    source_id: crate::SourceId,
    include_depth: usize,
    mut builder: &mut DocumentBuilder,
    mut environment: &mut Environment,
    mut diagnostics: Vec<Diagnostic>,
    mut deferred_post_validation_diagnostics: Vec<Diagnostic>,
    mut text_bytes: usize,
    mut expansion_steps: usize,
    mut truncated: bool,
    mut maximum_depth: usize,
    mut previous_conditional: Option<bool>,
    mut total_loop_iterations: usize,
    mut source_bytes: usize,
    mut source_files: usize,
    mut saw_mdoc_operating_system: bool,
    active_sources: &mut Vec<crate::SourceName>,
    resolver: &mut R,
) -> ScanOutcome {
    let limits = &config.limits;
    let root = DocumentBuilder::root();
    let mut scanner = Scanner::new(source.bytes, limits);
    let mut package_preprocessor_depth = 0_usize;
    let mut table_preprocessor_depth = 0_usize;
    // man(7)'s EX/EE style validation observes presentation toggles rather
    // than the nesting model used to retain no-fill AST flags.
    let mut man_example_fill_enabled = environment.is_filled();
    let mut man_indent_state = ManIndentState::default();
    let mut input_trap = InputTrap::default();
    // A bare `.if`/`.ie` owns the next physical line as a one-line scope.
    // Keeping this at the scanner boundary lets an active body take the
    // ordinary package parser path, while an inactive one is consumed before
    // it can create diagnostics, mutate state, or publish AST nodes.
    let mut next_line_condition = None::<bool>;
    // mandoc reports an open `.while` when its caller resumes after an inner
    // macro closed that loop.  Scope collection has already consumed the
    // closer, so publish the recovery finding on the next physical line.
    let mut pending_while_out_of_scope = false;
    'lines: while let Some(line) = scanner.next_line() {
        let pending_next_line_condition = next_line_condition.take();
        if pending_while_out_of_scope {
            let (start, end) = match &line {
                ScannedLine::TooLong { start, end }
                | ScannedLine::Text { start, end, .. }
                | ScannedLine::Comment { start, end, .. }
                | ScannedLine::Control { start, end, .. } => (*start, *end),
            };
            push_diagnostic(
                &mut diagnostics,
                limits,
                diagnostic(
                    DiagnosticCode::ROFF_WHILE_OUT_OF_SCOPE,
                    Severity::Unsupported,
                    source_id,
                    start,
                    end,
                    "end of scope with open .while loop",
                ),
                &mut truncated,
            );
            pending_while_out_of_scope = false;
        }
        // `.el` is the paired branch, not the preceding bare conditional's
        // next-line body.  In particular, a malformed predicate may end at
        // the physical line immediately before its `.el`; preserve the pair
        // so the false arm remains visible.
        let paired_else = matches!(&line, ScannedLine::Control { name, .. } if *name == b"el");
        if pending_next_line_condition == Some(false) && !paired_else {
            continue;
        }
        match line {
            ScannedLine::TooLong { start, end } => {
                push_diagnostic(
                    &mut diagnostics,
                    limits,
                    diagnostic(
                        DiagnosticCode::LIMIT_LINE_BYTES,
                        Severity::Warning,
                        source_id,
                        start,
                        end,
                        "physical source line exceeds max_line_bytes and was skipped",
                    ),
                    &mut truncated,
                );
            }
            ScannedLine::Text { start, end, bytes } => {
                let authored_has_tab = bytes.contains(&b'\t');
                let authored_trailing_whitespace = trailing_whitespace_start(bytes).is_some();
                if is_bad_comment_style(
                    bytes,
                    scanner.escape_character(),
                    scanner.control_character(),
                ) {
                    emit_bad_comment_style(
                        bytes,
                        scanner.escape_character(),
                        scanner.control_character(),
                        start,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                    // `roff_getcontrol()` recognizes the escaped control
                    // character before text dispatch. A following quote is a
                    // malformed comment request, so it emits only the style
                    // finding and no public text/input-trap event.
                    continue;
                }
                // roff arms `.it` against physical input *text* lines.  The
                // triggering line remains visible first, then the configured
                // macro is reparsed at this line's source location.
                let sprung_input_trap = input_trap.consume_text_line();
                if builder.macro_set() != MacroSet::None && package_preprocessor_depth == 0 {
                    if builder.macro_set() == MacroSet::Mdoc || environment.is_filled() {
                        emit_trailing_whitespace(
                            bytes,
                            start,
                            source_id,
                            limits,
                            &mut diagnostics,
                            &mut truncated,
                        );
                    }
                    emit_long_input_line(
                        bytes,
                        start,
                        end,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                    if environment.is_filled() {
                        emit_filled_text_tabs(
                            bytes,
                            start,
                            source_id,
                            limits,
                            &mut diagnostics,
                            &mut truncated,
                        );
                    }
                }
                let has_invalid_input_bytes = emit_invalid_input_bytes(
                    bytes,
                    start,
                    source_id,
                    limits,
                    &mut diagnostics,
                    &mut truncated,
                );
                let has_valid_utf8_non_ascii = contains_valid_utf8_non_ascii(bytes);
                let table_input_text = (has_invalid_input_bytes || has_valid_utf8_non_ascii)
                    .then(|| legacy_table_input_text(bytes));
                emit_unterminated_register_reference_escapes(
                    bytes,
                    scanner.escape_character(),
                    start,
                    end,
                    source_id,
                    limits,
                    &mut diagnostics,
                    &mut truncated,
                );
                emit_unterminated_string_reference_escapes(
                    bytes,
                    scanner.escape_character(),
                    start,
                    end,
                    source_id,
                    limits,
                    &mut diagnostics,
                    &mut truncated,
                );
                emit_outside_macro_argument_escapes(
                    bytes,
                    scanner.escape_character(),
                    start,
                    source_id,
                    limits,
                    &mut diagnostics,
                    &mut truncated,
                );
                let Some(bytes) = expand_environment(
                    environment,
                    bytes,
                    scanner.escape_character(),
                    &[],
                    limits,
                    source_id,
                    start,
                    end,
                    &mut expansion_steps,
                    &mut diagnostics,
                    &mut truncated,
                ) else {
                    break 'lines;
                };
                // A missing interpolation can leave the authored prefix with
                // terminal whitespace even when the physical source line did
                // not end in it (for example `name: \\*[missing]`).  The
                // validator observes that post-expansion line too, while an
                // authored trailing run was already checked above.
                if !authored_trailing_whitespace
                    && builder.macro_set() != MacroSet::None
                    && package_preprocessor_depth == 0
                    && (builder.macro_set() == MacroSet::Mdoc || environment.is_filled())
                {
                    emit_trailing_whitespace(
                        &bytes,
                        start,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                }
                // A recursive string expansion has a non-fatal legacy
                // recovery: its containing physical line disappears, while
                // the next input line remains independently parseable. Other
                // zero-byte results (notably blank/fill-mode input) retain
                // their normal package-level recovery behavior.
                let recursive_expansion = diagnostics.last().is_some_and(|diagnostic| {
                    diagnostic.code.as_str() == DiagnosticCode::LIMIT_EXPANSION_STEPS
                        && diagnostic.severity == Severity::Error
                        && diagnostic.message.as_ref()
                            == "input stack limit exceeded, infinite loop?"
                });
                if bytes.is_empty() && recursive_expansion {
                    continue;
                }
                let escape = scanner.escape_character();
                let Some(translated) = environment
                    .translate_text(&bytes, escape, limits.max_expanded_line_bytes)
                    .map_err(|error| {
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            environment_error_diagnostic(error, source_id, start, end),
                            &mut truncated,
                        );
                        truncated = true;
                    })
                    .ok()
                else {
                    break 'lines;
                };
                // Definition copy mode can inject a literal tab into an
                // otherwise tab-free text line.  The physical input scan
                // above already owns authored tabs; report this expanded
                // form only when it cannot duplicate one of those findings.
                // Its byte position is still relative to the visible input
                // line, as in `.ds x<TAB>text` followed by `\\*[x]`.
                if environment.is_filled() && !authored_has_tab {
                    emit_filled_text_tabs(
                        &translated,
                        start,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                }
                emit_declared_character_escape_warnings(
                    &translated,
                    escape,
                    environment,
                    source_id,
                    start,
                    end,
                    limits,
                    &mut diagnostics,
                    &mut truncated,
                );
                let translated =
                    expand_declared_character_escapes(&translated, escape, environment);
                let result = normalize_document_escapes(builder, &translated, escape, limits);
                if !record_expansion_steps(
                    &mut expansion_steps,
                    result.steps,
                    limits,
                    source_id,
                    start,
                    end,
                    &mut diagnostics,
                    &mut truncated,
                ) {
                    break 'lines;
                }
                let suppress_table_continuation_escape = table_preprocessor_depth > 0
                    && has_physical_line_continuation(&translated, escape);
                if suppress_table_continuation_escape {
                    let translated_len = u32::try_from(translated.len())
                        .expect("parser line limits keep translated offsets public");
                    let issues = result
                        .issues
                        .iter()
                        .filter(|issue| {
                            !(issue.kind == EscapeIssueKind::Unterminated
                                && issue.offset.saturating_add(issue.length) == translated_len)
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    emit_escape_issues(
                        &issues,
                        start,
                        end,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                } else {
                    emit_escape_issues(
                        &result.issues,
                        start,
                        end,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                }
                truncated |= result.truncated;
                let flags = NodeFlags {
                    line_start: true,
                    line_continuation: result.line_continuation,
                    ..NodeFlags::default()
                };
                if append_text_node(
                    &mut builder,
                    root,
                    source_id,
                    start,
                    end,
                    flags,
                    result.text,
                    limits,
                    &mut text_bytes,
                    &mut diagnostics,
                    &mut truncated,
                ) {
                    if (has_invalid_input_bytes || has_valid_utf8_non_ascii)
                        && let Some(node) = builder
                            .children(root)
                            .and_then(|nodes| nodes.last())
                            .copied()
                    {
                        let _ = builder.set_node_input_unicode_provenance(
                            node,
                            has_invalid_input_bytes,
                            has_valid_utf8_non_ascii,
                        );
                        if let Some(table_input_text) = table_input_text {
                            let _ = builder.set_node_table_input_text(node, table_input_text);
                        }
                    }
                    maximum_depth = maximum_depth.max(2);
                }
                if let Some(name) = sprung_input_trap {
                    let name_end = name
                        .iter()
                        .position(u8::is_ascii_whitespace)
                        .unwrap_or(name.len());
                    if name_end == 0 {
                        continue;
                    }
                    let trap = ScopeLine::Control {
                        start,
                        end,
                        argument_start: start
                            .saturating_add(u32::try_from(name_end).unwrap_or(u32::MAX))
                            .saturating_add(1),
                        name: name[..name_end].to_vec(),
                        arguments: trim_horizontal_space(&name[name_end..]).to_vec(),
                    };
                    if matches!(
                        execute_scope_line(
                            &trap,
                            &mut builder,
                            root,
                            source_id,
                            &mut scanner,
                            &mut environment,
                            limits,
                            &mut text_bytes,
                            &mut expansion_steps,
                            &mut maximum_depth,
                            &mut total_loop_iterations,
                            &mut diagnostics,
                            &mut truncated,
                        ),
                        ScopeFlow::Halt
                    ) {
                        break 'lines;
                    }
                }
            }
            ScannedLine::Comment { start, end, bytes } => {
                // libmandoc preserves a comment as a distinct node, but does
                // not mark it as an implicit no-print node. Consumers use the
                // node kind to omit comments from rendered output.
                let flags = NodeFlags::default();
                if append_textual_node(
                    &mut builder,
                    root,
                    NodeKind::Comment,
                    source_id,
                    start,
                    end,
                    flags,
                    visible_bytes(bytes),
                    limits,
                    &mut text_bytes,
                    &mut diagnostics,
                    &mut truncated,
                ) {
                    maximum_depth = maximum_depth.max(2);
                }
            }
            ScannedLine::Control {
                start,
                control_start,
                mut end,
                no_break: _,
                name,
                arguments,
                raw_arguments,
                argument_start,
            } => {
                // The physical scanner stops a control name at an adjacent
                // escape so condition openers such as `.el\{` can keep their
                // own grammar.  Roff names have a small, observable exception:
                // a doubled delimiter is a literal byte in a user-macro name.
                // Other adjacent escapes terminate the name and are diagnosed
                // before dispatching the valid prefix (for example
                // `.witharg\(enargument`).
                let attached_name = recover_attached_control_name(
                    name,
                    raw_arguments,
                    scanner.escape_character(),
                    matches!(name, b"de" | b"de1" | b"am" | b"dei" | b"ami")
                        || is_builtin_package_macro(builder.macro_set(), name)
                        || (builder.macro_set() == MacroSet::Mdoc
                            && std::str::from_utf8(name)
                                .is_ok_and(crate::mdoc::is_mdoc_callable_macro))
                        || environment.macro_definition(name).is_some()
                        || environment.is_suppressed_macro_name(name),
                );
                let attached_escape_width = attached_name
                    .as_ref()
                    .filter(|recovery| recovery.invalid_escape_preview.is_some())
                    .map(|_| roff_escape_name_width(raw_arguments, scanner.escape_character()));
                if let Some(recovery) = &attached_name
                    && let Some(preview) = &recovery.invalid_escape_preview
                {
                    push_diagnostic(
                        &mut diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_ESCAPED_NAME,
                            Severity::Error,
                            source_id,
                            start,
                            start.saturating_add(1),
                            format!(
                                "escaped character not allowed in a name: {}",
                                visible_bytes(preview)
                            ),
                        ),
                        &mut truncated,
                    );
                }
                let name = attached_name
                    .as_ref()
                    .map_or(name, |recovery| recovery.name.as_slice());
                let arguments = attached_name
                    .as_ref()
                    .map_or(arguments, |recovery| recovery.arguments.as_slice());
                let raw_arguments = attached_name
                    .as_ref()
                    .map_or(raw_arguments, |recovery| recovery.arguments.as_slice());
                // A physical control line is outside every user-macro
                // argument frame.  Validate active `\$1`-style selectors
                // before its request-specific parser consumes or reparses
                // the arguments; copy-mode definitions retain doubled forms
                // and are therefore intentionally skipped by the helper.
                emit_outside_macro_argument_escapes(
                    arguments,
                    scanner.escape_character(),
                    argument_start,
                    source_id,
                    limits,
                    &mut diagnostics,
                    &mut truncated,
                );
                let sanitized_outside_macro_arguments =
                    strip_outside_macro_argument_escapes(arguments, scanner.escape_character());
                let arguments = sanitized_outside_macro_arguments.as_slice();
                // A recovered package macro begins its retained argument
                // after the full attached escape, rather than at the virtual
                // cursor used while its name is first recognized.
                let argument_start = match attached_escape_width {
                    Some(width) => argument_start.saturating_add(
                        u32::try_from(width)
                            .expect("attached escape width fits public source spans"),
                    ),
                    None => argument_start,
                };
                if name == b"Os" {
                    saw_mdoc_operating_system = true;
                }
                let mut continued_arguments = None;
                let mut continued_raw_arguments = None;
                let mut physical_continuation = false;
                let mut terminal_continuation_at_eof = false;
                // A terminal `\\{\\` on a conditional opener belongs to
                // the scope collector, not to this control line's argument
                // list.  Consuming its first body line here would prevent
                // the explicit scope executor from seeing it.
                if !matches!(
                    name,
                    b"while" | b"if" | b"ie" | b"el" | b"cc" | b"c2" | b"ec"
                ) && has_physical_line_continuation(arguments, scanner.escape_character())
                {
                    let mut joined_arguments = arguments.to_vec();
                    let mut joined_raw_arguments = raw_arguments.to_vec();
                    while has_physical_line_continuation(
                        &joined_arguments,
                        scanner.escape_character(),
                    ) {
                        let Some(next_line) = scanner.next_line() else {
                            // Roff consumes a terminal odd escape together
                            // with the physical newline even at end of input.
                            // Retain the authored byte for the AST recovery,
                            // but remember that its otherwise generic escape
                            // finding must be suppressed below.
                            terminal_continuation_at_eof = true;
                            break;
                        };
                        match next_line {
                            ScannedLine::Text {
                                end: continuation_end,
                                bytes,
                                ..
                            } => {
                                let _ = joined_arguments.pop();
                                joined_arguments.extend_from_slice(bytes);
                                let _ = joined_raw_arguments.pop();
                                joined_raw_arguments.extend_from_slice(bytes);
                                physical_continuation = true;
                                end = continuation_end;
                            }
                            line => {
                                scanner.unread_line(line);
                                break;
                            }
                        }
                    }
                    if physical_continuation {
                        continued_arguments = Some(joined_arguments);
                        continued_raw_arguments = Some(joined_raw_arguments);
                    }
                }
                let arguments = continued_arguments.as_deref().unwrap_or(arguments);
                let raw_arguments = continued_raw_arguments.as_deref().unwrap_or(raw_arguments);
                let mut continued_argument_nodes = Vec::new();
                update_preprocessor_depth(&mut package_preprocessor_depth, name);
                update_table_preprocessor_depth(&mut table_preprocessor_depth, name);
                if let Some(message) = update_man_example_fill_presentation(
                    &mut man_example_fill_enabled,
                    builder.macro_set(),
                    name,
                ) {
                    push_diagnostic(
                        &mut diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::MAN_REDUNDANT_FILL_MODE,
                            Severity::Style,
                            source_id,
                            control_start,
                            end,
                            message,
                        ),
                        &mut truncated,
                    );
                }
                update_fill_mode(environment, builder.macro_set(), name, arguments);
                update_man_indent_register(
                    environment,
                    builder.macro_set(),
                    name,
                    arguments,
                    &mut man_indent_state,
                    limits,
                );
                if environment.is_filled()
                    && is_man_visible_argument_macro(builder.macro_set(), name)
                {
                    emit_filled_macro_argument_tabs(
                        arguments,
                        argument_start,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                }
                if builder.macro_set() == MacroSet::Mdoc {
                    // The argument parser owns the paired quote/tail
                    // recovery.  Emitting the generic mdoc tail finding
                    // first would both reverse mandoc's diagnostic order and
                    // duplicate the tail warning for an unterminated quote.
                    let unterminated_quote = matches!(
                        lex_arguments(raw_arguments, scanner.escape_character(), limits),
                        Err(ArgumentIssue::UnterminatedQuote)
                    );
                    if !unterminated_quote {
                        emit_mdoc_control_trailing_whitespace(
                            name,
                            raw_arguments,
                            end,
                            source_id,
                            limits,
                            &mut diagnostics,
                            &mut truncated,
                        );
                    }
                    emit_mdoc_implicit_trailing_delimiter_spacing(
                        name,
                        raw_arguments,
                        end,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                    emit_mdoc_empty_display(
                        name,
                        arguments,
                        raw_arguments,
                        control_start,
                        end,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                } else if builder.macro_set() == MacroSet::Man {
                    emit_man_alternating_font_trailing_whitespace(
                        name,
                        raw_arguments,
                        end,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                }
                if name == b"while"
                    && let Ok(arguments) =
                        lex_arguments(arguments, scanner.escape_character(), limits)
                {
                    let Some(predicate_template) = arguments.first() else {
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_CONDITION,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff while request is missing its predicate",
                            ),
                            &mut truncated,
                        );
                        continue;
                    };
                    let body_template = join_arguments(&arguments[1..]);
                    let empty_scope_finding = body_template.is_empty().then(|| {
                        diagnostic(
                            DiagnosticCode::ROFF_CONDITION,
                            Severity::Warning,
                            source_id,
                            control_start,
                            control_start.saturating_add(5),
                            "conditional request controls empty scope: while",
                        )
                    });
                    if let Some(finding) = &empty_scope_finding {
                        push_diagnostic(&mut diagnostics, limits, finding.clone(), &mut truncated);
                    }
                    let escape = scanner.escape_character();
                    let scope_remainder = scope_opener_remainder(&body_template, escape);
                    let scope_requested = scope_remainder.is_some();
                    // As with a multiline conditional, mandoc retains the
                    // trailing escape in a conventional `\\{\\` opener as
                    // the logical column of the first loop body node.  The
                    // token offset is relative to the raw argument slice.
                    let scope_opening_column = arguments
                        .get(1)
                        .filter(|argument| argument.bytes.starts_with(&[escape, b'{', escape]))
                        .and_then(|argument| {
                            u32::try_from(argument.offset)
                                .ok()
                                .and_then(|offset| argument_start.checked_add(offset))
                        })
                        .map(|body_start| body_start.saturating_add(2));
                    let mut scope = scope_requested.then(|| {
                        collect_scope(
                            &mut scanner,
                            source_id,
                            limits,
                            builder.macro_set(),
                            &mut diagnostics,
                            &mut truncated,
                            true,
                            control_start,
                            end,
                            Some(b"while"),
                        )
                    });
                    if let (Some(scope), Some(remainder)) = (&mut scope, scope_remainder)
                        && !remainder.is_empty()
                    {
                        scope.lines.insert(
                            0,
                            definition_scope_remainder_line(
                                remainder,
                                start,
                                end,
                                scanner.control_character(),
                                scanner.escape_character(),
                            ),
                        );
                    }
                    if scope_requested && scope.as_ref().is_some_and(|scope| !scope.terminated) {
                        continue;
                    }
                    let mut iterations = 0_usize;
                    loop {
                        let Some(predicate) = expand_environment(
                            environment,
                            &predicate_template.bytes,
                            scanner.escape_character(),
                            &[],
                            limits,
                            source_id,
                            start,
                            end,
                            &mut expansion_steps,
                            &mut diagnostics,
                            &mut truncated,
                        ) else {
                            break 'lines;
                        };
                        let Some(condition) = evaluate_condition(environment, &predicate) else {
                            push_diagnostic(
                                &mut diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ROFF_CONDITION,
                                    Severity::Warning,
                                    source_id,
                                    start,
                                    end,
                                    "roff while predicate is outside the M3 numeric/nroff subset",
                                ),
                                &mut truncated,
                            );
                            break;
                        };
                        if !condition {
                            break;
                        }
                        // A scope opener following an expanded numeric
                        // predicate is reparsed at the compacted roff input
                        // cursor.  The public node still points at the
                        // physical body line, but its logical column must use
                        // the expanded predicate width (and `\{`, not the
                        // continuation escape that follows it).
                        let virtual_scope_opening_position = scope_opening_column
                            .filter(|_| predicate.len() != predicate_template.bytes.len())
                            .and_then(|_| {
                                let opener = arguments.get(1)?;
                                let separator_width = opener.offset.checked_sub(
                                    predicate_template
                                        .offset
                                        .checked_add(predicate_template.bytes.len())?,
                                )?;
                                let control_span =
                                    SourceSpan::new(source_id, control_start, control_start)
                                        .ok()?;
                                let control_position = builder.source_position(&control_span)?;
                                let prefix_width =
                                    argument_start.saturating_sub(control_start) as usize;
                                let column = prefix_width
                                    .saturating_add(predicate.len())
                                    .saturating_add(separator_width)
                                    .saturating_add(2);
                                Some(SourcePosition {
                                    line: control_position.line,
                                    column: control_position.column.saturating_add(
                                        u32::try_from(column).expect(
                                            "bounded roff conditional widths fit source columns",
                                        ),
                                    ),
                                })
                            });
                        if iterations >= limits.max_loop_iterations {
                            truncated = true;
                            push_diagnostic(
                                &mut diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::LIMIT_LOOP_ITERATIONS,
                                    Severity::Warning,
                                    source_id,
                                    start,
                                    end,
                                    "roff while request exceeds max_loop_iterations",
                                ),
                                &mut truncated,
                            );
                            break;
                        }
                        if total_loop_iterations >= limits.max_total_loop_iterations {
                            truncated = true;
                            push_diagnostic(
                                &mut diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::LIMIT_TOTAL_LOOP_ITERATIONS,
                                    Severity::Warning,
                                    source_id,
                                    start,
                                    end,
                                    "roff while requests exceed max_total_loop_iterations",
                                ),
                                &mut truncated,
                            );
                            break;
                        }
                        if !record_expansion_steps(
                            &mut expansion_steps,
                            1,
                            limits,
                            source_id,
                            start,
                            end,
                            &mut diagnostics,
                            &mut truncated,
                        ) {
                            break 'lines;
                        }
                        iterations += 1;
                        total_loop_iterations += 1;
                        if let Some(scope) = &scope {
                            let first_scope_child =
                                builder.children(root).map_or(0, <[NodeId]>::len);
                            let scope_head_line = scope
                                .lines
                                .first()
                                .and_then(|line| {
                                    SourceSpan::new(
                                        source_id,
                                        scope_line_start(line),
                                        scope_line_start(line),
                                    )
                                    .ok()
                                })
                                .and_then(|span| builder.source_position(&span))
                                .map(|position| position.line);
                            let flow = execute_scope_lines(
                                &scope.lines,
                                &mut builder,
                                root,
                                source_id,
                                &mut scanner,
                                environment,
                                limits,
                                &mut text_bytes,
                                &mut expansion_steps,
                                &mut maximum_depth,
                                &mut total_loop_iterations,
                                &mut diagnostics,
                                &mut truncated,
                            );
                            let first_scope_child_is_scope_head = builder
                                .children(root)
                                .and_then(|children| children.get(first_scope_child))
                                .copied()
                                .and_then(|node| builder.node_source_position(node))
                                .zip(scope_head_line)
                                .is_some_and(|(position, line)| position.line == line);
                            // After the first replay, mandoc's roff input
                            // frame attributes retained scope output to the
                            // closing `\\}` line rather than repeatedly to
                            // the body's physical text line. Keep raw spans
                            // sliceable at their authored source while
                            // publishing that observable logical start.
                            if iterations == 1 {
                                if first_scope_child_is_scope_head
                                    && let Some(opener_start) = scope_opening_column
                                {
                                    set_first_scope_child_opening_column(
                                        &mut builder,
                                        root,
                                        first_scope_child,
                                        source_id,
                                        opener_start,
                                    );
                                }
                            } else if let Some(replay_position) =
                                scope_replay_logical_start(&builder, source_id, scope)
                            {
                                let position = scope_opening_column
                                    .filter(|_| first_scope_child_is_scope_head)
                                    .and_then(|opener_start| {
                                        SourceSpan::new(source_id, opener_start, opener_start)
                                            .ok()
                                            .and_then(|span| builder.source_position(&span))
                                    })
                                    .map_or(replay_position, |opening| SourcePosition {
                                        line: replay_position.line,
                                        column: opening.column,
                                    });
                                set_first_root_child_logical_start(
                                    &mut builder,
                                    root,
                                    first_scope_child,
                                    position,
                                );
                                set_new_root_children_logical_start(
                                    &mut builder,
                                    root,
                                    first_scope_child.saturating_add(1),
                                    replay_position,
                                );
                            }
                            match flow {
                                ScopeFlow::Break => break,
                                ScopeFlow::Continue | ScopeFlow::LoopContinue => continue,
                                ScopeFlow::CloseLoopInInnerScope { .. } => {
                                    if first_scope_child_is_scope_head
                                        && let Some(position) = virtual_scope_opening_position
                                    {
                                        set_first_scope_child_logical_start(
                                            &mut builder,
                                            root,
                                            first_scope_child,
                                            position,
                                        );
                                    }
                                    pending_while_out_of_scope = true;
                                    break;
                                }
                                ScopeFlow::Halt => {
                                    break 'lines;
                                }
                            }
                        }
                        let Some(body) = expand_environment(
                            environment,
                            &body_template,
                            scanner.escape_character(),
                            &[],
                            limits,
                            source_id,
                            start,
                            end,
                            &mut expansion_steps,
                            &mut diagnostics,
                            &mut truncated,
                        ) else {
                            break 'lines;
                        };
                        if let Some((request, raw_arguments)) = split_macro_control(
                            &body,
                            scanner.control_character(),
                            scanner.escape_character(),
                        ) && is_environment_request(request)
                        {
                            if matches!(request, b"ds" | b"as") {
                                if let Err(error) = apply_string_request(
                                    &mut environment,
                                    raw_arguments,
                                    scanner.escape_character(),
                                    request == b"as",
                                    limits,
                                    source_id,
                                    start,
                                    end,
                                    &mut expansion_steps,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        environment_error_diagnostic(error, source_id, start, end),
                                        &mut truncated,
                                    );
                                }
                                continue;
                            }
                            let Ok(arguments) =
                                lex_arguments(raw_arguments, scanner.escape_character(), limits)
                            else {
                                truncated = true;
                                push_diagnostic(
                                    &mut diagnostics,
                                    limits,
                                    diagnostic(
                                        DiagnosticCode::ARGUMENT_LIMIT,
                                        Severity::Warning,
                                        source_id,
                                        start,
                                        end,
                                        "roff while body arguments exceed configured parser limits",
                                    ),
                                    &mut truncated,
                                );
                                break;
                            };
                            if let Err(error) = apply_environment_request(
                                &mut environment,
                                builder,
                                request,
                                scanner.escape_character(),
                                &arguments,
                                limits,
                            ) {
                                truncated = true;
                                push_diagnostic(
                                    &mut diagnostics,
                                    limits,
                                    environment_error_diagnostic(error, source_id, start, end),
                                    &mut truncated,
                                );
                                break;
                            }
                            continue;
                        }
                        if let Some((request, raw_arguments)) = split_macro_control(
                            &body,
                            scanner.control_character(),
                            scanner.escape_character(),
                        ) && !is_builtin_package_macro(builder.macro_set(), request)
                            && let Some(definition) = environment.macro_definition(request).cloned()
                        {
                            let Ok(arguments) =
                                lex_arguments(raw_arguments, scanner.escape_character(), limits)
                            else {
                                truncated = true;
                                push_diagnostic(
                                    &mut diagnostics,
                                    limits,
                                    diagnostic(
                                        DiagnosticCode::ARGUMENT_LIMIT,
                                        Severity::Warning,
                                        source_id,
                                        start,
                                        end,
                                        "roff while macro arguments exceed configured parser limits",
                                    ),
                                    &mut truncated,
                                );
                                break;
                            };
                            let arguments = arguments
                                .into_iter()
                                .map(|argument| argument.bytes)
                                .collect::<Vec<_>>();
                            if !record_expansion_steps(
                                &mut expansion_steps,
                                1,
                                limits,
                                source_id,
                                start,
                                end,
                                &mut diagnostics,
                                &mut truncated,
                            ) {
                                break 'lines;
                            }
                            for line in definition.lines {
                                let line = copy_mode_reparse(&line, scanner.escape_character());
                                let Some(bytes) = expand_environment(
                                    environment,
                                    &line,
                                    scanner.escape_character(),
                                    &arguments,
                                    limits,
                                    source_id,
                                    start,
                                    end,
                                    &mut expansion_steps,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) else {
                                    break 'lines;
                                };
                                let result = normalize_document_escapes(
                                    builder,
                                    &bytes,
                                    scanner.escape_character(),
                                    limits,
                                );
                                if !record_expansion_steps(
                                    &mut expansion_steps,
                                    result.steps,
                                    limits,
                                    source_id,
                                    start,
                                    end,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) {
                                    break 'lines;
                                }
                                emit_escape_issues(
                                    &result.issues,
                                    start,
                                    end,
                                    source_id,
                                    limits,
                                    &mut diagnostics,
                                    &mut truncated,
                                );
                                truncated |= result.truncated;
                                if append_text_node(
                                    &mut builder,
                                    root,
                                    source_id,
                                    start,
                                    end,
                                    NodeFlags {
                                        line_start: true,
                                        line_continuation: result.line_continuation,
                                        ..NodeFlags::default()
                                    },
                                    result.text,
                                    limits,
                                    &mut text_bytes,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) {
                                    maximum_depth = maximum_depth.max(2);
                                }
                            }
                            continue;
                        }
                        let result = normalize_document_escapes(
                            builder,
                            &body,
                            scanner.escape_character(),
                            limits,
                        );
                        if !record_expansion_steps(
                            &mut expansion_steps,
                            result.steps,
                            limits,
                            source_id,
                            start,
                            end,
                            &mut diagnostics,
                            &mut truncated,
                        ) {
                            break 'lines;
                        }
                        emit_escape_issues(
                            &result.issues,
                            start,
                            end,
                            source_id,
                            limits,
                            &mut diagnostics,
                            &mut truncated,
                        );
                        truncated |= result.truncated;
                        let flags = NodeFlags {
                            line_start: true,
                            line_continuation: result.line_continuation,
                            ..NodeFlags::default()
                        };
                        let empty_while_body =
                            empty_scope_finding.is_some() && result.text.is_empty();
                        if append_text_node(
                            &mut builder,
                            root,
                            source_id,
                            start,
                            end,
                            flags,
                            result.text,
                            limits,
                            &mut text_bytes,
                            &mut diagnostics,
                            &mut truncated,
                        ) {
                            maximum_depth = maximum_depth.max(2);
                            if empty_while_body
                                && let Some(node) = builder
                                    .children(root)
                                    .and_then(|children| children.last())
                                    .copied()
                                && let Some(position) = builder.node_source_position(node)
                            {
                                let predicate_offset = u32::try_from(predicate_template.offset)
                                    .expect("argument offsets fit source positions");
                                let column = argument_start
                                    .saturating_sub(start)
                                    .saturating_add(predicate_offset)
                                    .saturating_add(2);
                                let _ = builder.set_node_logical_start(
                                    node,
                                    SourcePosition {
                                        line: position.line,
                                        column,
                                    },
                                );
                            }
                        }
                    }
                    if let Some(finding) = empty_scope_finding {
                        push_diagnostic(&mut diagnostics, limits, finding.clone(), &mut truncated);
                        // Reordering the deferred copy moves the first
                        // identical validator finding behind the physical
                        // input-line finding, yielding the upstream
                        // `while`, blank-line, `while` order.
                        deferred_post_validation_diagnostics.push(finding);
                    }
                    continue;
                }
                let raw_condition_arguments = arguments;
                if matches!(name, b"if" | b"ie" | b"el")
                    && let Ok(condition_arguments) =
                        lex_condition_arguments(arguments, scanner.escape_character(), limits)
                {
                    if environment.is_filled() {
                        let diagnostic_start = diagnostics.len();
                        emit_filled_macro_argument_tabs(
                            raw_condition_arguments,
                            argument_start,
                            source_id,
                            limits,
                            &mut diagnostics,
                            &mut truncated,
                        );
                        deferred_post_validation_diagnostics
                            .extend_from_slice(&diagnostics[diagnostic_start..]);
                    }
                    emit_escaped_condition_name(
                        &condition_arguments,
                        scanner.escape_character(),
                        argument_start,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                    let mut escaped_name_body_offset = None;
                    let (condition, body_start) = match name {
                        b"el" => (previous_conditional.take().map(|previous| !previous), 0),
                        b"if" | b"ie" => {
                            if name == b"ie"
                                && (condition_arguments.is_empty()
                                    || condition_arguments
                                        .first()
                                        .is_some_and(|argument| argument.bytes == b"!"))
                            {
                                // mandoc accepts an empty (also a lone
                                // negated-empty) `.ie` as false, leaving the
                                // following `.el` as the active arm.
                                previous_conditional = Some(false);
                                (Some(false), condition_arguments.len())
                            } else {
                                let Some((predicate, body_start)) =
                                    condition_parts(&condition_arguments)
                                else {
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ROFF_CONDITION,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "roff conditional is missing its predicate",
                                        ),
                                        &mut truncated,
                                    );
                                    continue;
                                };
                                let (predicate, escaped_body_offset) =
                                    split_escaped_condition_body(
                                        &condition_arguments,
                                        scanner.escape_character(),
                                        &predicate,
                                    )
                                    .map_or_else(
                                        || (predicate, None),
                                        |(predicate, offset)| (predicate, Some(offset)),
                                    );
                                escaped_name_body_offset = escaped_body_offset;
                                let Some(predicate) = expand_environment(
                                    environment,
                                    &predicate,
                                    scanner.escape_character(),
                                    &[],
                                    limits,
                                    source_id,
                                    start,
                                    end,
                                    &mut expansion_steps,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) else {
                                    break 'lines;
                                };
                                let condition = evaluate_condition(environment, &predicate);
                                if name == b"ie" {
                                    previous_conditional = condition;
                                }
                                (condition, body_start)
                            }
                        }
                        _ => unreachable!("matches! limits the conditional request names"),
                    };
                    let Some(condition) = condition else {
                        if name == b"el" {
                            // An orphaned `.el` is a no-op in mandoc.  In
                            // particular, only the first else consumes the
                            // immediately preceding `.ie` state.
                            continue;
                        }
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_CONDITION,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff conditional predicate is outside the M3 numeric/nroff subset",
                            ),
                            &mut truncated,
                        );
                        continue;
                    };
                    let body_template = condition_body_template_from_offset(
                        raw_condition_arguments,
                        &condition_arguments,
                        body_start,
                        escaped_name_body_offset,
                    );
                    let body_source_start = condition_body_source_start_from_offset(
                        raw_condition_arguments,
                        &condition_arguments,
                        body_start,
                        argument_start,
                        if body_template.is_empty() { end } else { start },
                        escaped_name_body_offset,
                    );
                    // In a same-line brace body, horizontal space directly
                    // after `\{` is scope grammar padding rather than
                    // visible prose.  Multiline scopes retain their original
                    // spelling for source-location accounting below.
                    let body_template_len = body_template.len();
                    let inline_scope_body = body_template
                        .strip_prefix(&[scanner.escape_character(), b'{'])
                        .and_then(|remainder| {
                            let trimmed = trim_horizontal_space(remainder);
                            scope_closer_offset(trimmed, scanner.escape_character())
                                .is_some()
                                .then_some(trimmed)
                        })
                        .filter(|trimmed| trimmed.len().saturating_add(2) != body_template_len);
                    let inline_scope_source_start = body_template
                        .strip_prefix(&[scanner.escape_character(), b'{'])
                        .filter(|remainder| {
                            scope_closer_offset(remainder, scanner.escape_character()).is_some()
                        })
                        .map(|_| {
                            scope_remainder_source_start(
                                &body_template,
                                body_source_start,
                                scanner.escape_character(),
                            )
                        });
                    let body_template = if let Some(trimmed) = inline_scope_body {
                        let mut normalized = Vec::with_capacity(trimmed.len().saturating_add(2));
                        normalized.extend_from_slice(&[scanner.escape_character(), b'{']);
                        normalized.extend_from_slice(trimmed);
                        normalized
                    } else {
                        body_template
                    };
                    let body_template =
                        inline_scope_body_template(&body_template, scanner.escape_character())
                            .unwrap_or(body_template);
                    let body_source_start = inline_scope_source_start.unwrap_or(body_source_start);
                    let escape = scanner.escape_character();
                    let scope_remainder = scope_opener_remainder(&body_template, escape);
                    let scope_requested = scope_remainder.is_some();
                    // The trailing escape in the conventional `\{\\` form
                    // owns the logical column of the first physical scope
                    // line, even though that line has its own byte span.
                    let scope_opening_column = body_template
                        .starts_with(&[escape, b'{', escape])
                        .then(|| body_source_start.saturating_add(2));
                    let bare_scope_opener = scope_remainder.is_some_and(<[u8]>::is_empty)
                        && !body_template.starts_with(&[escape, b'{', escape]);
                    let mut scope = scope_requested.then(|| {
                        collect_scope(
                            &mut scanner,
                            source_id,
                            limits,
                            builder.macro_set(),
                            &mut diagnostics,
                            &mut truncated,
                            condition,
                            control_start,
                            end,
                            Some(name),
                        )
                    });
                    // A bare `\{` (without the conventional continuation
                    // escape) starts its active roff scope with a vertical
                    // blank.  Preserve that event for man validation;
                    // `\{\` intentionally starts directly with the
                    // following physical line instead.
                    if builder.macro_set() == MacroSet::Man
                        && condition
                        && bare_scope_opener
                        && let Some(scope) = &mut scope
                    {
                        let blank_start =
                            scope_remainder_source_start(&body_template, body_source_start, escape);
                        scope.lines.insert(
                            0,
                            ScopeLine::Text {
                                start: blank_start,
                                end: blank_start,
                                bytes: Vec::new(),
                                terminal_inline: false,
                            },
                        );
                    }
                    if let (Some(scope), Some(remainder)) = (&mut scope, scope_remainder) {
                        let remainder = trim_horizontal_space(remainder);
                        if !remainder.is_empty() {
                            scope.lines.insert(
                                0,
                                definition_scope_remainder_line(
                                    remainder,
                                    scope_remainder_source_start(
                                        &body_template,
                                        body_source_start,
                                        escape,
                                    ),
                                    end,
                                    scanner.control_character(),
                                    scanner.escape_character(),
                                ),
                            );
                        }
                    }
                    if let Some(scope) = &scope {
                        if condition {
                            let first_scope_child =
                                builder.children(root).map_or(0, <[NodeId]>::len);
                            let flow = execute_scope_lines(
                                &scope.lines,
                                &mut builder,
                                root,
                                source_id,
                                &mut scanner,
                                environment,
                                limits,
                                &mut text_bytes,
                                &mut expansion_steps,
                                &mut maximum_depth,
                                &mut total_loop_iterations,
                                &mut diagnostics,
                                &mut truncated,
                            );
                            if let Some(opener_start) = scope_opening_column {
                                set_first_scope_child_opening_column(
                                    &mut builder,
                                    root,
                                    first_scope_child,
                                    source_id,
                                    opener_start,
                                );
                            }
                            if matches!(flow, ScopeFlow::Halt) {
                                break 'lines;
                            }
                        }
                        if !condition {
                            record_suppressed_scope_definitions(
                                &scope.lines,
                                scanner.escape_character(),
                                environment,
                                limits,
                            );
                        }
                        continue;
                    }
                    let predicate_end = body_start
                        .checked_sub(1)
                        .and_then(|index| condition_arguments.get(index))
                        .map_or(0, |argument| argument.offset + argument.bytes.len());
                    let next_line_scope = body_template.is_empty()
                        && raw_condition_arguments
                            .get(predicate_end..)
                            .is_some_and(<[u8]>::is_empty);
                    if next_line_scope {
                        // `roff_cond()` uses a next-line scope only when
                        // nothing follows the predicate.  The `.ie` state
                        // remains available for the subsequent `.el` after
                        // this one physical input line has been consumed.
                        // In man input, the active next-line form also
                        // materializes the empty vertical request that
                        // terminates the preceding paragraph before the next
                        // physical line is scanned.  It is an authored
                        // layout event (unlike the private bare-`\{` marker
                        // above), so keep it printable and source-bound.
                        if builder.macro_set() == MacroSet::Man && condition {
                            let _ = append_text_node(
                                &mut builder,
                                root,
                                source_id,
                                end,
                                end,
                                NodeFlags {
                                    line_start: true,
                                    ..NodeFlags::default()
                                },
                                String::new(),
                                limits,
                                &mut text_bytes,
                                &mut diagnostics,
                                &mut truncated,
                            );
                        }
                        next_line_condition = Some(condition);
                        continue;
                    }
                    if name != b"el" && body_template.is_empty() {
                        // Trailing horizontal input after the predicate turns
                        // an otherwise next-line conditional into an empty
                        // scope.  It neither consumes the next physical line
                        // nor depends on whether the predicate was true.
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_CONDITION,
                                Severity::Warning,
                                source_id,
                                control_start,
                                control_start.saturating_add(2),
                                format!(
                                    "conditional request controls empty scope: {}",
                                    visible_bytes(name)
                                ),
                            ),
                            &mut truncated,
                        );
                    }
                    if condition {
                        let Some(body) = expand_environment(
                            environment,
                            &body_template,
                            scanner.escape_character(),
                            &[],
                            limits,
                            source_id,
                            body_source_start,
                            end,
                            &mut expansion_steps,
                            &mut diagnostics,
                            &mut truncated,
                        ) else {
                            break 'lines;
                        };
                        if let Some((request, raw_arguments)) = split_macro_control(
                            &body,
                            scanner.control_character(),
                            scanner.escape_character(),
                        ) {
                            if matches!(request, b"cc" | b"c2" | b"ec") {
                                scanner.apply_character_request(request, raw_arguments);
                                continue;
                            }
                            if is_environment_request(request) {
                                if matches!(request, b"ds" | b"as") {
                                    if let Err(error) = apply_string_request(
                                        &mut environment,
                                        raw_arguments,
                                        scanner.escape_character(),
                                        request == b"as",
                                        limits,
                                        source_id,
                                        start,
                                        end,
                                        &mut expansion_steps,
                                        &mut diagnostics,
                                        &mut truncated,
                                    ) {
                                        truncated = true;
                                        push_diagnostic(
                                            &mut diagnostics,
                                            limits,
                                            environment_error_diagnostic(
                                                error, source_id, start, end,
                                            ),
                                            &mut truncated,
                                        );
                                    }
                                    continue;
                                }
                                let Ok(arguments) = lex_arguments(
                                    raw_arguments,
                                    scanner.escape_character(),
                                    limits,
                                ) else {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ARGUMENT_LIMIT,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "roff conditional body arguments exceed configured parser limits",
                                        ),
                                        &mut truncated,
                                    );
                                    continue;
                                };
                                if let Err(error) = apply_environment_request(
                                    &mut environment,
                                    builder,
                                    request,
                                    scanner.escape_character(),
                                    &arguments,
                                    limits,
                                ) {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        environment_error_diagnostic(error, source_id, start, end),
                                        &mut truncated,
                                    );
                                }
                                continue;
                            }
                            // A same-line conditional can dispatch a man or
                            // mdoc package macro just like ordinary physical
                            // input.  Treating it as raw text loses semantic
                            // constructs such as Pod's `.el .IP ...` option
                            // terms, because the normal scanner dispatch is
                            // bypassed by the conditional executor.
                            if is_builtin_package_macro(builder.macro_set(), request) {
                                let body = ScopeLine::Control {
                                    start: body_source_start,
                                    end,
                                    argument_start: body_source_start
                                        .saturating_add(1)
                                        .saturating_add(
                                            u32::try_from(request.len())
                                                .expect("request names fit source spans"),
                                        )
                                        .saturating_add(u32::from(!raw_arguments.is_empty())),
                                    name: request.to_vec(),
                                    arguments: raw_arguments.to_vec(),
                                };
                                if matches!(
                                    execute_scope_line(
                                        &body,
                                        &mut builder,
                                        root,
                                        source_id,
                                        &mut scanner,
                                        environment,
                                        limits,
                                        &mut text_bytes,
                                        &mut expansion_steps,
                                        &mut maximum_depth,
                                        &mut total_loop_iterations,
                                        &mut diagnostics,
                                        &mut truncated,
                                    ),
                                    ScopeFlow::Halt
                                ) {
                                    break 'lines;
                                }
                                continue;
                            }
                            if !is_builtin_package_macro(builder.macro_set(), request)
                                && let Some(definition) =
                                    environment.macro_definition(request).cloned()
                            {
                                let Ok(arguments) = lex_arguments(
                                    raw_arguments,
                                    scanner.escape_character(),
                                    limits,
                                ) else {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ARGUMENT_LIMIT,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "inline roff conditional macro arguments exceed configured parser limits",
                                        ),
                                        &mut truncated,
                                    );
                                    continue;
                                };
                                let arguments = arguments
                                    .into_iter()
                                    .map(|argument| argument.bytes)
                                    .collect::<Vec<_>>();
                                if matches!(
                                    execute_scope_macro_lines(
                                        definition.lines,
                                        &arguments,
                                        1,
                                        &mut builder,
                                        root,
                                        source_id,
                                        start,
                                        end,
                                        &mut scanner,
                                        environment,
                                        limits,
                                        &mut text_bytes,
                                        &mut expansion_steps,
                                        &mut maximum_depth,
                                        &mut total_loop_iterations,
                                        &mut diagnostics,
                                        &mut truncated,
                                    ),
                                    ScopeFlow::Halt
                                ) {
                                    break 'lines;
                                }
                                continue;
                            }
                        }
                        let result = normalize_document_escapes(
                            builder,
                            &body,
                            scanner.escape_character(),
                            limits,
                        );
                        if !record_expansion_steps(
                            &mut expansion_steps,
                            result.steps,
                            limits,
                            source_id,
                            body_source_start,
                            end,
                            &mut diagnostics,
                            &mut truncated,
                        ) {
                            break 'lines;
                        }
                        emit_escape_issues(
                            &result.issues,
                            body_source_start,
                            end,
                            source_id,
                            limits,
                            &mut diagnostics,
                            &mut truncated,
                        );
                        truncated |= result.truncated;
                        let flags = NodeFlags {
                            line_start: true,
                            line_continuation: result.line_continuation,
                            ..NodeFlags::default()
                        };
                        if append_text_node(
                            &mut builder,
                            root,
                            source_id,
                            body_source_start,
                            end,
                            flags,
                            result.text,
                            limits,
                            &mut text_bytes,
                            &mut diagnostics,
                            &mut truncated,
                        ) {
                            if let Some(node) = builder
                                .children(root)
                                .and_then(|children| children.last())
                                .copied()
                            {
                                let _ = builder.set_node_terminal_inline_conditional(node, true);
                            }
                            maximum_depth = maximum_depth.max(2);
                        }
                    }
                    continue;
                }
                if name == b"return" {
                    push_diagnostic(
                        &mut diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_RETURN_OUTSIDE_MACRO,
                            Severity::Error,
                            source_id,
                            control_start,
                            control_start
                                .saturating_add(u32::try_from(name.len()).unwrap_or(u32::MAX)),
                            "ignoring request outside macro: return",
                        ),
                        &mut truncated,
                    );
                    continue;
                }
                // `.ab` is a formatter-side abort request.  A semantic
                // manual parser cannot perform its process-control effect,
                // but must retain libmandoc's recoverable unsupported
                // finding instead of letting mdoc validation reinterpret it
                // as NAME-section prose.
                if name == b"ab" {
                    push_diagnostic(
                        &mut diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_UNKNOWN_MACRO,
                            Severity::Unsupported,
                            source_id,
                            control_start,
                            end,
                            "unsupported roff request: ab",
                        ),
                        &mut truncated,
                    );
                    continue;
                }
                if name == b"shift" {
                    push_diagnostic(
                        &mut diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_SHIFT,
                            Severity::Error,
                            source_id,
                            control_start,
                            control_start
                                .saturating_add(u32::try_from(name.len()).unwrap_or(u32::MAX)),
                            "ignoring request outside macro: shift",
                        ),
                        &mut truncated,
                    );
                    continue;
                }
                if name == b"so" {
                    let Some(target) = expand_environment(
                        environment,
                        trim_horizontal_space(arguments),
                        scanner.escape_character(),
                        &[],
                        limits,
                        source_id,
                        start,
                        end,
                        &mut expansion_steps,
                        &mut diagnostics,
                        &mut truncated,
                    ) else {
                        break 'lines;
                    };
                    let target = trim_horizontal_space(&target);
                    if target.is_empty() {
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_INCLUDE_UNAVAILABLE,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff include request has no target",
                            ),
                            &mut truncated,
                        );
                        continue;
                    }
                    if include_depth >= limits.max_include_depth {
                        truncated = true;
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::LIMIT_INCLUDE_DEPTH,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff include nesting exceeds max_include_depth",
                            ),
                            &mut truncated,
                        );
                        continue;
                    }
                    let remaining_bytes =
                        limits.max_total_source_bytes.saturating_sub(source_bytes);
                    let resolution = resolver.resolve(IncludeRequest {
                        including: source.name,
                        raw_target: target,
                        remaining_depth: limits.max_include_depth - include_depth,
                        remaining_bytes,
                    });
                    let Ok(resolution) = resolution else {
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_INCLUDE_RESOLVER,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff include resolver rejected the requested target",
                            ),
                            &mut truncated,
                        );
                        continue;
                    };
                    let Some(included) = resolution else {
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_INCLUDE_UNAVAILABLE,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff .so include target is unavailable from the configured resolver",
                            ),
                            &mut truncated,
                        );
                        continue;
                    };
                    if active_sources.iter().any(|active| active == &included.name) {
                        truncated = true;
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_INCLUDE_CYCLE,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff include target would re-enter the active include stack",
                            ),
                            &mut truncated,
                        );
                        continue;
                    }
                    if source_files >= limits.max_sources {
                        truncated = true;
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::LIMIT_SOURCES,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff include graph exceeds max_sources",
                            ),
                            &mut truncated,
                        );
                        continue;
                    }
                    if included.bytes.len() > limits.max_root_source_bytes
                        || source_bytes
                            .checked_add(included.bytes.len())
                            .is_none_or(|total| total > limits.max_total_source_bytes)
                    {
                        truncated = true;
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::LIMIT_SOURCE_BYTES,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "resolved roff include exceeds the configured source-byte budget",
                            ),
                            &mut truncated,
                        );
                        continue;
                    }
                    let resolved_lines = included.bytes.split(|byte| *byte == b'\n').count();
                    if resolved_lines > limits.max_source_lines {
                        truncated = true;
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::LIMIT_SOURCE_LINES,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "resolved roff include exceeds max_source_lines",
                            ),
                            &mut truncated,
                        );
                        continue;
                    }
                    let Some(resolved_source_id) =
                        builder.add_source(Source::new(&included.name, &included.bytes))
                    else {
                        truncated = true;
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::LIMIT_SOURCES,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "resolved roff include cannot be represented in the source map",
                            ),
                            &mut truncated,
                        );
                        continue;
                    };
                    source_bytes += included.bytes.len();
                    source_files += 1;
                    active_sources.push(included.name.clone());
                    let outcome = scan_source(
                        Source::new(&included.name, &included.bytes),
                        config,
                        resolved_source_id,
                        include_depth + 1,
                        builder,
                        environment,
                        diagnostics,
                        deferred_post_validation_diagnostics,
                        text_bytes,
                        expansion_steps,
                        truncated,
                        maximum_depth,
                        previous_conditional,
                        total_loop_iterations,
                        source_bytes,
                        source_files,
                        saw_mdoc_operating_system,
                        active_sources,
                        resolver,
                    );
                    active_sources.pop();
                    diagnostics = outcome.diagnostics;
                    deferred_post_validation_diagnostics =
                        outcome.deferred_post_validation_diagnostics;
                    text_bytes = outcome.text_bytes;
                    expansion_steps = outcome.expansion_steps;
                    truncated = outcome.truncated;
                    maximum_depth = outcome.maximum_depth;
                    previous_conditional = outcome.previous_conditional;
                    total_loop_iterations = outcome.total_loop_iterations;
                    source_bytes = outcome.source_bytes;
                    source_files = outcome.source_files;
                    saw_mdoc_operating_system = outcome.saw_mdoc_operating_system;
                    continue;
                }
                if matches!(name, b"de" | b"de1" | b"am" | b"dei" | b"ami")
                    && let Ok(arguments) =
                        lex_arguments(arguments, scanner.escape_character(), limits)
                {
                    let Some(definition_name) = arguments.first() else {
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_EMPTY_REQUEST,
                                Severity::Warning,
                                source_id,
                                control_start,
                                control_start.saturating_add(2),
                                format!("skipping empty request: {}", visible_bytes(name)),
                            ),
                            &mut truncated,
                        );
                        continue;
                    };
                    let indirect = matches!(name, b"dei" | b"ami");
                    let name_terminates_at_tab = definition_name.separator_after == Some(b'\t');
                    if !indirect && !name_terminates_at_tab && arguments.get(2).is_some() {
                        let ignored_after_tab = arguments
                            .get(1)
                            .is_some_and(|terminator| terminator.separator_after == Some(b'\t'));
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_ALL_ARGUMENTS,
                                Severity::Error,
                                source_id,
                                argument_start,
                                end,
                                format!(
                                    "skipping excess arguments: .{} ... {}",
                                    visible_bytes(name),
                                    if ignored_after_tab {
                                        "ignored"
                                    } else {
                                        "excess arguments"
                                    },
                                ),
                            ),
                            &mut truncated,
                        );
                    }
                    let definition_name = if indirect {
                        let Some(definition_name) =
                            environment.indirect_string(&definition_name.bytes)
                        else {
                            let name_start = argument_start.saturating_add(
                                u32::try_from(definition_name.offset)
                                    .expect("definition-name offsets fit source positions"),
                            );
                            push_diagnostic(
                                &mut diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ROFF_UNDEFINED_REFERENCE,
                                    Severity::Warning,
                                    source_id,
                                    name_start,
                                    end,
                                    format!(
                                        "undefined string, using \"\": {}",
                                        visible_bytes(&definition_name.bytes)
                                    ),
                                ),
                                &mut truncated,
                            );
                            push_diagnostic(
                                &mut diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ROFF_EMPTY_REQUEST,
                                    Severity::Warning,
                                    source_id,
                                    control_start,
                                    control_start.saturating_add(2),
                                    format!("skipping empty request: {}", visible_bytes(name)),
                                ),
                                &mut truncated,
                            );
                            continue;
                        };
                        definition_name
                    } else {
                        let normalized = normalize_roff_name_prefix(
                            &definition_name.bytes,
                            scanner.escape_character(),
                        );
                        if let Some(preview) = normalized.invalid_escape_preview {
                            push_diagnostic(
                                &mut diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ROFF_ESCAPED_NAME,
                                    Severity::Error,
                                    source_id,
                                    control_start,
                                    control_start.saturating_add(1),
                                    format!(
                                        "escaped character not allowed in a name: {}",
                                        visible_bytes(&preview)
                                    ),
                                ),
                                &mut truncated,
                            );
                        }
                        normalized.name
                    };
                    if definition_name.is_empty() {
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_EMPTY_REQUEST,
                                Severity::Warning,
                                source_id,
                                control_start,
                                control_start.saturating_add(2),
                                format!("skipping empty request: {}", visible_bytes(name)),
                            ),
                            &mut truncated,
                        );
                        continue;
                    }
                    let append = matches!(name, b"am" | b"ami");
                    let terminator = match arguments.get(1).filter(|_| !name_terminates_at_tab) {
                        None => vec![b'.'],
                        Some(argument) if !indirect => argument.bytes.clone(),
                        Some(argument) => {
                            if let Some(terminator) = environment.indirect_string(&argument.bytes) {
                                terminator
                            } else {
                                let terminator_start = argument_start.saturating_add(
                                    u32::try_from(argument.offset)
                                        .expect("terminator offsets fit source positions"),
                                );
                                push_diagnostic(
                                    &mut diagnostics,
                                    limits,
                                    diagnostic(
                                        DiagnosticCode::ROFF_UNDEFINED_REFERENCE,
                                        Severity::Warning,
                                        source_id,
                                        terminator_start,
                                        end,
                                        format!(
                                            "undefined string, using \"\": {}",
                                            visible_bytes(&argument.bytes)
                                        ),
                                    ),
                                    &mut truncated,
                                );
                                // An unresolved indirect end marker emits
                                // its string finding, then falls back to the
                                // traditional `..` terminator. The first
                                // indirect name is still a valid macro name,
                                // so following copy-mode input belongs to
                                // that recovered definition.
                                vec![b'.']
                            }
                        }
                    };
                    let definition_control = scanner.control_character();
                    let mut body = Vec::new();
                    let mut terminated = false;
                    while let Some(body_line) = scanner.next_raw_line() {
                        if is_definition_terminator(
                            body_line.bytes,
                            definition_control,
                            &terminator,
                        ) {
                            terminated = true;
                            break;
                        }
                        if body_line.too_long {
                            truncated = true;
                            push_diagnostic(
                                &mut diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::LIMIT_LINE_BYTES,
                                    Severity::Warning,
                                    source_id,
                                    body_line.start,
                                    body_line.end,
                                    "copy-mode macro line exceeds max_line_bytes and was skipped",
                                ),
                                &mut truncated,
                            );
                            continue;
                        }
                        let Some(copy_mode_line) = expand_copy_mode_definition(
                            environment,
                            body_line.bytes,
                            scanner.escape_character(),
                            limits,
                            source_id,
                            body_line.start,
                            body_line.end,
                            &mut expansion_steps,
                            &mut diagnostics,
                            &mut truncated,
                        ) else {
                            break 'lines;
                        };
                        body.push(copy_mode_reparse(
                            &copy_mode_line,
                            scanner.escape_character(),
                        ));
                    }
                    if !terminated {
                        truncated = true;
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_UNTERMINATED_DEFINITION,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff macro definition reached source end before its `..` terminator",
                            ),
                            &mut truncated,
                        );
                    }
                    let definition = if indirect {
                        environment.define_indirect_macro(&definition_name, body, append, limits)
                    } else {
                        environment.define_macro(&definition_name, body, append, limits)
                    };
                    if let Err(error) = definition {
                        truncated = true;
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            environment_error_diagnostic(error, source_id, start, end),
                            &mut truncated,
                        );
                    }
                    continue;
                }
                if name == b"ig" {
                    let arguments =
                        match lex_arguments(arguments, scanner.escape_character(), limits) {
                            Ok(arguments) => arguments,
                            Err(ArgumentIssue::UnterminatedQuote) => {
                                push_diagnostic(
                                    &mut diagnostics,
                                    limits,
                                    diagnostic(
                                        DiagnosticCode::ARGUMENT_UNTERMINATED_QUOTE,
                                        Severity::Warning,
                                        source_id,
                                        start,
                                        end,
                                        "roff ignore-block marker contains an unterminated quote",
                                    ),
                                    &mut truncated,
                                );
                                Vec::new()
                            }
                            Err(ArgumentIssue::Limit) => {
                                truncated = true;
                                push_diagnostic(
                                    &mut diagnostics,
                                    limits,
                                    diagnostic(
                                        DiagnosticCode::ARGUMENT_LIMIT,
                                        Severity::Warning,
                                        source_id,
                                        start,
                                        end,
                                        "roff ignore-block marker exceeds configured parser limits",
                                    ),
                                    &mut truncated,
                                );
                                Vec::new()
                            }
                        };
                    if let [marker, excess, ..] = arguments.as_slice() {
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_EXCESS_ARGUMENTS,
                                Severity::Error,
                                source_id,
                                argument_start,
                                argument_start.saturating_add(
                                    u32::try_from(marker.bytes.len()).unwrap_or(u32::MAX),
                                ),
                                format!(
                                    "skipping excess arguments: .ig ... {}",
                                    visible_bytes(&excess.bytes)
                                ),
                            ),
                            &mut truncated,
                        );
                    }
                    let marker = arguments
                        .first()
                        .map_or_else(|| vec![b'.'], |argument| argument.bytes.clone());
                    let mut terminated = false;
                    while let Some(ignored) = scanner.next_raw_line() {
                        if is_ignore_terminator(ignored.bytes, scanner.control_character(), &marker)
                        {
                            terminated = true;
                            break;
                        }
                    }
                    if !terminated {
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_UNCLOSED_IGNORE,
                                Severity::Error,
                                source_id,
                                control_start,
                                control_start.saturating_add(2),
                                "appending missing end of block: ig",
                            ),
                            &mut truncated,
                        );
                    }
                    continue;
                }
                if name == b"." {
                    push_diagnostic(
                        &mut diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_UNMATCHED_END,
                            Severity::Error,
                            source_id,
                            control_start,
                            control_start.saturating_add(1),
                            "skipping end of block that is not open: ..",
                        ),
                        &mut truncated,
                    );
                    continue;
                }
                if name == b"tr" {
                    emit_translation_request_diagnostics(
                        arguments,
                        scanner.escape_character(),
                        control_start,
                        argument_start,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                    environment.define_translation(arguments, scanner.escape_character());
                    continue;
                }
                if name == b"ft" {
                    emit_font_request_diagnostics(
                        arguments,
                        scanner.escape_character(),
                        argument_start,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                    if builder.macro_set() == MacroSet::Man
                        && let Ok(font_arguments) =
                            lex_arguments(arguments, scanner.escape_character(), limits)
                        && let Some(font) = font_arguments.first()
                        && !is_legacy_roff_font_selector(&font.bytes)
                    {
                        let diagnostic_start = diagnostics.len();
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_UNKNOWN_FONT,
                                Severity::Warning,
                                source_id,
                                control_start,
                                control_start.saturating_add(2),
                                format!(
                                    "unknown font, skipping request: ft {}",
                                    visible_bytes(&font.bytes)
                                ),
                            ),
                            &mut truncated,
                        );
                        deferred_post_validation_diagnostics
                            .extend_from_slice(&diagnostics[diagnostic_start..]);
                        continue;
                    }
                }
                if matches!(name, b"ds" | b"as") {
                    if let Ok(request_arguments) =
                        lex_arguments(arguments, scanner.escape_character(), limits)
                    {
                        emit_escaped_request_name(
                            &request_arguments,
                            scanner.escape_character(),
                            argument_start,
                            source_id,
                            limits,
                            &mut diagnostics,
                            &mut truncated,
                        );
                    }
                    match apply_string_request(
                        &mut environment,
                        arguments,
                        scanner.escape_character(),
                        name == b"as",
                        limits,
                        source_id,
                        start,
                        end,
                        &mut expansion_steps,
                        &mut diagnostics,
                        &mut truncated,
                    ) {
                        Ok(()) => continue,
                        Err(error) => {
                            truncated = true;
                            push_diagnostic(
                                &mut diagnostics,
                                limits,
                                environment_error_diagnostic(error, source_id, start, end),
                                &mut truncated,
                            );
                            continue;
                        }
                    }
                }
                if name == b"Os"
                    && (builder.macro_set() == MacroSet::Mdoc || config.syntax == Syntax::Mdoc)
                {
                    match lex_arguments(arguments, scanner.escape_character(), limits) {
                        Ok(arguments) if arguments.is_empty() => {
                            if let Some(operating_system) = config.operating_system.as_deref() {
                                builder.operating_system(operating_system);
                            }
                        }
                        Ok(arguments) => {
                            // The author-selected value wins over the session fallback.
                            // M6 will perform full mdoc argument semantics; scanner-stage
                            // metadata already uses the public visible-byte normalization.
                            builder.operating_system(visible_bytes(&join_arguments(&arguments)));
                        }
                        Err(ArgumentIssue::UnterminatedQuote) => push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ARGUMENT_UNTERMINATED_QUOTE,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "mdoc Os arguments contain an unterminated quote",
                            ),
                            &mut truncated,
                        ),
                        Err(ArgumentIssue::Limit) => {
                            truncated = true;
                            push_diagnostic(
                                &mut diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ARGUMENT_LIMIT,
                                    Severity::Warning,
                                    source_id,
                                    start,
                                    end,
                                    "mdoc Os arguments exceed configured parser limits",
                                ),
                                &mut truncated,
                            );
                        }
                    }
                }
                if name == b"char" {
                    validate_character_request(
                        arguments,
                        scanner.escape_character(),
                        environment,
                        source_id,
                        argument_start,
                        end,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                    continue;
                }
                if name == b"it" {
                    if !arm_input_trap(&mut input_trap, arguments) {
                        let display = visible_bytes(trim_horizontal_space(arguments));
                        let display = (!display.is_empty()).then(|| format!(" {display}"));
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_NON_NUMERIC_ARGUMENT,
                                Severity::Error,
                                source_id,
                                control_start,
                                control_start.saturating_add(2),
                                format!(
                                    "skipping request without numeric argument: it{}",
                                    display.unwrap_or_default()
                                ),
                            ),
                            &mut truncated,
                        );
                    }
                    continue;
                }
                if matches!(name, b"nr" | b"rr")
                    && let Ok(arguments) =
                        lex_arguments(arguments, scanner.escape_character(), limits)
                {
                    emit_escaped_request_name(
                        &arguments,
                        scanner.escape_character(),
                        argument_start,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                }
                if name == b"rm"
                    && let Ok(arguments) =
                        lex_arguments(arguments, scanner.escape_character(), limits)
                {
                    for argument in &arguments {
                        let normalized =
                            normalize_roff_name_prefix(&argument.bytes, scanner.escape_character());
                        if normalized.invalid_escape_preview.is_some() {
                            emit_escaped_request_name(
                                std::slice::from_ref(argument),
                                scanner.escape_character(),
                                argument_start,
                                source_id,
                                limits,
                                &mut diagnostics,
                                &mut truncated,
                            );
                            break;
                        }
                    }
                }
                if is_environment_request(name)
                    && let Ok(arguments) =
                        lex_arguments(arguments, scanner.escape_character(), limits)
                {
                    let division_by_zero = (name == b"nr")
                        .then(|| register_division_by_zero(&arguments))
                        .flatten();
                    match apply_environment_request(
                        &mut environment,
                        builder,
                        name,
                        scanner.escape_character(),
                        &arguments,
                        limits,
                    ) {
                        Ok(()) => {
                            if let Some(expression) = division_by_zero {
                                push_diagnostic(
                                    &mut diagnostics,
                                    limits,
                                    diagnostic(
                                        DiagnosticCode::ROFF_DIVISION_BY_ZERO,
                                        Severity::Error,
                                        source_id,
                                        control_start.saturating_add(2),
                                        control_start.saturating_add(3),
                                        format!(
                                            "divide by zero: {}",
                                            visible_bytes(&expression.bytes)
                                        ),
                                    ),
                                    &mut truncated,
                                );
                            }
                            continue;
                        }
                        Err(error) => {
                            truncated = true;
                            push_diagnostic(
                                &mut diagnostics,
                                limits,
                                environment_error_diagnostic(error, source_id, start, end),
                                &mut truncated,
                            );
                            continue;
                        }
                    }
                }
                let renamed_package_macro = environment.renamed_package_macro(name).is_some();
                let dispatched_package_macro =
                    environment.renamed_package_macro(name).unwrap_or(name);
                let builtin_package_macro =
                    is_builtin_package_macro(builder.macro_set(), dispatched_package_macro);
                if !builtin_package_macro && environment.is_suppressed_macro_name(name) {
                    push_diagnostic(
                        &mut diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_UNKNOWN_MACRO,
                            Severity::Error,
                            source_id,
                            control_start,
                            end,
                            format!(
                                "skipping unknown macro: .{}",
                                attached_name.as_ref().map_or_else(
                                    || visible_bytes(name),
                                    |recovery| { visible_bytes(&recovery.display_name) }
                                )
                            ),
                        ),
                        &mut truncated,
                    );
                    continue;
                }
                if !builtin_package_macro && environment.is_conditionally_unknown_macro(name) {
                    push_diagnostic(
                        &mut diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_UNKNOWN_MACRO,
                            Severity::Error,
                            source_id,
                            control_start,
                            end,
                            format!("skipping unknown macro: .{}", visible_bytes(name)),
                        ),
                        &mut truncated,
                    );
                    // `roff_userdef()` installs the observed unknown control
                    // as an empty user macro after reporting it.  A later
                    // `dname` condition therefore becomes true until a real
                    // `.de` replaces that placeholder.
                    if let Err(error) = environment.define_macro(name, Vec::new(), false, limits) {
                        truncated = true;
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            environment_error_diagnostic(error, source_id, start, end),
                            &mut truncated,
                        );
                    }
                    continue;
                }
                if !builtin_package_macro && environment.is_empty_string(name) {
                    continue;
                }
                let appended_package_macro =
                    builtin_package_macro && environment.has_appended_macro_definition(name);
                // mandoc keeps a renamed package macro's original argument
                // cursor while executing an `.am` body.  For a no-argument
                // invocation that cursor is the final byte of the authored
                // alias (for example `.myBc` after `.rn Bc myBc`), and its
                // generic argument reader emits the usual end-of-line style
                // finding there.  Keep this deliberately scoped to the
                // renamed-and-appended package path: ordinary no-argument
                // package controls do not imply trailing whitespace.
                if appended_package_macro && raw_arguments.is_empty() {
                    // The position advances by the original package name,
                    // not the (possibly longer) alias spelling.
                    let alias_end = control_start.saturating_add(
                        u32::try_from(dispatched_package_macro.len())
                            .expect("parsed control names fit public source offsets"),
                    );
                    push_diagnostic(
                        &mut diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::INPUT_TRAILING_WHITESPACE,
                            Severity::Style,
                            source_id,
                            alias_end,
                            alias_end,
                            "whitespace at end of input line",
                        ),
                        &mut truncated,
                    );
                }
                if (!builtin_package_macro || appended_package_macro)
                    && let Some(definition) = environment.macro_definition(name).cloned()
                {
                    if environment.is_filled() {
                        let diagnostic_start = diagnostics.len();
                        emit_user_macro_leading_tabs(
                            raw_arguments,
                            control_start,
                            name.len(),
                            source_id,
                            limits,
                            &mut diagnostics,
                            &mut truncated,
                        );
                        deferred_post_validation_diagnostics
                            .extend_from_slice(&diagnostics[diagnostic_start..]);
                    }
                    let unterminated_quote = matches!(
                        lex_user_macro_arguments(arguments, scanner.escape_character(), limits),
                        Err(ArgumentIssue::UnterminatedQuote)
                    );
                    if !unterminated_quote
                        && builder.macro_set() != MacroSet::Mdoc
                        && trailing_whitespace_start(arguments).is_some()
                    {
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::INPUT_TRAILING_WHITESPACE,
                                Severity::Style,
                                source_id,
                                end,
                                end,
                                "whitespace at end of input line",
                            ),
                            &mut truncated,
                        );
                    }
                    let mut arguments = match lex_user_macro_arguments(
                        arguments,
                        scanner.escape_character(),
                        limits,
                    ) {
                        Ok(arguments) => arguments,
                        Err(ArgumentIssue::UnterminatedQuote) => {
                            emit_unterminated_quoted_argument(
                                arguments,
                                argument_start,
                                end,
                                source_id,
                                limits,
                                &mut diagnostics,
                                &mut truncated,
                            );
                            match recover_unterminated_quoted_arguments(
                                arguments,
                                scanner.escape_character(),
                                limits,
                            ) {
                                Ok(arguments) => arguments,
                                Err(ArgumentIssue::UnterminatedQuote) => unreachable!(
                                    "the synthetic closing quote always completes a bounded token"
                                ),
                                Err(ArgumentIssue::Limit) => {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ARGUMENT_LIMIT,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "macro invocation arguments exceed configured parser limits",
                                        ),
                                        &mut truncated,
                                    );
                                    continue;
                                }
                            }
                        }
                        Err(ArgumentIssue::Limit) => {
                            truncated = true;
                            push_diagnostic(
                                &mut diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ARGUMENT_LIMIT,
                                    Severity::Warning,
                                    source_id,
                                    start,
                                    end,
                                    "macro invocation arguments exceed configured parser limits",
                                ),
                                &mut truncated,
                            );
                            continue;
                        }
                    };
                    retain_user_macro_tab_argument_prefix(&mut arguments, raw_arguments);
                    if appended_package_macro {
                        let Some(element) = append_node(
                            &mut builder,
                            root,
                            NodeKind::Element,
                            source_id,
                            control_start,
                            end,
                            NodeFlags {
                                line_start: true,
                                ..NodeFlags::default()
                            },
                            limits,
                            &mut diagnostics,
                            &mut truncated,
                        ) else {
                            continue;
                        };
                        if !builder.macro_name(element, visible_bytes(dispatched_package_macro)) {
                            truncated = true;
                            continue;
                        }
                        maximum_depth = maximum_depth.max(2);
                        for argument in &arguments {
                            let argument_offset = u32::try_from(argument.offset)
                                .expect("argument offsets are bounded by line length");
                            if !append_text_node(
                                &mut builder,
                                element,
                                source_id,
                                argument_start
                                    .checked_add(argument_offset)
                                    .expect("parser checks public span offsets first"),
                                end,
                                NodeFlags::default(),
                                visible_bytes(&argument.bytes),
                                limits,
                                &mut text_bytes,
                                &mut diagnostics,
                                &mut truncated,
                            ) {
                                break 'lines;
                            }
                            maximum_depth = maximum_depth.max(3);
                        }
                    }
                    // Macro-generated mdoc arguments normally inherit the
                    // caller's first argument column.  The empty alias form
                    // has no such byte: mandoc carries its argument cursor
                    // at the alias's final byte while it runs the appended
                    // body.  Preserve that source provenance for controls
                    // emitted by the body (notably `.Pq` in rn/append).
                    let macro_generated_argument_start =
                        if appended_package_macro && raw_arguments.is_empty() {
                            argument_start.saturating_sub(1)
                        } else {
                            argument_start
                        };
                    let arguments = arguments
                        .into_iter()
                        .map(|argument| {
                            macro_argument_copy_mode_reparse(
                                &argument.bytes,
                                scanner.escape_character(),
                            )
                        })
                        .collect::<Vec<_>>();
                    if !record_expansion_steps(
                        &mut expansion_steps,
                        1,
                        limits,
                        source_id,
                        start,
                        end,
                        &mut diagnostics,
                        &mut truncated,
                    ) {
                        break 'lines;
                    }
                    let mut pending = definition
                        .lines
                        .into_iter()
                        .rev()
                        .map(|line| (line, arguments.clone(), 1_usize, 0_u32, None, false))
                        .collect::<Vec<_>>();
                    let mut macro_conditionals = Vec::<(usize, bool)>::new();
                    while let Some((
                        source_line,
                        macro_arguments,
                        macro_depth,
                        macro_origin,
                        text_origin,
                        scope_reparse,
                    )) = pending.pop()
                    {
                        let body_line = normalize_macro_argument_number_escapes(
                            &copy_mode_reparse(&source_line, scanner.escape_character()),
                            scanner.escape_character(),
                            start,
                            &builder,
                            source_id,
                            limits,
                            &mut diagnostics,
                            &mut truncated,
                        );
                        if let Some((request, raw_arguments)) = split_macro_control(
                            &body_line,
                            scanner.control_character(),
                            scanner.escape_character(),
                        ) {
                            // Physical comments are removed by `Scanner`, but
                            // a copy-mode macro body is re-dispatched here.
                            // Treat its `\"` request identically instead of
                            // publishing Sphinx's bookkeeping comments as
                            // ordinary text between transparent indents.
                            if is_macro_comment_request(request, scanner.escape_character()) {
                                continue;
                            }
                            if matches!(request, b"cc" | b"c2" | b"ec") {
                                scanner.apply_character_request(request, raw_arguments);
                                continue;
                            }
                            if request == b"return" {
                                pending.retain(|(_, _, depth, _, _, _)| *depth < macro_depth);
                                continue;
                            }
                            if request == b"shift" {
                                let count = match lex_arguments(
                                    raw_arguments,
                                    scanner.escape_character(),
                                    limits,
                                ) {
                                    Ok(arguments) => {
                                        arguments.first().map_or(Ok(1_usize), |argument| {
                                            std::str::from_utf8(&argument.bytes)
                                                .ok()
                                                .and_then(|value| value.parse::<usize>().ok())
                                                .ok_or(())
                                        })
                                    }
                                    Err(_) => Err(()),
                                };
                                let request_argument_start = start.saturating_add(
                                    u32::try_from(
                                        body_line.len().saturating_sub(raw_arguments.len()),
                                    )
                                    .expect("bounded macro body offsets fit public spans"),
                                );
                                let count = if let Ok(count) = count {
                                    count
                                } else {
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ROFF_SHIFT,
                                            Severity::Error,
                                            source_id,
                                            request_argument_start,
                                            request_argument_start.saturating_add(
                                                u32::try_from(raw_arguments.len()).expect(
                                                    "bounded macro body offsets fit public spans",
                                                ),
                                            ),
                                            format!(
                                                "argument is not numeric, using 1: shift {}",
                                                visible_bytes(raw_arguments)
                                            ),
                                        ),
                                        &mut truncated,
                                    );
                                    1
                                };
                                let maximum = pending
                                    .iter()
                                    .filter(|(_, _, depth, _, _, _)| *depth == macro_depth)
                                    .map(|(_, arguments, _, _, _, _)| arguments.len())
                                    .max()
                                    .unwrap_or_default();
                                if count > maximum {
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ROFF_SHIFT,
                                            Severity::Error,
                                            source_id,
                                            start.saturating_add(
                                                u32::try_from(body_line.len()).expect(
                                                    "bounded macro body offsets fit public spans",
                                                ),
                                            ),
                                            start.saturating_add(
                                                u32::try_from(body_line.len()).expect(
                                                    "bounded macro body offsets fit public spans",
                                                ),
                                            ),
                                            format!(
                                                "excessive shift: {count}, but max is {maximum}"
                                            ),
                                        ),
                                        &mut truncated,
                                    );
                                }
                                for (_, pending_arguments, depth, _, _, _) in &mut pending {
                                    if *depth == macro_depth {
                                        let count = count.min(pending_arguments.len());
                                        pending_arguments.drain(..count);
                                    }
                                }
                                continue;
                            }
                            if request == b"tr" {
                                environment
                                    .define_translation(raw_arguments, scanner.escape_character());
                                continue;
                            }
                            if matches!(request, b"if" | b"ie" | b"el") {
                                let Ok(condition_arguments) = lex_condition_arguments(
                                    raw_arguments,
                                    scanner.escape_character(),
                                    limits,
                                ) else {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ARGUMENT_LIMIT,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "roff conditional arguments in a macro exceed configured parser limits",
                                        ),
                                        &mut truncated,
                                    );
                                    continue;
                                };
                                let (condition, body_start, predicate_width) = match request {
                                    b"el" => {
                                        let condition = macro_conditionals
                                            .iter()
                                            .rposition(|(depth, _)| *depth == macro_depth)
                                            .map(|index| !macro_conditionals.remove(index).1);
                                        (condition, 0, None)
                                    }
                                    b"if" | b"ie" => {
                                        if request == b"ie"
                                            && (condition_arguments.is_empty()
                                                || condition_arguments
                                                    .first()
                                                    .is_some_and(|argument| argument.bytes == b"!"))
                                        {
                                            macro_conditionals
                                                .retain(|(depth, _)| *depth != macro_depth);
                                            macro_conditionals.push((macro_depth, false));
                                            (Some(false), condition_arguments.len(), None)
                                        } else {
                                            let Some((predicate, body_start)) =
                                                condition_parts(&condition_arguments)
                                            else {
                                                push_diagnostic(
                                                    &mut diagnostics,
                                                    limits,
                                                    diagnostic(
                                                        DiagnosticCode::ROFF_CONDITION,
                                                        Severity::Warning,
                                                        source_id,
                                                        start,
                                                        end,
                                                        "roff conditional in a macro is missing its predicate",
                                                    ),
                                                    &mut truncated,
                                                );
                                                continue;
                                            };
                                            let Some(predicate) = expand_environment(
                                                &mut environment,
                                                &predicate,
                                                scanner.escape_character(),
                                                &macro_arguments,
                                                limits,
                                                source_id,
                                                start,
                                                end,
                                                &mut expansion_steps,
                                                &mut diagnostics,
                                                &mut truncated,
                                            ) else {
                                                break 'lines;
                                            };
                                            let condition =
                                                evaluate_condition(environment, &predicate);
                                            if request == b"ie"
                                                && let Some(condition) = condition
                                            {
                                                macro_conditionals
                                                    .retain(|(depth, _)| *depth != macro_depth);
                                                macro_conditionals.push((macro_depth, condition));
                                            }
                                            (condition, body_start, Some(predicate.len()))
                                        }
                                    }
                                    _ => unreachable!("conditional request was filtered above"),
                                };
                                let Some(condition) = condition else {
                                    if request == b"el" {
                                        continue;
                                    }
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ROFF_CONDITION,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "roff conditional in a macro is outside the M3 numeric/nroff subset",
                                        ),
                                        &mut truncated,
                                    );
                                    continue;
                                };
                                let body_template = condition_body_template(
                                    raw_arguments,
                                    &condition_arguments,
                                    body_start,
                                );
                                let escape = scanner.escape_character();
                                if is_scope_opener(&body_template, escape) {
                                    let Some(scope) = collect_pending_macro_scope(
                                        &mut pending,
                                        macro_depth,
                                        scanner.control_character(),
                                        escape,
                                        limits,
                                    ) else {
                                        truncated = true;
                                        push_diagnostic(
                                            &mut diagnostics,
                                            limits,
                                            diagnostic(
                                                DiagnosticCode::ROFF_UNTERMINATED_SCOPE,
                                                Severity::Warning,
                                                source_id,
                                                start,
                                                end,
                                                "roff macro conditional reached its caller before its `\\}` terminator",
                                            ),
                                            &mut truncated,
                                        );
                                        continue;
                                    };
                                    if condition {
                                        let mut scope = scope;
                                        if macro_origin == 0 {
                                            let scope_origin = macro_scope_body_origin(
                                                &body_line,
                                                scanner.control_character(),
                                                predicate_width,
                                            );
                                            for (index, line) in scope.iter_mut().enumerate() {
                                                if index == 0 {
                                                    if let Some(origin) = scope_origin {
                                                        line.3 = origin;
                                                    }
                                                } else {
                                                    line.3 = 0;
                                                }
                                                line.5 = true;
                                            }
                                        }
                                        pending.extend(scope.into_iter().rev());
                                    }
                                    continue;
                                }
                                if condition && !body_template.is_empty() {
                                    pending.push((
                                        body_template,
                                        macro_arguments,
                                        macro_depth,
                                        macro_origin,
                                        macro_conditional_body_origin(
                                            &body_line,
                                            raw_arguments,
                                            &condition_arguments,
                                            body_start,
                                            predicate_width,
                                        ),
                                        false,
                                    ));
                                }
                                continue;
                            }
                            if request == b"ig" {
                                let marker = match ignore_marker(
                                    raw_arguments,
                                    scanner.escape_character(),
                                    limits,
                                ) {
                                    Ok(marker) => marker,
                                    Err(ArgumentIssue::UnterminatedQuote) => {
                                        push_diagnostic(
                                            &mut diagnostics,
                                            limits,
                                            diagnostic(
                                                DiagnosticCode::ARGUMENT_UNTERMINATED_QUOTE,
                                                Severity::Warning,
                                                source_id,
                                                start,
                                                end,
                                                "roff ignore-block marker in a macro contains an unterminated quote",
                                            ),
                                            &mut truncated,
                                        );
                                        vec![b'.']
                                    }
                                    Err(ArgumentIssue::Limit) => {
                                        truncated = true;
                                        push_diagnostic(
                                            &mut diagnostics,
                                            limits,
                                            diagnostic(
                                                DiagnosticCode::ARGUMENT_LIMIT,
                                                Severity::Warning,
                                                source_id,
                                                start,
                                                end,
                                                "roff ignore-block marker in a macro exceeds configured parser limits",
                                            ),
                                            &mut truncated,
                                        );
                                        vec![b'.']
                                    }
                                };
                                consume_ignore_block(&mut scanner, &marker);
                                continue;
                            }
                            if matches!(request, b"de" | b"de1" | b"am" | b"dei" | b"ami") {
                                let Ok(definition_arguments) = lex_arguments(
                                    raw_arguments,
                                    scanner.escape_character(),
                                    limits,
                                ) else {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ARGUMENT_LIMIT,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "generated roff macro definition arguments exceed configured parser limits",
                                        ),
                                        &mut truncated,
                                    );
                                    continue;
                                };
                                let Some(definition_name) = definition_arguments.first() else {
                                    continue;
                                };
                                let indirect = matches!(request, b"dei" | b"ami");
                                let Some(definition_name) =
                                    (!indirect).then(|| definition_name.bytes.clone()).or_else(
                                        || environment.indirect_string(&definition_name.bytes),
                                    )
                                else {
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ROFF_UNDEFINED_REFERENCE,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "generated indirect roff macro definition names an undefined string",
                                        ),
                                        &mut truncated,
                                    );
                                    continue;
                                };
                                let terminator = match definition_arguments.get(1) {
                                    None => vec![b'.'],
                                    Some(argument) if !indirect => argument.bytes.clone(),
                                    Some(argument) => {
                                        let Some(terminator) =
                                            environment.indirect_string(&argument.bytes)
                                        else {
                                            push_diagnostic(
                                                &mut diagnostics,
                                                limits,
                                                diagnostic(
                                                    DiagnosticCode::ROFF_UNDEFINED_REFERENCE,
                                                    Severity::Warning,
                                                    source_id,
                                                    start,
                                                    end,
                                                    "generated indirect roff macro terminator names an undefined string",
                                                ),
                                                &mut truncated,
                                            );
                                            continue;
                                        };
                                        terminator
                                    }
                                };
                                let definition_control = scanner.control_character();
                                let mut body = Vec::new();
                                let mut terminated = false;
                                // A nested direct `.de` starts from the
                                // caller macro's remaining copy-mode lines;
                                // if its terminator lies beyond that stored
                                // body, capture the following physical input
                                // as one definition (`de/startde`).
                                if matches!(request, b"de" | b"de1") {
                                    while pending
                                        .last()
                                        .is_some_and(|(_, _, depth, _, _, _)| *depth == macro_depth)
                                    {
                                        let (body_line, _, _, _, _, _) =
                                            pending.pop().expect("checked macro depth");
                                        if is_definition_terminator(
                                            &body_line,
                                            definition_control,
                                            &terminator,
                                        ) {
                                            terminated = true;
                                            break;
                                        }
                                        body.push(body_line);
                                    }
                                }
                                while !terminated && let Some(body_line) = scanner.next_raw_line() {
                                    if is_definition_terminator(
                                        body_line.bytes,
                                        definition_control,
                                        &terminator,
                                    ) {
                                        terminated = true;
                                        break;
                                    }
                                    if body_line.too_long {
                                        truncated = true;
                                        push_diagnostic(
                                            &mut diagnostics,
                                            limits,
                                            diagnostic(
                                                DiagnosticCode::LIMIT_LINE_BYTES,
                                                Severity::Warning,
                                                source_id,
                                                body_line.start,
                                                body_line.end,
                                                "copy-mode generated macro line exceeds max_line_bytes and was skipped",
                                            ),
                                            &mut truncated,
                                        );
                                        continue;
                                    }
                                    body.push(body_line.bytes.to_vec());
                                }
                                if !terminated {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ROFF_UNTERMINATED_DEFINITION,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "generated roff macro definition reached source end before its terminator",
                                        ),
                                        &mut truncated,
                                    );
                                }
                                let definition = if matches!(request, b"dei" | b"ami") {
                                    environment.define_indirect_macro(
                                        &definition_name,
                                        body,
                                        matches!(request, b"am" | b"ami"),
                                        limits,
                                    )
                                } else {
                                    environment.define_macro(
                                        &definition_name,
                                        body,
                                        matches!(request, b"am" | b"ami"),
                                        limits,
                                    )
                                };
                                if let Err(error) = definition {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        environment_error_diagnostic(error, source_id, start, end),
                                        &mut truncated,
                                    );
                                }
                                continue;
                            }
                            if request == b"while"
                                && let Ok(while_arguments) =
                                    lex_arguments(raw_arguments, scanner.escape_character(), limits)
                                && let Some((predicate_template, body)) =
                                    while_arguments.split_first()
                                && is_scope_opener(
                                    &join_arguments(body),
                                    scanner.escape_character(),
                                )
                            {
                                let escape = scanner.escape_character();
                                let scope = collect_scope(
                                    &mut scanner,
                                    source_id,
                                    limits,
                                    builder.macro_set(),
                                    &mut diagnostics,
                                    &mut truncated,
                                    true,
                                    start,
                                    end,
                                    Some(b"while"),
                                );
                                if !scope.terminated {
                                    break 'lines;
                                }
                                // This `.while` originated in a macro body,
                                // while `collect_scope` consumed its closing
                                // `\\}` from the caller's physical input.  mandoc
                                // keeps the resulting AST recovery but reports
                                // both halves of that cross-input boundary.
                                push_diagnostic(
                                    &mut diagnostics,
                                    limits,
                                    diagnostic(
                                        DiagnosticCode::ROFF_WHILE_OUT_OF_SCOPE,
                                        Severity::Unsupported,
                                        source_id,
                                        start,
                                        end,
                                        "end of scope with open .while loop",
                                    ),
                                    &mut truncated,
                                );
                                if let Some(close_start) = scope.lines.last().map(|line| {
                                    let end = match line {
                                        ScopeLine::Text { end, .. }
                                        | ScopeLine::Control { end, .. }
                                        | ScopeLine::Loop { end, .. }
                                        | ScopeLine::Conditional { end, .. }
                                        | ScopeLine::Else { end, .. } => *end,
                                    };
                                    end.saturating_add(1)
                                }) {
                                    let diagnostic_start = close_start.saturating_add(3);
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ROFF_WHILE_CANNOT_CONTINUE,
                                            Severity::Unsupported,
                                            source_id,
                                            diagnostic_start,
                                            diagnostic_start,
                                            "cannot continue this .while loop",
                                        ),
                                        &mut truncated,
                                    );
                                }
                                let Some(predicate) = expand_environment(
                                    environment,
                                    &predicate_template.bytes,
                                    escape,
                                    &macro_arguments,
                                    limits,
                                    source_id,
                                    start,
                                    end,
                                    &mut expansion_steps,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) else {
                                    break 'lines;
                                };
                                let Some(condition) = evaluate_condition(environment, &predicate)
                                else {
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ROFF_CONDITION,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "roff while predicate in a macro is outside the M3 numeric/nroff subset",
                                        ),
                                        &mut truncated,
                                    );
                                    continue;
                                };
                                if !condition {
                                    continue;
                                }
                                // A macro-local loop reaches the end of that
                                // macro before the caller's collected `\\}`.
                                // Execute its retained body once, then let the
                                // caller's scope run once as ordinary input;
                                // iterating the caller body here incorrectly
                                // drives the register to zero.
                                let mut macro_loop_body = Vec::new();
                                while pending
                                    .last()
                                    .is_some_and(|(_, _, depth, _, _, _)| *depth == macro_depth)
                                {
                                    let (line, _, _, _, _, _) =
                                        pending.pop().expect("checked macro depth");
                                    macro_loop_body.push(line);
                                }
                                let first_macro_loop_child =
                                    builder.children(root).map_or(0, <[NodeId]>::len);
                                match execute_scope_macro_lines(
                                    macro_loop_body,
                                    &macro_arguments,
                                    macro_depth + 1,
                                    &mut builder,
                                    root,
                                    source_id,
                                    start,
                                    end,
                                    &mut scanner,
                                    environment,
                                    limits,
                                    &mut text_bytes,
                                    &mut expansion_steps,
                                    &mut maximum_depth,
                                    &mut total_loop_iterations,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) {
                                    ScopeFlow::Halt => break 'lines,
                                    ScopeFlow::CloseLoopInInnerScope { .. }
                                    | ScopeFlow::Break
                                    | ScopeFlow::Continue
                                    | ScopeFlow::LoopContinue => {}
                                }
                                // The copied `.while` body began in a user
                                // macro but closes in the caller's physical
                                // scope. Its first visible output inherits
                                // the macro-input cursor: the caller's
                                // invocation width followed by the copied
                                // opener line, rather than column one of the
                                // physical invocation span.
                                let scope_cursor = end.saturating_sub(start).saturating_add(
                                    u32::try_from(body_line.len()).expect(
                                        "bounded macro body lines fit public source columns",
                                    ),
                                );
                                set_first_scope_child_logical_start(
                                    &mut builder,
                                    root,
                                    first_macro_loop_child,
                                    SourcePosition {
                                        line: 0,
                                        column: scope_cursor,
                                    },
                                );
                                match execute_scope_lines(
                                    &scope.lines,
                                    &mut builder,
                                    root,
                                    source_id,
                                    &mut scanner,
                                    environment,
                                    limits,
                                    &mut text_bytes,
                                    &mut expansion_steps,
                                    &mut maximum_depth,
                                    &mut total_loop_iterations,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) {
                                    ScopeFlow::Halt => break 'lines,
                                    ScopeFlow::CloseLoopInInnerScope { .. } => {
                                        pending_while_out_of_scope = true;
                                    }
                                    ScopeFlow::Break
                                    | ScopeFlow::Continue
                                    | ScopeFlow::LoopContinue => {}
                                }
                                continue;
                            }
                            if matches!(request, b"ds" | b"as") {
                                if let Err(error) = apply_string_request(
                                    &mut environment,
                                    raw_arguments,
                                    scanner.escape_character(),
                                    request == b"as",
                                    limits,
                                    source_id,
                                    start,
                                    end,
                                    &mut expansion_steps,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        environment_error_diagnostic(error, source_id, start, end),
                                        &mut truncated,
                                    );
                                }
                                continue;
                            }
                            if is_environment_request(request) {
                                let Some(expanded_arguments) = expand_environment(
                                    &mut environment,
                                    raw_arguments,
                                    scanner.escape_character(),
                                    &macro_arguments,
                                    limits,
                                    source_id,
                                    start,
                                    end,
                                    &mut expansion_steps,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) else {
                                    break 'lines;
                                };
                                let Ok(arguments) = lex_arguments(
                                    &expanded_arguments,
                                    scanner.escape_character(),
                                    limits,
                                ) else {
                                    continue;
                                };
                                if let Err(error) = apply_environment_request(
                                    &mut environment,
                                    builder,
                                    request,
                                    scanner.escape_character(),
                                    &arguments,
                                    limits,
                                ) {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        environment_error_diagnostic(error, source_id, start, end),
                                        &mut truncated,
                                    );
                                }
                                continue;
                            }
                            if !is_builtin_package_macro(builder.macro_set(), request)
                                && let Some(nested) = environment.macro_definition(request).cloned()
                            {
                                if macro_definition_directly_invokes(
                                    &nested,
                                    request,
                                    scanner.control_character(),
                                ) {
                                    // A direct self-call exhausts mandoc's
                                    // input stack at the caller boundary.
                                    // Do not expand it through the generic
                                    // nesting budget: that produces a second,
                                    // later warning and leaves the wrong
                                    // recovery text in the public report.
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::LIMIT_EXPANSION_STEPS,
                                            Severity::Error,
                                            source_id,
                                            end,
                                            end,
                                            "input stack limit exceeded, infinite loop?",
                                        ),
                                        &mut truncated,
                                    );
                                    pending.retain(|(_, _, depth, _, _, _)| *depth < macro_depth);
                                    continue;
                                }
                                if macro_depth >= limits.max_macro_depth {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ROFF_MACRO_DEPTH_LIMIT,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "nested roff macro expansion exceeds max_macro_depth",
                                        ),
                                        &mut truncated,
                                    );
                                    break 'lines;
                                }
                                let Ok(nested_arguments) = lex_arguments(
                                    raw_arguments,
                                    scanner.escape_character(),
                                    limits,
                                ) else {
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ARGUMENT_LIMIT,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "nested macro invocation arguments exceed configured parser limits",
                                        ),
                                        &mut truncated,
                                    );
                                    continue;
                                };
                                let mut expanded_arguments =
                                    Vec::with_capacity(nested_arguments.len());
                                for argument in nested_arguments {
                                    let Some(bytes) = expand_environment(
                                        environment,
                                        &argument.bytes,
                                        scanner.escape_character(),
                                        &macro_arguments,
                                        limits,
                                        source_id,
                                        start,
                                        end,
                                        &mut expansion_steps,
                                        &mut diagnostics,
                                        &mut truncated,
                                    ) else {
                                        break 'lines;
                                    };
                                    expanded_arguments.push(bytes);
                                }
                                if !record_expansion_steps(
                                    &mut expansion_steps,
                                    1,
                                    limits,
                                    source_id,
                                    start,
                                    end,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) {
                                    break 'lines;
                                }
                                // A nested macro is reparsed from the current
                                // macro-input cursor.  That cursor is seeded
                                // by the invoking body line, then retained by
                                // recursive calls of the same nested frame.
                                let nested_origin = if scope_reparse {
                                    0
                                } else if macro_origin == 0 {
                                    u32::try_from(body_line.len().saturating_add(1))
                                        .expect("bounded macro body lines fit source columns")
                                } else {
                                    macro_origin
                                };
                                pending.extend(nested.lines.into_iter().rev().map(|line| {
                                    (
                                        line,
                                        expanded_arguments.clone(),
                                        macro_depth + 1,
                                        nested_origin,
                                        None,
                                        false,
                                    )
                                }));
                                continue;
                            }
                            let mdoc_callable = builder.macro_set() == MacroSet::Mdoc
                                && std::str::from_utf8(request)
                                    .is_ok_and(crate::mdoc::is_mdoc_callable_macro);
                            // Macro output keeps the caller's physical span
                            // for safe source slicing, but libmandoc exposes
                            // the generated control's column *inside the
                            // copied body*.  This is independently observable
                            // for both mdoc callable macros and their text.
                            let generated_control_start = control_start;
                            let generated_argument_start = if mdoc_callable {
                                macro_generated_argument_start
                            } else {
                                start
                            };
                            let flags = NodeFlags {
                                line_start: true,
                                ..NodeFlags::default()
                            };
                            let Some(element) = append_node(
                                &mut builder,
                                root,
                                NodeKind::Element,
                                source_id,
                                generated_control_start,
                                end,
                                flags,
                                limits,
                                &mut diagnostics,
                                &mut truncated,
                            ) else {
                                continue;
                            };
                            if !builder.macro_name(element, visible_bytes(request)) {
                                truncated = true;
                                continue;
                            }
                            let generated_control_position = builder
                                .node_source_position(element)
                                .map(|position| SourcePosition {
                                    line: position.line,
                                    column: macro_origin.saturating_add(macro_body_control_column(
                                        &body_line,
                                        scanner.control_character(),
                                    )),
                                });
                            if let Some(position) = generated_control_position {
                                let _ = builder.set_node_logical_start(element, position);
                            }
                            let generated_argument_position =
                                generated_control_position.map(|position| {
                                    let offset = u32::try_from(
                                        body_line.len().saturating_sub(raw_arguments.len()),
                                    )
                                    .expect(
                                        "parser line bounds keep macro argument offsets public",
                                    );
                                    SourcePosition {
                                        line: position.line,
                                        column: macro_origin
                                            .saturating_add(offset)
                                            .saturating_add(1),
                                    }
                                });
                            maximum_depth = maximum_depth.max(2);
                            if !raw_arguments.is_empty() {
                                let Some(bytes) = expand_environment(
                                    environment,
                                    raw_arguments,
                                    scanner.escape_character(),
                                    &macro_arguments,
                                    limits,
                                    source_id,
                                    start,
                                    end,
                                    &mut expansion_steps,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) else {
                                    break 'lines;
                                };
                                let escape = scanner.escape_character();
                                let macro_body_separator_widths =
                                    lex_arguments(raw_arguments, escape, limits)
                                        .ok()
                                        .map(|arguments| {
                                            arguments
                                                .into_iter()
                                                .map(|argument| {
                                                    u32::try_from(argument.separator_width).expect(
                                                "macro argument separators fit public columns",
                                            )
                                                })
                                                .collect::<Vec<_>>()
                                        })
                                        .unwrap_or_default();
                                let Ok(arguments) = lex_arguments(&bytes, escape, limits) else {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ARGUMENT_LIMIT,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "macro-generated control arguments exceed configured parser limits",
                                        ),
                                        &mut truncated,
                                    );
                                    continue;
                                };
                                // A macro body's `\$@` is one scanner atom,
                                // but libmandoc publishes every expanded
                                // argument at a distinct logical column: the
                                // next argument follows the previous visible
                                // spelling plus the three-byte `\$@` source
                                // atom. Retain that provenance without
                                // altering the physical invocation span.
                                let all_arguments_expansion = raw_arguments == b"\\$@";
                                let all_arguments_atom_width = u32::try_from(raw_arguments.len())
                                    .expect("macro argument atom length fits public columns");
                                let mut next_generated_argument_position =
                                    generated_argument_position;
                                let mut expanded_argument_index = 0_usize;
                                for argument in arguments {
                                    let Some(bytes) = translate_visible(
                                        environment,
                                        &argument.bytes,
                                        escape,
                                        limits,
                                        source_id,
                                        start,
                                        end,
                                        &mut diagnostics,
                                        &mut truncated,
                                    ) else {
                                        break 'lines;
                                    };
                                    let result =
                                        normalize_document_escapes(builder, &bytes, escape, limits);
                                    if !record_expansion_steps(
                                        &mut expansion_steps,
                                        result.steps,
                                        limits,
                                        source_id,
                                        start,
                                        end,
                                        &mut diagnostics,
                                        &mut truncated,
                                    ) {
                                        break 'lines;
                                    }
                                    emit_escape_issues(
                                        &result.issues,
                                        start,
                                        end,
                                        source_id,
                                        limits,
                                        &mut diagnostics,
                                        &mut truncated,
                                    );
                                    truncated |= result.truncated;
                                    let logical_text_width = u32::try_from(result.text.len())
                                        .expect("expanded macro arguments fit public columns");
                                    if append_text_node(
                                        &mut builder,
                                        element,
                                        source_id,
                                        generated_argument_start,
                                        end,
                                        NodeFlags {
                                            line_continuation: result.line_continuation,
                                            ..NodeFlags::default()
                                        },
                                        result.text,
                                        limits,
                                        &mut text_bytes,
                                        &mut diagnostics,
                                        &mut truncated,
                                    ) {
                                        if let Some(position) = next_generated_argument_position
                                            && let Some(argument) = builder
                                                .children(element)
                                                .and_then(|children| children.last())
                                                .copied()
                                        {
                                            let _ =
                                                builder.set_node_logical_start(argument, position);
                                        }
                                        if all_arguments_expansion {
                                            next_generated_argument_position =
                                                next_generated_argument_position.map(|position| {
                                                    SourcePosition {
                                                        line: position.line,
                                                        column: position
                                                            .column
                                                            .saturating_add(logical_text_width)
                                                            .saturating_add(
                                                                all_arguments_atom_width,
                                                            ),
                                                    }
                                                });
                                        } else {
                                            let separator_width = macro_body_separator_widths
                                                .get(expanded_argument_index)
                                                .copied()
                                                .unwrap_or_default();
                                            next_generated_argument_position =
                                                next_generated_argument_position.map(|position| {
                                                    SourcePosition {
                                                        line: position.line,
                                                        column: position
                                                            .column
                                                            .saturating_add(logical_text_width)
                                                            .saturating_add(separator_width),
                                                    }
                                                });
                                        }
                                        expanded_argument_index =
                                            expanded_argument_index.saturating_add(1);
                                        maximum_depth = maximum_depth.max(3);
                                    }
                                }
                            }
                            continue;
                        }
                        let Some(bytes) = expand_environment(
                            environment,
                            &body_line,
                            scanner.escape_character(),
                            &macro_arguments,
                            limits,
                            source_id,
                            start,
                            end,
                            &mut expansion_steps,
                            &mut diagnostics,
                            &mut truncated,
                        ) else {
                            break 'lines;
                        };
                        let escape = scanner.escape_character();
                        let Some(bytes) = translate_visible(
                            environment,
                            &bytes,
                            escape,
                            limits,
                            source_id,
                            start,
                            end,
                            &mut diagnostics,
                            &mut truncated,
                        ) else {
                            break 'lines;
                        };
                        let result = normalize_document_escapes(builder, &bytes, escape, limits);
                        if !record_expansion_steps(
                            &mut expansion_steps,
                            result.steps,
                            limits,
                            source_id,
                            start,
                            end,
                            &mut diagnostics,
                            &mut truncated,
                        ) {
                            break 'lines;
                        }
                        emit_escape_issues(
                            &result.issues,
                            start,
                            end,
                            source_id,
                            limits,
                            &mut diagnostics,
                            &mut truncated,
                        );
                        truncated |= result.truncated;
                        let flags = NodeFlags {
                            line_start: true,
                            line_continuation: result.line_continuation,
                            ..NodeFlags::default()
                        };
                        if append_text_node(
                            &mut builder,
                            root,
                            source_id,
                            start,
                            end,
                            flags,
                            result.text,
                            limits,
                            &mut text_bytes,
                            &mut diagnostics,
                            &mut truncated,
                        ) {
                            if let Some(column) = text_origin
                                && let Some(node) = builder
                                    .children(root)
                                    .and_then(|children| children.last())
                                    .copied()
                                && let Some(physical) = builder.node_source_position(node)
                            {
                                let _ = builder.set_node_logical_start(
                                    node,
                                    SourcePosition {
                                        line: physical.line,
                                        column: column.saturating_add(1),
                                    },
                                );
                            }
                            maximum_depth = maximum_depth.max(2);
                        }
                    }
                    continue;
                }
                let flags = NodeFlags {
                    line_start: true,
                    ..NodeFlags::default()
                };
                let Some(element) = append_node(
                    &mut builder,
                    root,
                    NodeKind::Element,
                    source_id,
                    control_start,
                    end,
                    flags,
                    limits,
                    &mut diagnostics,
                    &mut truncated,
                ) else {
                    continue;
                };
                if !builder.macro_name(element, visible_bytes(dispatched_package_macro)) {
                    truncated = true;
                    continue;
                }
                maximum_depth = maximum_depth.max(2);
                let character_request = matches!(dispatched_package_macro, b"cc" | b"c2" | b"ec");
                // A renamed package macro retains the original package
                // spelling's logical argument column.  Its physical byte span
                // remains anchored at the alias, so consumers can still slice
                // source safely while canonical locations match mandoc.
                let renamed_package_argument_position =
                    renamed_package_macro
                        .then(|| {
                            let span = SourceSpan::new(source_id, control_start, control_start)
                                .expect("control source offsets are monotonic");
                            builder.source_position(&span).map(|position| {
                                SourcePosition {
                            line: position.line,
                            column: position
                                .column
                                .saturating_add(
                                    u32::try_from(dispatched_package_macro.len()).expect(
                                        "parsed control names fit public source positions",
                                    ),
                                )
                                .saturating_add(
                                    u32::try_from(raw_arguments.len() - arguments.len()).expect(
                                        "scanner argument widths fit public source positions",
                                    ),
                                ),
                        }
                            })
                        })
                        .flatten();
                let argument_escape = if character_request {
                    b'\\'
                } else {
                    scanner.escape_character()
                };
                let parsed_arguments = match lex_arguments(arguments, argument_escape, limits) {
                    Ok(arguments) => Ok(arguments),
                    Err(ArgumentIssue::UnterminatedQuote) => {
                        emit_unterminated_quoted_argument(
                            arguments,
                            argument_start,
                            end,
                            source_id,
                            limits,
                            &mut diagnostics,
                            &mut truncated,
                        );
                        // Package macros still consume the recovered token:
                        // mandoc synthesizes the missing closing delimiter
                        // after publishing its style finding, so an `.IB
                        // "one` retains `one` instead of becoming an empty
                        // element.
                        recover_unterminated_quoted_arguments(arguments, argument_escape, limits)
                    }
                    Err(ArgumentIssue::Limit) => Err(ArgumentIssue::Limit),
                };
                match parsed_arguments {
                    Ok(mut arguments) => {
                        if terminal_continuation_at_eof
                            && let Some(argument) = arguments.last_mut()
                            && argument
                                .bytes
                                .last()
                                .is_some_and(|byte| *byte == scanner.escape_character())
                        {
                            // The terminal escape consumes its physical
                            // newline.  Keep the complete preceding argument
                            // text while removing that private continuation
                            // control before package-macro lowering.
                            let _ = argument.bytes.pop();
                        }
                        if character_request {
                            normalize_character_request_arguments(
                                dispatched_package_macro,
                                &mut arguments,
                                source_id,
                                argument_start,
                                limits,
                                &mut diagnostics,
                                &mut truncated,
                            );
                        }
                        for argument in arguments {
                            let argument_offset = u32::try_from(argument.offset)
                                .expect("argument offsets are bounded by line length");
                            let argument_quoted = argument.quoted;
                            let separator_after = argument.separator_after;
                            let separator_contains_tab = argument.separator_contains_tab;
                            let embedded_tab_count = argument.embedded_tab_count;
                            let separator_width = argument.separator_width;
                            let has_invalid_argument_bytes =
                                std::str::from_utf8(&argument.bytes).is_err();
                            let lexical_width = i32::try_from(argument.bytes.len())
                                .expect("argument bytes are bounded below i32::MAX");
                            // Copy-mode turns `\\\\e` into the public `\\e`
                            // spelling.  The AST intentionally exposes that
                            // shorter spelling, but libmandoc still anchors
                            // the following mdoc argument after all three
                            // authored source bytes.  It is therefore not an
                            // expansion-width delta for the later-argument
                            // rebasing pass.
                            let preserves_copy_mode_e_width =
                                argument.bytes.windows(3).any(|atom| {
                                    atom == [
                                        scanner.escape_character(),
                                        scanner.escape_character(),
                                        b'e',
                                    ]
                                });
                            let protected_tabulation_escape = !character_request
                                && has_protected_tabulation_escape(
                                    &argument.bytes,
                                    scanner.escape_character(),
                                );
                            let argument_start = argument_start
                                .checked_add(argument_offset)
                                .expect("parser checks public span offsets first");
                            let expanded = if character_request {
                                argument.bytes
                            } else {
                                // man(7) and mdoc(7) reparse control
                                // arguments in copy mode before resolving
                                // delayed strings. In particular, `\\\\*x`
                                // becomes the active `\\*x` reference, while
                                // ordinary roff text keeps its literal
                                // escaped spelling.
                                let reparsed = (builder.macro_set() != MacroSet::None).then(|| {
                                    copy_mode_reparse(&argument.bytes, scanner.escape_character())
                                });
                                let argument_bytes =
                                    reparsed.as_deref().unwrap_or(argument.bytes.as_slice());
                                let Some(bytes) = expand_environment(
                                    environment,
                                    argument_bytes,
                                    scanner.escape_character(),
                                    &[],
                                    limits,
                                    source_id,
                                    argument_start,
                                    end,
                                    &mut expansion_steps,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) else {
                                    break 'lines;
                                };
                                bytes
                            };
                            let result = (!character_request).then(|| {
                                normalize_document_escapes(
                                    builder,
                                    &expanded,
                                    scanner.escape_character(),
                                    limits,
                                )
                            });
                            if let Some(result) = &result {
                                if !record_expansion_steps(
                                    &mut expansion_steps,
                                    result.steps,
                                    limits,
                                    source_id,
                                    argument_start,
                                    end,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) {
                                    break 'lines;
                                }
                                emit_escape_issues(
                                    &result.issues,
                                    argument_start,
                                    end,
                                    source_id,
                                    limits,
                                    &mut diagnostics,
                                    &mut truncated,
                                );
                                truncated |= result.truncated;
                            }
                            let text = result
                                .map_or_else(|| visible_bytes(&expanded), |result| result.text);
                            let expansion_width_delta = if preserves_copy_mode_e_width
                                && text
                                    .as_bytes()
                                    .windows(2)
                                    .any(|atom| atom == [scanner.escape_character(), b'e'])
                            {
                                0
                            } else {
                                i32::try_from(text.len())
                                    .expect("normalized argument bytes are bounded below i32::MAX")
                                    .saturating_sub(lexical_width)
                            };
                            if append_text_node(
                                &mut builder,
                                element,
                                source_id,
                                argument_start,
                                end,
                                NodeFlags::default(),
                                text,
                                limits,
                                &mut text_bytes,
                                &mut diagnostics,
                                &mut truncated,
                            ) {
                                if let Some(argument_node) = builder
                                    .children(element)
                                    .and_then(|children| children.last())
                                    .copied()
                                {
                                    let _ = builder
                                        .set_node_separator_after(argument_node, separator_after);
                                    let _ = builder.set_node_separator_contains_tab(
                                        argument_node,
                                        separator_contains_tab,
                                    );
                                    let _ = builder.set_node_embedded_tab_count(
                                        argument_node,
                                        embedded_tab_count,
                                    );
                                    let _ = builder
                                        .set_node_separator_width(argument_node, separator_width);
                                    let _ = builder.set_node_protected_tabulation_escape(
                                        argument_node,
                                        protected_tabulation_escape,
                                    );
                                    let _ = builder.set_node_argument_expansion_width_delta(
                                        argument_node,
                                        expansion_width_delta,
                                    );
                                    let _ = builder
                                        .set_node_argument_quoted(argument_node, argument_quoted);
                                    // Package validators normally use the visible UTF-8 byte
                                    // offset to place a diagnostic inside an argument. Preserve
                                    // malformed-input provenance so they can instead count one
                                    // source byte per Latin-1-mapped character. Otherwise an
                                    // invalid byte before an ASCII finding makes a public span
                                    // run past its raw source range.
                                    if has_invalid_argument_bytes {
                                        let _ = builder.set_node_input_unicode_provenance(
                                            argument_node,
                                            true,
                                            false,
                                        );
                                    }
                                    if let Some(position) = renamed_package_argument_position {
                                        let _ = builder.set_node_logical_start(
                                            argument_node,
                                            SourcePosition {
                                                line: position.line,
                                                column: position
                                                    .column
                                                    .saturating_add(argument_offset),
                                            },
                                        );
                                    }
                                    if physical_continuation {
                                        continued_argument_nodes
                                            .push((argument_node, argument_offset));
                                    }
                                }
                                maximum_depth = maximum_depth.max(3);
                            } else {
                                break;
                            }
                        }
                    }
                    Err(ArgumentIssue::UnterminatedQuote) => {}
                    Err(ArgumentIssue::Limit) => {
                        truncated = true;
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ARGUMENT_LIMIT,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "control-line arguments exceed configured parser limits",
                            ),
                            &mut truncated,
                        );
                    }
                }
                if physical_continuation {
                    let _ = builder.rebase_node_location_to_final_line(element);
                    if let Some(children) = builder.children(element).map(<[NodeId]>::to_vec) {
                        for child in children {
                            let _ = builder.rebase_node_location_to_final_line(child);
                        }
                    }
                    let source_end =
                        usize::try_from(end).expect("parser checks public span offsets first");
                    // This is a bounded parser path, and adding a generic
                    // byte-counting dependency solely for this exceptional
                    // provenance calculation would widen the public supply
                    // chain without affecting normal scanning throughput.
                    #[allow(clippy::naive_bytecount)]
                    let final_line = u32::try_from(
                        source.bytes[..source_end]
                            .iter()
                            .filter(|byte| **byte == b'\n')
                            .count()
                            + 1,
                    )
                    .expect("source line count fits the public source limit");
                    let argument_offset = usize::try_from(argument_start)
                        .expect("parser checks public span offsets first");
                    let logical_base_column = source.bytes[..argument_offset]
                        .iter()
                        .rposition(|byte| *byte == b'\n')
                        .map_or(argument_start.saturating_add(1), |line_start| {
                            argument_start.saturating_sub(
                                u32::try_from(line_start)
                                    .expect("source offsets fit the public source limit"),
                            )
                        });
                    for (node, offset) in continued_argument_nodes {
                        let _ = builder.set_node_logical_start(
                            node,
                            SourcePosition {
                                line: final_line,
                                column: logical_base_column.saturating_add(offset),
                            },
                        );
                    }
                }
            }
        }
    }
    ScanOutcome {
        diagnostics,
        deferred_post_validation_diagnostics,
        source_bytes,
        source_files,
        text_bytes,
        expansion_steps,
        truncated,
        maximum_depth,
        previous_conditional,
        total_loop_iterations,
        saw_mdoc_operating_system,
    }
}

fn normalize_document_escapes(
    builder: &DocumentBuilder,
    bytes: &[u8],
    escape: u8,
    limits: &Limits,
) -> crate::escape::EscapeResult {
    if builder.macro_set() == MacroSet::None {
        normalize_escapes(bytes, escape, limits)
    } else {
        normalize_ast_escapes(bytes, escape, limits)
    }
}

/// `\\.` is not a comment request: it remains visible text.  libmandoc still
/// flags the following quote as the historical "bad comment style" while
/// retaining that text in the public tree.  Diagnose from raw scanner bytes so
/// escape normalization cannot erase the distinction.
#[allow(clippy::too_many_arguments)]
fn emit_bad_comment_style(
    bytes: &[u8],
    escape: u8,
    control: u8,
    start: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    debug_assert!(is_bad_comment_style(bytes, escape, control));
    let quote_start = start.saturating_add(2);
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::INPUT_BAD_COMMENT_STYLE,
            Severity::Style,
            source_id,
            quote_start,
            quote_start.saturating_add(1),
            "bad comment style",
        ),
        truncated,
    );
}

fn is_bad_comment_style(bytes: &[u8], escape: u8, control: u8) -> bool {
    bytes.starts_with(&[escape, control, b'"'])
}

/// Preserve mandoc's diagnostics for exceptional `.tr` request shapes while
/// leaving the executor's pair-to-space recovery unchanged.
#[allow(clippy::too_many_arguments)]
fn emit_translation_request_diagnostics(
    glyphs: &[u8],
    escape: u8,
    control_start: u32,
    argument_start: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    match translation_request_issue(glyphs, escape) {
        Some(TranslationRequestIssue::Empty) => push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::ROFF_EMPTY_REQUEST,
                Severity::Warning,
                source_id,
                control_start,
                control_start.saturating_add(2),
                "skipping empty request: tr",
            ),
            truncated,
        ),
        Some(TranslationRequestIssue::Odd { start, end }) => {
            let glyph = visible_bytes(&glyphs[start..end]);
            let start = argument_start.saturating_add(u32::try_from(start).unwrap_or(u32::MAX));
            let end = argument_start.saturating_add(u32::try_from(end).unwrap_or(u32::MAX));
            push_diagnostic(
                diagnostics,
                limits,
                diagnostic(
                    DiagnosticCode::ROFF_ODD_TRANSLATION,
                    Severity::Warning,
                    source_id,
                    start,
                    end,
                    format!("odd number of characters in request: tr {glyph}"),
                ),
                truncated,
            );
        }
        None => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn append_text_node(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    flags: NodeFlags,
    text: String,
    limits: &Limits,
    text_bytes: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> bool {
    append_textual_node(
        builder,
        parent,
        NodeKind::Text,
        source_id,
        start,
        end,
        flags,
        text,
        limits,
        text_bytes,
        diagnostics,
        truncated,
    )
}

#[allow(clippy::too_many_arguments)]
fn append_textual_node(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    kind: NodeKind,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    flags: NodeFlags,
    text: String,
    limits: &Limits,
    text_bytes: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> bool {
    let Some(total) = text_bytes.checked_add(text.len()) else {
        *truncated = true;
        return false;
    };
    if total > limits.max_text_bytes {
        *truncated = true;
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::LIMIT_TEXT_BYTES,
                Severity::Warning,
                source_id,
                start,
                end,
                "scanner-stage visible text exceeds max_text_bytes and was skipped",
            ),
            truncated,
        );
        return false;
    }
    let Some(node) = append_node(
        builder,
        parent,
        kind,
        source_id,
        start,
        end,
        flags,
        limits,
        diagnostics,
        truncated,
    ) else {
        return false;
    };
    if !builder.text(node, text) {
        *truncated = true;
        return false;
    }
    *text_bytes = total;
    true
}

#[allow(clippy::too_many_arguments)]
fn append_node(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    kind: NodeKind,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    flags: NodeFlags,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> Option<NodeId> {
    if builder.node_count() >= limits.max_nodes {
        *truncated = true;
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::LIMIT_NODES,
                Severity::Warning,
                source_id,
                start,
                end,
                "scanner-stage AST node count exceeds max_nodes and was truncated",
            ),
            truncated,
        );
        return None;
    }
    let node = builder.push(parent, kind)?;
    let span = SourceSpan::new(source_id, start, end).expect("scanner spans are monotonic");
    if !builder.location(node, span) || !builder.flags(node, flags) {
        *truncated = true;
        return None;
    }
    Some(node)
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)] // Ordered diagnostics need one exhaustive escape taxonomy.
fn emit_escape_issues(
    issues: &[EscapeIssue],
    line_start: u32,
    line_end: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let reverse_unicode_issues = issues.len() > 1
        && issues
            .iter()
            .all(|issue| issue.kind == EscapeIssueKind::UnsupportedUnicode);
    let has_bracket_validation_issues = issues.iter().any(|issue| {
        matches!(
            issue.kind,
            EscapeIssueKind::InvalidBracketAcuteAccent
                | EscapeIssueKind::InvalidBracketGraveAccent
                | EscapeIssueKind::InvalidBracketWhitespaceControl(_)
                | EscapeIssueKind::InvalidBracketIgnoredEscape(_)
        )
    });
    let ordered_issues = if has_bracket_validation_issues {
        issues
            .iter()
            .filter(|issue| {
                matches!(
                    issue.kind,
                    EscapeIssueKind::InvalidBracketAcuteAccent
                        | EscapeIssueKind::InvalidBracketGraveAccent
                        | EscapeIssueKind::InvalidBracketWhitespaceControl(_)
                        | EscapeIssueKind::InvalidBracketIgnoredEscape(_)
                )
            })
            .rev()
            .chain(
                issues
                    .iter()
                    .filter(|issue| {
                        !matches!(
                            issue.kind,
                            EscapeIssueKind::InvalidBracketAcuteAccent
                                | EscapeIssueKind::InvalidBracketGraveAccent
                                | EscapeIssueKind::InvalidBracketWhitespaceControl(_)
                                | EscapeIssueKind::InvalidBracketIgnoredEscape(_)
                        )
                    })
                    .rev(),
            )
            .collect::<Vec<_>>()
    } else if reverse_unicode_issues {
        issues.iter().rev().collect()
    } else {
        issues.iter().collect()
    };
    for issue in ordered_issues {
        // mandoc emits malformed `\[u…]` diagnostics in reverse encounter
        // order for one physical line, while retaining the normal
        // escape-start source anchor for each individual spelling.
        // Environment expansion may consume earlier formatter-size controls
        // before the AST normalizer sees a terminal `\\s-`.  That malformed
        // form is necessarily at the end of its physical source line, so use
        // the retained physical end rather than the post-expansion offset.
        let start = if issue.kind == EscapeIssueKind::InvalidTerminalSize {
            line_end.saturating_sub(issue.length)
        } else {
            line_start.saturating_add(issue.offset).min(line_end)
        };
        let end = start.saturating_add(issue.length).min(line_end).max(start);
        let (code, message) = match issue.kind {
            EscapeIssueKind::Unterminated => (
                DiagnosticCode::ESCAPE_UNTERMINATED,
                issue.spelling.as_deref().map_or_else(
                    || "roff escape is missing required bytes".to_owned(),
                    |spelling| format!("invalid escape sequence: {spelling}"),
                ),
            ),
            EscapeIssueKind::UnknownSpecialCharacter => (
                DiagnosticCode::ESCAPE_UNKNOWN_SPECIAL_CHARACTER,
                "named roff special character is not known by the scanner-stage catalog".to_owned(),
            ),
            EscapeIssueKind::UnknownEscape => (
                DiagnosticCode::ESCAPE_UNKNOWN,
                issue.spelling.as_deref().map_or_else(
                    || "roff escape is not known by the scanner stage".to_owned(),
                    |spelling| format!("undefined escape, printing literally: {spelling}"),
                ),
            ),
            EscapeIssueKind::UnsupportedEscape => {
                push_diagnostic(
                    diagnostics,
                    limits,
                    diagnostic(
                        DiagnosticCode::ESCAPE_UNKNOWN,
                        Severity::Unsupported,
                        source_id,
                        start,
                        end,
                        issue.spelling.as_deref().map_or_else(
                            || "unsupported roff escape sequence".to_owned(),
                            |spelling| format!("unsupported escape sequence: {spelling}"),
                        ),
                    ),
                    truncated,
                );
                continue;
            }
            EscapeIssueKind::InvalidSyntax => (
                DiagnosticCode::ESCAPE_INVALID,
                issue.spelling.as_deref().map_or_else(
                    || "roff escape uses an invalid syntax shape".to_owned(),
                    |spelling| format!("invalid escape sequence: {spelling}"),
                ),
            ),
            EscapeIssueKind::InvalidBracketIgnoredEscape(control) => (
                DiagnosticCode::ESCAPE_INVALID,
                format!("invalid escape sequence: \\[{}]", char::from(control)),
            ),
            EscapeIssueKind::InvalidTerminalSize => (
                DiagnosticCode::ESCAPE_INVALID,
                issue.spelling.as_deref().map_or_else(
                    || "invalid escape sequence: \\s-".to_owned(),
                    |spelling| format!("invalid escape sequence: {spelling}"),
                ),
            ),
            EscapeIssueKind::LegacyUnicodeEscape => (
                DiagnosticCode::ESCAPE_UNSUPPORTED_UNICODE,
                "undefined escape, printing literally: \\U".to_owned(),
            ),
            EscapeIssueKind::UnsupportedUnicode => (
                DiagnosticCode::ESCAPE_UNSUPPORTED_UNICODE,
                issue.spelling.as_deref().map_or_else(
                    || "legacy Unicode escape is retained but unsupported by mandoc".to_owned(),
                    |spelling| format!("invalid escape sequence: {spelling}"),
                ),
            ),
            EscapeIssueKind::InvalidBracketAcuteAccent => (
                DiagnosticCode::ESCAPE_INVALID,
                "invalid escape sequence: \\[']".to_owned(),
            ),
            EscapeIssueKind::InvalidBracketGraveAccent => (
                DiagnosticCode::ESCAPE_INVALID,
                "invalid escape sequence: \\[`]".to_owned(),
            ),
            EscapeIssueKind::InvalidBracketWhitespaceControl(control) => (
                DiagnosticCode::ESCAPE_INVALID,
                if control == b' ' {
                    "invalid escape sequence: \\[".to_owned()
                } else {
                    format!("invalid escape sequence: \\[{}]", char::from(control))
                },
            ),
            // An escaped string/register reference is deliberate literal
            // input after the execution pass.  Keep the event available to
            // the low-level escape normalizer, but do not invent a public
            // diagnostic that mandoc does not emit for it.
            EscapeIssueKind::DeferredExpansion => continue,
            EscapeIssueKind::ExpansionLimit => (
                DiagnosticCode::ESCAPE_EXPANSION_LIMIT,
                "scanner-stage escape work exceeds max_line_expansion_steps".to_owned(),
            ),
            EscapeIssueKind::OutputLimit => (
                DiagnosticCode::ESCAPE_OUTPUT_LIMIT,
                "scanner-stage visible output exceeds max_expanded_line_bytes".to_owned(),
            ),
        };
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(code, Severity::Warning, source_id, start, end, message),
            truncated,
        );
    }
}

fn emit_invalid_input_bytes(
    bytes: &[u8],
    line_start: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> bool {
    let offsets = invalid_input_byte_offsets(bytes);
    let has_invalid_input_bytes = !offsets.is_empty();
    for (offset, byte) in offsets {
        let offset = u32::try_from(offset).expect("scanned line offsets fit public u32 spans");
        let start = line_start.saturating_add(offset);
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::INPUT_INVALID_BYTE,
                Severity::Error,
                source_id,
                start,
                start.saturating_add(1),
                format!("skipping bad character: 0x{byte:x}"),
            ),
            truncated,
        );
    }
    has_invalid_input_bytes
}

/// Locate valid UTF-8 runs without treating one malformed span as evidence
/// that the entire physical line was non-Unicode input.
fn contains_valid_utf8_non_ascii(mut bytes: &[u8]) -> bool {
    while !bytes.is_empty() {
        match std::str::from_utf8(bytes) {
            Ok(text) => return !text.is_ascii(),
            Err(error) => {
                let valid = &bytes[..error.valid_up_to()];
                let text = std::str::from_utf8(valid)
                    .expect("the valid prefix reported by UTF-8 validation is UTF-8");
                if !text.is_ascii() {
                    return true;
                }
                let consumed = error
                    .valid_up_to()
                    .saturating_add(error.error_len().unwrap_or(1));
                bytes = bytes.get(consumed..).unwrap_or_default();
            }
        }
    }
    false
}

/// Reproduce tbl's byte-facing input projection before generic escape
/// normalization can merge a malformed byte with a following ASCII byte.
fn legacy_table_input_text(bytes: &[u8]) -> String {
    let mut projected = String::with_capacity(bytes.len());
    let mut remaining = bytes;
    loop {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                append_legacy_table_utf8(&mut projected, valid);
                return projected;
            }
            Err(error) => {
                let valid = &remaining[..error.valid_up_to()];
                append_legacy_table_utf8(
                    &mut projected,
                    std::str::from_utf8(valid)
                        .expect("the valid prefix reported by UTF-8 validation is UTF-8"),
                );
                let invalid_length = error.error_len().unwrap_or(remaining.len() - valid.len());
                projected.extend(std::iter::repeat_n('?', invalid_length));
                remaining = &remaining[valid.len() + invalid_length..];
            }
        }
    }
}

fn append_legacy_table_utf8(projected: &mut String, text: &str) {
    use std::fmt::Write as _;

    for character in text.chars() {
        if character == '\t' {
            projected.push(character);
        } else if character.is_ascii_control() {
            projected.push('?');
        } else if character.is_ascii() {
            projected.push(character);
        } else {
            write!(projected, r"\[u{:04X}]", u32::from(character))
                .expect("writing to a String cannot fail");
        }
    }
}

fn emit_trailing_whitespace(
    bytes: &[u8],
    line_start: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let Some(trailing_start) = trailing_whitespace_start(bytes) else {
        return;
    };
    let offset = if bytes[..trailing_start].ends_with(b"\\\"") {
        bytes.len().saturating_sub(1)
    } else {
        trailing_start
    };
    let offset = u32::try_from(offset).expect("scanned line offsets fit public u32 spans");
    let start = line_start.saturating_add(offset);
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::INPUT_TRAILING_WHITESPACE,
            Severity::Style,
            source_id,
            start,
            start.saturating_add(1),
            "whitespace at end of input line",
        ),
        truncated,
    );
}

/// Emit mandoc's style finding for an incomplete control-line quote and keep
/// its recovered token available to package or user-macro execution.
#[allow(clippy::too_many_arguments)] // Keeps ordered quote and tail recovery together.
fn emit_unterminated_quoted_argument(
    arguments: &[u8],
    argument_start: u32,
    line_end: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let quote_offset = arguments
        .iter()
        .enumerate()
        .find_map(|(offset, byte)| {
            (*byte == b'"' && (offset == 0 || arguments[offset - 1].is_ascii_whitespace()))
                .then_some(offset)
        })
        .unwrap_or(0);
    let quote_start = argument_start.saturating_add(
        u32::try_from(quote_offset).expect("bounded control-line offsets fit public spans"),
    );
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::ARGUMENT_UNTERMINATED_QUOTE,
            Severity::Style,
            source_id,
            quote_start,
            line_end,
            "unterminated quoted argument",
        ),
        truncated,
    );
    if trailing_whitespace_start(arguments).is_some() {
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::INPUT_TRAILING_WHITESPACE,
                Severity::Style,
                source_id,
                line_end,
                line_end,
                "whitespace at end of input line",
            ),
            truncated,
        );
    }
}

/// Complete the one malformed quoted token locally after its public finding
/// has been emitted.  The synthetic delimiter is never published in text;
/// it only lets the normal lexer retain the same recovery argument as mandoc.
fn recover_unterminated_quoted_arguments(
    arguments: &[u8],
    escape: u8,
    limits: &Limits,
) -> Result<Vec<Argument>, ArgumentIssue> {
    let mut recovered = Vec::with_capacity(arguments.len().saturating_add(1));
    recovered.extend_from_slice(arguments);
    recovered.push(b'"');
    lex_arguments(&recovered, escape, limits)
}

#[allow(clippy::too_many_arguments)] // Reuses the parser's shared bounded diagnostic context.
fn emit_mdoc_control_trailing_whitespace(
    name: &[u8],
    raw_arguments: &[u8],
    line_end: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    // 空 D1/Dl 的定位由其专用恢复路径处理，避免重复诊断。
    // `.It` uses terminal tabs as a column-cell boundary.  mandoc's mdoc
    // argument grammar consumes that separator rather than issuing the
    // generic end-of-line style warning, including outside a column list.
    if matches!(name, b"D1" | b"Dl" | b"It") || trailing_whitespace_start(raw_arguments).is_none() {
        return;
    }
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::INPUT_TRAILING_WHITESPACE,
            Severity::Style,
            source_id,
            line_end,
            line_end,
            "whitespace at end of input line",
        ),
        truncated,
    );
}

/// Flag an unseparated trailing delimiter on the mdoc macros that validate it.
///
/// This mirrors the narrow `post_delim_nb()` validator rather than treating
/// every implicit enclosure punctuation mark as a style error.  In particular,
/// a multi-word Pq sentence ending is ordinary prose, not an attached
/// delimiter error.
#[allow(clippy::too_many_arguments)]
fn emit_mdoc_implicit_trailing_delimiter_spacing(
    name: &[u8],
    raw_arguments: &[u8],
    line_end: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    const DELIMITER_VALIDATORS: [&[u8]; 10] = [
        b"Aq", b"Ar", b"Brq", b"Bx", b"No", b"Op", b"Pq", b"Ql", b"Qq", b"Sq",
    ];
    if !DELIMITER_VALIDATORS.contains(&name) {
        return;
    }
    let arguments = raw_arguments.trim_ascii();
    let Some((&delimiter, prefix)) = arguments.split_last() else {
        return;
    };
    if !matches!(
        delimiter,
        b',' | b'.' | b';' | b':' | b'!' | b'?' | b')' | b']' | b'|'
    ) || prefix.last().is_none_or(u8::is_ascii_whitespace)
        || mdoc_trailing_delimiter_is_allowed(
            name,
            arguments,
            mdoc_final_argument(arguments),
            delimiter,
        )
    {
        return;
    }
    let (display, has_prior_argument) = mdoc_trailing_delimiter_display(arguments);
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::MDOC_TRAILING_DELIMITER_SPACING,
            Severity::Style,
            source_id,
            line_end.saturating_sub(1),
            line_end,
            format!(
                "no blank before trailing delimiter: {}{} {display}",
                decode_visible_bytes(name),
                if has_prior_argument { " ..." } else { "" },
            ),
        ),
        truncated,
    );
}

/// Extract the final mdoc argument as `post_delim_nb()` displays it. Quoted
/// phrases remain one argument; an earlier argument is represented by `...`.
fn mdoc_trailing_delimiter_display(arguments: &[u8]) -> (String, bool) {
    let mut index = 0_usize;
    let mut count = 0_usize;
    let mut last = &[][..];
    while index < arguments.len() {
        while arguments.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if index == arguments.len() {
            break;
        }
        let start = index;
        if arguments[index] == b'"' {
            index += 1;
            let content_start = index;
            while index < arguments.len() {
                if arguments[index] == b'\\' {
                    index = index.saturating_add(2).min(arguments.len());
                    continue;
                }
                index += 1;
                if arguments[index - 1] == b'"' {
                    break;
                }
            }
            let content_end = index.min(arguments.len()).saturating_sub(usize::from(
                arguments.get(index.saturating_sub(1)) == Some(&b'"'),
            ));
            last = &arguments[content_start..content_end];
        } else {
            while index < arguments.len() && !arguments[index].is_ascii_whitespace() {
                index += 1;
            }
            last = &arguments[start..index];
        }
        if !matches!(last, b"(" | b"[") {
            count += 1;
        }
    }
    (String::from_utf8_lossy(last).into_owned(), count > 1)
}

/// Return whether `post_delim_nb()` accepts an otherwise attached delimiter.
fn mdoc_trailing_delimiter_is_allowed(
    name: &[u8],
    arguments: &[u8],
    final_argument: &[u8],
    delimiter: u8,
) -> bool {
    let Some((&last, prefix)) = final_argument.split_last() else {
        return true;
    };
    debug_assert_eq!(last, delimiter);

    // A zero-width escape deliberately turns punctuation into authored text.
    if prefix.len() >= 2
        && prefix[prefix.len() - 2] == b'\\'
        && matches!(prefix[prefix.len() - 1], b'&' | b'e')
    {
        return true;
    }

    match delimiter {
        b')' if prefix.contains(&b'(') => return true,
        b'.' if prefix.ends_with(b"..") || prefix.last() == Some(&b'.') => return true,
        b';' if name == b"Vt" => return true,
        b'?' if prefix.last() == Some(&b'?') => return true,
        b']' if prefix.contains(&b'[') => return true,
        b'|' if prefix.len() == 1 && prefix[0] == b'|' => return true,
        _ => {}
    }

    // A two-byte non-word pair has no meaningful delimiter attachment.
    if prefix.len() == 1 && !prefix[0].is_ascii_alphanumeric() {
        return true;
    }

    // The upstream false-positive filter treats a complete multi-word
    // sentence as prose for these four macro families.
    matches!(name, b"Em" | b"Li" | b"Pq" | b"Sy")
        && matches!(delimiter, b'!' | b'.' | b':' | b'?')
        && has_three_trailing_ascii_words(&arguments[..arguments.len().saturating_sub(1)])
}

/// Return the last source argument without applying package-level joining.
/// Quotes stay present because callers only inspect delimiter byte spelling.
fn mdoc_final_argument(arguments: &[u8]) -> &[u8] {
    let mut index = 0_usize;
    let mut last = &[][..];
    while index < arguments.len() {
        while arguments.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if index == arguments.len() {
            break;
        }
        let start = index;
        if arguments[index] == b'"' {
            index += 1;
            while index < arguments.len() {
                if arguments[index] == b'\\' {
                    index = index.saturating_add(2);
                    continue;
                }
                index += 1;
                if arguments[index - 1] == b'"' {
                    break;
                }
            }
        } else {
            while index < arguments.len() && !arguments[index].is_ascii_whitespace() {
                index += 1;
            }
        }
        last = &arguments[start..index.min(arguments.len())];
    }
    last
}

/// Match the backwards word walk used by mandoc's delimiter validator.
fn has_three_trailing_ascii_words(prefix: &[u8]) -> bool {
    let mut index = prefix.len();
    let mut spaces = 0_usize;
    while index > 0 {
        index -= 1;
        match prefix[index] {
            b' ' => {
                spaces += 1;
                if index > 0 && prefix[index - 1] == b',' {
                    index -= 1;
                }
            }
            byte if byte.is_ascii_alphabetic() => {
                if spaces > 1 {
                    return true;
                }
            }
            _ => return false,
        }
    }
    false
}

#[allow(clippy::too_many_arguments)] // Reuses the parser's shared bounded diagnostic context.
fn emit_mdoc_empty_display(
    name: &[u8],
    arguments: &[u8],
    raw_arguments: &[u8],
    control_start: u32,
    line_end: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    if !matches!(name, b"D1" | b"Dl") || !arguments.is_empty() {
        return;
    }
    if trailing_whitespace_start(raw_arguments).is_some() {
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::INPUT_TRAILING_WHITESPACE,
                Severity::Style,
                source_id,
                line_end.saturating_sub(1),
                line_end,
                "whitespace at end of input line",
            ),
            truncated,
        );
    }
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::MDOC_EMPTY_BLOCK,
            Severity::Warning,
            source_id,
            control_start,
            control_start.saturating_add(2),
            if name == b"D1" {
                "empty block: D1"
            } else {
                "empty block: Dl"
            },
        ),
        truncated,
    );
}

#[allow(clippy::too_many_arguments)] // Reuses the parser's shared bounded diagnostic context.
fn emit_man_alternating_font_trailing_whitespace(
    name: &[u8],
    raw_arguments: &[u8],
    line_end: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    const ALTERNATING_FONT_MACROS: [&[u8]; 6] = [b"BI", b"BR", b"IB", b"IR", b"RB", b"RI"];
    if !ALTERNATING_FONT_MACROS.contains(&name)
        || trailing_whitespace_start(raw_arguments).is_none()
    {
        return;
    }
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::INPUT_TRAILING_WHITESPACE,
            Severity::Style,
            source_id,
            // libmandoc's alternating-font argument parser reports the
            // post-argument cursor (one column after the final space), unlike
            // an empty mdoc `.Dl`, whose finding points at the final byte.
            line_end,
            line_end,
            "whitespace at end of input line",
        ),
        truncated,
    );
}

fn emit_filled_text_tabs(
    bytes: &[u8],
    line_start: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    for (offset, _) in bytes.iter().enumerate().filter(|(_, byte)| **byte == b'\t') {
        let offset = u32::try_from(offset).expect("scanned line offsets fit public u32 spans");
        let start = line_start.saturating_add(offset);
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::INPUT_TAB_IN_FILLED_TEXT,
                Severity::Warning,
                source_id,
                start,
                start.saturating_add(1),
                "tab in filled text",
            ),
            truncated,
        );
    }
}

/// Validate the parser-visible portion of a roff `.ft` request.
///
/// Font selection affects rendering rather than the owned syntax tree, but
/// mandoc still emits request diagnostics.  Keep this at scanner scope so a
/// rejected selection has no accidental AST effect.
#[allow(clippy::too_many_arguments)]
fn emit_font_request_diagnostics(
    bytes: &[u8],
    escape: u8,
    argument_start: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let Ok(arguments) = lex_arguments(bytes, escape, limits) else {
        return;
    };
    if arguments.is_empty() {
        return;
    }
    if let Some(excess) = arguments.get(1) {
        let start = argument_start.saturating_add(
            u32::try_from(excess.offset).expect("argument offsets are bounded by line length"),
        );
        let end = start.saturating_add(
            u32::try_from(excess.bytes.len()).expect("argument bytes are bounded by line length"),
        );
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::ROFF_EXCESS_ARGUMENTS,
                Severity::Error,
                source_id,
                start,
                end,
                format!(
                    "skipping excess arguments: ft ... {}",
                    visible_bytes(&excess.bytes)
                ),
            ),
            truncated,
        );
    }
}

/// The finite font selector catalogue accepted by mandoc's roff validator.
///
/// mdoc applies its copy during structural validation to retain its established
/// recovery ordering; man has no equivalent pass, so scanner recovery uses the
/// same catalogue directly.
fn is_legacy_roff_font_selector(font: &[u8]) -> bool {
    matches!(
        font,
        b"C" | b"V"
            | b"B"
            | b"3"
            | b"I"
            | b"2"
            | b"P"
            | b"R"
            | b"1"
            | b"4"
            | b"BI"
            | b"CB"
            | b"CI"
            | b"CR"
            | b"CW"
            | b"VB"
            | b"VI"
    )
}

/// man(7) validates tab input inside visible macro arguments separately from
/// ordinary roff text.  Both a literal tab and the single-byte `\t` escape
/// are layout tabulation in this context; an escaped backslash remains
/// authored text and must not manufacture a warning.
fn emit_filled_macro_argument_tabs(
    bytes: &[u8],
    argument_start: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let mut cursor = 0;
    // The man argument lexer consumes a tabulation escape as one logical
    // argument character, although it occupies two source bytes. Findings
    // for later tabs follow that parser cursor rather than raw byte columns.
    let mut prior_tab_escapes = 0_u32;
    while let Some(byte) = bytes.get(cursor) {
        let offset = match *byte {
            b'\t' => Some(cursor),
            b'\\' if bytes.get(cursor + 1) == Some(&b'\\') => {
                cursor += 2;
                continue;
            }
            b'\\' if bytes.get(cursor + 1) == Some(&b't') => Some(cursor),
            _ => None,
        };
        if let Some(offset) = offset {
            let offset = u32::try_from(offset).expect("scanned line offsets fit public u32 spans");
            let start = argument_start.saturating_add(offset.saturating_sub(prior_tab_escapes));
            push_diagnostic(
                diagnostics,
                limits,
                diagnostic(
                    DiagnosticCode::INPUT_TAB_IN_FILLED_TEXT,
                    Severity::Warning,
                    source_id,
                    start,
                    start.saturating_add(1),
                    "tab in filled text",
                ),
                truncated,
            );
            if *byte == b'\\' {
                prior_tab_escapes = prior_tab_escapes.saturating_add(1);
            }
            cursor += usize::from(*byte == b'\\') + 1;
        } else {
            cursor += 1;
        }
    }
}

/// A direct user-macro invocation treats its first tab as the request
/// separator. A second adjacent tab is filled-text tabulation, and mandoc
/// reports it at the shared post-separator cursor. Package macros own richer
/// argument grammars and are handled by their package-specific validators.
#[allow(clippy::too_many_arguments)] // Shares the parser's source-relative diagnostic boundary.
fn emit_user_macro_leading_tabs(
    raw_arguments: &[u8],
    control_start: u32,
    name_len: usize,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    if !raw_arguments.starts_with(b"\t\t") {
        return;
    }
    let name_len = u32::try_from(name_len).expect("scanned request names fit source offsets");
    let start = control_start.saturating_add(name_len).saturating_add(2);
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::INPUT_TAB_IN_FILLED_TEXT,
            Severity::Warning,
            source_id,
            start,
            start.saturating_add(1),
            "tab in filled text",
        ),
        truncated,
    );
}

/// Roff consumes the first tab after a user-macro name as the request
/// separator. Exactly one adjacent tab before visible text remains the first
/// macro argument's prefix; a third tab (or a later horizontal-space run)
/// instead follows the normal argument separator recovery.
fn retain_user_macro_tab_argument_prefix(arguments: &mut Vec<Argument>, raw_arguments: &[u8]) {
    if !raw_arguments.starts_with(b"\t\t") {
        return;
    }
    let tab_argument = || Argument {
        offset: 1,
        quoted: false,
        separator_after: Some(b'\t'),
        separator_contains_tab: true,
        embedded_tab_count: 1,
        separator_width: 1,
        bytes: vec![b'\t'],
    };
    if raw_arguments.get(2).is_some_and(u8::is_ascii_whitespace) {
        arguments.insert(0, tab_argument());
    } else if let Some(first) = arguments.first_mut() {
        first.bytes.insert(0, b'\t');
    } else {
        arguments.push(tab_argument());
    }
}

/// Implemented man macros whose arguments are visible text rather than pure
/// layout state. Their parser path applies the special tabulation warning.
fn is_man_visible_argument_macro(macro_set: MacroSet, name: &[u8]) -> bool {
    macro_set == MacroSet::Man
        && matches!(
            name,
            b"B" | b"I"
                | b"R"
                | b"SM"
                | b"SB"
                | b"BR"
                | b"BI"
                | b"IB"
                | b"IR"
                | b"RB"
                | b"RI"
                | b"IP"
                | b"HP"
                | b"TP"
                | b"TQ"
        )
}

/// A trailing odd escape consumes the physical newline in roff input.  The
/// caller joins only the immediately following text line, leaving any control
/// line available for the ordinary scanner path.
fn has_physical_line_continuation(bytes: &[u8], escape: u8) -> bool {
    let trailing_escapes = bytes
        .iter()
        .rev()
        .take_while(|byte| **byte == escape)
        .count();
    trailing_escapes % 2 == 1
}

fn update_fill_mode(
    environment: &mut Environment,
    macro_set: MacroSet,
    name: &[u8],
    arguments: &[u8],
) {
    match name {
        b"nf" => environment.no_fill(true),
        b"fi" => environment.no_fill(false),
        b"EX" if macro_set == MacroSet::Man => {
            environment.push_package_fill_scope(PackageFillScope::ManExample, true);
        }
        b"EE" if macro_set == MacroSet::Man => {
            environment.pop_package_fill_scope(PackageFillScope::ManExample);
        }
        b"Bd" if macro_set == MacroSet::Mdoc => {
            let no_fill = arguments
                .split(u8::is_ascii_whitespace)
                .any(|argument| matches!(argument, b"-literal" | b"-unfilled"));
            environment.push_package_fill_scope(PackageFillScope::MdocDisplay, no_fill);
        }
        b"Ed" if macro_set == MacroSet::Mdoc => {
            environment.pop_package_fill_scope(PackageFillScope::MdocDisplay);
        }
        b"Bl" if macro_set == MacroSet::Mdoc => {
            let no_fill = arguments
                .split(u8::is_ascii_whitespace)
                .any(|argument| argument == b"-column");
            environment.push_package_fill_scope(PackageFillScope::MdocList, no_fill);
        }
        b"El" if macro_set == MacroSet::Mdoc => {
            environment.pop_package_fill_scope(PackageFillScope::MdocList);
        }
        _ => {}
    }
}

/// Implemented package request names remain package syntax even when roff
/// copy mode has a user definition with the same name. `mandoc` dispatches
/// these requests to the package validator after syntax selection; it does
/// not let a preceding `.de BI` replace the alternating-font macro. The
/// scanner must make that choice before executing a user macro, otherwise the
/// structural pass sees the generated request instead of the authored node.
///
/// Keep this deliberately limited to implemented package semantics. Unknown
/// names still take the ordinary roff macro path, so document-local helpers
/// remain executable.
fn is_builtin_package_macro(macro_set: MacroSet, name: &[u8]) -> bool {
    macro_set == MacroSet::Man
        && matches!(
            name,
            b"TH"
                | b"SH"
                | b"SS"
                | b"TP"
                | b"TQ"
                | b"LP"
                | b"PP"
                | b"P"
                | b"IP"
                | b"HP"
                | b"RS"
                | b"RE"
                | b"UR"
                | b"UE"
                | b"MT"
                | b"ME"
                | b"SY"
                | b"YS"
                | b"SM"
                | b"SB"
                | b"R"
                | b"B"
                | b"I"
                | b"BR"
                | b"BI"
                | b"IB"
                | b"IR"
                | b"RB"
                | b"RI"
                | b"EX"
                | b"EE"
                | b"nf"
                | b"fi"
                | b"ce"
                | b"rj"
                | b"PD"
                | b"in"
                | b"br"
                | b"sp"
                | b"na"
                | b"ad"
                | b"nh"
                | b"hy"
        )
        // `At` already has a validator-defined default word and `Bc` closes
        // an mdoc `Bo` block. Preserve their package dispatch even when roff
        // has a same-named user definition; otherwise `.am Bc` turns the
        // authored closer into macro output and leaves the `Bo` unclosed.
        || (macro_set == MacroSet::Mdoc && matches!(name, b"At" | b"Bc"))
}

/// One armed roff input-line trap (`.it`).
///
/// The trap counts only physical text input lines.  It is session-local,
/// intentionally replacing an older arm instead of stacking, as upstream's
/// `roffit_lines`/`roffit_macro` pair does.
#[derive(Default)]
struct InputTrap {
    remaining: usize,
    invocation: Vec<u8>,
}

impl InputTrap {
    fn consume_text_line(&mut self) -> Option<Vec<u8>> {
        match self.remaining {
            0 => None,
            1 => {
                self.remaining = 0;
                Some(std::mem::take(&mut self.invocation))
            }
            _ => {
                self.remaining -= 1;
                None
            }
        }
    }
}

/// Arm a roff `.it` input-line trap for a bounded scaled-number subset. roff
/// permits both a scale suffix and the macro invocation immediately after the
/// number, so `.it 1vtrap` means one line and `trap`, while `.it 1 trap arg`
/// preserves `arg` for the injected macro invocation.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)] // The finite, nonnegative f64 is clamped before matching C's integer trap counter.
fn arm_input_trap(trap: &mut InputTrap, arguments: &[u8]) -> bool {
    let mut parser = InputTrapNumberParser::new(arguments);
    let Some(count) = parser.parse_expression() else {
        return false;
    };
    let count = if count.is_finite() && count > 0.0 {
        count.min(usize::MAX as f64) as usize
    } else {
        0
    };
    let mut invocation = trim_horizontal_space(&arguments[parser.cursor..]).to_vec();
    // groff's an-ext macro uses this exact special case to arrange an
    // unconditional break.  Preserve its public effect without exposing the
    // formatter-private trap request itself.
    if count == 1 && invocation == b"an-trap" {
        invocation = b"br".to_vec();
    }
    trap.remaining = count;
    trap.invocation = invocation;
    true
}

/// Small, allocation-free reader for the scaled numeric prefix accepted by
/// `.it`. Its result deliberately ignores unit conversion: upstream parses
/// this request with an integer target and therefore counts `1c + 1i` as two
/// input lines while retaining the suffix syntax.
struct InputTrapNumberParser<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> InputTrapNumberParser<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }

    fn parse_expression(&mut self) -> Option<f64> {
        self.skip_space();
        let mut total = self.parse_signed_term()?;
        loop {
            self.skip_space();
            let Some(operator) = self.bytes.get(self.cursor).copied() else {
                break;
            };
            if operator == b')' {
                self.cursor += 1;
                break;
            }
            if !matches!(operator, b'+' | b'-') {
                break;
            }
            self.cursor += 1;
            let term = self.parse_term()?;
            total = if operator == b'+' {
                total + term
            } else {
                total - term
            };
        }
        Some(total)
    }

    fn parse_signed_term(&mut self) -> Option<f64> {
        self.skip_space();
        let sign = match self.bytes.get(self.cursor).copied() {
            Some(b'+') => {
                self.cursor += 1;
                1.0
            }
            Some(b'-') => {
                self.cursor += 1;
                -1.0
            }
            _ => 1.0,
        };
        self.parse_term().map(|term| sign * term)
    }

    fn parse_term(&mut self) -> Option<f64> {
        self.skip_space();
        if self.bytes.get(self.cursor) == Some(&b'(') {
            self.cursor += 1;
            return self.parse_expression();
        }
        let start = self.cursor;
        while self
            .bytes
            .get(self.cursor)
            .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'.')
        {
            self.cursor += 1;
        }
        (self.cursor > start).then_some(())?;
        let number = std::str::from_utf8(&self.bytes[start..self.cursor])
            .ok()?
            .parse::<f64>()
            .ok()?;
        // Standard roff scale suffixes are single bytes.  Do not consume an
        // arbitrary letter here: the following bytes are the trap macro name.
        if self.bytes.get(self.cursor).is_some_and(|byte| {
            matches!(
                *byte,
                b'u' | b'i' | b'c' | b'P' | b'p' | b'm' | b'n' | b'v' | b'M'
            )
        }) {
            self.cursor += 1;
        }
        Some(number)
    }

    fn skip_space(&mut self) {
        while self
            .bytes
            .get(self.cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.cursor += 1;
        }
    }
}

/// Scanner-stage subset of man(7)'s `an-margin` register bookkeeping.
///
/// The C parser updates this register while parsing `.RS`/`.RE`; ordinary
/// source text may interpolate it before the later man structural pass runs.
/// Keep the state in the parse session so those source-order expansions retain
/// the same observable values without exposing layout state in the AST.
#[derive(Default)]
struct ManIndentState {
    current: i64,
    frames: Vec<i64>,
}

fn update_man_indent_register(
    environment: &mut Environment,
    macro_set: MacroSet,
    name: &[u8],
    arguments: &[u8],
    state: &mut ManIndentState,
    limits: &Limits,
) {
    if macro_set != MacroSet::Man {
        return;
    }
    match name {
        b"RS" => {
            // man(7) initializes the internal margin at seven ens (168
            // basic units) when the first reset block opens. Its optional
            // numeric argument is an additive indent measured in ens.
            if state.current == 0 {
                state.current = 7 * 24;
            }
            let indent = man_indent_units(arguments);
            state.current = state.current.saturating_add(indent);
            state.frames.push(indent);
        }
        b"RE" => {
            let levels = trim_horizontal_space(arguments)
                .split(u8::is_ascii_whitespace)
                .next()
                .and_then(|value| std::str::from_utf8(value).ok())
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|levels| *levels > 0)
                .unwrap_or(1);
            for _ in 0..levels {
                let Some(indent) = state.frames.pop() else {
                    break;
                };
                state.current = state.current.saturating_sub(indent);
            }
        }
        _ => return,
    }
    let value = state.current.to_string();
    let _ = environment.define_register(b"an-margin", value.as_bytes(), None, limits);
}

#[allow(clippy::cast_possible_truncation)] // Match C's `strtod() * 24.0` integer auxiliary value.
fn man_indent_units(arguments: &[u8]) -> i64 {
    let argument = trim_horizontal_space(arguments)
        .split(u8::is_ascii_whitespace)
        .next()
        .unwrap_or_default();
    let numeric_end = argument
        .iter()
        .position(|byte| !matches!(*byte, b'+' | b'-' | b'.' | b'0'..=b'9'))
        .unwrap_or(argument.len());
    let Ok(value) = std::str::from_utf8(&argument[..numeric_end]) else {
        return 0;
    };
    let Ok(value) = value.parse::<f64>() else {
        return 0;
    };
    // `man_macro.c` applies `strtod(argument) * 24.0` and ignores a
    // non-positive result for the stored RS auxiliary value.
    (value * 24.0).max(0.0) as i64
}

fn update_preprocessor_depth(depth: &mut usize, name: &[u8]) {
    match name {
        b"EQ" | b"TS" => *depth = depth.saturating_add(1),
        b"EN" | b"TE" => *depth = depth.saturating_sub(1),
        _ => {}
    }
}

/// Track tbl input separately because its physical-line continuation grammar
/// owns a terminal escape before ordinary roff escape recovery runs.
fn update_table_preprocessor_depth(depth: &mut usize, name: &[u8]) {
    match name {
        b"TS" => *depth = depth.saturating_add(1),
        b"TE" => *depth = depth.saturating_sub(1),
        _ => {}
    }
}

/// Update man(7)'s presentation-mode validator independently of AST scopes.
///
/// Nested `.EX` blocks retain nested parser scopes so their source ranges
/// stay recoverable, but mandoc's fill style check is a simple on/off state:
/// a second disable or enable warns and leaves that presentation state intact.
fn update_man_example_fill_presentation(
    fill_enabled: &mut bool,
    macro_set: MacroSet,
    name: &[u8],
) -> Option<&'static str> {
    if macro_set != MacroSet::Man {
        return None;
    }
    match name {
        b"nf" => {
            let redundant = !*fill_enabled;
            *fill_enabled = false;
            redundant.then_some("fill mode already disabled, skipping: nf")
        }
        b"fi" => {
            let redundant = *fill_enabled;
            *fill_enabled = true;
            redundant.then_some("fill mode already enabled, skipping: fi")
        }
        b"EX" => {
            let redundant = !*fill_enabled;
            *fill_enabled = false;
            redundant.then_some("fill mode already disabled, skipping: EX")
        }
        b"EE" => {
            let redundant = *fill_enabled;
            *fill_enabled = true;
            redundant.then_some("fill mode already enabled, skipping: EE")
        }
        _ => None,
    }
}

fn trailing_whitespace_start(bytes: &[u8]) -> Option<usize> {
    let offset = bytes
        .iter()
        .rposition(|byte| !matches!(*byte, b' ' | b'\t'));
    let Some(offset) = offset else {
        return (!bytes.is_empty()).then_some(0);
    };
    let trailing_start = offset.saturating_add(1);
    (trailing_start < bytes.len()).then_some(trailing_start)
}

/// Emit mandoc's portable-width style finding for ordinary package text.
///
/// tbl and eqn ranges bypass this helper because their fields have independent
/// grammar and are normalized by preprocessing rather than paragraph layout.
#[allow(clippy::too_many_arguments)]
fn emit_long_input_line(
    bytes: &[u8],
    line_start: u32,
    line_end: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    const STYLE_LINE_BYTES: usize = 80;
    const PREVIEW_CHARACTERS: usize = 20;
    if bytes.len() <= STYLE_LINE_BYTES {
        return;
    }
    let preview = decode_visible_bytes(bytes)
        .chars()
        .take(PREVIEW_CHARACTERS)
        .collect::<String>();
    let location = line_start.saturating_add(
        u32::try_from(bytes.len().saturating_sub(1))
            .expect("scanner source lines fit the public offset boundary"),
    );
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::INPUT_LINE_TOO_LONG,
            Severity::Style,
            source_id,
            location,
            line_end,
            format!("input text line longer than 80 bytes: {preview}..."),
        ),
        truncated,
    );
}

fn invalid_input_byte_offsets(bytes: &[u8]) -> Vec<(usize, u8)> {
    let mut invalid = bytes
        .iter()
        .enumerate()
        .filter_map(|(offset, byte)| {
            matches!(*byte, 0x00..=0x08 | 0x0b..=0x1f | 0x7f).then_some((offset, *byte))
        })
        .collect::<Vec<_>>();
    let mut cursor = 0;
    while cursor < bytes.len() {
        match std::str::from_utf8(&bytes[cursor..]) {
            Ok(_) => break,
            Err(error) => {
                cursor = cursor.saturating_add(error.valid_up_to());
                let width = error.error_len().unwrap_or(bytes.len() - cursor);
                invalid.extend(
                    bytes[cursor..cursor.saturating_add(width).min(bytes.len())]
                        .iter()
                        .enumerate()
                        .map(|(offset, byte)| (cursor + offset, *byte)),
                );
                cursor = cursor.saturating_add(width);
            }
        }
    }
    invalid.sort_unstable_by_key(|(offset, _)| *offset);
    invalid
}

fn push_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    limits: &Limits,
    diagnostic: Diagnostic,
    truncated: &mut bool,
) {
    if diagnostics.len() < limits.max_diagnostics {
        diagnostics.push(diagnostic);
    } else {
        *truncated = true;
    }
}

#[allow(clippy::too_many_arguments)]
fn record_expansion_steps(
    total: &mut usize,
    additional: usize,
    limits: &Limits,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> bool {
    let Some(next) = total.checked_add(additional) else {
        *truncated = true;
        return false;
    };
    if next > limits.max_expansion_steps {
        *truncated = true;
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::LIMIT_EXPANSION_STEPS,
                Severity::Warning,
                source_id,
                start,
                end,
                "scanner-stage aggregate escape work exceeds max_expansion_steps",
            ),
            truncated,
        );
        return false;
    }
    *total = next;
    true
}

#[allow(clippy::too_many_arguments)] // Keep parser call sites explicit about source-relative limits and recovery.
fn expand_environment(
    environment: &mut Environment,
    bytes: &[u8],
    escape: u8,
    arguments: &[Vec<u8>],
    limits: &Limits,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    expansion_steps: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> Option<Vec<u8>> {
    expand_environment_with_missing_reference_policy(
        environment,
        bytes,
        escape,
        arguments,
        limits,
        source_id,
        start,
        end,
        expansion_steps,
        diagnostics,
        truncated,
        true,
        false,
    )
}

/// Report source-level macro argument interpolation where no macro invocation
/// owns an argument frame. The environment normalizer still removes the
/// escaped argument from visible output; this scanner-stage finding retains
/// libmandoc's distinct error rather than treating it as an undefined string.
#[allow(clippy::too_many_arguments)] // Mirrors the other source-relative diagnostic emitters.
fn emit_outside_macro_argument_escapes(
    bytes: &[u8],
    escape: u8,
    start: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let mut offset = 0_usize;
    while offset.saturating_add(2) < bytes.len() {
        if bytes[offset] != escape {
            offset += 1;
            continue;
        }
        if bytes[offset + 1] == escape {
            offset += 2;
            continue;
        }
        if bytes[offset + 1] != b'$' || !matches!(bytes[offset + 2], b'1'..=b'9' | b'*' | b'@') {
            offset += 1;
            continue;
        }
        let finding_start = start.saturating_add(
            u32::try_from(offset).expect("parser bounds physical line offsets before diagnostics"),
        );
        let spelling = visible_bytes(&bytes[offset..offset + 3]);
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::ROFF_MACRO_ARGUMENT_OUTSIDE,
                Severity::Error,
                source_id,
                finding_start,
                finding_start.saturating_add(3),
                format!("using macro argument outside macro: {spelling}"),
            ),
            truncated,
        );
        offset += 3;
    }
}

/// Apply the recovery paired with [`emit_outside_macro_argument_escapes`].
/// A top-level `\$1` is diagnosed but cannot become an argument value for a
/// later user macro: retaining it would manufacture a recursive interpolation
/// that does not exist in mandoc's execution state.
fn strip_outside_macro_argument_escapes(bytes: &[u8], escape: u8) -> Vec<u8> {
    let mut output = Vec::with_capacity(bytes.len());
    let mut offset = 0_usize;
    while offset < bytes.len() {
        let selector = bytes.get(offset + 2).copied();
        if bytes.get(offset) == Some(&escape)
            && bytes.get(offset + 1) == Some(&b'$')
            && matches!(selector, Some(b'1'..=b'9' | b'*' | b'@'))
        {
            offset += 3;
            continue;
        }
        output.push(bytes[offset]);
        offset += 1;
    }
    output
}

/// Validate argument selectors while replaying a user macro body.
///
/// Copy mode leaves `\\$x` dormant in the stored definition and reactivates
/// it at invocation.  At that point mandoc diagnoses a non-numeric selector
/// against the caller's logical line and removes the three-byte escape; it is
/// neither ordinary visible text nor a generic unknown formatter escape.
#[allow(clippy::too_many_arguments)] // Keeps this source-relative rewrite beside its diagnostic policy.
fn normalize_macro_argument_number_escapes(
    bytes: &[u8],
    escape: u8,
    start: u32,
    builder: &DocumentBuilder,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> Vec<u8> {
    let mut normalized = Vec::with_capacity(bytes.len());
    let mut offset = 0_usize;
    while offset < bytes.len() {
        let invalid_selector = bytes.get(offset) == Some(&escape)
            && bytes.get(offset + 1) == Some(&b'$')
            && bytes
                .get(offset + 2)
                .is_some_and(|selector| !matches!(*selector, b'1'..=b'9' | b'*' | b'@'));
        if !invalid_selector {
            normalized.push(bytes[offset]);
            offset += 1;
            continue;
        }
        let spelling = visible_bytes(&bytes[offset..offset + 3]);
        let mut finding = diagnostic(
            DiagnosticCode::ROFF_MACRO_ARGUMENT_OUTSIDE,
            Severity::Error,
            source_id,
            start,
            start,
            format!("argument number is not numeric: {spelling}"),
        );
        if let Some(primary) = finding.primary.as_mut()
            && let Some(position) = builder.source_position(primary)
        {
            primary.logical_start = Some(SourcePosition {
                line: position.line,
                column: u32::try_from(offset + 1)
                    .expect("bounded macro body offsets fit public positions"),
            });
        }
        push_diagnostic(diagnostics, limits, finding, truncated);
        offset += 3;
    }
    normalized
}

/// The roff environment expands bracketed number-register names before the
/// visible-escape normalizer sees them. Preserve the validator diagnostic for
/// a missing closing bracket rather than silently turning the complete tail
/// into an empty register value.
#[allow(clippy::too_many_arguments)] // Mirrors the other source-relative diagnostic emitters.
fn emit_unterminated_register_reference_escapes(
    bytes: &[u8],
    escape: u8,
    start: u32,
    end: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let mut offset = 0_usize;
    while offset.saturating_add(2) < bytes.len() {
        if bytes[offset] != escape {
            offset += 1;
            continue;
        }
        if bytes[offset + 1] == escape {
            offset += 2;
            continue;
        }
        if bytes[offset + 1] != b'n' || bytes[offset + 2] != b'[' {
            offset += 1;
            continue;
        }
        if bytes[offset + 3..].contains(&b']') {
            offset += 3;
            continue;
        }
        let finding_start = start.saturating_add(
            u32::try_from(offset).expect("parser bounds physical line offsets before diagnostics"),
        );
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::ESCAPE_INVALID,
                Severity::Warning,
                source_id,
                finding_start,
                end,
                format!(
                    "invalid escape sequence: {}",
                    visible_bytes(&bytes[offset..])
                ),
            ),
            truncated,
        );
        if offset > 0 && matches!(bytes[offset - 1], b' ' | b'\t') {
            let whitespace_start = finding_start.saturating_sub(1);
            push_diagnostic(
                diagnostics,
                limits,
                diagnostic(
                    DiagnosticCode::INPUT_TRAILING_WHITESPACE,
                    Severity::Style,
                    source_id,
                    whitespace_start,
                    finding_start,
                    "whitespace at end of input line",
                ),
                truncated,
            );
        }
        return;
    }
}

/// Preserve the validator finding for a bracketed string interpolation whose
/// closing bracket is absent.  Environment expansion separately records the
/// remaining name as an undefined string and consumes it to an empty value.
#[allow(clippy::too_many_arguments)] // Mirrors the register-reference validator above.
fn emit_unterminated_string_reference_escapes(
    bytes: &[u8],
    escape: u8,
    start: u32,
    end: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let mut offset = 0_usize;
    while offset.saturating_add(2) < bytes.len() {
        if bytes[offset] != escape {
            offset += 1;
            continue;
        }
        if bytes[offset + 1] == escape {
            offset += 2;
            continue;
        }
        if bytes[offset + 1] != b'*' || bytes[offset + 2] != b'[' {
            offset += 1;
            continue;
        }
        if bytes[offset + 3..].contains(&b']') {
            offset += 3;
            continue;
        }
        let finding_start = start.saturating_add(
            u32::try_from(offset).expect("parser bounds physical line offsets before diagnostics"),
        );
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::ESCAPE_INVALID,
                Severity::Warning,
                source_id,
                finding_start,
                end,
                format!(
                    "invalid escape sequence: {}",
                    visible_bytes(&bytes[offset..])
                ),
            ),
            truncated,
        );
        return;
    }
}

/// Expand a definition while it is copied into session-owned storage.  Roff
/// interpolates ordinary references at definition time, but a doubled escape
/// remains literal until the later macro or string use.  Undefined references
/// have the legacy copy-mode recovery of producing no bytes and no public
/// diagnostic.
#[allow(clippy::too_many_arguments)] // Shares the bounded source-relative expansion boundary above.
fn expand_copy_mode_definition(
    environment: &mut Environment,
    bytes: &[u8],
    escape: u8,
    limits: &Limits,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    expansion_steps: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> Option<Vec<u8>> {
    expand_environment_with_missing_reference_policy(
        environment,
        bytes,
        escape,
        &[],
        limits,
        source_id,
        start,
        end,
        expansion_steps,
        diagnostics,
        truncated,
        false,
        true,
    )
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // Keep environment output, source-relative recovery, and shared budgets in one auditable boundary.
fn expand_environment_with_missing_reference_policy(
    environment: &mut Environment,
    bytes: &[u8],
    escape: u8,
    arguments: &[Vec<u8>],
    limits: &Limits,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    expansion_steps: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
    report_missing_references: bool,
    copy_mode_definition: bool,
) -> Option<Vec<u8>> {
    let remaining_steps = limits.max_expansion_steps.saturating_sub(*expansion_steps);
    let expansion = if copy_mode_definition {
        environment.expand_copy_mode_definition(
            bytes,
            escape,
            remaining_steps,
            limits.max_expanded_line_bytes,
        )
    } else {
        environment.expand(
            bytes,
            escape,
            arguments,
            remaining_steps,
            limits.max_expanded_line_bytes,
        )
    };
    match expansion {
        Ok(result) => {
            if !record_expansion_steps(
                expansion_steps,
                result.steps,
                limits,
                source_id,
                start,
                end,
                diagnostics,
                truncated,
            ) {
                return None;
            }
            if report_missing_references {
                // Mandoc's string-reference validator drains the findings
                // collected on one physical line in reverse source order.
                // Reset the direct-source matcher for each distinct pending
                // finding so this delayed order does not turn an earlier
                // reference into a line-start fallback.
                for missing in result.missing_references.into_iter().rev() {
                    // Roff installs an implicit empty value after the first
                    // failed interpolation.  It suppresses duplicate
                    // warnings and makes a following `dname` predicate true
                    // until `.rm`, but is not an explicit `.ds` definition
                    // and consequently must not move with `.rn`.
                    if let Err(error) =
                        environment.materialize_implicit_empty_string(&missing, limits)
                    {
                        *truncated = true;
                        push_diagnostic(
                            diagnostics,
                            limits,
                            environment_error_diagnostic(error, source_id, start, end),
                            truncated,
                        );
                        continue;
                    }
                    let mut missing_reference_cursor = 0_usize;
                    let finding_start = next_missing_reference_offset(
                        bytes,
                        escape,
                        &missing,
                        &mut missing_reference_cursor,
                    )
                    .and_then(|offset| u32::try_from(offset).ok())
                    .map_or(start, |offset| start.saturating_add(offset));
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_UNDEFINED_REFERENCE,
                            Severity::Warning,
                            source_id,
                            finding_start,
                            end,
                            format!("undefined string, using \"\": {}", visible_bytes(&missing)),
                        ),
                        truncated,
                    );
                }
            }
            for offset in result.malformed_escape_offsets {
                let finding_start = start.saturating_add(
                    u32::try_from(offset).expect("parser bounds every expanded source line"),
                );
                let finding_end = finding_start.saturating_add(2).min(end);
                push_diagnostic(
                    diagnostics,
                    limits,
                    diagnostic(
                        DiagnosticCode::ESCAPE_UNTERMINATED,
                        Severity::Warning,
                        source_id,
                        finding_start,
                        finding_end,
                        format!(
                            "invalid escape sequence: {}",
                            visible_bytes(bytes.get(offset..).unwrap_or_default())
                        ),
                    ),
                    truncated,
                );
            }
            Some(result.bytes)
        }
        Err(EnvironmentError::ExpansionLimit) => {
            *truncated = true;
            push_diagnostic(
                diagnostics,
                limits,
                diagnostic(
                    DiagnosticCode::LIMIT_EXPANSION_STEPS,
                    Severity::Warning,
                    source_id,
                    start,
                    end,
                    "roff environment expansion exceeds max_expansion_steps",
                ),
                truncated,
            );
            None
        }
        Err(EnvironmentError::RecursionLimit) => {
            let reference_offset = bytes
                .windows(2)
                .position(|window| window == [escape, b'*'])
                .unwrap_or(0);
            let finding_start = start.saturating_add(
                u32::try_from(reference_offset).expect("parser bounds every expanded source line"),
            );
            let finding_end = finding_start.saturating_add(2).min(end);
            push_diagnostic(
                diagnostics,
                limits,
                diagnostic(
                    DiagnosticCode::LIMIT_EXPANSION_STEPS,
                    Severity::Error,
                    source_id,
                    finding_start,
                    finding_end,
                    "input stack limit exceeded, infinite loop?",
                ),
                truncated,
            );
            Some(Vec::new())
        }
        Err(EnvironmentError::OutputLimit) => {
            *truncated = true;
            push_diagnostic(
                diagnostics,
                limits,
                diagnostic(
                    DiagnosticCode::ESCAPE_OUTPUT_LIMIT,
                    Severity::Warning,
                    source_id,
                    start,
                    end,
                    "roff environment output exceeds max_expanded_line_bytes",
                ),
                truncated,
            );
            None
        }
        Err(error) => {
            *truncated = true;
            push_diagnostic(
                diagnostics,
                limits,
                environment_error_diagnostic(error, source_id, start, end),
                truncated,
            );
            Some(bytes.to_vec())
        }
    }
}

/// Locate the next source-spelled missing string interpolation in one input
/// line. Environment expansion deliberately owns recursive and dynamic names,
/// so it returns only missing names; this pass restores mandoc's direct
/// reference column while a nested definition safely falls back to line start.
fn next_missing_reference_offset(
    bytes: &[u8],
    escape: u8,
    name: &[u8],
    cursor: &mut usize,
) -> Option<usize> {
    while *cursor < bytes.len() {
        let offset = bytes[*cursor..]
            .iter()
            .position(|byte| *byte == escape)
            .map(|relative| cursor.saturating_add(relative))?;
        if bytes.get(offset + 1) != Some(&b'*') {
            *cursor = offset.saturating_add(1);
            continue;
        }
        let name_start = offset.saturating_add(2);
        let (candidate, next) = match bytes.get(name_start).copied() {
            Some(b'[') => {
                let content_start = name_start.saturating_add(1);
                match bytes[content_start..].iter().position(|byte| *byte == b']') {
                    Some(relative_end) => {
                        let content_end = content_start.saturating_add(relative_end);
                        (
                            &bytes[content_start..content_end],
                            content_end.saturating_add(1),
                        )
                    }
                    None => (&bytes[content_start..], bytes.len()),
                }
            }
            Some(b'(') if bytes.len() >= name_start.saturating_add(3) => {
                let content_start = name_start.saturating_add(1);
                let content_end = content_start.saturating_add(2);
                (&bytes[content_start..content_end], content_end)
            }
            Some(_) => (
                &bytes[name_start..name_start.saturating_add(1)],
                name_start.saturating_add(1),
            ),
            None => return None,
        };
        *cursor = next;
        if candidate == name {
            return Some(offset);
        }
    }
    None
}

fn environment_error_diagnostic(
    error: EnvironmentError,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
) -> Diagnostic {
    let (code, message) = match error {
        EnvironmentError::DefinitionLimit => (
            DiagnosticCode::ROFF_DEFINITION_LIMIT,
            "roff environment definition count exceeds max_definitions",
        ),
        EnvironmentError::DefinitionBytesLimit => (
            DiagnosticCode::ROFF_DEFINITION_BYTES_LIMIT,
            "roff environment definition bytes exceed max_definition_bytes",
        ),
        EnvironmentError::RegisterExpression => (
            DiagnosticCode::ROFF_REGISTER_EXPRESSION,
            "number-register expression is not an integral basic-unit value",
        ),
        EnvironmentError::ExpansionLimit => (
            DiagnosticCode::LIMIT_EXPANSION_STEPS,
            "roff environment expansion exceeds max_expansion_steps",
        ),
        EnvironmentError::RecursionLimit => (
            DiagnosticCode::LIMIT_EXPANSION_STEPS,
            "input stack limit exceeded, infinite loop?",
        ),
        EnvironmentError::OutputLimit => (
            DiagnosticCode::ESCAPE_OUTPUT_LIMIT,
            "roff environment output exceeds max_expanded_line_bytes",
        ),
    };
    diagnostic(code, Severity::Warning, source_id, start, end, message)
}

#[allow(clippy::too_many_arguments)] // Translation shares the established source-aware recovery boundary.
fn translate_visible(
    environment: &Environment,
    bytes: &[u8],
    escape: u8,
    limits: &Limits,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> Option<Vec<u8>> {
    match environment.translate_text(bytes, escape, limits.max_expanded_line_bytes) {
        Ok(bytes) => Some(bytes),
        Err(error) => {
            *truncated = true;
            push_diagnostic(
                diagnostics,
                limits,
                environment_error_diagnostic(error, source_id, start, end),
                truncated,
            );
            None
        }
    }
}

fn diagnostic(
    code: &'static str,
    severity: Severity,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    message: impl Into<Box<str>>,
) -> Diagnostic {
    let code = DiagnosticCode::new(code).expect("static diagnostic code is valid");
    let span = SourceSpan::new(source_id, start, end).expect("scanner spans are monotonic");
    Diagnostic::new(code, severity, message).with_primary(span)
}

/// Keep the one byte accepted by roff's `.cc`, `.c2`, and `.ec` requests.
///
/// The scanner has already applied that first byte to the subsequent physical
/// input stream. This public-AST projection additionally mirrors mandoc's
/// validator: attached or later operands are discarded and produce one
/// source-precise excess-argument diagnostic.
fn normalize_character_request_arguments(
    request: &[u8],
    arguments: &mut Vec<Argument>,
    source_id: crate::SourceId,
    argument_start: u32,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let Some(first) = arguments.first() else {
        return;
    };
    let (excess_offset, excess_bytes) = if first.bytes.len() > 1 {
        (first.offset.saturating_add(1), &first.bytes[1..])
    } else if let Some(second) = arguments.get(1) {
        (second.offset, second.bytes.as_slice())
    } else {
        return;
    };
    let excess = visible_bytes(excess_bytes);
    let start = argument_start
        .checked_add(
            u32::try_from(excess_offset).expect("argument offsets are bounded by line length"),
        )
        .expect("parser checks public span offsets first");
    let end = start
        .checked_add(
            u32::try_from(excess_bytes.len()).expect("argument bytes are bounded by line length"),
        )
        .expect("parser checks public span offsets first");
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::ROFF_EXCESS_ARGUMENTS,
            Severity::Error,
            source_id,
            start,
            end,
            format!(
                "skipping excess arguments: {} ... {excess}",
                visible_bytes(request)
            ),
        ),
        truncated,
    );
    if let Some(first) = arguments.first_mut() {
        first.bytes.truncate(1);
    }
    arguments.truncate(1);
}

/// Validate and retain the declared-name state for a roff `.char` request.
///
/// libmandoc excludes these formatter definitions from the package AST. Its
/// old parser nevertheless validates the left operand independently of the
/// replacement string and carries unknown bracketed names into later escape
/// recovery, which is the observable contract preserved here.
#[allow(clippy::too_many_arguments)]
fn validate_character_request(
    raw_arguments: &[u8],
    escape: u8,
    environment: &mut Environment,
    source_id: crate::SourceId,
    argument_start: u32,
    line_end: u32,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let arguments = match lex_arguments(raw_arguments, escape, limits) {
        Ok(arguments) => arguments,
        Err(ArgumentIssue::UnterminatedQuote) => {
            push_diagnostic(
                diagnostics,
                limits,
                diagnostic(
                    DiagnosticCode::ARGUMENT_UNTERMINATED_QUOTE,
                    Severity::Warning,
                    source_id,
                    line_end,
                    line_end,
                    "roff char arguments contain an unterminated quote",
                ),
                truncated,
            );
            return;
        }
        Err(ArgumentIssue::Limit) => {
            *truncated = true;
            push_diagnostic(
                diagnostics,
                limits,
                diagnostic(
                    DiagnosticCode::ARGUMENT_LIMIT,
                    Severity::Warning,
                    source_id,
                    line_end,
                    line_end,
                    "roff char arguments exceed configured parser limits",
                ),
                truncated,
            );
            return;
        }
    };
    let Some(first) = arguments.first() else {
        emit_invalid_character_argument(
            raw_arguments,
            source_id,
            line_end,
            line_end,
            limits,
            diagnostics,
            truncated,
        );
        return;
    };
    let start = argument_start
        .checked_add(
            u32::try_from(first.offset).expect("argument offsets are bounded by line length"),
        )
        .expect("parser checks public span offsets first");
    if let Some(name) = bracketed_character_name(&first.bytes, escape) {
        environment.declare_character(name);
        emit_invalid_declared_character_warning(
            name,
            escape,
            source_id,
            start,
            limits,
            diagnostics,
            truncated,
        );
        if first.bytes.len() == name.len().saturating_add(3) {
            environment.define_character(name, join_arguments(&arguments[1..]));
            return;
        }
    }
    if first.bytes.len() == 1 {
        environment.define_character(&first.bytes, join_arguments(&arguments[1..]));
        return;
    }
    emit_invalid_character_argument(
        raw_arguments,
        source_id,
        start,
        line_end,
        limits,
        diagnostics,
        truncated,
    );
}

/// Return the leading `\\[name]` spelling, even when an invalid request
/// attaches trailing bytes that must separately produce an argument error.
fn bracketed_character_name(bytes: &[u8], escape: u8) -> Option<&[u8]> {
    let remainder = bytes.strip_prefix(&[escape, b'['])?;
    let close = remainder.iter().position(|byte| *byte == b']')?;
    (!remainder[..close].is_empty()).then_some(&remainder[..close])
}

#[allow(clippy::too_many_arguments)]
fn emit_declared_character_escape_warnings(
    bytes: &[u8],
    escape: u8,
    environment: &Environment,
    source_id: crate::SourceId,
    line_start: u32,
    line_end: u32,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let mut occurrences = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != escape {
            cursor += 1;
            continue;
        }
        let Some(name) = bracketed_character_name(&bytes[cursor..], escape) else {
            cursor += 1;
            continue;
        };
        if environment.has_declared_character(name) {
            occurrences.push((cursor, name));
        }
        cursor = cursor.saturating_add(name.len()).saturating_add(3);
    }
    // libmandoc emits multiple unknown special-character findings from one
    // source line in reverse encounter order.
    for (offset, name) in occurrences.into_iter().rev() {
        let start = line_start
            .checked_add(u32::try_from(offset).expect("line bytes fit source offsets"))
            .expect("parser checks public span offsets first");
        let _ = line_end;
        emit_invalid_declared_character_warning(
            name,
            escape,
            source_id,
            start,
            limits,
            diagnostics,
            truncated,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_invalid_declared_character_warning(
    name: &[u8],
    escape: u8,
    source_id: crate::SourceId,
    start: u32,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let spelling = format!("{}[{}]", char::from(escape), visible_bytes(name));
    let end = start
        .checked_add(
            u32::try_from(name.len().saturating_add(3)).expect("name bytes fit source offsets"),
        )
        .expect("parser checks public span offsets first");
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::ESCAPE_UNKNOWN_SPECIAL_CHARACTER,
            Severity::Warning,
            source_id,
            start,
            end,
            format!("invalid escape sequence: {spelling}"),
        ),
        truncated,
    );
}

/// Replace only names previously accepted by `.char`, retaining the original
/// source bytes for diagnostics. Formatter font escapes in a character value
/// receive mandoc's synthetic reset before following literal source flow.
fn expand_declared_character_escapes(
    bytes: &[u8],
    escape: u8,
    environment: &Environment,
) -> Vec<u8> {
    let mut expanded = Vec::with_capacity(bytes.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != escape {
            if let Some(value) = environment.character_definition(&bytes[cursor..=cursor]) {
                expanded.extend_from_slice(value);
            } else {
                expanded.push(bytes[cursor]);
            }
            cursor += 1;
            continue;
        }
        let Some(name) = bracketed_character_name(&bytes[cursor..], escape) else {
            expanded.push(bytes[cursor]);
            cursor += 1;
            continue;
        };
        let consumed = name.len().saturating_add(3);
        if let Some(value) = environment.character_definition(name) {
            expanded.extend_from_slice(value);
            if value.starts_with(&[escape, b'f']) {
                expanded.extend_from_slice(&[escape, b'f', b'P']);
            }
        } else {
            expanded.extend_from_slice(&bytes[cursor..cursor.saturating_add(consumed)]);
        }
        cursor = cursor.saturating_add(consumed);
    }
    expanded
}

#[allow(clippy::too_many_arguments)]
fn emit_invalid_character_argument(
    raw_arguments: &[u8],
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let display = visible_bytes(raw_arguments);
    let display = (!display.is_empty()).then(|| format!(" {display}"));
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::ROFF_INVALID_CHARACTER_ARGUMENT,
            Severity::Error,
            source_id,
            start,
            end,
            format!(
                "argument is not a character: char{}",
                display.as_deref().unwrap_or("")
            ),
        ),
        truncated,
    );
}

fn visible_bytes(bytes: &[u8]) -> String {
    decode_visible_bytes(bytes)
}

fn is_scope_opener(bytes: &[u8], escape: u8) -> bool {
    bytes == [escape, b'{', escape]
}

/// Identify a conditional scope opener and retain any same-line body bytes.
///
/// Standard multiline roff uses `\{\`, but mandoc also accepts `\{text`.
/// The optional escape after the opener is a line-continuation control, not
/// part of the executed first body line.
fn scope_opener_remainder(bytes: &[u8], escape: u8) -> Option<&[u8]> {
    let remainder = bytes.strip_prefix(&[escape, b'{'])?;
    let remainder = remainder.strip_prefix(&[escape]).unwrap_or(remainder);
    scope_closer_offset(remainder, escape)
        .is_none()
        .then_some(trim_horizontal_space(remainder))
}

/// Return the first visible source byte following a `\{` scope opener.
///
/// The opener and an optional physical-line continuation are roff grammar,
/// and horizontal padding after them is not part of the inline scope body.
/// Keep that offset distinct from the opener so projected legacy locations
/// anchor same-line conditional content at its first visible character.
fn scope_remainder_source_start(bytes: &[u8], start: u32, escape: u8) -> u32 {
    let Some(remainder) = bytes.strip_prefix(&[escape, b'{']) else {
        return start;
    };
    let (continuation_width, remainder) = remainder
        .strip_prefix(&[escape])
        .map_or((0_usize, remainder), |remainder| (1, remainder));
    let padding_width = remainder
        .len()
        .saturating_sub(trim_horizontal_space(remainder).len());
    let offset = 2_usize
        .saturating_add(continuation_width)
        .saturating_add(padding_width);
    start.saturating_add(u32::try_from(offset).unwrap_or(u32::MAX))
}

/// Normalize a same-line `\{ ... \}` conditional body into the legacy
/// public text spelling. The opening delimiter is grammar, while its closing
/// delimiter becomes the zero-width `\&` control that preserves adjacent
/// source text without exposing the scope marker itself.
fn inline_scope_body_template(bytes: &[u8], escape: u8) -> Option<Vec<u8>> {
    let remainder = bytes.strip_prefix(&[escape, b'{'])?;
    let close = scope_closer_offset(remainder, escape)?;
    let prefix = trim_horizontal_space(&remainder[..close]);
    let suffix = &remainder[close + 2..];
    let mut normalized = Vec::with_capacity(prefix.len().saturating_add(suffix.len() + 2));
    normalized.extend_from_slice(prefix);
    normalized.extend_from_slice(&[escape, b'&']);
    normalized.extend_from_slice(suffix);
    Some(normalized)
}

fn is_scope_closer(bytes: &[u8], escape: u8) -> bool {
    bytes == [escape, b'}']
}

fn scope_closer_offset(bytes: &[u8], escape: u8) -> Option<usize> {
    bytes.windows(2).enumerate().find_map(|(offset, pair)| {
        if pair != [escape, b'}'] {
            return None;
        }
        let escapes_before = bytes[..offset]
            .iter()
            .rev()
            .take_while(|byte| **byte == escape)
            .count();
        (escapes_before % 2 == 0).then_some(offset)
    })
}

/// Whether the innermost collected scope is a condition that is certainly
/// inactive in the parser's fixed nroff execution mode.  This is intentionally
/// narrow: dynamic numeric/register predicates remain executable scope data,
/// while `t` and `!n` let collection preserve mandoc's rule that an attached
/// physical-line tail stays suppressed with its inactive inner branch.
fn innermost_scope_is_statically_inactive(frames: &[PendingScope]) -> bool {
    matches!(
        frames.last().and_then(|frame| frame.kind.as_ref()),
        Some(ScopeKind::Conditional { predicate, .. }) if matches!(predicate.as_slice(), b"t" | b"!n")
    )
}

/// Retain one physical text line containing multiple `\}` closers.
///
/// The closers are grammar only, but the owned legacy tree retains a
/// zero-width `\&` boundary for each removed closer that precedes later
/// source text.  This remains observable even when the suffix starts with a
/// blank (`on\} the\} same` becomes `on\& the\& same`).
fn scope_closer_text(bytes: &[u8], escape: u8) -> Vec<u8> {
    let mut retained = Vec::with_capacity(bytes.len());
    let mut remaining = bytes;
    while let Some(close) = scope_closer_offset(remaining, escape) {
        retained.extend_from_slice(&remaining[..close]);
        remaining = &remaining[close + 2..];
        if !remaining.is_empty() {
            retained.extend_from_slice(&[escape, b'&']);
        }
    }
    retained.extend_from_slice(remaining);
    retained
}

/// Remove structural closers from the arguments of a malformed, attached man
/// font macro. Such macros retain their regular argument grammar, including
/// the no-space joins around the removed closers.
fn font_macro_arguments_without_scope_closers(bytes: &[u8], escape: u8) -> Vec<u8> {
    let mut retained = Vec::with_capacity(bytes.len());
    let mut remaining = bytes;
    while let Some(close) = scope_closer_offset(remaining, escape) {
        retained.extend_from_slice(&remaining[..close]);
        remaining = &remaining[close + 2..];
        // A closer embedded in an argument is grammar, but it may sit
        // directly between two visible word fragments.  The legacy tree
        // retains a zero-width no-space atom so later package/renderer paths
        // keep the authored join rather than treating the fragments as a
        // freshly separated token.
        if !retained.is_empty()
            && !remaining.is_empty()
            && !retained.last().is_some_and(u8::is_ascii_whitespace)
            && !remaining.first().is_some_and(u8::is_ascii_whitespace)
        {
            retained.extend_from_slice(&[escape, b'&']);
        }
    }
    retained.extend_from_slice(remaining);
    retained
}

#[allow(clippy::too_many_arguments)] // Scope collection shares parser session state and ordered diagnostics.
#[allow(clippy::too_many_lines)] // Collection mirrors scanner cases while retaining nested scopes without recursion.
fn collect_scope(
    scanner: &mut Scanner<'_>,
    source_id: crate::SourceId,
    limits: &Limits,
    macro_set: MacroSet,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
    emit_definition_tail_diagnostics: bool,
    scope_start: u32,
    scope_end: u32,
    unterminated_scope_name: Option<&[u8]>,
) -> CollectedScope {
    let character_state = scanner.character_state();
    macro_rules! finish_scope {
        ($scope:expr) => {{
            scanner.restore_character_state(character_state);
            return $scope;
        }};
    }
    let mut frames = vec![PendingScope {
        start: scope_start,
        end: scope_end,
        kind: None,
        lines: Vec::new(),
    }];
    let mut discarded_nesting = 0_usize;
    loop {
        // `next_line` applies `.cc`/`.c2`/`.ec` after lexing a request.  Use
        // the state that was active before consuming this physical line, then
        // observe its replacement on the next line just as normal execution
        // does.
        let escape = scanner.escape_character();
        let Some(line) = scanner.next_line() else {
            break;
        };
        match line {
            ScannedLine::TooLong { start, end } => {
                *truncated = true;
                push_diagnostic(
                    diagnostics,
                    limits,
                    diagnostic(
                        DiagnosticCode::LIMIT_LINE_BYTES,
                        Severity::Warning,
                        source_id,
                        start,
                        end,
                        "roff scope line exceeds max_line_bytes and was skipped",
                    ),
                    truncated,
                );
            }
            ScannedLine::Comment { .. } => {}
            ScannedLine::Control { start, name, .. } if is_scope_closer(name, escape) => {
                if discarded_nesting > 0 {
                    discarded_nesting -= 1;
                    continue;
                }
                if let Some(scope) = close_collected_scope(&mut frames, start) {
                    finish_scope!(scope);
                }
            }
            ScannedLine::Control {
                start,
                no_break: _,
                name,
                arguments,
                ..
            } if name.starts_with(&[escape, b'}']) => {
                // The scanner keeps `\}middle` as one control name so that
                // normal macro parsing can preserve it.  Inside a collected
                // scope, however, it is a closing request: mandoc discards
                // its argument tail.  Further closers in that tail still
                // unwind nested frames, but their intervening text is also
                // not a scope body.
                let mut remaining = name[2..].to_vec();
                remaining.extend_from_slice(arguments);
                if discarded_nesting > 0 {
                    discarded_nesting -= 1;
                } else if let Some(scope) = close_collected_scope(&mut frames, start) {
                    finish_scope!(scope);
                }
                while let Some(close) = scope_closer_offset(&remaining, escape) {
                    remaining.drain(..close + 2);
                    if discarded_nesting > 0 {
                        discarded_nesting -= 1;
                        continue;
                    }
                    if let Some(scope) = close_collected_scope(&mut frames, start) {
                        finish_scope!(scope);
                    }
                }
            }
            ScannedLine::Control {
                start,
                end,
                no_break: _,
                name,
                arguments,
                argument_start,
                ..
            } => {
                // A scope closer can be appended to a request, most commonly
                // `.br\}`.  Retain the request itself, then close the active
                // scope.  Treating only a standalone `.\}` as a closer lets
                // an outer scope consume subsequent `.el` branches as its
                // body and eventually exposes the opener as ordinary text.
                let close = scope_closer_offset(arguments, escape);
                // In a collected conditional body, an attached scope closer
                // belongs to the scope grammar even when it occurs *inside*
                // a visible man font argument (`.B word\}suffix`).  Replay
                // the package macro with the closer removed, retain the
                // legacy `\&` join, then unwind the scope.  Restricting this
                // recovery to a leading closer loses the middle-of-argument
                // form exercised by regress/roff/cond/if.
                let attached_font_scope_closer =
                    is_man_visible_argument_macro(macro_set, name) && close.is_some();
                let malformed_attached_font_name =
                    attached_font_scope_closer && arguments.starts_with(&[escape, b'}']);
                if malformed_attached_font_name && !innermost_scope_is_statically_inactive(&frames)
                {
                    let mut preview = Vec::with_capacity(name.len().saturating_add(2));
                    preview.extend_from_slice(name);
                    preview.extend_from_slice(&[escape, b'&']);
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_ESCAPED_NAME,
                            Severity::Error,
                            source_id,
                            start,
                            start.saturating_add(1),
                            format!(
                                "escaped character not allowed in a name: {}",
                                visible_bytes(&preview)
                            ),
                        ),
                        truncated,
                    );
                }
                if attached_font_scope_closer {
                    let retained_arguments =
                        font_macro_arguments_without_scope_closers(arguments, escape);
                    if discarded_nesting == 0 && !retained_arguments.is_empty() {
                        frames
                            .last_mut()
                            .expect("scope collector always retains a root frame")
                            .lines
                            .push(ScopeLine::Control {
                                start,
                                end,
                                argument_start: argument_start.saturating_add(
                                    if arguments.starts_with(&[escape, b'}']) {
                                        2
                                    } else {
                                        0
                                    },
                                ),
                                name: name.to_vec(),
                                arguments: retained_arguments,
                            });
                    }
                    let mut remaining = arguments;
                    while let Some(close) = scope_closer_offset(remaining, escape) {
                        remaining = &remaining[close + 2..];
                        if discarded_nesting > 0 {
                            discarded_nesting -= 1;
                            continue;
                        }
                        if let Some(scope) = close_collected_scope(&mut frames, start) {
                            finish_scope!(scope);
                        }
                    }
                    continue;
                }
                if emit_definition_tail_diagnostics
                    && name == b"."
                    && let Some(close) = close
                    && !arguments[close + 2..].is_empty()
                {
                    let diagnostic_start = argument_start
                        .checked_add(
                            u32::try_from(close).expect("scope line offsets fit source positions"),
                        )
                        .expect("scope scanner spans are monotonic");
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_ALL_ARGUMENTS,
                            Severity::Error,
                            source_id,
                            diagnostic_start,
                            end,
                            format!(
                                "skipping all arguments: .. \\&{}",
                                visible_bytes(&arguments[close + 2..])
                            ),
                        ),
                        truncated,
                    );
                }
                // A closer attached to an ordinary request ends the enclosing
                // conditional scope, but it is not an argument boundary for
                // that request.  In `.B bold\}tail`, mandoc gives `B` the
                // visible `boldtail` argument before closing the scope. Keep
                // a later closer separate so it can still unwind an outer
                // collected frame.
                let (retained_arguments, attached_suffix_width) = close.map_or_else(
                    || (arguments.to_vec(), 0_usize),
                    |offset| {
                        let suffix = &arguments[offset + 2..];
                        let suffix_width =
                            scope_closer_offset(suffix, escape).unwrap_or(suffix.len());
                        let mut retained = Vec::with_capacity(offset.saturating_add(suffix_width));
                        retained.extend_from_slice(&arguments[..offset]);
                        retained.extend_from_slice(&suffix[..suffix_width]);
                        (retained, suffix_width)
                    },
                );
                let scope_kind =
                    scoped_request_kind(name, &retained_arguments, argument_start, escape, limits);
                if let Some(kind) = scope_kind {
                    if discarded_nesting > 0 {
                        discarded_nesting += 1;
                        continue;
                    }
                    if frames.len() >= limits.max_tree_depth {
                        *truncated = true;
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::LIMIT_SCOPE_DEPTH,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "nested roff scope exceeds max_tree_depth and was skipped",
                            ),
                            truncated,
                        );
                        discarded_nesting = 1;
                        continue;
                    }
                    frames.push(PendingScope {
                        start,
                        end,
                        kind: Some(kind),
                        lines: Vec::new(),
                    });
                } else if discarded_nesting == 0 {
                    frames
                        .last_mut()
                        .expect("scope collector always retains a root frame")
                        .lines
                        .push(ScopeLine::Control {
                            start,
                            end,
                            argument_start,
                            name: name.to_vec(),
                            arguments: retained_arguments,
                        });
                }
                if let Some(close) = close {
                    let discard_line_tail = innermost_scope_is_statically_inactive(&frames);
                    let mut remaining = &arguments[close + 2 + attached_suffix_width..];
                    if discarded_nesting > 0 {
                        discarded_nesting -= 1;
                    } else if let Some(scope) = close_collected_scope(&mut frames, start) {
                        finish_scope!(scope);
                    }
                    while let Some(next_close) = scope_closer_offset(remaining, escape) {
                        if discarded_nesting == 0 && !discard_line_tail && next_close > 0 {
                            frames
                                .last_mut()
                                .expect("scope collector always retains a root frame")
                                .lines
                                .push(ScopeLine::Text {
                                    start,
                                    end,
                                    bytes: remaining[..next_close].to_vec(),
                                    terminal_inline: false,
                                });
                        }
                        remaining = &remaining[next_close + 2..];
                        if discarded_nesting > 0 {
                            discarded_nesting -= 1;
                            continue;
                        }
                        if let Some(mut scope) = close_collected_scope(&mut frames, start) {
                            if !discard_line_tail && !remaining.is_empty() {
                                scope.lines.push(ScopeLine::Text {
                                    start,
                                    end,
                                    bytes: remaining.to_vec(),
                                    terminal_inline: false,
                                });
                            }
                            finish_scope!(scope);
                        }
                    }
                    if discarded_nesting == 0 && !discard_line_tail && !remaining.is_empty() {
                        frames
                            .last_mut()
                            .expect("scope collector always retains a root frame")
                            .lines
                            .push(ScopeLine::Text {
                                start,
                                end,
                                bytes: remaining.to_vec(),
                                terminal_inline: false,
                            });
                    }
                }
            }
            ScannedLine::Text { start, end, bytes } => {
                let first_close = scope_closer_offset(bytes, escape);
                let has_nested_close = first_close.is_some_and(|close| {
                    scope_closer_offset(&bytes[close + 2..], escape).is_some()
                });
                if has_nested_close && frames.len() > 1 {
                    let text = scope_closer_text(bytes, escape);
                    if discarded_nesting == 0 && !text.is_empty() {
                        frames
                            .last_mut()
                            .expect("scope collector always retains a root frame")
                            .lines
                            .push(ScopeLine::Text {
                                start,
                                end,
                                bytes: text,
                                terminal_inline: false,
                            });
                    }
                    let mut remaining = bytes;
                    while let Some(close) = scope_closer_offset(remaining, escape) {
                        remaining = &remaining[close + 2..];
                        if discarded_nesting > 0 {
                            discarded_nesting -= 1;
                            continue;
                        }
                        if let Some(scope) = close_collected_scope(&mut frames, start) {
                            finish_scope!(scope);
                        }
                    }
                    continue;
                }
                let mut remaining = bytes;
                let mut discard_line_tail = false;
                let mut terminal_inline = false;
                while let Some(close) = scope_closer_offset(remaining, escape) {
                    if discarded_nesting == 0 && !discard_line_tail && frames.len() == 1 {
                        // A suffix after the outermost closer was historically
                        // part of this physical body line (for example
                        // `\\n[count]\\},`).  Keep both visible fragments in
                        // one authored text node before ending the scope.
                        let mut retained = Vec::with_capacity(remaining.len().saturating_sub(2));
                        retained.extend_from_slice(&remaining[..close]);
                        let suffix = &remaining[close + 2..];
                        if !suffix.is_empty() {
                            // The legacy tree puts an invisible `\\&` between
                            // the body and an attached suffix, so punctuation
                            // after an inline closer remains source-visible
                            // rather than being folded into the scope marker.
                            retained.extend_from_slice(&[escape, b'&']);
                            retained.extend_from_slice(suffix);
                        }
                        if !retained.is_empty() {
                            frames
                                .last_mut()
                                .expect("scope collector always retains a root frame")
                                .lines
                                .push(ScopeLine::Text {
                                    start,
                                    end,
                                    bytes: retained,
                                    terminal_inline: true,
                                });
                        }
                        finish_scope!(
                            close_collected_scope(&mut frames, start)
                                .expect("the root scope always closes into a result")
                        );
                    }
                    let closes_inactive_scope = innermost_scope_is_statically_inactive(&frames);
                    if discarded_nesting == 0 && !discard_line_tail && close > 0 {
                        frames
                            .last_mut()
                            .expect("scope collector always retains a root frame")
                            .lines
                            .push(ScopeLine::Text {
                                start,
                                end,
                                bytes: remaining[..close].to_vec(),
                                terminal_inline: false,
                            });
                    }
                    remaining = &remaining[close + 2..];
                    terminal_inline = true;
                    if discarded_nesting > 0 {
                        discarded_nesting -= 1;
                        continue;
                    }
                    discard_line_tail |= closes_inactive_scope;
                    if let Some(scope) = close_collected_scope(&mut frames, start) {
                        // The current scanner API owns a physical line once it
                        // has been read.  Retain suffix text after the outer
                        // closer instead of dropping it; future structural
                        // phases can give that suffix its exact sibling role.
                        finish_scope!(scope);
                    }
                }
                if discarded_nesting == 0 && !discard_line_tail && !remaining.is_empty() {
                    frames
                        .last_mut()
                        .expect("scope collector always retains a root frame")
                        .lines
                        .push(ScopeLine::Text {
                            start,
                            end,
                            bytes: remaining.to_vec(),
                            terminal_inline,
                        });
                }
            }
        }
    }
    *truncated = true;
    let incomplete_start = frames.last().map_or(0, |frame| frame.start);
    let incomplete_end = frames.last().map_or(0, |frame| frame.end);
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::ROFF_UNTERMINATED_SCOPE,
            Severity::Error,
            source_id,
            incomplete_start,
            incomplete_end,
            unterminated_scope_name.map_or_else(
                || "roff scope reached source end before its `\\}` terminator".to_owned(),
                |name| format!("appending missing end of block: {}", visible_bytes(name)),
            ),
        ),
        truncated,
    );
    let scope = CollectedScope {
        lines: frames
            .into_iter()
            .next()
            .expect("scope collector always retains a root frame")
            .lines,
        terminated: false,
        closer_start: None,
    };
    scanner.restore_character_state(character_state);
    scope
}

/// Remove one brace-delimited body from the active copy-reparsed macro frame.
///
/// The main macro executor already stores deferred lines on an explicit LIFO
/// stack.  A macro-local conditional must consume only entries from that same
/// invocation depth: touching a shallower entry would steal the caller's next
/// request.  The returned lines remain in copy mode until the caller selects
/// and pushes them back onto the execution stack.
fn collect_pending_macro_scope(
    pending: &mut Vec<PendingMacroLine>,
    macro_depth: usize,
    control: u8,
    escape: u8,
    limits: &Limits,
) -> Option<Vec<PendingMacroLine>> {
    let mut lines = Vec::new();
    let mut nested_scopes = 0_usize;
    while pending
        .last()
        .is_some_and(|(_, _, depth, _, _, _)| *depth == macro_depth)
    {
        let mut line = pending.pop().expect("checked pending macro entry");
        let reparsed = copy_mode_reparse(&line.0, escape);
        let Some((request, arguments)) = split_macro_control(&reparsed, control, escape) else {
            lines.push(line);
            continue;
        };
        if is_scope_closer(request, escape) {
            if nested_scopes == 0 {
                return Some(lines);
            }
            nested_scopes -= 1;
            lines.push(line);
            continue;
        }
        if let Some(retained_request) = request.strip_suffix(&[escape, b'}']) {
            let mut retained_line = Vec::with_capacity(
                1 + retained_request.len() + usize::from(!arguments.is_empty()) + arguments.len(),
            );
            retained_line.push(control);
            retained_line.extend_from_slice(retained_request);
            if !arguments.is_empty() {
                retained_line.push(b' ');
                retained_line.extend_from_slice(arguments);
            }
            line.0 = retained_line;
            if nested_scopes == 0 {
                lines.push(line);
                return Some(lines);
            }
            nested_scopes -= 1;
            lines.push(line);
            continue;
        }
        let opens_scope = match request {
            b"while" => lex_arguments(arguments, escape, limits).is_ok_and(|arguments| {
                arguments
                    .split_first()
                    .is_some_and(|(_, body)| is_scope_opener(&join_arguments(body), escape))
            }),
            b"if" | b"ie" => lex_condition_arguments(arguments, escape, limits)
                .ok()
                .and_then(|arguments| {
                    condition_parts(&arguments)
                        .map(|(_, body_start)| join_arguments(&arguments[body_start..]))
                })
                .is_some_and(|body| is_scope_opener(&body, escape)),
            _ => false,
        };
        if opens_scope {
            nested_scopes = nested_scopes.saturating_add(1);
        }
        lines.push(line);
    }
    None
}

fn close_collected_scope(
    frames: &mut Vec<PendingScope>,
    closer_start: u32,
) -> Option<CollectedScope> {
    let closed = frames
        .pop()
        .expect("scope collector only closes non-empty frame stacks");
    let Some(kind) = closed.kind else {
        return Some(CollectedScope {
            lines: closed.lines,
            terminated: true,
            closer_start: Some(closer_start),
        });
    };
    let (initial_body, kind) = match kind {
        ScopeKind::Loop {
            predicate,
            initial_body,
        } => (
            initial_body,
            ScopeKind::Loop {
                predicate,
                initial_body: None,
            },
        ),
        ScopeKind::Conditional {
            predicate,
            initial_body,
            else_eligible,
        } => (
            initial_body,
            ScopeKind::Conditional {
                predicate,
                initial_body: None,
                else_eligible,
            },
        ),
        ScopeKind::Else { initial_body } => (initial_body, ScopeKind::Else { initial_body: None }),
    };
    let mut lines = closed.lines;
    if let Some((bytes, start)) = initial_body.filter(|(bytes, _)| !bytes.is_empty()) {
        lines.insert(
            0,
            ScopeLine::Text {
                start,
                end: closed.end,
                bytes,
                terminal_inline: false,
            },
        );
    }
    let line = match kind {
        ScopeKind::Loop { predicate, .. } => ScopeLine::Loop {
            start: closed.start,
            end: closed.end,
            predicate,
            lines,
        },
        ScopeKind::Conditional {
            predicate,
            else_eligible,
            ..
        } => ScopeLine::Conditional {
            start: closed.start,
            end: closed.end,
            predicate,
            lines,
            else_eligible,
        },
        ScopeKind::Else { .. } => ScopeLine::Else {
            start: closed.start,
            end: closed.end,
            lines,
        },
    };
    frames
        .last_mut()
        .expect("nested scope has a parent frame")
        .lines
        .push(line);
    None
}

fn scoped_request_kind(
    name: &[u8],
    arguments: &[u8],
    argument_start: u32,
    escape: u8,
    limits: &Limits,
) -> Option<ScopeKind> {
    match name {
        b"while" => {
            let arguments = lex_arguments(arguments, escape, limits).ok()?;
            let (predicate, body_arguments) = arguments.split_first()?;
            let body_argument = body_arguments.first()?;
            let body = join_arguments(body_arguments);
            scope_opener_remainder(&body, escape).map(|initial_body| ScopeKind::Loop {
                predicate: predicate.bytes.clone(),
                initial_body: (!initial_body.is_empty()).then(|| {
                    let start = argument_start.saturating_add(
                        u32::try_from(body_argument.offset)
                            .expect("scope argument offsets fit source spans"),
                    );
                    (
                        initial_body.to_vec(),
                        scope_remainder_source_start(&body, start, escape),
                    )
                }),
            })
        }
        b"if" | b"ie" => {
            let arguments = lex_condition_arguments(arguments, escape, limits).ok()?;
            let (predicate, body_start) = condition_parts(&arguments)?;
            let body_arguments = &arguments[body_start..];
            let body_argument = body_arguments.first()?;
            let body = join_arguments(body_arguments);
            scope_opener_remainder(&body, escape).map(|initial_body| ScopeKind::Conditional {
                predicate,
                initial_body: (!initial_body.is_empty()).then(|| {
                    let start = argument_start.saturating_add(
                        u32::try_from(body_argument.offset)
                            .expect("scope argument offsets fit source spans"),
                    );
                    (
                        initial_body.to_vec(),
                        scope_remainder_source_start(&body, start, escape),
                    )
                }),
                else_eligible: name == b"ie",
            })
        }
        b"el" => scope_opener_remainder(arguments, escape).map(|initial_body| ScopeKind::Else {
            initial_body: (!initial_body.is_empty()).then(|| {
                (
                    initial_body.to_vec(),
                    scope_remainder_source_start(arguments, argument_start, escape),
                )
            }),
        }),
        _ => None,
    }
}

/// A same-line conditional body is usually raw text, except that a copy-mode
/// definition must remain a control event so the scope executor can collect
/// and install it before the following physical source resumes.
fn definition_scope_remainder_line(
    bytes: &[u8],
    start: u32,
    end: u32,
    control: u8,
    escape: u8,
) -> ScopeLine {
    let Some((name, arguments)) = split_macro_control(bytes, control, escape) else {
        return ScopeLine::Text {
            start,
            end,
            bytes: bytes.to_vec(),
            terminal_inline: false,
        };
    };
    if matches!(
        name,
        b"de" | b"de1" | b"am" | b"dei" | b"ami" | b"ds" | b"as"
    ) {
        ScopeLine::Control {
            start,
            end,
            argument_start: start
                .saturating_add(1)
                .saturating_add(
                    u32::try_from(name.len()).expect("scope request names fit source spans"),
                )
                .saturating_add(u32::from(!arguments.is_empty())),
            name: name.to_vec(),
            arguments: arguments.to_vec(),
        }
    } else {
        ScopeLine::Text {
            start,
            end,
            bytes: bytes.to_vec(),
            terminal_inline: false,
        }
    }
}

/// Retain only the names that an inactive scope would otherwise have defined.
/// A subsequent invocation is an upstream error rather than a public unknown
/// element, while unrelated unknown requests keep their existing behavior.
fn record_suppressed_scope_definitions(
    lines: &[ScopeLine],
    escape: u8,
    environment: &mut Environment,
    limits: &Limits,
) {
    for line in lines {
        let ScopeLine::Control {
            name, arguments, ..
        } = line
        else {
            continue;
        };
        if !matches!(name.as_slice(), b"de" | b"de1" | b"am" | b"dei" | b"ami") {
            continue;
        }
        if let Ok(arguments) = lex_arguments(arguments, escape, limits)
            && let Some(name) = arguments.first()
        {
            environment.suppress_macro_name(&name.bytes);
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)] // An explicit frame stack avoids recursive execution of untrusted nested scopes.
fn execute_scope_lines(
    lines: &[ScopeLine],
    builder: &mut DocumentBuilder,
    root: NodeId,
    source_id: crate::SourceId,
    scanner: &mut Scanner<'_>,
    environment: &mut Environment,
    limits: &Limits,
    text_bytes: &mut usize,
    expansion_steps: &mut usize,
    maximum_depth: &mut usize,
    total_loop_iterations: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> ScopeFlow {
    let mut closed_loop_from_inner_scope = None;
    let mut frames = vec![ScopeExecutionFrame::Lines {
        lines,
        next: 0,
        previous_conditional: None,
    }];
    while let Some(frame) = frames.pop() {
        match frame {
            ScopeExecutionFrame::SetNewRootChildrenLogicalStart {
                first_child,
                position,
            } => {
                set_new_root_children_logical_start(builder, root, first_child, position);
            }
            ScopeExecutionFrame::Lines {
                lines,
                next,
                previous_conditional,
            } => {
                let Some(line) = lines.get(next) else {
                    continue;
                };
                if let Some(consumed) = execute_collected_scope_definition(
                    line,
                    &lines[next + 1..],
                    scanner,
                    environment,
                    limits,
                    source_id,
                    diagnostics,
                    truncated,
                ) {
                    frames.push(ScopeExecutionFrame::Lines {
                        lines,
                        next: next + consumed + 1,
                        previous_conditional: None,
                    });
                    continue;
                }
                if let ScopeLine::Control {
                    start,
                    end,
                    name,
                    arguments,
                    ..
                } = line
                    && matches!(name.as_slice(), b"ie" | b"el")
                {
                    if name == b"el" {
                        frames.push(ScopeExecutionFrame::Lines {
                            lines,
                            next: next + 1,
                            previous_conditional: None,
                        });
                        if previous_conditional != Some(false) {
                            continue;
                        }
                        let body = trim_horizontal_space(arguments);
                        if body.is_empty() {
                            continue;
                        }
                        let body = inline_scope_body_line(
                            body.to_vec(),
                            *start,
                            *end,
                            scanner.control_character(),
                            scanner.escape_character(),
                        );
                        match execute_scope_line(
                            &body,
                            builder,
                            root,
                            source_id,
                            scanner,
                            environment,
                            limits,
                            text_bytes,
                            expansion_steps,
                            maximum_depth,
                            total_loop_iterations,
                            diagnostics,
                            truncated,
                        ) {
                            ScopeFlow::Continue => {}
                            flow => return flow,
                        }
                        continue;
                    }
                    let Ok(condition_arguments) =
                        lex_condition_arguments(arguments, scanner.escape_character(), limits)
                    else {
                        *truncated = true;
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ARGUMENT_LIMIT,
                                Severity::Warning,
                                source_id,
                                *start,
                                *end,
                                "inline roff ie arguments in a scope exceed configured parser limits",
                            ),
                            truncated,
                        );
                        frames.push(ScopeExecutionFrame::Lines {
                            lines,
                            next: next + 1,
                            previous_conditional: None,
                        });
                        continue;
                    };
                    let Some((predicate, body_start)) = condition_parts(&condition_arguments)
                    else {
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_CONDITION,
                                Severity::Warning,
                                source_id,
                                *start,
                                *end,
                                "inline roff ie in a scope is missing its predicate",
                            ),
                            truncated,
                        );
                        frames.push(ScopeExecutionFrame::Lines {
                            lines,
                            next: next + 1,
                            previous_conditional: None,
                        });
                        continue;
                    };
                    let Some(predicate) = expand_environment(
                        environment,
                        &predicate,
                        scanner.escape_character(),
                        &[],
                        limits,
                        source_id,
                        *start,
                        *end,
                        expansion_steps,
                        diagnostics,
                        truncated,
                    ) else {
                        return ScopeFlow::Halt;
                    };
                    let Some(condition) = evaluate_condition(environment, &predicate) else {
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_CONDITION,
                                Severity::Warning,
                                source_id,
                                *start,
                                *end,
                                "inline roff ie predicate in a scope is outside the M3 numeric/nroff subset",
                            ),
                            truncated,
                        );
                        frames.push(ScopeExecutionFrame::Lines {
                            lines,
                            next: next + 1,
                            previous_conditional: None,
                        });
                        continue;
                    };
                    let body = condition_body_template(arguments, &condition_arguments, body_start);
                    frames.push(ScopeExecutionFrame::Lines {
                        lines,
                        next: next + 1,
                        previous_conditional: Some(condition),
                    });
                    if !condition || body.is_empty() {
                        continue;
                    }
                    let body = inline_scope_body_line(
                        body,
                        *start,
                        *end,
                        scanner.control_character(),
                        scanner.escape_character(),
                    );
                    match execute_scope_line(
                        &body,
                        builder,
                        root,
                        source_id,
                        scanner,
                        environment,
                        limits,
                        text_bytes,
                        expansion_steps,
                        maximum_depth,
                        total_loop_iterations,
                        diagnostics,
                        truncated,
                    ) {
                        ScopeFlow::Continue => {}
                        flow => return flow,
                    }
                    continue;
                }
                if let ScopeLine::Control {
                    start,
                    end,
                    name,
                    arguments,
                    ..
                } = line
                    && name == b"ig"
                {
                    let marker = match ignore_marker(arguments, scanner.escape_character(), limits)
                    {
                        Ok(marker) => marker,
                        Err(ArgumentIssue::UnterminatedQuote) => {
                            push_diagnostic(
                                diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ARGUMENT_UNTERMINATED_QUOTE,
                                    Severity::Warning,
                                    source_id,
                                    *start,
                                    *end,
                                    "roff ignore-block marker in a collected scope contains an unterminated quote",
                                ),
                                truncated,
                            );
                            vec![b'.']
                        }
                        Err(ArgumentIssue::Limit) => {
                            *truncated = true;
                            push_diagnostic(
                                diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ARGUMENT_LIMIT,
                                    Severity::Warning,
                                    source_id,
                                    *start,
                                    *end,
                                    "roff ignore-block marker in a collected scope exceeds configured parser limits",
                                ),
                                truncated,
                            );
                            vec![b'.']
                        }
                    };
                    let next = lines[next + 1..]
                        .iter()
                        .position(|candidate| is_scope_ignore_terminator(candidate, &marker))
                        .map_or(lines.len(), |offset| next + offset + 2);
                    frames.push(ScopeExecutionFrame::Lines {
                        lines,
                        next,
                        previous_conditional: None,
                    });
                    continue;
                }
                if let ScopeLine::Conditional {
                    start,
                    end,
                    predicate,
                    else_eligible,
                    lines: conditional_lines,
                } = line
                {
                    if frames.len() >= limits.max_tree_depth {
                        *truncated = true;
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::LIMIT_SCOPE_DEPTH,
                                Severity::Warning,
                                source_id,
                                *start,
                                *end,
                                "nested roff scope execution exceeds max_tree_depth",
                            ),
                            truncated,
                        );
                        frames.push(ScopeExecutionFrame::Lines {
                            lines,
                            next: next + 1,
                            previous_conditional: None,
                        });
                        continue;
                    }
                    let Some(expanded_predicate) = expand_environment(
                        environment,
                        predicate,
                        scanner.escape_character(),
                        &[],
                        limits,
                        source_id,
                        *start,
                        *end,
                        expansion_steps,
                        diagnostics,
                        truncated,
                    ) else {
                        return ScopeFlow::Halt;
                    };
                    let Some(condition) = evaluate_condition(environment, &expanded_predicate)
                    else {
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_CONDITION,
                                Severity::Warning,
                                source_id,
                                *start,
                                *end,
                                "nested roff conditional predicate is outside the M3 numeric/nroff subset",
                            ),
                            truncated,
                        );
                        frames.push(ScopeExecutionFrame::Lines {
                            lines,
                            next: next + 1,
                            previous_conditional: None,
                        });
                        continue;
                    };
                    frames.push(ScopeExecutionFrame::Lines {
                        lines,
                        next: next + 1,
                        previous_conditional: else_eligible.then_some(condition),
                    });
                    if condition {
                        frames.push(ScopeExecutionFrame::Lines {
                            lines: conditional_lines,
                            next: 0,
                            previous_conditional: None,
                        });
                    }
                    continue;
                }
                if let ScopeLine::Else {
                    lines: else_lines, ..
                } = line
                {
                    frames.push(ScopeExecutionFrame::Lines {
                        lines,
                        next: next + 1,
                        previous_conditional: None,
                    });
                    if previous_conditional == Some(false) {
                        frames.push(ScopeExecutionFrame::Lines {
                            lines: else_lines,
                            next: 0,
                            previous_conditional: None,
                        });
                    }
                    continue;
                }
                frames.push(ScopeExecutionFrame::Lines {
                    lines,
                    next: next + 1,
                    previous_conditional: None,
                });
                if let ScopeLine::Loop {
                    start,
                    end,
                    predicate,
                    lines: loop_lines,
                } = line
                {
                    if environment.mark_nested_while_recovery(*start) {
                        // mandoc's roff input buffer retains the nested
                        // request and its first replayed body line together.
                        // Preserve that observable logical column while the
                        // physical span remains sliceable at the request.
                        let logical_start =
                            loop_lines.first().map(scope_line_end).and_then(|body_end| {
                                let body_end =
                                    SourceSpan::new(source_id, body_end, body_end).ok()?;
                                let position = builder.source_position(&body_end)?;
                                Some(SourcePosition {
                                    line: position.line,
                                    column: position.column.saturating_add(
                                        end.saturating_sub(*start).saturating_sub(1),
                                    ),
                                })
                            });
                        let nested_span = SourceSpan::new(source_id, *start, *end)
                            .expect("collected scope spans are ordered")
                            .with_logical_start(logical_start.unwrap_or_else(|| {
                                builder
                                    .source_position(
                                        &SourceSpan::new(source_id, *start, *start)
                                            .expect("collected scope starts are ordered"),
                                    )
                                    .unwrap_or(SourcePosition { line: 1, column: 1 })
                            }));
                        push_diagnostic(
                            diagnostics,
                            limits,
                            Diagnostic::new(
                                DiagnosticCode::new(DiagnosticCode::ROFF_WHILE_NESTED)
                                    .expect("static diagnostic code is valid"),
                                Severity::Unsupported,
                                "nested .while loops",
                            )
                            .with_primary(nested_span),
                            truncated,
                        );
                        if let Some(outer_closer) = lines.get(next + 1).map(scope_line_end) {
                            let closer_start = outer_closer.saturating_add(4);
                            push_diagnostic(
                                diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ROFF_WHILE_CANNOT_CONTINUE,
                                    Severity::Unsupported,
                                    source_id,
                                    closer_start,
                                    closer_start,
                                    "cannot continue this .while loop",
                                ),
                                truncated,
                            );
                        }
                    }
                    // Mandoc's recovery puts the inner loop into the active
                    // input frame.  Once that loop is exhausted, it abandons
                    // the enclosing `.while` rather than replaying its
                    // remaining sibling lines (notably the outer register
                    // decrement).  Model that explicitly rather than
                    // flattening one body line into the parent frame.
                    frames.clear();
                    frames.push(ScopeExecutionFrame::Loop {
                        start: *start,
                        end: *end,
                        predicate,
                        lines: loop_lines,
                        iterations: 0,
                        break_after: true,
                    });
                    continue;
                }
                match execute_scope_line(
                    line,
                    builder,
                    root,
                    source_id,
                    scanner,
                    environment,
                    limits,
                    text_bytes,
                    expansion_steps,
                    maximum_depth,
                    total_loop_iterations,
                    diagnostics,
                    truncated,
                ) {
                    ScopeFlow::Continue => {}
                    ScopeFlow::Halt => return ScopeFlow::Halt,
                    ScopeFlow::Break => {
                        let mut consumed = false;
                        while let Some(frame) = frames.pop() {
                            if matches!(frame, ScopeExecutionFrame::Loop { .. }) {
                                consumed = true;
                                break;
                            }
                        }
                        if !consumed {
                            return ScopeFlow::Break;
                        }
                    }
                    ScopeFlow::CloseLoopInInnerScope { invocation_start } => {
                        // A macro reparses in a nested input frame in mandoc.
                        // Its `\\}` closes the active outer loop but the caller's
                        // remaining physical scope lines still run.  Drop only
                        // the loop frame, retain those continuations, and
                        // propagate the later out-of-scope recovery to the
                        // scanner boundary.
                        let diagnostic_start = invocation_start.saturating_add(4);
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_WHILE_INNER_SCOPE,
                                Severity::Unsupported,
                                source_id,
                                diagnostic_start,
                                diagnostic_start,
                                "end of .while loop in inner scope",
                            ),
                            truncated,
                        );
                        let mut continuations = Vec::new();
                        let mut consumed = false;
                        while let Some(frame) = frames.pop() {
                            if matches!(frame, ScopeExecutionFrame::Loop { .. }) {
                                consumed = true;
                                break;
                            }
                            continuations.push(frame);
                        }
                        for frame in continuations.into_iter().rev() {
                            frames.push(frame);
                        }
                        // The outermost `.while` is driven by the caller of
                        // this function rather than a `Loop` frame.  In that
                        // case there is nothing local to remove, but the
                        // continuation lines must still execute before the
                        // recovery is returned to that caller.
                        if !consumed {
                            closed_loop_from_inner_scope = Some(invocation_start);
                            continue;
                        }
                        closed_loop_from_inner_scope = Some(invocation_start);
                    }
                    ScopeFlow::LoopContinue => {
                        let mut loop_frame = None;
                        while let Some(frame) = frames.pop() {
                            if matches!(frame, ScopeExecutionFrame::Loop { .. }) {
                                loop_frame = Some(frame);
                                break;
                            }
                        }
                        let Some(loop_frame) = loop_frame else {
                            return ScopeFlow::LoopContinue;
                        };
                        frames.push(loop_frame);
                    }
                }
            }
            ScopeExecutionFrame::Loop {
                start,
                end,
                predicate,
                lines,
                iterations,
                break_after,
            } => {
                let Some(expanded_predicate) = expand_environment(
                    environment,
                    predicate,
                    scanner.escape_character(),
                    &[],
                    limits,
                    source_id,
                    start,
                    end,
                    expansion_steps,
                    diagnostics,
                    truncated,
                ) else {
                    return ScopeFlow::Halt;
                };
                let Some(condition) = evaluate_condition(environment, &expanded_predicate) else {
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_CONDITION,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "nested roff while predicate is outside the M3 numeric/nroff subset",
                        ),
                        truncated,
                    );
                    continue;
                };
                if !condition {
                    if break_after {
                        return ScopeFlow::Break;
                    }
                    continue;
                }
                if iterations >= limits.max_loop_iterations {
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::LIMIT_LOOP_ITERATIONS,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "nested roff while request exceeds max_loop_iterations",
                        ),
                        truncated,
                    );
                    continue;
                }
                if *total_loop_iterations >= limits.max_total_loop_iterations {
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::LIMIT_TOTAL_LOOP_ITERATIONS,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "nested roff while requests exceed max_total_loop_iterations",
                        ),
                        truncated,
                    );
                    continue;
                }
                if !record_expansion_steps(
                    expansion_steps,
                    1,
                    limits,
                    source_id,
                    start,
                    end,
                    diagnostics,
                    truncated,
                ) {
                    return ScopeFlow::Halt;
                }
                *total_loop_iterations += 1;
                frames.push(ScopeExecutionFrame::Loop {
                    start,
                    end,
                    predicate,
                    lines,
                    iterations: iterations + 1,
                    break_after,
                });
                if break_after {
                    let control_column = lines
                        .first()
                        .and_then(|line| match line {
                            ScopeLine::Control {
                                start,
                                argument_start,
                                name,
                                ..
                            } => argument_start.saturating_sub(*start).checked_sub(
                                u32::try_from(name.len())
                                    .expect("scope request names fit public source columns"),
                            ),
                            _ => None,
                        })
                        .unwrap_or(1);
                    let replay_offset = if iterations == 0 {
                        lines.first().map(scope_line_start)
                    } else {
                        lines
                            .last()
                            .map(scope_line_end)
                            .map(|end| end.saturating_add(1))
                    };
                    if let Some(replay_offset) = replay_offset
                        && let Some(replay_position) = builder.source_position(
                            &SourceSpan::new(source_id, replay_offset, replay_offset)
                                .expect("collected scope positions are ordered"),
                        )
                    {
                        frames.push(ScopeExecutionFrame::SetNewRootChildrenLogicalStart {
                            first_child: builder.children(root).map_or(0, <[NodeId]>::len),
                            position: SourcePosition {
                                line: replay_position.line,
                                column: end
                                    .saturating_sub(start)
                                    .saturating_add(control_column)
                                    .saturating_sub(1),
                            },
                        });
                    }
                }
                frames.push(ScopeExecutionFrame::Lines {
                    lines,
                    next: 0,
                    previous_conditional: None,
                });
            }
        }
    }
    closed_loop_from_inner_scope.map_or(ScopeFlow::Continue, |invocation_start| {
        ScopeFlow::CloseLoopInInnerScope { invocation_start }
    })
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)] // Macro copy-mode reparsing stays iterative at the scope boundary.
fn execute_scope_macro_lines(
    lines: Vec<Vec<u8>>,
    arguments: &[Vec<u8>],
    scope_depth: usize,
    builder: &mut DocumentBuilder,
    root: NodeId,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    scanner: &mut Scanner<'_>,
    environment: &mut Environment,
    limits: &Limits,
    text_bytes: &mut usize,
    expansion_steps: &mut usize,
    maximum_depth: &mut usize,
    total_loop_iterations: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> ScopeFlow {
    if !record_expansion_steps(
        expansion_steps,
        1,
        limits,
        source_id,
        start,
        end,
        diagnostics,
        truncated,
    ) {
        return ScopeFlow::Halt;
    }
    let mut pending = lines
        .into_iter()
        .rev()
        .map(|line| (line, arguments.to_vec(), 1_usize, 0_u32, None, false))
        .collect::<Vec<_>>();
    let mut macro_conditionals = Vec::<(usize, bool)>::new();
    while let Some((
        source_line,
        macro_arguments,
        macro_depth,
        macro_origin,
        text_origin,
        _scope_reparse,
    )) = pending.pop()
    {
        let line = copy_mode_reparse(&source_line, scanner.escape_character());
        if let Some((request, raw_arguments)) = split_macro_control(
            &line,
            scanner.control_character(),
            scanner.escape_character(),
        ) {
            if is_macro_comment_request(request, scanner.escape_character()) {
                continue;
            }
            if is_scope_closer(request, scanner.escape_character()) {
                return ScopeFlow::CloseLoopInInnerScope {
                    invocation_start: start,
                };
            }
            if request == b"continue" {
                return ScopeFlow::LoopContinue;
            }
            if matches!(request, b"cc" | b"c2" | b"ec") {
                scanner.apply_character_request(request, raw_arguments);
                continue;
            }
            if request == b"return" {
                break;
            }
            // `.nop` suppresses only its own request spelling.  The
            // remainder is re-read as ordinary input, rather than becoming
            // an observable unknown roff element.  Requeue it so copied
            // macro arguments and escapes follow the normal text path.
            if request == b"nop" {
                pending.push((
                    raw_arguments.to_vec(),
                    macro_arguments,
                    macro_depth,
                    macro_origin,
                    text_origin,
                    false,
                ));
                continue;
            }
            if request == b"tr" {
                environment.define_translation(raw_arguments, scanner.escape_character());
                continue;
            }
            if request == b"while"
                && let Ok(while_arguments) =
                    lex_arguments(raw_arguments, scanner.escape_character(), limits)
                && let Some((predicate_template, body)) = while_arguments.split_first()
                && is_scope_opener(&join_arguments(body), scanner.escape_character())
            {
                if scope_depth >= limits.max_tree_depth {
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::LIMIT_SCOPE_DEPTH,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "nested roff while scope in a scope macro exceeds max_tree_depth",
                        ),
                        truncated,
                    );
                    continue;
                }
                let Some(scope) = collect_pending_macro_scope(
                    &mut pending,
                    macro_depth,
                    scanner.control_character(),
                    scanner.escape_character(),
                    limits,
                ) else {
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_UNTERMINATED_SCOPE,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "roff while in a scope macro reached its caller before its `\\}` terminator",
                        ),
                        truncated,
                    );
                    continue;
                };
                let scope_lines = scope
                    .into_iter()
                    .map(|(line, _, _, _, _, _)| line)
                    .collect::<Vec<_>>();
                let mut iterations = 0_usize;
                loop {
                    let Some(predicate) = expand_environment(
                        environment,
                        &predicate_template.bytes,
                        scanner.escape_character(),
                        &macro_arguments,
                        limits,
                        source_id,
                        start,
                        end,
                        expansion_steps,
                        diagnostics,
                        truncated,
                    ) else {
                        return ScopeFlow::Halt;
                    };
                    let Some(condition) = evaluate_condition(environment, &predicate) else {
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_CONDITION,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff while predicate in a scope macro is outside the M3 numeric/nroff subset",
                            ),
                            truncated,
                        );
                        break;
                    };
                    if !condition {
                        break;
                    }
                    if iterations >= limits.max_loop_iterations
                        || *total_loop_iterations >= limits.max_total_loop_iterations
                    {
                        *truncated = true;
                        let (code, message) = if iterations >= limits.max_loop_iterations {
                            (
                                DiagnosticCode::LIMIT_LOOP_ITERATIONS,
                                "roff while request in a scope macro exceeds max_loop_iterations",
                            )
                        } else {
                            (
                                DiagnosticCode::LIMIT_TOTAL_LOOP_ITERATIONS,
                                "roff while requests in scope macros exceed max_total_loop_iterations",
                            )
                        };
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(code, Severity::Warning, source_id, start, end, message),
                            truncated,
                        );
                        break;
                    }
                    iterations += 1;
                    *total_loop_iterations += 1;
                    match execute_scope_macro_lines(
                        scope_lines.clone(),
                        &macro_arguments,
                        scope_depth + 1,
                        builder,
                        root,
                        source_id,
                        start,
                        end,
                        scanner,
                        environment,
                        limits,
                        text_bytes,
                        expansion_steps,
                        maximum_depth,
                        total_loop_iterations,
                        diagnostics,
                        truncated,
                    ) {
                        ScopeFlow::Continue | ScopeFlow::LoopContinue => {}
                        ScopeFlow::Break => break,
                        flow @ ScopeFlow::CloseLoopInInnerScope { .. } => return flow,
                        ScopeFlow::Halt => return ScopeFlow::Halt,
                    }
                }
                continue;
            }
            if matches!(request, b"if" | b"ie" | b"el") {
                let Ok(condition_arguments) =
                    lex_condition_arguments(raw_arguments, scanner.escape_character(), limits)
                else {
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ARGUMENT_LIMIT,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "roff conditional arguments in a scope macro exceed configured parser limits",
                        ),
                        truncated,
                    );
                    continue;
                };
                let (condition, body_start) = match request {
                    b"el" => {
                        let condition = macro_conditionals
                            .iter()
                            .rposition(|(depth, _)| *depth == macro_depth)
                            .map(|index| !macro_conditionals.remove(index).1);
                        (condition, 0)
                    }
                    b"if" | b"ie" => {
                        if request == b"ie"
                            && (condition_arguments.is_empty()
                                || condition_arguments
                                    .first()
                                    .is_some_and(|argument| argument.bytes == b"!"))
                        {
                            macro_conditionals.retain(|(depth, _)| *depth != macro_depth);
                            macro_conditionals.push((macro_depth, false));
                            (Some(false), condition_arguments.len())
                        } else {
                            let Some((predicate, body_start)) =
                                condition_parts(&condition_arguments)
                            else {
                                push_diagnostic(
                                    diagnostics,
                                    limits,
                                    diagnostic(
                                        DiagnosticCode::ROFF_CONDITION,
                                        Severity::Warning,
                                        source_id,
                                        start,
                                        end,
                                        "roff conditional in a scope macro is missing its predicate",
                                    ),
                                    truncated,
                                );
                                continue;
                            };
                            let Some(predicate) = expand_environment(
                                environment,
                                &predicate,
                                scanner.escape_character(),
                                &macro_arguments,
                                limits,
                                source_id,
                                start,
                                end,
                                expansion_steps,
                                diagnostics,
                                truncated,
                            ) else {
                                return ScopeFlow::Halt;
                            };
                            let condition = evaluate_condition(environment, &predicate);
                            if request == b"ie"
                                && let Some(condition) = condition
                            {
                                macro_conditionals.retain(|(depth, _)| *depth != macro_depth);
                                macro_conditionals.push((macro_depth, condition));
                            }
                            (condition, body_start)
                        }
                    }
                    _ => unreachable!("conditional request was filtered above"),
                };
                let Some(condition) = condition else {
                    if request == b"el" {
                        continue;
                    }
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_CONDITION,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "roff conditional in a scope macro is outside the M3 numeric/nroff subset",
                        ),
                        truncated,
                    );
                    continue;
                };
                let body_template =
                    condition_body_template(raw_arguments, &condition_arguments, body_start);
                let escape = scanner.escape_character();
                if is_scope_opener(&body_template, escape) {
                    let Some(scope) = collect_pending_macro_scope(
                        &mut pending,
                        macro_depth,
                        scanner.control_character(),
                        escape,
                        limits,
                    ) else {
                        *truncated = true;
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_UNTERMINATED_SCOPE,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff scope macro conditional reached its caller before its `\\}` terminator",
                            ),
                            truncated,
                        );
                        continue;
                    };
                    if condition {
                        pending.extend(scope.into_iter().rev());
                    }
                    continue;
                }
                if condition && !body_template.is_empty() {
                    pending.push((
                        body_template,
                        macro_arguments,
                        macro_depth,
                        macro_origin,
                        text_origin,
                        false,
                    ));
                }
                continue;
            }
            if matches!(request, b"de" | b"de1" | b"am" | b"dei" | b"ami") {
                let Ok(definition_arguments) =
                    lex_arguments(raw_arguments, scanner.escape_character(), limits)
                else {
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ARGUMENT_LIMIT,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "generated roff macro definition in a scope macro exceeds configured parser limits",
                        ),
                        truncated,
                    );
                    continue;
                };
                let Some(definition_name) = definition_arguments.first() else {
                    continue;
                };
                let indirect = matches!(request, b"dei" | b"ami");
                let Some(definition_name) = (!indirect)
                    .then(|| definition_name.bytes.clone())
                    .or_else(|| environment.indirect_string(&definition_name.bytes))
                else {
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_UNDEFINED_REFERENCE,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "generated indirect roff macro definition in a scope names an undefined string",
                        ),
                        truncated,
                    );
                    continue;
                };
                let terminator = match definition_arguments.get(1) {
                    None => vec![b'.'],
                    Some(argument) if !indirect => argument.bytes.clone(),
                    Some(argument) => {
                        let Some(terminator) = environment.indirect_string(&argument.bytes) else {
                            push_diagnostic(
                                diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ROFF_UNDEFINED_REFERENCE,
                                    Severity::Warning,
                                    source_id,
                                    start,
                                    end,
                                    "generated indirect roff macro terminator in a scope names an undefined string",
                                ),
                                truncated,
                            );
                            continue;
                        };
                        terminator
                    }
                };
                let definition_control = scanner.control_character();
                let mut body = Vec::new();
                let mut terminated = false;
                // A definition opened from a macro body first consumes that
                // caller's remaining copy-mode lines.  The original outer
                // definition may have stopped at the first `..`, while this
                // nested definition continues into physical input after the
                // macro invocation (`de/startde`).  Only then resume the
                // scanner for the remainder of the new definition.
                if matches!(request, b"de" | b"de1") {
                    while pending
                        .last()
                        .is_some_and(|(_, _, depth, _, _, _)| *depth == macro_depth)
                    {
                        let (body_line, _, _, _, _, _) =
                            pending.pop().expect("checked macro depth");
                        if is_definition_terminator(&body_line, definition_control, &terminator) {
                            terminated = true;
                            break;
                        }
                        body.push(body_line);
                    }
                }
                while !terminated && let Some(body_line) = scanner.next_raw_line() {
                    if is_definition_terminator(body_line.bytes, definition_control, &terminator) {
                        terminated = true;
                        break;
                    }
                    if body_line.too_long {
                        *truncated = true;
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::LIMIT_LINE_BYTES,
                                Severity::Warning,
                                source_id,
                                body_line.start,
                                body_line.end,
                                "copy-mode generated macro line in a scope exceeds max_line_bytes and was skipped",
                            ),
                            truncated,
                        );
                        continue;
                    }
                    body.push(body_line.bytes.to_vec());
                }
                if !terminated {
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_UNTERMINATED_DEFINITION,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "generated roff macro definition in a scope reached source end before its terminator",
                        ),
                        truncated,
                    );
                }
                let definition = if matches!(request, b"dei" | b"ami") {
                    environment.define_indirect_macro(
                        &definition_name,
                        body,
                        matches!(request, b"am" | b"ami"),
                        limits,
                    )
                } else {
                    environment.define_macro(
                        &definition_name,
                        body,
                        matches!(request, b"am" | b"ami"),
                        limits,
                    )
                };
                if let Err(error) = definition {
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        environment_error_diagnostic(error, source_id, start, end),
                        truncated,
                    );
                }
                continue;
            }
            if request == b"ig" {
                let marker = match ignore_marker(raw_arguments, scanner.escape_character(), limits)
                {
                    Ok(marker) => marker,
                    Err(ArgumentIssue::UnterminatedQuote) => {
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ARGUMENT_UNTERMINATED_QUOTE,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff ignore-block marker in a scope macro contains an unterminated quote",
                            ),
                            truncated,
                        );
                        vec![b'.']
                    }
                    Err(ArgumentIssue::Limit) => {
                        *truncated = true;
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ARGUMENT_LIMIT,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff ignore-block marker in a scope macro exceeds configured parser limits",
                            ),
                            truncated,
                        );
                        vec![b'.']
                    }
                };
                consume_ignore_block(scanner, &marker);
                continue;
            }
            if is_environment_request(request) {
                if matches!(request, b"ds" | b"as") {
                    if let Err(error) = apply_string_request(
                        environment,
                        raw_arguments,
                        scanner.escape_character(),
                        request == b"as",
                        limits,
                        source_id,
                        start,
                        end,
                        expansion_steps,
                        diagnostics,
                        truncated,
                    ) {
                        *truncated = true;
                        push_diagnostic(
                            diagnostics,
                            limits,
                            environment_error_diagnostic(error, source_id, start, end),
                            truncated,
                        );
                    }
                    continue;
                }
                let Ok(arguments) =
                    lex_arguments(raw_arguments, scanner.escape_character(), limits)
                else {
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ARGUMENT_LIMIT,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "macro body arguments in a loop scope exceed configured parser limits",
                        ),
                        truncated,
                    );
                    continue;
                };
                if let Err(error) = apply_environment_request(
                    environment,
                    builder,
                    request,
                    scanner.escape_character(),
                    &arguments,
                    limits,
                ) {
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        environment_error_diagnostic(error, source_id, start, end),
                        truncated,
                    );
                }
                continue;
            }
            let Some(element) = append_node(
                builder,
                root,
                NodeKind::Element,
                source_id,
                start,
                end,
                NodeFlags {
                    line_start: true,
                    ..NodeFlags::default()
                },
                limits,
                diagnostics,
                truncated,
            ) else {
                continue;
            };
            if !builder.macro_name(element, visible_bytes(request)) {
                *truncated = true;
                continue;
            }
            *maximum_depth = (*maximum_depth).max(2);
            if raw_arguments.is_empty() {
                continue;
            }
            let Some(bytes) = expand_environment(
                environment,
                raw_arguments,
                scanner.escape_character(),
                &macro_arguments,
                limits,
                source_id,
                start,
                end,
                expansion_steps,
                diagnostics,
                truncated,
            ) else {
                return ScopeFlow::Halt;
            };
            let escape = scanner.escape_character();
            let Some(bytes) = translate_visible(
                environment,
                &bytes,
                escape,
                limits,
                source_id,
                start,
                end,
                diagnostics,
                truncated,
            ) else {
                return ScopeFlow::Halt;
            };
            let result = normalize_document_escapes(builder, &bytes, escape, limits);
            if !record_expansion_steps(
                expansion_steps,
                result.steps,
                limits,
                source_id,
                start,
                end,
                diagnostics,
                truncated,
            ) {
                return ScopeFlow::Halt;
            }
            emit_escape_issues(
                &result.issues,
                start,
                end,
                source_id,
                limits,
                diagnostics,
                truncated,
            );
            *truncated |= result.truncated;
            if append_text_node(
                builder,
                element,
                source_id,
                start,
                end,
                NodeFlags {
                    line_continuation: result.line_continuation,
                    ..NodeFlags::default()
                },
                result.text,
                limits,
                text_bytes,
                diagnostics,
                truncated,
            ) {
                *maximum_depth = (*maximum_depth).max(3);
            }
            continue;
        }
        let Some(bytes) = expand_environment(
            environment,
            &line,
            scanner.escape_character(),
            &macro_arguments,
            limits,
            source_id,
            start,
            end,
            expansion_steps,
            diagnostics,
            truncated,
        ) else {
            return ScopeFlow::Halt;
        };
        let escape = scanner.escape_character();
        let Some(bytes) = translate_visible(
            environment,
            &bytes,
            escape,
            limits,
            source_id,
            start,
            end,
            diagnostics,
            truncated,
        ) else {
            return ScopeFlow::Halt;
        };
        let result = normalize_document_escapes(builder, &bytes, escape, limits);
        if !record_expansion_steps(
            expansion_steps,
            result.steps,
            limits,
            source_id,
            start,
            end,
            diagnostics,
            truncated,
        ) {
            return ScopeFlow::Halt;
        }
        emit_escape_issues(
            &result.issues,
            start,
            end,
            source_id,
            limits,
            diagnostics,
            truncated,
        );
        *truncated |= result.truncated;
        if append_text_node(
            builder,
            root,
            source_id,
            start,
            end,
            NodeFlags {
                line_start: true,
                line_continuation: result.line_continuation,
                ..NodeFlags::default()
            },
            result.text,
            limits,
            text_bytes,
            diagnostics,
            truncated,
        ) {
            *maximum_depth = (*maximum_depth).max(2);
        }
    }
    ScopeFlow::Continue
}

/// Turn an inline conditional body back into one dispatchable scope line.
fn inline_scope_body_line(
    bytes: Vec<u8>,
    start: u32,
    end: u32,
    control: u8,
    escape: u8,
) -> ScopeLine {
    match split_macro_control(&bytes, control, escape) {
        Some((name, arguments)) => ScopeLine::Control {
            start,
            end,
            argument_start: start
                .saturating_add(1)
                .saturating_add(
                    u32::try_from(name.len()).expect("inline scope request names fit source spans"),
                )
                .saturating_add(u32::from(!arguments.is_empty())),
            name: name.to_vec(),
            arguments: arguments.to_vec(),
        },
        None => ScopeLine::Text {
            start,
            end,
            bytes,
            terminal_inline: false,
        },
    }
}

/// Define a macro whose copy-mode body was already retained by a surrounding
/// brace scope.
///
/// A physical `.de` normally advances the scanner through its body.  When the
/// request sits inside a collected scope, those physical lines are instead
/// represented by `following`; consume precisely that local range so neither a
/// later sibling nor the caller's scanner position is stolen.  The returned
/// count excludes the definition request and includes its terminator.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)] // Direct and indirect definition recovery mirrors the physical path.
fn execute_collected_scope_definition(
    line: &ScopeLine,
    following: &[ScopeLine],
    scanner: &Scanner<'_>,
    environment: &mut Environment,
    limits: &Limits,
    source_id: crate::SourceId,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> Option<usize> {
    let ScopeLine::Control {
        start,
        end,
        name,
        arguments,
        ..
    } = line
    else {
        return None;
    };
    if !matches!(name.as_slice(), b"de" | b"de1" | b"am" | b"dei" | b"ami") {
        return None;
    }
    let definition_arguments = lex_arguments(arguments, scanner.escape_character(), limits).ok()?;
    let definition_name = definition_arguments.first()?;
    let indirect = matches!(name.as_slice(), b"dei" | b"ami");
    let definition_name = (!indirect)
        .then(|| definition_name.bytes.clone())
        .or_else(|| environment.indirect_string(&definition_name.bytes));
    let Some(definition_name) = definition_name else {
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::ROFF_UNDEFINED_REFERENCE,
                Severity::Warning,
                source_id,
                *start,
                *end,
                "indirect roff macro definition in a collected scope names an undefined string",
            ),
            truncated,
        );
        return Some(0);
    };
    let terminator = match definition_arguments.get(1) {
        None => vec![b'.'],
        Some(argument) if !indirect => argument.bytes.clone(),
        Some(argument) => {
            let Some(terminator) = environment.indirect_string(&argument.bytes) else {
                push_diagnostic(
                    diagnostics,
                    limits,
                    diagnostic(
                        DiagnosticCode::ROFF_UNDEFINED_REFERENCE,
                        Severity::Warning,
                        source_id,
                        *start,
                        *end,
                        "indirect roff macro terminator in a collected scope names an undefined string",
                    ),
                    truncated,
                );
                return Some(0);
            };
            terminator
        }
    };
    let control = scanner.control_character();
    let escape = scanner.escape_character();
    let mut body = Vec::new();
    let mut consumed = 0_usize;
    let mut terminated = false;
    for candidate in following {
        let copy_mode_lines = scope_line_copy_mode_lines(candidate, control, escape);
        if copy_mode_lines
            .first()
            .is_some_and(|bytes| is_definition_terminator(bytes, control, &terminator))
        {
            consumed += 1;
            terminated = true;
            break;
        }
        consumed += 1;
        body.extend(copy_mode_lines);
    }
    if !terminated {
        *truncated = true;
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::ROFF_UNTERMINATED_DEFINITION,
                Severity::Warning,
                source_id,
                *start,
                *end,
                "roff macro definition in a collected scope reached its scope end before its terminator",
            ),
            truncated,
        );
    }
    let definition = if matches!(name.as_slice(), b"dei" | b"ami") {
        environment.define_indirect_macro(
            &definition_name,
            body,
            matches!(name.as_slice(), b"am" | b"ami"),
            limits,
        )
    } else {
        environment.define_macro(
            &definition_name,
            body,
            matches!(name.as_slice(), b"am" | b"ami"),
            limits,
        )
    };
    if let Err(error) = definition {
        *truncated = true;
        push_diagnostic(
            diagnostics,
            limits,
            environment_error_diagnostic(error, source_id, *start, *end),
            truncated,
        );
    }
    Some(consumed)
}

/// Reconstruct one retained scope line as copy-mode macro bytes.
///
/// Nested scope frames were structurally recognized before a surrounding macro
/// definition could claim them.  Re-emitting their request spelling keeps the
/// definition's later iterative macro execution independent from the collector
/// and preserves the same control/escape characters that delimit it.
fn scope_line_copy_mode_lines(line: &ScopeLine, control: u8, escape: u8) -> Vec<Vec<u8>> {
    match line {
        ScopeLine::Text { bytes, .. } => vec![bytes.clone()],
        ScopeLine::Control {
            name, arguments, ..
        } => {
            let mut bytes = Vec::with_capacity(
                1 + name.len() + usize::from(!arguments.is_empty()) + arguments.len(),
            );
            bytes.push(control);
            bytes.extend_from_slice(name);
            if !arguments.is_empty() {
                bytes.push(b' ');
                bytes.extend_from_slice(arguments);
            }
            vec![bytes]
        }
        ScopeLine::Loop {
            predicate, lines, ..
        } => scope_line_copy_mode_scope(b"while", predicate, lines, control, escape),
        ScopeLine::Conditional {
            predicate,
            else_eligible,
            lines,
            ..
        } => scope_line_copy_mode_scope(
            if *else_eligible { b"ie" } else { b"if" },
            predicate,
            lines,
            control,
            escape,
        ),
        ScopeLine::Else { lines, .. } => {
            scope_line_copy_mode_scope(b"el", &[], lines, control, escape)
        }
    }
}

fn scope_line_copy_mode_scope(
    request: &[u8],
    predicate: &[u8],
    lines: &[ScopeLine],
    control: u8,
    escape: u8,
) -> Vec<Vec<u8>> {
    let mut opener = Vec::with_capacity(
        1 + request.len() + predicate.len() + usize::from(!predicate.is_empty()) + 4,
    );
    opener.push(control);
    opener.extend_from_slice(request);
    if !predicate.is_empty() {
        opener.push(b' ');
        opener.extend_from_slice(predicate);
    }
    opener.extend_from_slice(&[b' ', escape, b'{', escape]);
    let mut copy_mode = vec![opener];
    for line in lines {
        copy_mode.extend(scope_line_copy_mode_lines(line, control, escape));
    }
    copy_mode.push(vec![control, escape, b'}']);
    copy_mode
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)] // Iterative scope dispatch keeps untrusted roff control flow non-recursive.
fn execute_scope_line(
    line: &ScopeLine,
    builder: &mut DocumentBuilder,
    root: NodeId,
    source_id: crate::SourceId,
    scanner: &mut Scanner<'_>,
    environment: &mut Environment,
    limits: &Limits,
    text_bytes: &mut usize,
    expansion_steps: &mut usize,
    maximum_depth: &mut usize,
    total_loop_iterations: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> ScopeFlow {
    let (start, end) = match line {
        ScopeLine::Text { start, end, .. }
        | ScopeLine::Control { start, end, .. }
        | ScopeLine::Loop { start, end, .. }
        | ScopeLine::Conditional { start, end, .. }
        | ScopeLine::Else { start, end, .. } => (*start, *end),
    };
    match line {
        ScopeLine::Text {
            bytes,
            terminal_inline,
            ..
        } => {
            let Some(bytes) = expand_environment(
                environment,
                bytes,
                scanner.escape_character(),
                &[],
                limits,
                source_id,
                start,
                end,
                expansion_steps,
                diagnostics,
                truncated,
            ) else {
                return ScopeFlow::Halt;
            };
            let escape = scanner.escape_character();
            let Some(bytes) = translate_visible(
                environment,
                &bytes,
                escape,
                limits,
                source_id,
                start,
                end,
                diagnostics,
                truncated,
            ) else {
                return ScopeFlow::Halt;
            };
            let result = normalize_document_escapes(builder, &bytes, escape, limits);
            if !record_expansion_steps(
                expansion_steps,
                result.steps,
                limits,
                source_id,
                start,
                end,
                diagnostics,
                truncated,
            ) {
                return ScopeFlow::Halt;
            }
            emit_escape_issues(
                &result.issues,
                start,
                end,
                source_id,
                limits,
                diagnostics,
                truncated,
            );
            *truncated |= result.truncated;
            if append_text_node(
                builder,
                root,
                source_id,
                start,
                end,
                NodeFlags {
                    line_start: true,
                    // A bare conditional opener contributes a vertical blank
                    // with the opener's nonempty source span.
                    generated: result.text.is_empty() && start < end,
                    line_continuation: result.line_continuation,
                    ..NodeFlags::default()
                },
                result.text,
                limits,
                text_bytes,
                diagnostics,
                truncated,
            ) {
                if *terminal_inline
                    && let Some(node) = builder
                        .children(root)
                        .and_then(|children| children.last())
                        .copied()
                {
                    let _ = builder.set_node_terminal_inline_conditional(node, true);
                }
                *maximum_depth = (*maximum_depth).max(2);
            }
        }
        ScopeLine::Control {
            argument_start,
            name,
            arguments,
            ..
        } => {
            // Collected scope controls retain their full physical line span,
            // whose first byte is the roff control character. Public macro
            // locations instead begin at the request name, and their
            // arguments begin after that name and its separating blank.
            // Ordinary scanning records these offsets directly; recover the
            // equivalent positions while replaying a stored scope line.
            let control_start = start.saturating_add(1);
            let control_argument_start = *argument_start;
            if matches!(name.as_slice(), b"cc" | b"c2" | b"ec") {
                scanner.apply_character_request(name, arguments);
                return ScopeFlow::Continue;
            }
            if name == b"break" {
                return ScopeFlow::Break;
            }
            if name == b"continue" {
                return ScopeFlow::LoopContinue;
            }
            // `.nop` consumes its request name and lets the remainder flow
            // through the ordinary text parser.  In particular, it must not
            // leave an unknown `nop` element in the public AST.
            if name == b"nop" {
                let text = ScopeLine::Text {
                    start,
                    end,
                    bytes: arguments.clone(),
                    terminal_inline: false,
                };
                return execute_scope_line(
                    &text,
                    builder,
                    root,
                    source_id,
                    scanner,
                    environment,
                    limits,
                    text_bytes,
                    expansion_steps,
                    maximum_depth,
                    total_loop_iterations,
                    diagnostics,
                    truncated,
                );
            }
            if matches!(name.as_slice(), b"ds" | b"as") {
                if let Err(error) = apply_string_request(
                    environment,
                    arguments,
                    scanner.escape_character(),
                    name == b"as",
                    limits,
                    source_id,
                    start,
                    end,
                    expansion_steps,
                    diagnostics,
                    truncated,
                ) {
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        environment_error_diagnostic(error, source_id, start, end),
                        truncated,
                    );
                }
                return ScopeFlow::Continue;
            }
            let raw_arguments = arguments.as_slice();
            let Ok(arguments) = lex_arguments(arguments, scanner.escape_character(), limits) else {
                *truncated = true;
                push_diagnostic(
                    diagnostics,
                    limits,
                    diagnostic(
                        DiagnosticCode::ARGUMENT_LIMIT,
                        Severity::Warning,
                        source_id,
                        start,
                        end,
                        "roff scope control arguments exceed configured parser limits",
                    ),
                    truncated,
                );
                return ScopeFlow::Continue;
            };
            if name == b"tr" {
                environment
                    .define_translation(&join_arguments(&arguments), scanner.escape_character());
                return ScopeFlow::Continue;
            }
            if name == b"if" {
                let Some((predicate_template, body_start)) = condition_parts(&arguments) else {
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_CONDITION,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "roff conditional in a loop scope is missing its predicate",
                        ),
                        truncated,
                    );
                    return ScopeFlow::Continue;
                };
                let Some(predicate) = expand_environment(
                    environment,
                    &predicate_template,
                    scanner.escape_character(),
                    &[],
                    limits,
                    source_id,
                    start,
                    end,
                    expansion_steps,
                    diagnostics,
                    truncated,
                ) else {
                    return ScopeFlow::Halt;
                };
                let Some(condition) = evaluate_condition(environment, &predicate) else {
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_CONDITION,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "roff conditional in a loop scope is outside the M3 numeric/nroff subset",
                        ),
                        truncated,
                    );
                    return ScopeFlow::Continue;
                };
                if !condition {
                    return ScopeFlow::Continue;
                }
                let body_template = condition_body_template(raw_arguments, &arguments, body_start);
                let body_source_start = condition_body_source_start_from_offset(
                    raw_arguments,
                    &arguments,
                    body_start,
                    control_argument_start,
                    start,
                    None,
                );
                let Some(body) = expand_environment(
                    environment,
                    &body_template,
                    scanner.escape_character(),
                    &[],
                    limits,
                    source_id,
                    body_source_start,
                    end,
                    expansion_steps,
                    diagnostics,
                    truncated,
                ) else {
                    return ScopeFlow::Halt;
                };
                if let Some((request, raw_arguments)) = split_macro_control(
                    &body,
                    scanner.control_character(),
                    scanner.escape_character(),
                ) {
                    if matches!(request, b"break" | b"continue") {
                        return if request == b"break" {
                            ScopeFlow::Break
                        } else {
                            ScopeFlow::LoopContinue
                        };
                    }
                    if matches!(request, b"cc" | b"c2" | b"ec") {
                        scanner.apply_character_request(request, raw_arguments);
                        return ScopeFlow::Continue;
                    }
                    if request == b"tr" {
                        let Ok(arguments) =
                            lex_arguments(raw_arguments, scanner.escape_character(), limits)
                        else {
                            *truncated = true;
                            push_diagnostic(
                                diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ARGUMENT_LIMIT,
                                    Severity::Warning,
                                    source_id,
                                    start,
                                    end,
                                    "inline roff conditional translation arguments in a scope exceed configured parser limits",
                                ),
                                truncated,
                            );
                            return ScopeFlow::Continue;
                        };
                        environment.define_translation(
                            &join_arguments(&arguments),
                            scanner.escape_character(),
                        );
                        return ScopeFlow::Continue;
                    }
                    if is_environment_request(request) {
                        if matches!(request, b"ds" | b"as") {
                            if let Err(error) = apply_string_request(
                                environment,
                                raw_arguments,
                                scanner.escape_character(),
                                request == b"as",
                                limits,
                                source_id,
                                start,
                                end,
                                expansion_steps,
                                diagnostics,
                                truncated,
                            ) {
                                *truncated = true;
                                push_diagnostic(
                                    diagnostics,
                                    limits,
                                    environment_error_diagnostic(error, source_id, start, end),
                                    truncated,
                                );
                            }
                            return ScopeFlow::Continue;
                        }
                        let Ok(arguments) =
                            lex_arguments(raw_arguments, scanner.escape_character(), limits)
                        else {
                            *truncated = true;
                            push_diagnostic(
                                diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ARGUMENT_LIMIT,
                                    Severity::Warning,
                                    source_id,
                                    start,
                                    end,
                                    "roff conditional body in a loop scope exceeds argument limits",
                                ),
                                truncated,
                            );
                            return ScopeFlow::Continue;
                        };
                        if let Err(error) = apply_environment_request(
                            environment,
                            builder,
                            request,
                            scanner.escape_character(),
                            &arguments,
                            limits,
                        ) {
                            *truncated = true;
                            push_diagnostic(
                                diagnostics,
                                limits,
                                environment_error_diagnostic(error, source_id, start, end),
                                truncated,
                            );
                        }
                        return ScopeFlow::Continue;
                    }
                    if !is_builtin_package_macro(builder.macro_set(), request)
                        && let Some(definition) = environment.macro_definition(request).cloned()
                    {
                        let Ok(arguments) =
                            lex_arguments(raw_arguments, scanner.escape_character(), limits)
                        else {
                            *truncated = true;
                            push_diagnostic(
                                diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ARGUMENT_LIMIT,
                                    Severity::Warning,
                                    source_id,
                                    start,
                                    end,
                                    "inline roff conditional macro arguments in a scope exceed configured parser limits",
                                ),
                                truncated,
                            );
                            return ScopeFlow::Continue;
                        };
                        let arguments = arguments
                            .into_iter()
                            .map(|argument| argument.bytes)
                            .collect::<Vec<_>>();
                        return execute_scope_macro_lines(
                            definition.lines,
                            &arguments,
                            1,
                            builder,
                            root,
                            source_id,
                            start,
                            end,
                            scanner,
                            environment,
                            limits,
                            text_bytes,
                            expansion_steps,
                            maximum_depth,
                            total_loop_iterations,
                            diagnostics,
                            truncated,
                        );
                    }
                }
                let result =
                    normalize_document_escapes(builder, &body, scanner.escape_character(), limits);
                if !record_expansion_steps(
                    expansion_steps,
                    result.steps,
                    limits,
                    source_id,
                    body_source_start,
                    end,
                    diagnostics,
                    truncated,
                ) {
                    return ScopeFlow::Halt;
                }
                emit_escape_issues(
                    &result.issues,
                    body_source_start,
                    end,
                    source_id,
                    limits,
                    diagnostics,
                    truncated,
                );
                *truncated |= result.truncated;
                if append_text_node(
                    builder,
                    root,
                    source_id,
                    body_source_start,
                    end,
                    NodeFlags {
                        line_start: true,
                        line_continuation: result.line_continuation,
                        ..NodeFlags::default()
                    },
                    result.text,
                    limits,
                    text_bytes,
                    diagnostics,
                    truncated,
                ) {
                    *maximum_depth = (*maximum_depth).max(2);
                }
                return ScopeFlow::Continue;
            }
            if is_environment_request(name) {
                if let Err(error) = apply_environment_request(
                    environment,
                    builder,
                    name,
                    scanner.escape_character(),
                    &arguments,
                    limits,
                ) {
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        environment_error_diagnostic(error, source_id, start, end),
                        truncated,
                    );
                }
                return ScopeFlow::Continue;
            }
            if !is_builtin_package_macro(builder.macro_set(), name)
                && let Some(definition) = environment.macro_definition(name).cloned()
            {
                let arguments = arguments
                    .into_iter()
                    .map(|argument| argument.bytes)
                    .collect::<Vec<_>>();
                return execute_scope_macro_lines(
                    definition.lines,
                    &arguments,
                    1,
                    builder,
                    root,
                    source_id,
                    start,
                    end,
                    scanner,
                    environment,
                    limits,
                    text_bytes,
                    expansion_steps,
                    maximum_depth,
                    total_loop_iterations,
                    diagnostics,
                    truncated,
                );
            }
            let Some(element) = append_node(
                builder,
                root,
                NodeKind::Element,
                source_id,
                // `br` parsed while replaying a conditional scope keeps the
                // physical control-column location in the legacy tree.  It
                // is a roff layout request rather than a visible package
                // macro, unlike `.B` and the other font controls above.
                if name == b"br" { start } else { control_start },
                end,
                NodeFlags {
                    line_start: true,
                    ..NodeFlags::default()
                },
                limits,
                diagnostics,
                truncated,
            ) else {
                return ScopeFlow::Continue;
            };
            if !builder.macro_name(element, visible_bytes(name)) {
                *truncated = true;
                return ScopeFlow::Continue;
            }
            *maximum_depth = (*maximum_depth).max(2);
            for argument in arguments {
                let argument_source_start = control_argument_start.saturating_add(
                    u32::try_from(argument.offset)
                        .expect("scope argument offsets fit source spans"),
                );
                let Some(bytes) = expand_environment(
                    environment,
                    &argument.bytes,
                    scanner.escape_character(),
                    &[],
                    limits,
                    source_id,
                    argument_source_start,
                    end,
                    expansion_steps,
                    diagnostics,
                    truncated,
                ) else {
                    return ScopeFlow::Halt;
                };
                // Package-macro arguments keep the source-visible roff
                // formatter spelling in the public AST.  The normal control
                // scanner expands environments here but deliberately does
                // not apply `.tr`: translation is an execution concern and
                // would erase controls such as the `\&` synthesized around
                // an attached scope closer.  Scope replay must use that same
                // projection.
                let escape = scanner.escape_character();
                let result = normalize_document_escapes(builder, &bytes, escape, limits);
                if !record_expansion_steps(
                    expansion_steps,
                    result.steps,
                    limits,
                    source_id,
                    argument_source_start,
                    end,
                    diagnostics,
                    truncated,
                ) {
                    return ScopeFlow::Halt;
                }
                emit_escape_issues(
                    &result.issues,
                    start,
                    end,
                    source_id,
                    limits,
                    diagnostics,
                    truncated,
                );
                *truncated |= result.truncated;
                if append_text_node(
                    builder,
                    element,
                    source_id,
                    argument_source_start,
                    end,
                    NodeFlags {
                        line_continuation: result.line_continuation,
                        ..NodeFlags::default()
                    },
                    result.text,
                    limits,
                    text_bytes,
                    diagnostics,
                    truncated,
                ) {
                    *maximum_depth = (*maximum_depth).max(3);
                }
            }
        }
        ScopeLine::Loop { .. } | ScopeLine::Conditional { .. } | ScopeLine::Else { .. } => {
            unreachable!("nested scopes are dispatched by the explicit scope execution stack")
        }
    }
    ScopeFlow::Continue
}

fn is_environment_request(name: &[u8]) -> bool {
    // `.ftr`, `.na`, `.pl`, and `.ps` are formatter-side state in libmandoc.
    // Classifying them here consumes the requests without exposing AST nodes;
    // the shared no-op fallback in `apply_environment_request` is their
    // intentional semantic implementation.
    matches!(
        name,
        b"ds" | b"as" | b"nr" | b"rr" | b"rm" | b"rn" | b"als" | b"ftr" | b"na" | b"pl" | b"ps"
    )
}

/// Split a copy-reparsed roff request with scanner-equivalent input-comment tails.
///
/// Macro replay bypasses the physical scanner, so request-local input comments
/// must be removed before a package macro or roff state request observes them.
fn split_macro_control(bytes: &[u8], control: u8, escape: u8) -> Option<(&[u8], &[u8])> {
    let remainder = trim_horizontal_space(bytes.strip_prefix(&[control])?);
    let name_end = remainder
        .iter()
        .position(u8::is_ascii_whitespace)
        .unwrap_or(remainder.len());
    let name = &remainder[..name_end];
    let arguments = trim_horizontal_space(strip_inline_comment(&remainder[name_end..], escape));
    (!name.is_empty()).then_some((name, arguments))
}

/// Recognize comments after a copy-mode macro body has been re-dispatched.
///
/// Physical input reaches `Scanner::next_line`, which already handles both
/// the standard `."` spelling and the active escape-character variant. Macro
/// bodies bypass that scanner path, so they need the equivalent guard before
/// treating a copied comment as a normal roff request.
fn is_macro_comment_request(name: &[u8], escape: u8) -> bool {
    name == b"\"" || name == [escape, b'"']
}

/// Return the one-based source column of a control stored in a copy-mode
/// macro body.  The body is replayed at its caller's physical span, so this
/// is used only as logical provenance on generated public nodes.
fn macro_body_control_column(bytes: &[u8], control: u8) -> u32 {
    let Some(remainder) = bytes.strip_prefix(&[control]) else {
        return 1;
    };
    let leading = remainder
        .iter()
        .take_while(|byte| matches!(**byte, b' ' | b'\t'))
        .count();
    u32::try_from(leading)
        .expect("bounded macro body whitespace fits public source columns")
        .saturating_add(2)
}

/// Whether a macro begins with the unconditional self-call that mandoc treats
/// as an input-stack recursion rather than ordinary nested macro depth.
fn macro_definition_directly_invokes(
    definition: &crate::roff::MacroDefinition,
    name: &[u8],
    control: u8,
) -> bool {
    definition.lines.first().is_some_and(|line| {
        let line = copy_mode_reparse(line, b'\\');
        split_macro_control(&line, control, b'\\')
            .is_some_and(|(request, arguments)| request == name && arguments.is_empty())
    })
}

fn is_macro_terminator(bytes: &[u8], control: u8) -> bool {
    bytes.starts_with(&[control, b'.'])
        && bytes
            .get(2..)
            .is_none_or(|remaining| remaining.is_empty() || remaining[0].is_ascii_whitespace())
}

/// Whether a copy-mode macro definition ends at the selected request name.
///
/// Traditional `..` always remains a valid closer, even when a custom
/// delimiter was supplied. A custom delimiter is also a request name and may
/// carry trailing argument text, as in `.end-marker explanatory words`.
fn is_definition_terminator(bytes: &[u8], control: u8, marker: &[u8]) -> bool {
    if is_macro_terminator(bytes, control) {
        return true;
    }
    if marker == b"." {
        return false;
    }
    let Some(remainder) = bytes.strip_prefix(&[control]) else {
        return false;
    };
    let remainder = trim_horizontal_space(remainder);
    let name_end = remainder
        .iter()
        .position(u8::is_ascii_whitespace)
        .unwrap_or(remainder.len());
    remainder[..name_end] == *marker
}

fn ignore_marker(
    raw_arguments: &[u8],
    escape: u8,
    limits: &Limits,
) -> Result<Vec<u8>, ArgumentIssue> {
    let arguments = lex_arguments(raw_arguments, escape, limits)?;
    Ok(arguments
        .first()
        .map_or_else(|| vec![b'.'], |argument| argument.bytes.clone()))
}

fn consume_ignore_block(scanner: &mut Scanner<'_>, marker: &[u8]) {
    while let Some(ignored) = scanner.next_raw_line() {
        if is_ignore_terminator(ignored.bytes, scanner.control_character(), marker) {
            break;
        }
    }
}

fn is_scope_ignore_terminator(line: &ScopeLine, marker: &[u8]) -> bool {
    let ScopeLine::Control {
        name, arguments, ..
    } = line
    else {
        return false;
    };
    name == marker && arguments.iter().all(u8::is_ascii_whitespace)
}

/// Whether one physical line closes a roff `.ig` block.
///
/// The default marker is the traditional `..`; an explicit marker is a
/// request name following the active control character.  Both forms accept
/// trailing horizontal whitespace but no trailing argument text.
fn is_ignore_terminator(bytes: &[u8], control: u8, marker: &[u8]) -> bool {
    if marker == b"." {
        return is_macro_terminator(bytes, control);
    }
    let Some(remainder) = bytes.strip_prefix(&[control]) else {
        return false;
    };
    let remainder = trim_horizontal_space(remainder);
    let name_end = remainder
        .iter()
        .position(u8::is_ascii_whitespace)
        .unwrap_or(remainder.len());
    remainder[..name_end] == *marker
        && remainder[name_end..]
            .iter()
            .all(|byte| matches!(*byte, b' ' | b'\t'))
}

fn trim_horizontal_space(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !matches!(*byte, b' ' | b'\t'))
        .unwrap_or(bytes.len());
    &bytes[start..]
}

fn copy_mode_reparse(bytes: &[u8], escape: u8) -> Vec<u8> {
    let mut reparsed = Vec::with_capacity(bytes.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        // A bracketed string or register name remains copy-mode opaque until
        // the environment resolves that reference.  In particular, in
        // `\\*[std\\\\esc]`, the inner doubled delimiter denotes a literal
        // delimiter in the *name*; collapsing it here would turn the result
        // into `\\e` and make the later name resolver consume the `e`.
        if matches!(bytes.get(cursor + 1), Some(b'*' | b'n'))
            && bytes.get(cursor) == Some(&escape)
            && bytes.get(cursor + 2) == Some(&b'[')
        {
            let end = bracketed_reference_name_end(bytes, cursor + 3).unwrap_or(bytes.len());
            reparsed.extend_from_slice(&bytes[cursor..end]);
            cursor = end;
            continue;
        }
        if bytes[cursor] == escape && bytes.get(cursor + 1) == Some(&escape) {
            // A doubled outer delimiter does become active in copy mode, but
            // retain a following bracketed reference name verbatim for the
            // same reason as the active form above.
            if matches!(bytes.get(cursor + 2), Some(b'*' | b'n'))
                && bytes.get(cursor + 3) == Some(&b'[')
            {
                let end = bracketed_reference_name_end(bytes, cursor + 4).unwrap_or(bytes.len());
                reparsed.push(escape);
                reparsed.extend_from_slice(&bytes[cursor + 2..end]);
                cursor = end;
                continue;
            }
            reparsed.push(escape);
            cursor += 2;
        } else {
            reparsed.push(bytes[cursor]);
            cursor += 1;
        }
    }
    reparsed
}

/// Reparse a user-macro argument for delayed `\$` substitution.
///
/// Roff's argument reader treats an embedded literal quote as data when it
/// appears inside an unquoted argument.  If that value is later substituted
/// into a quoted macro-body argument, leaving the byte literal would instead
/// terminate the surrounding quote during the second parse.  mandoc rewrites
/// that injected byte as its standard `\(dq` spelling; escaped source quotes
/// are already explicit roff controls and remain untouched.
fn macro_argument_copy_mode_reparse(bytes: &[u8], escape: u8) -> Vec<u8> {
    let mut bytes = copy_mode_reparse(bytes, escape);
    // An active delimiter at physical end of a macro invocation is roff's
    // line-continuation marker.  Keeping it would escape the first literal
    // byte after `\$n` in the macro body (commonly the closing `)`), whereas
    // mandoc consumes it before delayed argument substitution.
    if bytes.last() == Some(&escape) {
        bytes.pop();
    }
    let mut reparsed = Vec::with_capacity(bytes.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == escape && cursor + 1 < bytes.len() {
            reparsed.extend_from_slice(&bytes[cursor..cursor + 2]);
            cursor += 2;
            continue;
        }
        if bytes[cursor] == b'"' {
            reparsed.extend_from_slice(&[escape, b'(', b'd', b'q']);
        } else {
            reparsed.push(bytes[cursor]);
        }
        cursor += 1;
    }
    reparsed
}

/// Return the exclusive end of the bracketed name whose first byte follows
/// the opening `[`; an unterminated name is left to the normal escape recovery
/// path rather than being partially reparsed here.
fn bracketed_reference_name_end(bytes: &[u8], name_start: usize) -> Option<usize> {
    bytes
        .get(name_start..)?
        .iter()
        .position(|byte| *byte == b']')
        .map(|offset| name_start + offset + 1)
}

/// Detect the copy-mode spelling whose provenance cannot be recovered from
/// the public text alone: both `\t` and `\\t` can project as `\t` after
/// reparsing, but only the latter is authored literal text.
fn has_protected_tabulation_escape(bytes: &[u8], escape: u8) -> bool {
    bytes
        .windows(3)
        .any(|window| window == [escape, escape, b't'])
}

fn apply_environment_request(
    environment: &mut Environment,
    builder: &mut DocumentBuilder,
    request: &[u8],
    escape: u8,
    arguments: &[Argument],
    limits: &Limits,
) -> Result<(), EnvironmentError> {
    let result = match request {
        b"ds" | b"as" => {
            if let Some((name, value)) = arguments.split_first() {
                environment.define_string(
                    &name.bytes,
                    &join_arguments(value),
                    request == b"as",
                    limits,
                )
            } else {
                Ok(())
            }
        }
        b"nr" => {
            // mandoc's `.nr` accepts only a literal space after the register
            // name.  A tab terminates the name but makes the whole request a
            // no-op; preserve that request-specific distinction from the
            // scanner rather than weakening generic argument lexing.
            if arguments.first().and_then(|name| name.separator_after) == Some(b'\t') {
                Ok(())
            } else {
                let Some((name, expression, increment)) = number_register_arguments(arguments)
                else {
                    return Ok(());
                };
                environment.define_register(&name.bytes, &expression, increment, limits)
            }
        }
        b"rr" => {
            // Unlike `.rm`, legacy `.rr` accepts exactly one register name.
            // Additional tokens (including one separated by a tab) are not
            // independent removals.
            if let Some(name) = arguments.first() {
                // A non-literal escape in a register name is diagnosed, then
                // mandoc removes the valid name prefix.  In contrast `.nr`
                // itself leaves such a definition untouched.
                let name = malformed_register_name_prefix(&name.bytes).unwrap_or(&name.bytes);
                environment.remove_register(name);
            }
            Ok(())
        }
        b"rm" => {
            for argument in arguments {
                let normalized = normalize_roff_name_prefix(&argument.bytes, escape);
                // A later invocation of a user macro that `.rm` removed is
                // not ordinary unknown roff syntax: mandoc drops it and
                // reports the deleted callable spelling.  Strings and
                // registers do not receive that request-level treatment.
                let removed_macro = environment.macro_removal_is_diagnosable(&normalized.name);
                environment.remove(&normalized.name);
                if removed_macro {
                    environment.suppress_macro_name(&normalized.name);
                }
                // A prohibited escape is diagnosed by the request dispatcher.
                // Mandoc still removes the valid prefix, but abandons the
                // remaining names in the same `.rm` request.
                if normalized.invalid_escape_preview.is_some() {
                    break;
                }
                // `.rm` accepts a space-delimited name list. A tab ends the
                // first name but leaves the remaining tail outside that list.
                if argument.separator_after == Some(b'\t') {
                    break;
                }
            }
            Ok(())
        }
        b"rn" => {
            // roff's rename request requires a literal space after the old
            // name. A tab there makes the whole request a no-op; tabs after
            // the new name are merely ignored tail input.
            if arguments.first().and_then(|old| old.separator_after) == Some(b'\t') {
                if let Some(new) = arguments.get(1) {
                    // The request is rejected, but mandoc still remembers
                    // the attempted target as a user macro spelling and
                    // diagnoses a later call instead of retaining it as an
                    // arbitrary roff element.
                    environment.suppress_macro_name(&new.bytes);
                }
                return Ok(());
            }
            if let [old, new, ..] = arguments {
                environment.rename(&old.bytes, &new.bytes);
                environment.suppress_macro_name(&old.bytes);
                environment.clear_suppressed_macro_name(&new.bytes);
                if is_builtin_package_macro(builder.macro_set(), &old.bytes) {
                    environment.rename_package_macro(&old.bytes, &new.bytes);
                }
            }
            Ok(())
        }
        b"als" => {
            if let [target, alias, ..] = arguments {
                environment.alias_macro(&target.bytes, &alias.bytes, limits)?;
            }
            Ok(())
        }
        _ => Ok(()),
    };
    if result.is_ok() {
        record_mdoc_synopsis_register_state(builder, environment, request, arguments);
    }
    result
}

/// Return the valid prefix preceding a prohibited escape in a register name.
/// A doubled delimiter is the one literal form and remains part of the name.
fn malformed_register_name_prefix(name: &[u8]) -> Option<&[u8]> {
    let mut offset = 0_usize;
    while offset < name.len() {
        if name[offset] != b'\\' {
            offset += 1;
            continue;
        }
        if name.get(offset + 1) == Some(&b'\\') {
            offset += 2;
            continue;
        }
        return Some(&name[..offset]);
    }
    None
}

/// Reconstruct the parenthesized `.nr` value grammar from generic roff
/// argument tokens.  Whitespace terminates ordinary request arguments, but
/// mandoc accepts it inside a parenthesized numeric expression, where the
/// token after the matching close parenthesis becomes the optional increment.
type NumberRegisterArguments<'a> = (&'a Argument, Vec<u8>, Option<&'a [u8]>);

fn number_register_arguments(arguments: &[Argument]) -> Option<NumberRegisterArguments<'_>> {
    let (name, remainder) = arguments.split_first()?;
    let expression = remainder.first()?;
    if !expression.bytes.contains(&b'(') {
        return Some((
            name,
            expression.bytes.clone(),
            arguments.get(2).map(|increment| increment.bytes.as_slice()),
        ));
    }
    let mut depth = 0_usize;
    for (index, argument) in arguments[1..].iter().enumerate() {
        for byte in &argument.bytes {
            match byte {
                b'(' => depth = depth.saturating_add(1),
                b')' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        if depth == 0 {
            let last = index + 1;
            return Some((
                name,
                join_arguments(&arguments[1..=last]),
                arguments
                    .get(last + 1)
                    .map(|increment| increment.bytes.as_slice()),
            ));
        }
    }
    // An unclosed parenthesis remains a permissive numeric prefix in the
    // legacy evaluator. Its whitespace-separated tail still belongs to that
    // value rather than becoming an accidental increment argument.
    Some((name, join_arguments(&arguments[1..]), None))
}

/// Return the `.nr` expression when its parsed arithmetic contains a `/ 0`
/// or `% 0` operand.  The numeric evaluator deliberately recovers that form
/// to zero so subsequent interpolation remains deterministic; the scanner
/// owns the source-addressable legacy error finding.
fn register_division_by_zero(arguments: &[Argument]) -> Option<&Argument> {
    let [name, expression, ..] = arguments else {
        return None;
    };
    if name.separator_after == Some(b'\t') || !has_zero_divisor(&expression.bytes) {
        return None;
    }
    Some(expression)
}

fn has_zero_divisor(expression: &[u8]) -> bool {
    expression.iter().enumerate().any(|(index, operator)| {
        if !matches!(operator, b'/' | b'%') {
            return false;
        }
        let mut cursor = index + 1;
        while expression.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if expression
            .get(cursor)
            .is_some_and(|byte| matches!(byte, b'+' | b'-'))
        {
            cursor += 1;
        }
        let start = cursor;
        while expression.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        cursor > start && expression[start..cursor].iter().all(|digit| *digit == b'0')
    })
}

/// Preserve mdoc's private `nS` execution state across the scanner/semantic
/// boundary.  The `.nr` request itself remains transparent in the public AST;
/// its effect is consumed in source order by the mdoc structural pass.
fn record_mdoc_synopsis_register_state(
    builder: &mut DocumentBuilder,
    environment: &Environment,
    request: &[u8],
    arguments: &[Argument],
) {
    if builder.macro_set() != MacroSet::Mdoc
        || request != b"nr"
        || arguments
            .first()
            .is_none_or(|argument| argument.bytes != b"nS")
    {
        return;
    }
    builder.record_mdoc_synopsis_state(
        environment
            .register_value(b"nS")
            .is_some_and(|value| value != 0),
    );
}

/// Apply roff's string-definition syntax without treating data quotes as
/// generic macro-argument delimiters.
///
/// In `.ds name value` and `.as name value`, the first double quote of the
/// value is a copy-mode control character.  It is removed even if no closing
/// quote appears; later quotes are retained literally.  This differs from the
/// argument grammar used by ordinary control macros (and is why this logic is
/// kept at the request boundary).
#[allow(clippy::too_many_arguments)] // Definition-time interpolation shares the session-wide budget and source-relative diagnostics.
fn apply_string_request(
    environment: &mut Environment,
    raw_arguments: &[u8],
    escape: u8,
    append: bool,
    limits: &Limits,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    expansion_steps: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> Result<(), EnvironmentError> {
    let raw_arguments = trim_horizontal_space(raw_arguments);
    let mut name_end = 0;
    while name_end < raw_arguments.len() && !raw_arguments[name_end].is_ascii_whitespace() {
        name_end += if raw_arguments[name_end] == escape && name_end + 1 < raw_arguments.len() {
            2
        } else {
            1
        };
    }
    let Some(name) = raw_arguments
        .get(..name_end)
        .filter(|name| !name.is_empty())
    else {
        return Ok(());
    };
    let Some(name) = normalize_roff_definition_name(name, escape) else {
        // The scanner-stage caller emits the source-precise invalid-name
        // diagnostic. A definition with a prohibited escape has no state
        // effect, while a doubled delimiter is retained as one literal byte.
        return Ok(());
    };
    // Roff consumes separating spaces after a definition name, but a tab at
    // this boundary belongs to the copied value.  That distinction is
    // observable later when the string is expanded into filled text.
    let value = &raw_arguments[name_end..];
    let value = &value[value.iter().take_while(|byte| **byte == b' ').count()..];
    let value = value.strip_prefix(b"\"").unwrap_or(value);
    let Some(value) = expand_copy_mode_definition(
        environment,
        value,
        escape,
        limits,
        source_id,
        start,
        end,
        expansion_steps,
        diagnostics,
        truncated,
    ) else {
        return Ok(());
    };
    let value = copy_mode_reparse(&value, escape);
    environment.define_string(&name, &value, append, limits)
}

/// Normalize a roff definition name after its request-specific validation.
/// A doubled delimiter denotes one literal delimiter; every other escape is
/// prohibited in string-definition names.
fn normalize_roff_definition_name(name: &[u8], escape: u8) -> Option<Vec<u8>> {
    let normalized = normalize_roff_name_prefix(name, escape);
    normalized
        .invalid_escape_preview
        .is_none()
        .then_some(normalized.name)
}

/// Roff name recovery shared by macro definitions, removals, and control-line
/// dispatch.  A doubled delimiter is one literal byte.  Any other escape is
/// illegal in a name; mandoc keeps the prefix before it for the request's
/// state change and stops inspecting the rest of that name.
#[derive(Debug)]
struct NormalizedRoffName {
    name: Vec<u8>,
    invalid_escape_preview: Option<Vec<u8>>,
}

fn normalize_roff_name_prefix(name: &[u8], escape: u8) -> NormalizedRoffName {
    let mut normalized = Vec::with_capacity(name.len());
    let mut cursor = 0_usize;
    while let Some(byte) = name.get(cursor).copied() {
        if byte != escape {
            normalized.push(byte);
            cursor += 1;
            continue;
        }
        if name.get(cursor + 1) == Some(&escape) {
            normalized.push(escape);
            cursor += 2;
            continue;
        }
        let preview_end = if matches!(name.get(cursor + 1), Some(b' ' | b'\t')) {
            cursor.saturating_add(1)
        } else {
            cursor.saturating_add(2).min(name.len())
        };
        return NormalizedRoffName {
            name: normalized,
            invalid_escape_preview: Some(name[..preview_end].to_vec()),
        };
    }
    NormalizedRoffName {
        name: normalized,
        invalid_escape_preview: None,
    }
}

/// A physical control name is normally cut before an adjacent escape by the
/// scanner.  Retain that split for scope delimiters, while recovering the
/// roff-name cases that need it: literal `\\` names and a known macro or
/// definition request followed by a prohibited escape.
#[derive(Debug)]
struct AttachedControlName {
    name: Vec<u8>,
    display_name: Vec<u8>,
    arguments: Vec<u8>,
    invalid_escape_preview: Option<Vec<u8>>,
}

fn recover_attached_control_name(
    name: &[u8],
    raw_arguments: &[u8],
    escape: u8,
    recover_prohibited_escape: bool,
) -> Option<AttachedControlName> {
    let escaped = raw_arguments.strip_prefix(&[escape])?;
    if escaped.first() == Some(&escape) {
        let name_tail_end = raw_arguments
            .iter()
            .position(u8::is_ascii_whitespace)
            .unwrap_or(raw_arguments.len());
        let mut raw_name = Vec::with_capacity(name.len() + name_tail_end);
        raw_name.extend_from_slice(name);
        raw_name.extend_from_slice(&raw_arguments[..name_tail_end]);
        let normalized = normalize_roff_name_prefix(&raw_name, escape);
        return Some(AttachedControlName {
            name: normalized.name,
            display_name: raw_name,
            arguments: trim_horizontal_space(&raw_arguments[name_tail_end..]).to_vec(),
            invalid_escape_preview: normalized.invalid_escape_preview,
        });
    }
    // `\{` opens a roff scope and belongs to the conditional grammar, not a
    // request name.  All other illegal escapes recover the valid prefix.
    if !recover_prohibited_escape || escaped.first() == Some(&b'{') {
        return None;
    }
    let escape_width = roff_escape_name_width(raw_arguments, escape);
    let preview_end = 2.min(raw_arguments.len());
    let mut preview = Vec::with_capacity(name.len() + preview_end);
    preview.extend_from_slice(name);
    preview.extend_from_slice(&raw_arguments[..preview_end]);
    Some(AttachedControlName {
        name: name.to_vec(),
        display_name: name.to_vec(),
        arguments: trim_horizontal_space(&raw_arguments[escape_width..]).to_vec(),
        invalid_escape_preview: Some(preview),
    })
}

fn roff_escape_name_width(bytes: &[u8], escape: u8) -> usize {
    debug_assert_eq!(bytes.first(), Some(&escape));
    match bytes.get(1).copied() {
        Some(b'(') => 4.min(bytes.len()),
        Some(b'[') => bytes
            .get(2..)
            .and_then(|tail| tail.iter().position(|byte| *byte == b']'))
            .map_or(bytes.len(), |offset| offset + 3),
        _ => 2.min(bytes.len()),
    }
}

fn condition_parts(arguments: &[Argument]) -> Option<(Vec<u8>, usize)> {
    let first = arguments.first()?;
    if matches!(first.bytes.as_slice(), b"d" | b"r" | b"!d" | b"!r") {
        let name = arguments.get(1)?;
        let mut predicate = first.bytes.clone();
        predicate.extend_from_slice(&name.bytes);
        Some((predicate, 2))
    } else {
        Some((first.bytes.clone(), 1))
    }
}

/// Validate the register and name-defined condition forms before expansion.
///
/// Their operand is an identifier, so an escape is not a deferred expansion:
/// mandoc diagnoses it at the beginning of the name and continues with the
/// unexpanded predicate. The lexer deliberately keeps the authored spelling
/// here, which also preserves the location of a two-token `r name` form.
#[allow(clippy::too_many_arguments)]
fn emit_escaped_condition_name(
    arguments: &[Argument],
    escape: u8,
    argument_start: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let Some(first) = arguments.first() else {
        return;
    };
    let (negation_width, predicate) = first
        .bytes
        .strip_prefix(b"!")
        .map_or((0_usize, first.bytes.as_slice()), |predicate| {
            (1, predicate)
        });
    let Some(name) = predicate
        .strip_prefix(b"r")
        .or_else(|| predicate.strip_prefix(b"d"))
    else {
        return;
    };
    let (name, source_offset) = if name.is_empty() {
        let Some(name) = arguments.get(1) else {
            return;
        };
        (name.bytes.as_slice(), name.offset)
    } else {
        (name, first.offset.saturating_add(negation_width + 1))
    };
    let Some(escape_offset) = name.iter().position(|byte| *byte == escape) else {
        return;
    };
    // Retain the escape's one-byte delimiter in the message: this is the
    // short spelling reported by mandoc for both `\\(` and `\\[` names.
    let preview_end = escape_offset.saturating_add(2).min(name.len());
    let start = argument_start.saturating_add(
        u32::try_from(source_offset).expect("argument offsets are bounded by line length"),
    );
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::ROFF_ESCAPED_NAME,
            Severity::Error,
            source_id,
            start,
            start.saturating_add(1),
            format!(
                "escaped character not allowed in a name: {}",
                visible_bytes(&name[..preview_end])
            ),
        ),
        truncated,
    );
}

/// Reject an escape in the first name argument of requests such as `.nr` and
/// `.rr`. A doubled delimiter is a literal backslash in a roff name, but any
/// other escape is rejected before the request's existing recovery executes.
#[allow(clippy::too_many_arguments)]
fn emit_escaped_request_name(
    arguments: &[Argument],
    escape: u8,
    argument_start: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let Some(name) = arguments.first() else {
        return;
    };
    let mut cursor = 0;
    let escape_offset = loop {
        let Some(offset) = name.bytes[cursor..]
            .iter()
            .position(|byte| *byte == escape)
            .map(|offset| cursor + offset)
        else {
            return;
        };
        if name.bytes.get(offset + 1) == Some(&escape) {
            cursor = offset + 2;
            continue;
        }
        break offset;
    };
    let preview_end = if matches!(name.bytes.get(escape_offset + 1), Some(b' ' | b'\t')) {
        escape_offset.saturating_add(1)
    } else {
        escape_offset.saturating_add(2).min(name.bytes.len())
    };
    let start = argument_start.saturating_add(
        u32::try_from(name.offset).expect("argument offsets are bounded by line length"),
    );
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::ROFF_ESCAPED_NAME,
            Severity::Error,
            source_id,
            start,
            start.saturating_add(1),
            format!(
                "escaped character not allowed in a name: {}",
                visible_bytes(&name.bytes[..preview_end])
            ),
        ),
        truncated,
    );
}

/// Preserve the source spelling of an inline conditional body.
///
/// The condition predicate needs roff-aware tokenization, but its body is a
/// request or text fragment in its own right.  Rejoining lexer tokens would
/// discard a `.ds`/`.as` value's leading copy-mode quote before that request
/// can interpret it.  Argument offsets let us parse only the predicate while
/// slicing the body from the original bytes.
fn condition_body_template(
    raw_arguments: &[u8],
    arguments: &[Argument],
    body_start: usize,
) -> Vec<u8> {
    condition_body_template_from_offset(raw_arguments, arguments, body_start, None)
}

/// Return the copied-input cursor for a one-predicate inline macro body.
///
/// A user macro is first reparsed in copy mode.  If its condition's predicate
/// shrinks while expanding a `\$n` argument, mandoc continues at the reduced
/// cursor for the inline body, not at the original definition byte offset.
/// Keep this deliberately narrow: the two-token `r`/`d` forms have distinct
/// condition grammar and are not needed for the macro-body recovery path.
fn macro_conditional_body_origin(
    body_line: &[u8],
    raw_arguments: &[u8],
    arguments: &[Argument],
    body_start: usize,
    predicate_width: Option<usize>,
) -> Option<u32> {
    if body_start != 1 {
        return None;
    }
    let predicate = arguments.first()?;
    let body = arguments.get(body_start)?;
    let predicate_width = predicate_width?;
    if predicate_width == predicate.bytes.len() {
        return None;
    }
    let control_width = body_line.len().checked_sub(raw_arguments.len())?;
    let separator_width = body
        .offset
        .checked_sub(predicate.offset.checked_add(predicate.bytes.len())?)?;
    u32::try_from(
        control_width
            .saturating_add(predicate_width)
            .saturating_add(separator_width),
    )
    .ok()
}

/// Return the copied-input cursor inherited by the first line of a braced
/// macro conditional.  `roff_cond()` reruns from immediately after the
/// compacted predicate and opening `\{`; the following retained line is not
/// a new macro invocation at column one.
fn macro_scope_body_origin(
    body_line: &[u8],
    control: u8,
    predicate_width: Option<usize>,
) -> Option<u32> {
    let predicate_width = u32::try_from(predicate_width?).ok()?;
    Some(macro_body_control_column(body_line, control).saturating_add(predicate_width))
}

fn condition_body_template_from_offset(
    raw_arguments: &[u8],
    arguments: &[Argument],
    body_start: usize,
    escaped_name_body_offset: Option<usize>,
) -> Vec<u8> {
    if let Some(body_offset) = escaped_name_body_offset {
        return raw_arguments
            .get(body_offset..)
            .unwrap_or_default()
            .to_vec();
    }
    let Some(body) = arguments.get(body_start) else {
        return Vec::new();
    };
    let separator_start = body_start
        .checked_sub(1)
        .and_then(|index| arguments.get(index))
        .and_then(|predicate| predicate.offset.checked_add(predicate.bytes.len()))
        .unwrap_or(body.offset);
    let body_offset = raw_arguments
        .get(separator_start..body.offset)
        .and_then(|separator| separator.iter().rposition(|byte| *byte == b'\t'))
        .and_then(|offset| separator_start.checked_add(offset))
        .unwrap_or(body.offset);
    raw_arguments
        .get(body_offset..)
        .unwrap_or_default()
        .to_vec()
}

/// Split an escaped name condition into its accepted identifier and retained
/// inline body. `roff_cond()` stops the identifier at the invalid escape, then
/// reparses that escape as the beginning of visible body text.
fn split_escaped_condition_body(
    arguments: &[Argument],
    escape: u8,
    fallback_predicate: &[u8],
) -> Option<(Vec<u8>, usize)> {
    let first = arguments.first()?;
    let (negation_width, predicate) = first
        .bytes
        .strip_prefix(b"!")
        .map_or((0_usize, first.bytes.as_slice()), |predicate| {
            (1, predicate)
        });
    let kind = *predicate.first()?;
    if !matches!(kind, b'r' | b'd') {
        return None;
    }
    let (name, name_offset, prefix) = if predicate.len() == 1 {
        let name = arguments.get(1)?;
        (name.bytes.as_slice(), name.offset, first.bytes.clone())
    } else {
        (
            &predicate[1..],
            first.offset.saturating_add(negation_width + 1),
            first.bytes[..=negation_width].to_vec(),
        )
    };
    let escape_offset = name.iter().position(|byte| *byte == escape)?;
    let mut predicate = prefix;
    predicate.extend_from_slice(&name[..escape_offset]);
    if predicate == fallback_predicate {
        return None;
    }
    Some((predicate, name_offset.saturating_add(escape_offset)))
}

/// Select the public source start for an inline conditional body.
///
/// A literal tab separating a register/name predicate from visible text is
/// consumed by the argument lexer, but mandoc anchors the visible text at
/// that tab. This preserves both its diagnostic position and the source-aware
/// renderer's distinction from an ordinary separating space.
fn condition_body_source_start_from_offset(
    raw_arguments: &[u8],
    arguments: &[Argument],
    body_start: usize,
    argument_start: u32,
    fallback: u32,
    escaped_name_body_offset: Option<usize>,
) -> u32 {
    if let Some(source_offset) = escaped_name_body_offset {
        return u32::try_from(source_offset)
            .ok()
            .and_then(|offset| argument_start.checked_add(offset))
            .unwrap_or(fallback);
    }
    let Some(body) = arguments.get(body_start) else {
        return fallback;
    };
    let separator_start = body_start
        .checked_sub(1)
        .and_then(|index| arguments.get(index))
        .and_then(|predicate| predicate.offset.checked_add(predicate.bytes.len()))
        .unwrap_or(body.offset);
    let source_offset = raw_arguments
        .get(separator_start..body.offset)
        .and_then(|separator| separator.iter().rposition(|byte| *byte == b'\t'))
        .and_then(|offset| separator_start.checked_add(offset))
        .unwrap_or(body.offset);
    u32::try_from(source_offset)
        .ok()
        .and_then(|offset| argument_start.checked_add(offset))
        .unwrap_or(fallback)
}

#[allow(clippy::naive_bytecount)] // This bounded compatibility lexer counts literal tabs without a runtime dependency.
fn lex_condition_arguments(
    bytes: &[u8],
    escape: u8,
    limits: &Limits,
) -> Result<Vec<Argument>, ArgumentIssue> {
    let leading = bytes
        .len()
        .saturating_sub(trim_horizontal_space(bytes).len());
    let bytes = &bytes[leading..];
    if bytes.first() != Some(&b'"') {
        return lex_arguments(bytes, escape, limits);
    }
    let mut delimiters = 0_usize;
    let mut end = None;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'"' {
            delimiters += 1;
            if delimiters == 3 {
                end = Some(index + 1);
                break;
            }
        }
    }
    let Some(end) = end else {
        return lex_arguments(bytes, escape, limits);
    };
    let mut arguments = vec![Argument {
        offset: leading,
        quoted: true,
        separator_after: bytes.get(end).copied().filter(u8::is_ascii_whitespace),
        separator_contains_tab: bytes[end..]
            .iter()
            .take_while(|byte| byte.is_ascii_whitespace())
            .any(|byte| *byte == b'\t'),
        embedded_tab_count: bytes[..end].iter().filter(|byte| **byte == b'\t').count(),
        separator_width: bytes[end..]
            .iter()
            .take_while(|byte| byte.is_ascii_whitespace())
            .count(),
        bytes: bytes[..end].to_vec(),
    }];
    let mut tail = lex_arguments(&bytes[end..], escape, limits)?;
    for argument in &mut tail {
        argument.offset += leading + end;
    }
    if arguments.len() + tail.len() > limits.max_arguments {
        return Err(ArgumentIssue::Limit);
    }
    let bytes_used = arguments
        .iter()
        .chain(&tail)
        .map(|argument| argument.bytes.len())
        .sum::<usize>();
    if bytes_used > limits.max_argument_bytes {
        return Err(ArgumentIssue::Limit);
    }
    arguments.append(&mut tail);
    Ok(arguments)
}

fn evaluate_condition(environment: &mut Environment, bytes: &[u8]) -> Option<bool> {
    let (negated, bytes) = bytes
        .strip_prefix(b"!")
        .map_or((false, bytes), |remaining| (true, remaining));
    // A bare opening parenthesis has started numeric parsing in mandoc, then
    // failed before an operand.  That is an invalid condition rather than an
    // ordinary false string comparison, so a preceding `!` does not turn it
    // true (`roff_evalcond()` returns false directly once its cursor moved).
    if bytes == b"(" {
        return Some(false);
    }
    let value = if let Some(name) = bytes.strip_prefix(b"r").filter(|name| !name.is_empty()) {
        Some(environment.is_register_defined(name))
    } else if let Some(name) = bytes.strip_prefix(b"d").filter(|name| !name.is_empty()) {
        let defined = environment.is_name_defined(name)
            || is_builtin_request(name)
            // `.if dBR` and peers are evaluated after the man parser has
            // selected its package, even though the generic roff condition
            // evaluator does not receive that selection explicitly.
            || is_builtin_package_macro(MacroSet::Man, name)
            || is_builtin_package_macro(MacroSet::Mdoc, name);
        if !defined {
            environment.observe_undefined_name_condition(name);
        }
        Some(defined)
    } else {
        match bytes {
            b"n" => Some(true),
            b"t" => Some(false),
            _ => evaluate_numeric_condition(bytes).or_else(|| evaluate_string_condition(bytes)),
        }
    }?;
    Some(if negated { !value } else { value })
}

fn is_builtin_request(name: &[u8]) -> bool {
    matches!(
        name,
        b"br"
            | b"ce"
            | b"ft"
            | b"ll"
            | b"ps"
            | b"na"
            | b"nf"
            | b"fi"
            | b"PP"
            | b"LP"
            | b"P"
            | b"TH"
            | b"SH"
            | b"SS"
            | b"TP"
            | b"TQ"
    )
}

fn evaluate_string_condition(bytes: &[u8]) -> Option<bool> {
    let (&delimiter, remainder) = bytes.split_first()?;
    if delimiter.is_ascii_digit() || matches!(delimiter, b'+' | b'-' | b'<' | b'>' | b'=') {
        return None;
    }
    let Some(middle) = remainder.iter().position(|byte| *byte == delimiter) else {
        return Some(false);
    };
    let right = &remainder[middle + 1..];
    let Some(end) = right.iter().position(|byte| *byte == delimiter) else {
        return Some(false);
    };
    Some(remainder[..middle] == right[..end])
}

fn evaluate_numeric_condition(bytes: &[u8]) -> Option<bool> {
    // A leading unmatched opening parenthesis groups as much numeric input as
    // follows it in groff/mandoc condition syntax.  A bare `(` is instead the
    // false, malformed-string form handled below.
    let bytes = bytes.strip_prefix(b"(").unwrap_or(bytes);
    if bytes.is_empty() {
        return None;
    }
    if let Some(operator) = bytes
        .iter()
        .enumerate()
        .find_map(|(index, byte)| matches!(*byte, b'&' | b':').then_some(index))
    {
        let left = evaluate_sum(&bytes[..operator]).ok()?;
        let right = evaluate_sum(&bytes[operator + 1..]).ok()?;
        return Some(match bytes[operator] {
            b'&' => left.magnitude != 0 && right.magnitude != 0,
            b':' => left.magnitude != 0 || right.magnitude != 0,
            _ => unreachable!("boolean condition operators are exhaustive"),
        });
    }
    let operator = bytes
        .iter()
        .enumerate()
        .find_map(|(index, byte)| matches!(*byte, b'<' | b'>' | b'=' | b'!').then_some(index));
    let Some(operator) = operator else {
        // `roff_evalcond()` selects the true branch only for a *positive*
        // numeric result.  A negative scaled value is well-formed but false;
        // this differs from the usual Rust/C nonzero truthiness.
        return evaluate_sum(bytes).ok().map(|value| value.magnitude > 0);
    };
    let left = evaluate_sum(&bytes[..operator]).ok()?;
    let (operation, right_start): (&[u8], usize) = match bytes.get(operator..)? {
        [b'<', b'=', ..] => (b"<=", operator + 2),
        [b'>', b'=', ..] => (b">=", operator + 2),
        [b'!', b'=', ..] | [b'<', b'>', ..] => (b"!=", operator + 2),
        [b'=', b'=', ..] => (b"==", operator + 2),
        [b'<', ..] => (b"<", operator + 1),
        [b'>', ..] => (b">", operator + 1),
        [b'=', ..] => (b"==", operator + 1),
        _ => return None,
    };
    let Some(right) = evaluate_sum(&bytes[right_start..]).ok() else {
        // mandoc selects the false branch for an incomplete/malformed numeric
        // comparison (for example `42=bad`), rather than treating it as an
        // unsupported extension.
        return Some(false);
    };
    let ordering = left.compare(right)?;
    Some(match operation {
        b"<" => ordering.is_lt(),
        b"<=" => ordering.is_le(),
        b">" => ordering.is_gt(),
        b">=" => ordering.is_ge(),
        b"==" => ordering.is_eq(),
        b"!=" => ordering.is_ne(),
        _ => unreachable!("condition operators are exhaustive"),
    })
}

fn join_arguments(arguments: &[Argument]) -> Vec<u8> {
    let retained: usize = arguments.iter().map(|argument| argument.bytes.len()).sum();
    let mut joined = Vec::with_capacity(retained.saturating_add(arguments.len().saturating_sub(1)));
    for (index, argument) in arguments.iter().enumerate() {
        if index > 0 {
            joined.push(b' ');
        }
        joined.extend_from_slice(&argument.bytes);
    }
    joined
}

/// Immutable parser result plus non-fatal findings and work counters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseReport {
    /// Bounded immutable syntax document.
    pub document: Document,
    /// Recoverable diagnostics in source order.
    pub diagnostics: Vec<Diagnostic>,
    /// Observable work counters for debugging and benchmark evidence.
    pub statistics: ParseStatistics,
}

/// Counters recorded without exposing mutable parser internals.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ParseStatistics {
    /// Total uncompressed bytes accepted across sources.
    pub source_bytes: usize,
    /// Number of top-level and resolved source files.
    pub source_files: usize,
    /// Roff expansion and reparse steps.
    pub expansion_steps: usize,
    /// Nodes in the final immutable AST.
    pub emitted_nodes: usize,
    /// Maximum structural or equation nesting depth observed.
    pub maximum_depth: usize,
    /// Whether a coherent prefix was truncated by a deterministic limit.
    pub truncated: bool,
}

/// Fatal session failure that prevents a coherent bounded report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FatalError {
    /// Stable failure category.
    pub kind: FatalErrorKind,
    /// Human explanation not used as a programmatic discriminator.
    pub message: Box<str>,
}

impl FatalError {
    fn invalid_configuration(error: LimitViolation) -> Self {
        Self {
            kind: FatalErrorKind::InvalidConfiguration,
            message: error.to_string().into(),
        }
    }

    fn source_limit(name: &str, actual: usize, maximum: usize) -> Self {
        Self {
            kind: FatalErrorKind::SourceLimit,
            message: format!("{name}: source has {actual} bytes; configured limit is {maximum}")
                .into(),
        }
    }

    fn source_line_limit(name: &str, actual: usize, maximum: usize) -> Self {
        Self {
            kind: FatalErrorKind::SourceLineLimit,
            message: format!(
                "{name}: source has {actual} physical lines; configured limit is {maximum}"
            )
            .into(),
        }
    }
}

/// Stable categories for a fatal parser boundary failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FatalErrorKind {
    /// `Limits` contains an impossible or zero budget.
    InvalidConfiguration,
    /// Top-level source bytes exceeded the configured input budget.
    SourceLimit,
    /// Top-level source lines exceeded the bounded source-map budget.
    SourceLineLimit,
    /// A source cannot fit the public byte-offset representation.
    SourceTooLargeForSpans,
    /// Future I/O adapters could not read a caller-requested source.
    Read,
    /// Future transport adapter could not decode a requested source.
    Decompression,
    /// A caller selected a feature-gated transport adapter that is unavailable.
    Unsupported,
    /// Internal invariant violation, reserved for bugs rather than source errors.
    Invariant,
}

impl fmt::Display for FatalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for FatalError {}

fn select_macro_set(syntax: Syntax, bytes: &[u8]) -> MacroSet {
    match syntax {
        Syntax::Roff => MacroSet::None,
        Syntax::Man => MacroSet::Man,
        Syntax::Mdoc => MacroSet::Mdoc,
        Syntax::Auto => bytes
            .split(|byte| *byte == b'\n')
            .find_map(|line| match line.get(..3) {
                Some(b".Dd") => Some(MacroSet::Mdoc),
                Some(b".TH" | b".SH" | b".SS") => Some(MacroSet::Man),
                _ => None,
            })
            .unwrap_or(MacroSet::None),
    }
}

/// Detect a physical root-source `.Os` request before the semantic mdoc pass
/// runs.  The scanner may intentionally suppress an otherwise empty prologue
/// node when a document contains no visible body, but that source request is
/// still distinct from a truly missing operating-system prologue.
fn source_has_mdoc_operating_system_request(bytes: &[u8]) -> bool {
    source_mdoc_operating_system_request(bytes).is_some()
}

/// Return the arguments of the first physical root-source `.Os` request.
/// The semantic scanner remains authoritative for normal documents; this is
/// the bounded prologue fallback for an otherwise body-less mdoc input.
fn source_mdoc_operating_system_request(bytes: &[u8]) -> Option<&[u8]> {
    bytes.split(|byte| *byte == b'\n').find_map(|line| {
        line.strip_prefix(b".Os")
            .or_else(|| line.strip_prefix(b"'Os"))
            .filter(|tail| tail.is_empty() || tail[0].is_ascii_whitespace())
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        DiagnosticCode, FatalErrorKind, Limits, MacroSet, NodeKind, Parser, ParserConfig, Severity,
        Source, SourceBundle, SourceName, Syntax,
    };

    fn maximum_document_depth(document: &crate::Document) -> usize {
        let root = document.node(document.root()).unwrap();
        let mut maximum = 0;
        let mut pending = vec![(root, 1_usize)];
        while let Some((node, depth)) = pending.pop() {
            maximum = maximum.max(depth);
            pending.extend(node.children().map(|child| (child, depth + 1)));
        }
        maximum
    }

    #[test]
    fn physical_os_request_detection_distinguishes_absent_and_bare_forms() {
        assert!(super::source_has_mdoc_operating_system_request(b".Os\n"));
        assert!(super::source_has_mdoc_operating_system_request(
            b".Os OpenBSD\n"
        ));
        assert!(!super::source_has_mdoc_operating_system_request(
            b".Dt TEST 1\n"
        ));
    }

    #[test]
    fn tbl_projection_keeps_utf8_and_malformed_byte_origins_distinct() {
        assert_eq!(
            super::legacy_table_input_text(b"\\[u0080]\xc2\x80"),
            "\\[u0080]\\[u0080]"
        );
        assert_eq!(super::legacy_table_input_text(b"\xc2x"), "?x");
        assert_eq!(
            super::legacy_table_input_text(b"\xc2\xc3\x80"),
            "?\\[u00C0]"
        );
    }

    #[test]
    fn m2_scanner_accepts_arbitrary_bytes_without_utf8_replacement() {
        let name = SourceName::new("arbitrary.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(&name, b".TH TEST 1\n\xff"))
            .unwrap();
        assert_eq!(report.document.macro_set(), MacroSet::Man);
        assert_eq!(
            report
                .document
                .source_name(report.document.root_source())
                .map(crate::SourceName::as_str),
            Some("arbitrary.1")
        );
        assert_eq!(report.statistics.source_bytes, 12);
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            crate::DiagnosticCode::INPUT_INVALID_BYTE
        );
        assert_eq!(
            report.diagnostics[0].message.as_ref(),
            "skipping bad character: 0xff"
        );
        let children = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .collect::<Vec<_>>();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].text(), Some("ÿ"));
    }

    #[test]
    fn lowercase_man_title_keeps_the_legacy_visible_diagnostic() {
        let name = SourceName::new("lowercase-title.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(&name, b".TH bar-man 1\n"))
            .unwrap();
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            DiagnosticCode::MAN_TITLE_NOT_UPPERCASE
        );
        assert_eq!(
            report.diagnostics[0].message.as_ref(),
            "lower case character in document title: TH bar-man"
        );
    }

    #[test]
    fn lowercase_mdoc_title_keeps_the_legacy_visible_diagnostic() {
        let name = SourceName::new("lowercase-mdoc-title.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt Cm-PUNCT 1\n.Os\n.Sh NAME\n.Nm cm-punct\n.Nd title validation\n",
            ))
            .unwrap();
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            DiagnosticCode::MDOC_TITLE_NOT_UPPERCASE
        );
        assert_eq!(report.diagnostics[0].severity, Severity::Style);
        assert_eq!(
            report.diagnostics[0].message.as_ref(),
            "lower case character in document title: Dt Cm-PUNCT"
        );
    }

    #[test]
    fn mdoc_date_validation_distinguishes_missing_legacy_and_unparseable_dates() {
        let cases = [
            (
                b".Dd\n.Dt DATE 1\n.Os\n.Sh NAME\n.Nm date\n.Nd validation\n".as_slice(),
                DiagnosticCode::MDOC_DATE_MISSING,
                Severity::Warning,
                "missing date, using \"\": Dd",
            ),
            (
                b".Dd \"not a date\"\n.Dt DATE 1\n.Os\n.Sh NAME\n.Nm date\n.Nd validation\n"
                    .as_slice(),
                DiagnosticCode::MDOC_DATE_UNPARSEABLE,
                Severity::Warning,
                "cannot parse date, using it verbatim: Dd not a date",
            ),
            (
                b".Dd 2014-08-07\n.Dt DATE 1\n.Os\n.Sh NAME\n.Nm date\n.Nd validation\n".as_slice(),
                DiagnosticCode::MDOC_DATE_LEGACY,
                Severity::Style,
                "legacy man(7) date format: Dd 2014-08-07",
            ),
        ];
        let name = SourceName::new("mdoc-date-validation.1").unwrap();
        for (source, code, severity, message) in cases {
            let report = Parser::default().parse(Source::new(&name, source)).unwrap();
            assert_eq!(report.diagnostics.len(), 1, "{:#?}", report.diagnostics);
            assert_eq!(report.diagnostics[0].code.as_str(), code);
            assert_eq!(report.diagnostics[0].severity, severity);
            assert_eq!(report.diagnostics[0].message.as_ref(), message);
        }
    }

    #[test]
    fn mdoc_date_prologue_order_recovery_preserves_the_last_authored_date() {
        let name = SourceName::new("mdoc-date-prologue-order.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dt DATE 1\n.Dd August 5, 2014\n.Os\n.Sh NAME\n.Nm date\n.Nd validation\n.Sh DESCRIPTION\ntext\n.Dd August 6, 2014\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>(),
            [
                DiagnosticCode::MDOC_PROLOGUE_ORDER,
                DiagnosticCode::MDOC_DUPLICATE_PROLOGUE,
            ]
        );
        assert_eq!(
            report.document.metadata().date.as_deref(),
            Some("August 6, 2014")
        );
    }

    #[test]
    fn filled_text_tab_keeps_the_legacy_visible_diagnostic() {
        let name = SourceName::new("filled-tab.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH TABS 1\n.SH DESCRIPTION\nleft\tright\n",
            ))
            .unwrap();
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            DiagnosticCode::INPUT_TAB_IN_FILLED_TEXT
        );
        assert_eq!(report.diagnostics[0].message.as_ref(), "tab in filled text");
    }

    #[test]
    fn copy_mode_string_tabs_survive_expansion_and_warn_in_filled_text() {
        let name = SourceName::new("string-tab.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH TABS 1\n.SH DESCRIPTION\n.ds value\ttext\n>>\\*[value]<<\n",
            ))
            .unwrap();
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert!(visible.contains(&">>\ttext<<"));
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            DiagnosticCode::INPUT_TAB_IN_FILLED_TEXT
        );
        assert_eq!(
            report
                .document
                .source_position(report.diagnostics[0].primary.as_ref().unwrap()),
            Some(crate::SourcePosition { line: 4, column: 3 })
        );
    }

    #[test]
    fn undefined_string_warning_starts_at_its_interpolation() {
        let name = SourceName::new("missing-string.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH STRING 1\n.SH DESCRIPTION\n>>>\\*[missing]<<<\n",
            ))
            .unwrap();
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            DiagnosticCode::ROFF_UNDEFINED_REFERENCE
        );
        assert_eq!(
            report
                .document
                .source_position(report.diagnostics[0].primary.as_ref().unwrap()),
            Some(crate::SourcePosition { line: 3, column: 4 })
        );
    }

    #[test]
    fn missing_strings_on_one_line_report_in_reverse_source_order() {
        let name = SourceName::new("missing-strings.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH STRING 1\n.SH DESCRIPTION\n\\*[first] and \\*[second]\n",
            ))
            .unwrap();
        assert_eq!(report.diagnostics.len(), 3);
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "undefined string, using \"\": second",
                "undefined string, using \"\": first",
                "whitespace at end of input line",
            ]
        );
        let positions = report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                report
                    .document
                    .source_position(diagnostic.primary.as_ref().unwrap())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            positions,
            [
                Some(crate::SourcePosition {
                    line: 3,
                    column: 15,
                }),
                Some(crate::SourcePosition { line: 3, column: 1 }),
                Some(crate::SourcePosition { line: 3, column: 5 }),
            ]
        );
    }

    #[test]
    fn nested_man_examples_keep_non_stack_fill_style_diagnostics() {
        let name = SourceName::new("nested-examples.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH EXAMPLE 1\n.SH DESCRIPTION\n.EX\nouter\n.EX\ninner\n.EE\nouter\n.EE\n",
            ))
            .unwrap();
        assert_eq!(report.diagnostics.len(), 2);
        assert!(
            report
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code.as_str()
                    == DiagnosticCode::MAN_REDUNDANT_FILL_MODE)
        );
        assert!(
            report
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.severity == Severity::Style)
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "fill mode already disabled, skipping: EX",
                "fill mode already enabled, skipping: EE",
            ]
        );
        let positions = report
            .diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.primary.as_ref())
            .filter_map(|span| report.document.source_position(span))
            .map(|position| (position.line, position.column))
            .collect::<Vec<_>>();
        assert_eq!(positions, [(5, 2), (9, 2)]);
    }

    #[test]
    fn redundant_man_fill_request_keeps_the_legacy_style_diagnostic() {
        let name = SourceName::new("redundant-fill.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(&name, b".TH FILL 1\n.SH DESCRIPTION\n.fi\n"))
            .unwrap();
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            DiagnosticCode::MAN_REDUNDANT_FILL_MODE
        );
        assert_eq!(report.diagnostics[0].severity, Severity::Style);
        assert_eq!(
            report.diagnostics[0].message.as_ref(),
            "fill mode already enabled, skipping: fi"
        );
        let position = report.diagnostics[0]
            .primary
            .as_ref()
            .and_then(|span| report.document.source_position(span))
            .unwrap();
        assert_eq!((position.line, position.column), (3, 2));
    }

    #[test]
    fn implicit_mdoc_enclosures_require_a_blank_before_trailing_delimiters() {
        let name = SourceName::new("implicit-delimiter.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt AQ 1\n.Os\n.Sh DESCRIPTION\n.Aq user@host:\n",
            ))
            .unwrap();
        let diagnostics = report
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.code.as_str() == DiagnosticCode::MDOC_TRAILING_DELIMITER_SPACING
            })
            .collect::<Vec<_>>();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].code.as_str(),
            DiagnosticCode::MDOC_TRAILING_DELIMITER_SPACING
        );
        assert_eq!(diagnostics[0].severity, Severity::Style);
        assert_eq!(
            diagnostics[0].message.as_ref(),
            "no blank before trailing delimiter: Aq user@host:"
        );
        let position = diagnostics[0]
            .primary
            .as_ref()
            .and_then(|span| report.document.source_position(span))
            .unwrap();
        assert_eq!((position.line, position.column), (5, 14));

        let prose = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt PQ 1\n.Os\n.Sh DESCRIPTION\n.Pq Like in this case.\n.Pq \\&.\n",
            ))
            .unwrap();
        assert!(prose.diagnostics.iter().all(|diagnostic| {
            diagnostic.code.as_str() == DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME
        }));
    }

    #[test]
    fn selected_syntax_is_deterministic_before_scanning_is_implemented() {
        let name = SourceName::new("syntax.1").unwrap();
        let parser = Parser::new(ParserConfig {
            syntax: Syntax::Mdoc,
            ..ParserConfig::default()
        });
        let report = parser
            .parse(Source::new(&name, b".TH ignored 1\n"))
            .unwrap();
        assert_eq!(report.document.macro_set(), MacroSet::Mdoc);
    }

    #[test]
    fn explicit_roff_syntax_does_not_select_or_structure_macro_packages() {
        let name = SourceName::new("raw-roff.in").unwrap();
        let parser = Parser::new(ParserConfig {
            syntax: Syntax::Roff,
            ..ParserConfig::default()
        });
        let report = parser
            .parse(Source::new(&name, b".TH RAW 1\n.SH BODY\ntext\n"))
            .unwrap();
        assert_eq!(report.document.macro_set(), MacroSet::None);
        let nodes = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .collect::<Vec<_>>();
        assert_eq!(nodes[0].kind(), NodeKind::Element);
        assert_eq!(nodes[1].kind(), NodeKind::Element);
        assert_eq!(nodes[1].macro_name(), Some("SH"));
    }

    #[test]
    fn point_size_and_page_length_requests_are_non_public_formatter_requests() {
        let name = SourceName::new("point-size.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ps 36\n.pl 8000\n.if dps active\nvisible\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty());
        let nodes = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .collect::<Vec<_>>();
        assert_eq!(nodes.len(), 2);
        assert!(nodes.iter().all(|node| node.kind() == NodeKind::Text));
        assert_eq!(nodes[0].text(), Some("active"));
        assert_eq!(nodes[1].text(), Some("visible"));
    }

    #[test]
    fn font_family_requests_are_non_public_formatter_requests() {
        let name = SourceName::new("font-family.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(&name, b".ftr V CR\n.ftr VI CI\nvisible\n"))
            .unwrap();
        assert!(report.diagnostics.is_empty());
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(text, ["visible"]);
    }

    #[test]
    fn conditional_font_family_setup_consumes_both_scope_closers() {
        let name = SourceName::new("conditional-font-family.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b"'\\\" t\r\n.\\\" comment\r\n.ie \"\\f[CB]x\\f[]\"x\" \\{\\\r\n. ftr V B\r\n.\\}\r\n.el \\{\\\r\n. ftr V CR\r\n.\\}\r\n.TH CONDITIONAL 1\r\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        assert!(
            report
                .document
                .preorder()
                .all(|node| node.text() != Some("\r"))
        );
    }

    #[test]
    fn no_adjust_request_is_a_non_public_formatter_request() {
        let name = SourceName::new("no-adjust.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b"before\n.na ignored arguments\nafter\n",
            ))
            .unwrap();
        assert!(report.diagnostics.iter().all(|diagnostic| {
            diagnostic.code.as_str() == DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME
        }));
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(text, ["before", "after"]);
    }

    #[test]
    fn man_rs_updates_the_an_margin_register_before_text_expansion() {
        let name = SourceName::new("an-margin.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH AN-MARGIN 1\n.SH DESCRIPTION\n.RS 0.0\n\\n[an-margin]\n.RS 3.5\n\\n[an-margin]\n.RE\n\\n[an-margin]\n.RE\n\\n[an-margin]\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let values = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .filter(|text| text.chars().all(|character| character.is_ascii_digit()))
            .collect::<Vec<_>>();
        assert_eq!(values, ["168", "252", "168", "168"]);
    }

    #[test]
    fn m3_mdoc_os_uses_the_session_fallback_only_when_the_source_is_bare() {
        let name = SourceName::new("operating-system.1").unwrap();
        let parser = Parser::new(ParserConfig {
            syntax: Syntax::Mdoc,
            operating_system: Some("PinnedOS 1.0".into()),
            ..ParserConfig::default()
        });

        let bare = parser
            .parse(Source::new(
                &name,
                b".Dd August 24, 2026\n.Dt BARE-OS 1\n.Os\n",
            ))
            .unwrap();
        assert_eq!(bare.document.metadata().os.as_deref(), Some("PinnedOS 1.0"));

        let authored = parser
            .parse(Source::new(
                &name,
                b".Dd August 24, 2026\n.Dt AUTHORED-OS 1\n.Os AuthoredOS\n",
            ))
            .unwrap();
        assert_eq!(
            authored.document.metadata().os.as_deref(),
            Some("AuthoredOS")
        );

        let man = parser
            .parse(Source::new(
                &name,
                b".TH FALLBACK-OS 1\n.SH NAME\nfallback-os\n",
            ))
            .unwrap();
        assert_eq!(man.document.metadata().os.as_deref(), Some("PinnedOS 1.0"));
    }

    #[test]
    fn m3_string_definition_quotes_are_not_generic_argument_quotes() {
        let name = SourceName::new("string-quote.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ds foo \"first part\n.as foo \" second part\n\\*[foo]\n.ds bar \"string value\"\n\\*[bar]\n",
            ))
            .unwrap();

        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["first part second part", "string value\""]);
    }

    #[test]
    fn m3_string_definition_copy_mode_is_preserved_in_generated_and_scoped_execution() {
        let name = SourceName::new("generated-string-quote.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de rootdef\n.ds root \"root \"quote\n..\n.de scopedef\n.ds scoped \"scoped \"quote\n..\n.rootdef\n.if 1 .ds inline \"inline \"quote\n.if 1 \\{\\\n.ds block \"block \"quote\n.scopedef\n.if 1 .ds nested \"nested \"quote\n.\\}\n\\*[root]\n\\*[inline]\n\\*[block]\n\\*[scoped]\n\\*[nested]\n",
            ))
            .unwrap();

        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(
            visible,
            [
                "root \"quote",
                "inline \"quote",
                "block \"quote",
                "scoped \"quote",
                "nested \"quote"
            ]
        );
    }

    #[test]
    fn m3_nested_string_names_are_resolved_before_the_outer_lookup() {
        let name = SourceName::new("nested-string.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ds foo bar\n.ds bar output\nThis is \\*[\\*[foo]].\n",
            ))
            .unwrap();

        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["This is output."]);
    }

    #[test]
    fn m3_ignore_blocks_consume_source_without_retaining_their_contents() {
        let name = SourceName::new("ignore.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b"before\n.ig custom\nignored one\n.ig\nignored two\n..\n.custom\nafter\n.ig\nignored through eof\n",
            ))
            .unwrap();

        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>(),
            [DiagnosticCode::ROFF_UNCLOSED_IGNORE]
        );
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["before", "after"]);
    }

    #[test]
    fn m3_macro_generated_ignore_requests_consume_following_physical_input() {
        let name = SourceName::new("macro-ignore.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de top\n.ig top-end\n..\n.top\ntop-hidden\n.top-end\ntop-visible\n.de scoped\n.ig scope-end\n..\n.if 1 \\{\\\n.scoped\n.\\}\nscope-hidden\n.scope-end\nscope-visible\n",
            ))
            .unwrap();

        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["top-visible", "scope-visible"]);
    }

    #[test]
    fn m3_macro_generated_definition_consumes_following_copy_mode_input() {
        let name = SourceName::new("macro-definition.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de outer\n.de inner\n..\n.outer\ninner body\n..\n.inner\n",
            ))
            .unwrap();

        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["inner body"]);
    }

    #[test]
    fn m3_macro_replay_discards_input_comments_after_control_arguments() {
        let name = SourceName::new("macro-inline-comment.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                br#".de annotated
.IR troff s, \" formatter annotation
..
.annotated
"#,
            ))
            .unwrap();

        let visible = report
            .document
            .preorder()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(
            visible,
            ["troff", "s,"],
            "input comment leaked through macro replay"
        );
    }

    #[test]
    fn m3_macro_replay_honors_active_escape_for_input_comments() {
        let name = SourceName::new("macro-custom-inline-comment.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                br#".ec @
.de annotated
.IR troff s, @" formatter annotation
..
.annotated
"#,
            ))
            .unwrap();

        let visible = report
            .document
            .preorder()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(
            visible,
            ["@", "troff", "s,"],
            "custom input comment leaked through macro replay"
        );
    }

    #[test]
    fn m3_macro_generated_indirect_definitions_resolve_names_before_copy_mode() {
        let name = SourceName::new("macro-indirect-definition.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ds target inner\n.ds marker done\n.de outer\n.dei target marker\n.ami target marker\n..\n.outer\nfirst\n.done\nsecond\n.done\n.inner\n",
            ))
            .unwrap();

        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["first", "second"]);
    }

    #[test]
    fn m3_macro_conditional_scopes_select_their_immediate_else_branch() {
        let name = SourceName::new("macro-conditional-scope.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de decide\n.ie \\$1 \\{\\\nhit \\$1\n.br\\}\n.el \\{\\\nmiss\n.br\\}\n..\n.decide 1\n.decide 0\n",
            ))
            .unwrap();

        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["hit 1", "miss"]);
    }

    #[test]
    fn m3_scope_macros_execute_their_own_conditional_brace_frames() {
        let name = SourceName::new("scope-macro-conditional.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de decide\n.ie \\$1 \\{\\\nhit \\$1\n.br\\}\n.el \\{\\\nmiss\n.br\\}\n..\n.if 1 \\{\\\n.decide 1\n.decide 0\n.\\}\n",
            ))
            .unwrap();

        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["hit 1", "miss"]);
    }

    #[test]
    fn m3_scope_macros_can_install_indirect_definitions_from_following_input() {
        let name = SourceName::new("scope-macro-definition.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ds target inner\n.ds marker done\n.de outer\n.dei target marker\n.ami target marker\n..\n.if 1 \\{\\\n.outer\n.\\}\nfirst\n.done\nsecond\n.done\n.inner\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["first", "second"]);
    }

    #[test]
    fn m3_collected_scope_ignores_direct_lines_through_its_local_marker() {
        let name = SourceName::new("scope-ignore.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".if 1 \\{\\\nbefore\n.ig stop\nhidden\n.stop\nafter\n.\\}\noutside\n",
            ))
            .unwrap();

        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["before", "after", "outside"]);
    }

    #[test]
    fn root_input_limit_is_fatal_before_ast_allocation() {
        let name = SourceName::new("too-large.1").unwrap();
        let mut config = ParserConfig::default();
        config.limits.max_root_source_bytes = 3;
        let error = Parser::new(config)
            .parse(Source::new(&name, b"four"))
            .unwrap_err();
        assert_eq!(error.kind, FatalErrorKind::SourceLimit);
    }

    #[test]
    fn source_line_limit_bounds_document_line_index_allocation() {
        let name = SourceName::new("many-lines.1").unwrap();
        let mut config = ParserConfig::default();
        config.limits.max_source_lines = 2;
        let error = Parser::new(config)
            .parse(Source::new(&name, b"one\ntwo\nthree"))
            .unwrap_err();
        assert_eq!(error.kind, FatalErrorKind::SourceLineLimit);
    }

    #[test]
    fn scanner_emits_control_arguments_and_honors_dynamic_characters() {
        let name = SourceName::new("dynamic.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".cc !\n!ec @\nvisible @(em @\\ tail\n!TH \"two words\" 1\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let nodes = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .collect::<Vec<_>>();
        assert_eq!(nodes[0].macro_name(), Some("cc"));
        assert_eq!(nodes[1].macro_name(), Some("ec"));
        assert_eq!(nodes[2].text(), Some("visible — @ tail"));
        assert_eq!(nodes[3].macro_name(), Some("TH"));
        assert_eq!(
            nodes[3]
                .children()
                .map(|node| node.text().unwrap().to_owned())
                .collect::<Vec<_>>(),
            ["two words", "1"]
        );
    }

    #[test]
    fn character_control_requests_are_private_and_discard_excess_bytes_in_man_input() {
        let name = SourceName::new("character-control.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CHARACTER-CONTROL 1\n.SH DESCRIPTION\n.cc :\n:cc ;bogus\ntext\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [(
                DiagnosticCode::ROFF_EXCESS_ARGUMENTS,
                "skipping excess arguments: cc ... bogus",
            )]
        );
        let position = report
            .document
            .source_position(report.diagnostics[0].primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((position.line, position.column), (4, 6));
        assert!(
            report
                .document
                .preorder()
                .all(|node| !matches!(node.macro_name(), Some("cc" | "c2" | "ec")))
        );
    }

    #[test]
    fn roff_font_requests_validate_and_project_the_legacy_ft_shape() {
        let name = SourceName::new("font-request.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd January 1, 2020\n.Dt FONT-REQUEST 1\n.Os\n.Sh NAME\n.Nm font-request\n.Nd font validation\n.Sh DESCRIPTION\n.ft B\n.ft foo\n.ft I bogus\n.ft P\n.ft\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                (
                    DiagnosticCode::ROFF_EXCESS_ARGUMENTS,
                    "skipping excess arguments: ft ... bogus",
                ),
                (
                    DiagnosticCode::ROFF_UNKNOWN_FONT,
                    "unknown font, skipping request: ft foo",
                ),
            ]
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    report
                        .document
                        .source_position(diagnostic.primary.as_ref().unwrap())
                        .map(|position| (position.line, position.column))
                })
                .collect::<Vec<_>>(),
            [Some((10, 7)), Some((9, 2))]
        );
        let fonts = report
            .document
            .preorder()
            .filter(|node| node.macro_name() == Some("ft"))
            .map(|node| {
                node.children()
                    .map(crate::NodeRef::text)
                    .collect::<Option<Vec<_>>>()
            })
            .collect::<Option<Vec<_>>>()
            .unwrap();
        assert_eq!(fonts, [vec!["B"], vec!["I"], vec!["P"], vec!["P"]]);
    }

    #[test]
    fn char_requests_are_private_but_expand_declared_character_values() {
        let name = SourceName::new("character-definitions.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CHARACTER-DEFINITIONS 1\n.SH DESCRIPTION\n.char \\[myc] myval\n.char x y\n.char \\[boldX] \\fBX\n\\[boldX] \\[myc]\nfinal text\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "invalid escape sequence: \\[myc]",
                "invalid escape sequence: \\[boldX]",
                "invalid escape sequence: \\[myc]",
                "invalid escape sequence: \\[boldX]",
            ]
        );
        let text = report
            .document
            .preorder()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert!(text.contains(&"\\fBX\\fP myval"));
        assert!(text.contains(&"final teyt"));
        assert!(
            report
                .document
                .preorder()
                .all(|node| node.macro_name() != Some("char"))
        );
    }

    #[test]
    fn char_requests_report_invalid_left_operands_at_their_precise_source_spans() {
        let name = SourceName::new("character-invalid.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CHARACTER-INVALID 1\n.SH DESCRIPTION\n.char\n.char \\fR myval\n.char \\[myc]x myval\n.char xy myval\nmyc: <\\[myc]> x\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                (
                    DiagnosticCode::ROFF_INVALID_CHARACTER_ARGUMENT,
                    "argument is not a character: char",
                ),
                (
                    DiagnosticCode::ROFF_INVALID_CHARACTER_ARGUMENT,
                    "argument is not a character: char \\fR myval",
                ),
                (
                    DiagnosticCode::ESCAPE_UNKNOWN_SPECIAL_CHARACTER,
                    "invalid escape sequence: \\[myc]",
                ),
                (
                    DiagnosticCode::ROFF_INVALID_CHARACTER_ARGUMENT,
                    "argument is not a character: char \\[myc]x myval",
                ),
                (
                    DiagnosticCode::ROFF_INVALID_CHARACTER_ARGUMENT,
                    "argument is not a character: char xy myval",
                ),
                (
                    DiagnosticCode::ESCAPE_UNKNOWN_SPECIAL_CHARACTER,
                    "invalid escape sequence: \\[myc]",
                ),
            ]
        );
        let positions = report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                let position = report
                    .document
                    .source_position(diagnostic.primary.as_ref().unwrap())
                    .unwrap();
                (position.line, position.column)
            })
            .collect::<Vec<_>>();
        assert_eq!(positions, [(3, 6), (4, 7), (5, 7), (5, 7), (6, 7), (7, 7)]);
    }

    #[test]
    fn scanner_limits_return_a_bounded_prefix_and_typed_findings() {
        let name = SourceName::new("bounded.1").unwrap();
        let mut config = ParserConfig::default();
        config.limits.max_nodes = 2;
        config.limits.max_diagnostics = 4;
        let report = Parser::new(config)
            .parse(Source::new(&name, b"one\ntwo\nthree\n"))
            .unwrap();
        assert_eq!(report.document.node_count(), 2);
        assert!(report.statistics.truncated);
        assert_eq!(report.diagnostics[0].code.as_str(), "limits.nodes");
    }

    #[test]
    fn default_tree_limit_matches_the_legacy_finite_prefix_boundary() {
        let name = SourceName::new("deep-man.1").unwrap();
        let mut source = String::from(".TH DEEP 1\n.SH BODY\n");
        for _ in 0..300 {
            source.push_str(".RS\n");
        }
        source.push_str("retained prefix\n");
        for _ in 0..300 {
            source.push_str(".RE\n");
        }

        let report = Parser::default()
            .parse(Source::new(&name, source.as_bytes()))
            .unwrap();
        assert!(report.statistics.truncated);
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_str() == DiagnosticCode::LEGACY_SYNTAX_TREE_DEPTH_LIMIT
        }));
        assert_eq!(
            report.document.node_count(),
            report.document.preorder().count()
        );
        assert_eq!(maximum_document_depth(&report.document), 256);
        assert_eq!(
            report.statistics.emitted_nodes,
            report.document.node_count()
        );
    }

    #[test]
    fn caller_selected_tree_limit_uses_the_native_limit_code() {
        let name = SourceName::new("narrow-tree.1").unwrap();
        let mut config = ParserConfig::default();
        config.limits.max_tree_depth = 4;
        let mut source = String::from(".TH NARROW 1\n.SH BODY\n");
        for _ in 0..10 {
            source.push_str(".RS\n");
        }
        source.push_str("text\n");
        let report = Parser::new(config)
            .parse(Source::new(&name, source.as_bytes()))
            .unwrap();

        assert!(report.statistics.truncated);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code.as_str() == DiagnosticCode::LIMIT_TREE_DEPTH)
        );
        assert_eq!(maximum_document_depth(&report.document), 4);
    }

    #[test]
    fn m3_semantic_staging_respects_the_node_budget_before_adding_man_parts() {
        let name = SourceName::new("bounded-man.1").unwrap();
        let mut config = ParserConfig {
            syntax: Syntax::Man,
            ..ParserConfig::default()
        };
        config.limits.max_nodes = 4;
        let report = Parser::new(config)
            .parse(Source::new(&name, b".SH BOUNDED\n"))
            .unwrap();

        // The scanner emitted root, SH, and its argument.  Forming the
        // staging Block/Head/Body shape needs two more nodes, so the original
        // event remains reachable rather than exceeding max_nodes.
        assert_eq!(report.document.node_count(), 3);
        let section = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .next()
            .unwrap();
        assert_eq!(section.kind(), NodeKind::Element);
        assert!(report.statistics.truncated);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|finding| finding.code.as_str() == "limits.nodes")
        );
    }

    #[test]
    fn m5_semantic_staging_respects_the_node_budget_before_adding_mdoc_parts() {
        let name = SourceName::new("bounded-mdoc.1").unwrap();
        let mut config = ParserConfig {
            syntax: Syntax::Mdoc,
            ..ParserConfig::default()
        };
        config.limits.max_nodes = 4;
        let report = Parser::new(config)
            .parse(Source::new(&name, b".Sh BOUNDED\n"))
            .unwrap();

        // The scanner emitted root, Sh, and its argument. Forming the
        // staging Block/Head/Body shape needs two more nodes, so the original
        // event remains reachable rather than exceeding max_nodes.
        assert_eq!(report.document.node_count(), 3);
        let section = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .next()
            .unwrap();
        assert_eq!(section.kind(), NodeKind::Element);
        assert!(report.statistics.truncated);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|finding| finding.code.as_str() == "limits.nodes")
        );
    }

    #[test]
    fn aggregate_escape_work_limit_stops_before_unbounded_scanner_output() {
        let name = SourceName::new("escapes.1").unwrap();
        let mut config = ParserConfig::default();
        config.limits.max_expansion_steps = 1;
        let report = Parser::new(config)
            .parse(Source::new(&name, b"\\&\\&\nnext\n"))
            .unwrap();
        assert_eq!(report.document.node_count(), 1);
        assert!(report.statistics.truncated);
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            "limits.expansion-steps"
        );
    }

    #[test]
    fn scanner_is_total_and_source_bounded_for_every_two_byte_prefix() {
        let name = SourceName::new("all-byte-prefixes.roff").unwrap();
        let parser = Parser::default();
        for first in u8::MIN..=u8::MAX {
            for second in u8::MIN..=u8::MAX {
                let bytes = [first, second];
                let report = parser.parse(Source::new(&name, &bytes)).unwrap();
                assert_eq!(
                    report.document.node_count(),
                    report.statistics.emitted_nodes
                );
                for node in report.document.preorder() {
                    if let Some(span) = node.location() {
                        assert!(span.start <= span.end);
                        assert!(usize::try_from(span.end).unwrap() <= bytes.len());
                        assert!(report.document.source_position(span).is_some());
                    }
                }
                for finding in &report.diagnostics {
                    for span in finding
                        .primary
                        .iter()
                        .chain(finding.related.iter().map(|related| &related.span))
                    {
                        assert!(span.start <= span.end);
                        assert!(usize::try_from(span.end).unwrap() <= bytes.len());
                        assert!(report.document.source_position(span).is_some());
                    }
                }
            }
        }
    }

    #[test]
    fn dynamic_character_requests_keep_public_spans_inside_source_bytes() {
        let name = SourceName::new("fuzz-dynamic-control.roff").unwrap();
        let bytes = b".cc !\x8c";
        let report = Parser::default().parse(Source::new(&name, bytes)).unwrap();
        for node in report.document.preorder() {
            if let Some(span) = node.location() {
                assert!(span.start <= span.end);
                assert!(usize::try_from(span.end).unwrap() <= bytes.len());
                assert!(report.document.source_position(span).is_some());
            }
        }
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            DiagnosticCode::ROFF_EXCESS_ARGUMENTS
        );
        for finding in &report.diagnostics {
            if let Some(span) = &finding.primary {
                assert!(span.start <= span.end);
                assert!(usize::try_from(span.end).unwrap() <= bytes.len());
                assert!(report.document.source_position(span).is_some());
            }
        }
    }

    #[test]
    fn man_title_validation_keeps_malformed_input_diagnostic_spans_in_bounds() {
        let name = SourceName::new("fuzz-man-title.roff").unwrap();
        let bytes = b".TH A\xc7n";
        let report = Parser::default().parse(Source::new(&name, bytes)).unwrap();
        let finding = report
            .diagnostics
            .iter()
            .find(|finding| finding.code.as_str() == DiagnosticCode::MAN_TITLE_NOT_UPPERCASE)
            .expect("malformed title still contains an ASCII lower-case character");
        let span = finding.primary.as_ref().expect("title finding has a span");
        assert_eq!((span.start, span.end), (6, 7));
        assert!(usize::try_from(span.end).unwrap() <= bytes.len());
        assert!(report.document.source_position(span).is_some());
    }

    #[test]
    fn pinned_head_c_escape_shape_is_visible_before_roff_scope_execution() {
        let name = SourceName::new("c_man.in").unwrap();
        let report = Parser::default()
            .parse(Source::new(&name, b".B\none\\c\nword\n"))
            .unwrap();
        let nodes = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .collect::<Vec<_>>();
        assert_eq!(nodes[0].macro_name(), Some("B"));
        assert_eq!(nodes[1].text(), Some("one"));
        assert!(nodes[1].flags().line_continuation);
        assert_eq!(nodes[2].text(), Some("word"));
        assert!(report.diagnostics.iter().all(|diagnostic| {
            diagnostic.code.as_str() == DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME
        }));
    }

    #[test]
    fn package_ast_retains_a_final_no_space_escape() {
        let name = SourceName::new("package-c.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH PACKAGE-C 1\n.SH DESCRIPTION\none\\c\nword\n",
            ))
            .unwrap();
        let text = report
            .document
            .preorder()
            .find(|node| node.text() == Some("one\\c"))
            .expect("the package AST retains the authored escape");
        assert!(text.flags().line_continuation);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_environment_requests_expand_text_and_control_arguments() {
        let name = SourceName::new("environment.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ds title mantdoc\n.nr count 7\n.TH \\*[title] \\n[count]\ntext \\*[title] \\n[count]\n.as title -rs\n\\*[title]\n.rm title count\n\\*[title] \\n[count]\n",
            ))
            .unwrap();
        let nodes = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .collect::<Vec<_>>();
        assert_eq!(report.document.metadata().title.as_deref(), Some("mantdoc"));
        assert_eq!(report.document.metadata().section.as_deref(), Some("7"));
        assert_eq!(nodes[0].text(), Some("text mantdoc 7"));
        assert_eq!(nodes[1].text(), Some("mantdoc-rs"));
        assert_eq!(nodes[2].text(), Some(" 0"));
        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter(|finding| finding.code.as_str() == "roff.undefined-reference")
                .count(),
            1
        );
    }

    #[test]
    fn empty_user_strings_are_silent_in_control_position() {
        let name = SourceName::new("empty-string-control.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH EMPTY-STRING 1\n.SH DESCRIPTION\n.ds empty \"\n.empty\nvisible\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        assert!(
            report
                .document
                .preorder()
                .all(|node| node.macro_name() != Some("empty"))
        );
        assert!(
            report
                .document
                .preorder()
                .any(|node| node.text() == Some("visible"))
        );
    }

    #[test]
    fn mdoc_control_arguments_expand_unescaped_string_references() {
        let name = SourceName::new("mdoc-string-argument.1").unwrap();
        let report = Parser::new(ParserConfig {
            operating_system: Some("mantdoc canonical differential".into()),
            ..ParserConfig::default()
        })
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt STRING-ARG 1\n.Os\n.Sh DESCRIPTION\n.ds o \\(Fo\n.Eo \\*o\nbody\n.Ec \\*o\n.Pp\n.Eo \\\\*o\nbody\n.Ec \\\\*o\n",
            ))
            .unwrap();
        let texts = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert!(texts.iter().filter(|text| **text == "\\(Fo").count() >= 4);
        assert!(!texts.contains(&"\\*o"));
        assert!(report.diagnostics.iter().all(|diagnostic| {
            diagnostic.code.as_str() == DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME
        }));
    }

    #[test]
    fn m3_string_definitions_retain_the_full_unquoted_value() {
        let name = SourceName::new("string-value.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ds phrase native rust parser\n\\*[phrase]\n.as phrase with bounds\n\\*[phrase]\n",
            ))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            text,
            ["native rust parser", "native rust parserwith bounds"]
        );
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn recursive_string_expansion_drops_only_its_own_input_line() {
        let name = SourceName::new("recursive-string.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH RECURSIVE-STRING 1\n.SH DESCRIPTION\n.ds recur \\\\*[recur]\nbefore recursion\n(and do not \\*[recur] print this)\nafter recursion\n",
            ))
            .unwrap();
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            (
                report.diagnostics[0].code.as_str(),
                report.diagnostics[0].severity,
                report.diagnostics[0].message.as_ref(),
            ),
            (
                DiagnosticCode::LIMIT_EXPANSION_STEPS,
                Severity::Error,
                "input stack limit exceeded, infinite loop?",
            )
        );
        assert_eq!(
            report.diagnostics[0]
                .primary
                .as_ref()
                .and_then(|span| report.document.source_position(span))
                .map(|position| (position.line, position.column)),
            Some((5, 13))
        );
        let text = report
            .document
            .preorder()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert!(text.contains(&"before recursion"));
        assert!(text.contains(&"after recursion"));
        assert!(!text.iter().any(|value| value.contains("print this")));
    }

    #[test]
    fn string_definition_names_normalize_literal_escapes_and_reject_other_escapes() {
        let name = SourceName::new("string-escaped-name.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ds std\\\\esc stdval\n\\*[std\\\\esc]\n.ds esc\\eesc ignored\n\\*[esc]\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                (
                    DiagnosticCode::ROFF_ESCAPED_NAME,
                    "escaped character not allowed in a name: esc\\e",
                ),
                (
                    DiagnosticCode::ROFF_UNDEFINED_REFERENCE,
                    "undefined string, using \"\": esc",
                ),
            ]
        );
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert!(text.contains(&"stdval"));
        assert!(!text.contains(&"ignored"));
    }

    #[test]
    fn mdoc_bracketed_string_name_preserves_its_literal_escape_until_lookup() {
        let name = SourceName::new("mdoc-string-escaped-name.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt ESCAPED-NAME 1\n.Os\n.Sh NAME\n.Nm escaped-name\n.Nd test\n.Sh DESCRIPTION\n.ds std\\\\esc stdval\n.Sq \\*[std\\\\esc] .\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        assert!(
            report
                .document
                .preorder()
                .any(|node| node.text() == Some("stdval"))
        );
        assert!(
            !report
                .document
                .preorder()
                .any(|node| node.text() == Some(""))
        );
    }

    #[test]
    fn m3_copy_mode_macro_body_expands_arguments_at_invocation() {
        let name = SourceName::new("macro.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ds salutation welcome\n.de greet\nHello, \\$1!\n\\*[salutation]\n..\n.ds salutation later\n.greet mantdoc\n",
            ))
            .unwrap();
        let nodes = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .collect::<Vec<_>>();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].text(), Some("Hello, mantdoc!"));
        assert_eq!(nodes[1].text(), Some("welcome"));
        assert!(nodes.iter().all(|node| !node.flags().generated));
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_generated_controls_relex_expanded_macro_arguments() {
        let name = SourceName::new("macro-expanded-control.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH MACRO 1\n.SH DESCRIPTION\n.de show\n.BI \\$@\n..\n.show one two three\n",
            ))
            .unwrap();
        let bold_italic = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("BI"))
            .unwrap();
        assert_eq!(
            bold_italic
                .children()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            ["one", "two", "three"]
        );
        assert_eq!(
            bold_italic
                .children()
                .map(|argument| {
                    report
                        .document
                        .source_position(argument.location().expect("argument location"))
                        .expect("argument source position")
                })
                .collect::<Vec<_>>(),
            [
                crate::SourcePosition { line: 6, column: 5 },
                crate::SourcePosition {
                    line: 6,
                    column: 11
                },
                crate::SourcePosition {
                    line: 6,
                    column: 17
                },
            ]
        );
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    }

    #[test]
    fn man_attached_name_escape_rebases_the_first_visible_argument() {
        let name = SourceName::new("attached-man-escape.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH ATTACHED 1\n.SH DESCRIPTION\n.IB\\(lqone two\n",
            ))
            .unwrap();
        let macro_node = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("IB"))
            .expect("recovered IB macro");
        let first = macro_node.children().next().expect("first argument");
        assert_eq!(first.text(), Some("one"));
        assert_eq!(
            report
                .document
                .source_position(first.location().expect("argument location")),
            Some(crate::SourcePosition { line: 3, column: 8 })
        );
    }

    #[test]
    fn m3_direct_definition_in_a_macro_spans_pending_and_following_input() {
        let name = SourceName::new("nested-definition.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de outer\nouter macro\n.de inner\ninner macro\n..\nouter definition ended\n.outer\nfollowing caller input\n..\ninner definition ended\n.inner\nfinal text\n",
            ))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(
            text,
            [
                "outer definition ended",
                "outer macro",
                "inner definition ended",
                "inner macro",
                "following caller input",
                "final text",
            ]
        );
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    }

    #[test]
    fn roff_input_traps_reparse_the_armed_macro_after_the_matching_text_line() {
        let name = SourceName::new("input-trap.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH INPUT-TRAP 1\n.SH DESCRIPTION\n.de first\nfirst trap\n..\n.de second\nsecond trap\n..\n.it 1first\none\n.it 2 second\ntwo\nthree\nfour\n",
            ))
            .unwrap();
        let text = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(
            text,
            [
                "DESCRIPTION",
                "one",
                "first trap",
                "two",
                "three",
                "second trap",
                "four"
            ]
        );
        assert!(
            report
                .document
                .preorder()
                .all(|node| node.macro_name() != Some("it"))
        );
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn man_builtin_macro_names_take_precedence_over_roff_definitions() {
        let name = SourceName::new("defined-man-macro.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH DEFINED-MAN 1\n.de BI\n.IB \\$1 \\$2 \\$3\n..\n.SH DESCRIPTION\n.BI bold italic bold\n",
            ))
            .unwrap();
        let macro_node = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("BI"))
            .expect("the authored BI remains a man element");
        let children = macro_node
            .children()
            .map(|node| node.text().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(children, ["bold", "italic", "bold"]);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn implemented_mdoc_macro_names_take_precedence_over_roff_definitions() {
        let name = SourceName::new("defined-mdoc-macro.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt DEFINED-MDOC 1\n.Os\n.de At\nBSD\n..\n.Sh DESCRIPTION\n.At\n",
            ))
            .unwrap();
        let macro_node = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("At"))
            .expect("the authored At remains an mdoc element");
        let child = macro_node.children().next().expect("At default child");
        assert_eq!(child.text(), Some("AT&T UNIX"));
        assert!(child.flags().generated);
        assert!(report.diagnostics.iter().all(|diagnostic| {
            diagnostic.code.as_str() == DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME
        }));
    }

    #[test]
    fn at_expands_standard_versions_and_recovers_unknown_selectors() {
        let name = SourceName::new("at-versions.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt AT-VERSIONS 1\n.Os\n.Sh DESCRIPTION\n.At v7\n.At murks \"Sy\" bold\n",
            ))
            .unwrap();
        let at_nodes = report
            .document
            .preorder()
            .filter(|node| node.macro_name() == Some("At"))
            .collect::<Vec<_>>();
        assert_eq!(at_nodes.len(), 2);
        let valid_children = at_nodes[0].children().collect::<Vec<_>>();
        assert_eq!(
            valid_children
                .iter()
                .copied()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            ["Version\\~7 AT&T UNIX", "v7"]
        );
        assert!(valid_children[0].flags().generated);
        assert!(valid_children[1].flags().no_print);
        let invalid_children = at_nodes[1].children().collect::<Vec<_>>();
        assert_eq!(
            invalid_children
                .iter()
                .copied()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            ["AT&T UNIX", "murks"]
        );
        assert!(invalid_children[0].flags().generated);
        assert!(report.document.preorder().any(|node| {
            node.macro_name() == Some("Sy")
                && node.children().next().and_then(crate::NodeRef::text) == Some("bold")
        }));
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>(),
            [
                DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME,
                "mdoc.unknown-at-version",
            ]
        );
        assert_eq!(
            report.diagnostics[1].message.as_ref(),
            "unknown AT&T UNIX version: At murks"
        );
    }

    #[test]
    fn appended_mdoc_closer_keeps_its_builtin_scope_action() {
        let name = SourceName::new("appended-mdoc-closer.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt APPENDED-CLOSER 1\n.Os\n.Sh DESCRIPTION\n.Bo in brackets\n.Bc end\n.am Bc\n.Pq appended words\n..\n.Bo in brackets\n.Bc end\n",
            ))
            .unwrap();
        assert!(report.diagnostics.iter().all(|diagnostic| {
            diagnostic.code.as_str() == DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME
        }));
        let bracket_bodies = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bo"))
            .map(|body| {
                body.children()
                    .filter_map(crate::NodeRef::text)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(bracket_bodies, [["in brackets"], ["in brackets"]]);
        assert!(
            report
                .document
                .preorder()
                .any(|node| node.macro_name() == Some("Pq"))
        );
    }

    #[test]
    fn renamed_appended_mdoc_closer_keeps_scope_and_caller_provenance() {
        let name = SourceName::new("renamed-appended-mdoc-closer.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt RENAMED-APPENDED-CLOSER 1\n.Os\n.Sh NAME\n.Nm renamed-appended-closer\n.Nd package macro alias\n.Sh DESCRIPTION\n.rn Bc myBc\n.Bo first brackets\n.myBc\n.am myBc\n.Pq appended words\n..\n.Bo second brackets\n.myBc\n",
            ))
            .unwrap();
        assert_eq!(report.diagnostics.len(), 1, "{:#?}", report.diagnostics);
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            DiagnosticCode::INPUT_TRAILING_WHITESPACE
        );
        let diagnostic_position = report
            .document
            .source_position(report.diagnostics[0].primary.as_ref().unwrap())
            .unwrap();
        assert_eq!(
            (diagnostic_position.line, diagnostic_position.column),
            (15, 4)
        );

        let bracket_bodies = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bo"))
            .map(|body| {
                body.children()
                    .filter_map(crate::NodeRef::text)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(bracket_bodies, [["first brackets"], ["second brackets"]]);

        let appended_text = report
            .document
            .preorder()
            .find(|node| node.text() == Some("appended words"))
            .unwrap();
        let appended_position = report
            .document
            .source_position(appended_text.location().unwrap())
            .unwrap();
        assert_eq!((appended_position.line, appended_position.column), (15, 5));
    }

    #[test]
    fn m3_indirect_macro_definitions_expand_names_and_custom_terminators() {
        let name = SourceName::new("indirect-definition.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ds target delayed\n.ds end-marker done\n.dei target end-marker\nfirst\n.done trailing words\n.ami target end-marker\nsecond\n.done\n.delayed\n",
            ))
            .unwrap();

        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["first", "second"]);
    }

    #[test]
    fn m3_copy_mode_reparses_delayed_register_adjustments_on_invocation() {
        let name = SourceName::new("copy-register.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr count 2 1\n.de decrement\n\\\\n-[count]\n..\n.decrement\ncount \\n[count]\n",
            ))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(text, ["1", "count 1"]);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_while_brace_scope_reexecutes_controls_and_closes_inline_text() {
        let name = SourceName::new("while-scope.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr count 3\n.while \\n[count] \\{\\\n.nr count -1\n\\n[count]\\},\nafter\n",
            ))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(text, ["2,", "1,", "0,", "after"]);
        assert!(report.diagnostics.is_empty());
        assert!(!report.statistics.truncated);
    }

    #[test]
    fn m3_break_in_a_scoped_conditional_stops_only_the_current_while() {
        let name = SourceName::new("while-break.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr count 3 1\n.while n \\{\\\n\\n-[count]\n.if !\\n[count] .break\nnext\n.\\}\nafter\n",
            ))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(text, ["2", "next", "1", "next", "0", "after"]);
        assert!(report.diagnostics.is_empty());
        assert!(!report.statistics.truncated);
    }

    #[test]
    fn m3_nested_while_scopes_execute_on_an_explicit_frame_stack() {
        let name = SourceName::new("nested-while.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr outer 2\n.while \\n[outer] \\{\\\n.nr inner 2\n.while \\n[inner] \\{\\\n\\n[outer]:\\n[inner]\n.nr inner -1\n.\\}\n.nr outer -1\n.\\}\nafter\n",
            ))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(text, ["2:2", "2:1", "after"]);
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>(),
            [
                DiagnosticCode::ROFF_WHILE_NESTED,
                DiagnosticCode::ROFF_WHILE_CANNOT_CONTINUE,
            ]
        );
        assert!(!report.statistics.truncated);
    }

    #[test]
    fn m3_macro_body_can_close_the_active_while_scope() {
        let name = SourceName::new("while-macro-close.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr count 2\n.de close\n.nr count -1\n.\\}\n..\n.while \\n[count] \\{\\\n\\n[count]\n.close\ninside-never\n.\\}\nafter\n",
            ))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(text, ["2", "inside-never", "after"]);
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.severity))
                .collect::<Vec<_>>(),
            [
                (
                    DiagnosticCode::ROFF_WHILE_INNER_SCOPE,
                    Severity::Unsupported
                ),
                (
                    DiagnosticCode::ROFF_WHILE_OUT_OF_SCOPE,
                    Severity::Unsupported
                ),
            ]
        );
        assert!(!report.statistics.truncated);
    }

    #[test]
    fn m3_copy_mode_does_not_apply_control_changes_before_macro_invocation() {
        let name = SourceName::new("copy-control.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(&name, b".de delayed\n.cc !\n..\noutside\n"))
            .unwrap();
        let nodes = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .collect::<Vec<_>>();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].text(), Some("outside"));
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|finding| finding.code.as_str() == "roff.unterminated-definition")
        );
    }

    #[test]
    fn m3_macro_control_changes_activate_only_when_the_macro_runs() {
        let name = SourceName::new("copy-control-run.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de delayed\n.cc !\n!B generated\n..\n.delayed\n!TH title 1\n",
            ))
            .unwrap();
        let nodes = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .collect::<Vec<_>>();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].macro_name(), Some("B"));
        assert!(!nodes[0].flags().generated);
        assert_eq!(
            nodes[0].children().next().unwrap().text(),
            Some("generated")
        );
        assert_eq!(nodes[1].macro_name(), Some("TH"));
        assert_eq!(
            nodes[1]
                .children()
                .map(|node| node.text().unwrap().to_owned())
                .collect::<Vec<_>>(),
            ["title", "1"]
        );
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_macro_body_control_requests_become_generated_events() {
        let name = SourceName::new("macro-controls.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de show\n.ds prefix welcome\n.B \\$1\n..\n.show mantdoc\n\\*[prefix]\n",
            ))
            .unwrap();
        let nodes = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .collect::<Vec<_>>();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].macro_name(), Some("B"));
        assert!(!nodes[0].flags().generated);
        assert_eq!(nodes[0].children().next().unwrap().text(), Some("mantdoc"));
        assert_eq!(nodes[1].text(), Some("welcome"));
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_macro_generated_man_controls_use_the_invocation_control_column() {
        let name = SourceName::new("generated-man-control.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH GENERATED 1\n.de list\n.TP 6n\ntag\n..\n.list\ntext\n",
            ))
            .unwrap();
        let term = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("TP"))
            .expect("generated TP block");
        let position = report
            .document
            .source_position(term.location().expect("TP location"))
            .expect("TP source position");
        assert_eq!((position.line, position.column), (6, 2));
        let head = term.children().next().expect("TP head");
        let width = head.children().next().expect("TP width argument");
        let width_position = report
            .document
            .source_position(width.location().expect("width location"))
            .expect("width source position");
        assert_eq!((width_position.line, width_position.column), (6, 5));
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_macros_can_invoke_nested_macros_with_their_own_arguments() {
        let name = SourceName::new("nested-macros.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de inner\ninner: \\$1\n..\n.de outer\n.inner \\$1\n..\n.outer mantdoc\n",
            ))
            .unwrap();
        let nodes = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .collect::<Vec<_>>();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].text(), Some("inner: mantdoc"));
        assert!(!nodes[0].flags().generated);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_recursive_macros_reparse_delayed_register_and_argument_escapes() {
        let name = SourceName::new("recursive-macro.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de count\n. ie \\\\$1>0 \\{\\\n.  No \\\\$1\n.  nr next \\\\$1-1\n.  count \\\\n[next]\n. \\}\n..\n.count 3\n",
            ))
            .unwrap();
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["3", "2", "1"]);
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    }

    #[test]
    fn m3_macro_shift_return_and_argument_count_are_frame_local() {
        let name = SourceName::new("macro-control-flow.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de inner\ninner \\$1 \\n[.$]\n.return\ninner-never\n..\n.de outer\nouter-before \\$1 \\$2\n.shift\n.inner \\$1\nouter-after \\$1\n.return\nouter-never\n..\n.outer one two\n",
            ))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            text,
            ["outer-before one two", "inner two 1", "outer-after two"]
        );
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn shift_recovers_outside_calls_and_invalid_macro_selectors() {
        let name = SourceName::new("shift-recovery.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH SHIFT-RECOVERY 1 \"August 26, 2026\"\n.SH NAME\nshift-recovery - shift validation\n.SH DESCRIPTION\n.shift\n.de mym\nselector: \"\\\\$x\"\n.shift bad\nafter invalid: \"\\\\$1\"\n.shift 2\nafter excessive: \"\\\\$1\"\n..\n.mym one two\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.severity, diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                (Severity::Error, "ignoring request outside macro: shift"),
                (Severity::Error, "argument number is not numeric: \\$x"),
                (
                    Severity::Error,
                    "argument is not numeric, using 1: shift bad"
                ),
                (Severity::Error, "excessive shift: 2, but max is 1"),
            ]
        );
        let text = report
            .document
            .preorder()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert!(text.contains(&"after invalid: \"two\""), "{text:#?}");
        assert!(text.contains(&"after excessive: \"\""), "{text:#?}");
        assert!(!text.iter().any(|value| value.contains("$x")), "{text:#?}");
    }

    #[test]
    fn empty_while_scope_keeps_validator_order_and_logical_blank_location() {
        let name = SourceName::new("while-empty-scope.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt WHILE-EMPTY 1\n.Os\n.Sh NAME\n.Nm while-empty\n.Nd test\n.Sh DESCRIPTION\nbefore\n.nr cnt 2 1\n.while \\n-[cnt]\n\\n[cnt]\n.Pp\nfinal text\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "conditional request controls empty scope: while",
                "blank line in fill mode, using .sp",
                "conditional request controls empty scope: while",
            ]
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter_map(|diagnostic| diagnostic.primary.as_ref())
                .filter_map(|span| report.document.source_position(span))
                .map(|position| (position.line, position.column))
                .collect::<Vec<_>>(),
            [(10, 2), (10, 9), (10, 2)]
        );
    }

    #[test]
    fn roff_return_and_argument_escapes_outside_macros_are_errors() {
        let name = SourceName::new("return-outside.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(&name, b".return\noutside \\$1\n.return\n"))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                (
                    DiagnosticCode::ROFF_RETURN_OUTSIDE_MACRO,
                    "ignoring request outside macro: return",
                ),
                (
                    DiagnosticCode::ROFF_MACRO_ARGUMENT_OUTSIDE,
                    "using macro argument outside macro: \\$1",
                ),
                (
                    DiagnosticCode::ROFF_RETURN_OUTSIDE_MACRO,
                    "ignoring request outside macro: return",
                ),
            ]
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter_map(|diagnostic| diagnostic.primary.as_ref())
                .filter_map(|span| report.document.source_position(span))
                .map(|position| (position.line, position.column))
                .collect::<Vec<_>>(),
            [(1, 2), (2, 9), (3, 2)]
        );
        assert_eq!(
            report
                .document
                .preorder()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            ["outside "]
        );
    }

    #[test]
    fn m3_macro_depth_limit_returns_a_coherent_prefix() {
        let name = SourceName::new("macro-depth.roff").unwrap();
        let limits = Limits {
            max_macro_depth: 1,
            ..Limits::default()
        };
        let report = Parser::new(ParserConfig {
            limits,
            ..ParserConfig::default()
        })
        .parse(Source::new(
            &name,
            b".de second\nsecond\n..\n.de first\nfirst-text\n.second\n..\n.first\n",
        ))
        .unwrap();
        let nodes = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .collect::<Vec<_>>();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].text(), Some("first-text"));
        assert!(report.statistics.truncated);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|finding| finding.code.as_str() == "limits.macro-depth")
        );
    }

    #[test]
    fn m3_resolved_includes_preserve_order_source_maps_and_session_state() {
        let root = SourceName::new("root.roff").unwrap();
        let mut bundle = SourceBundle::default();
        bundle
            .insert(
                "part.roff",
                b"inside \\*[word]\n.ds word changed\n".to_vec(),
            )
            .unwrap();
        let report = Parser::default()
            .parse_with_resolver(
                Source::new(
                    &root,
                    b".ds word welcome\n.so part.roff\noutside \\*[word]\n",
                ),
                &mut bundle,
            )
            .unwrap();
        let nodes = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .collect::<Vec<_>>();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].text(), Some("inside welcome"));
        assert_eq!(nodes[1].text(), Some("outside changed"));
        assert_eq!(report.document.source_count(), 2);
        let child_span = nodes[0].location().unwrap();
        assert_eq!(
            report
                .document
                .source_name(child_span.source)
                .map(SourceName::as_str),
            Some("part.roff")
        );
        assert_eq!(report.statistics.source_files, 2);
        assert_eq!(
            report.statistics.source_bytes,
            b".ds word welcome\n.so part.roff\noutside \\*[word]\n".len()
                + b"inside \\*[word]\n.ds word changed\n".len()
        );
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_include_cycles_and_missing_targets_are_recoverable() {
        let root = SourceName::new("root.roff").unwrap();
        let mut bundle = SourceBundle::default();
        bundle.insert("root.roff", b"ignored".to_vec()).unwrap();
        bundle
            .insert("part.roff", b".so root.roff\n".to_vec())
            .unwrap();
        let cyclic = Parser::default()
            .parse_with_resolver(Source::new(&root, b".so part.roff\n"), &mut bundle)
            .unwrap();
        assert_eq!(cyclic.document.source_count(), 2);
        assert!(cyclic.statistics.truncated);
        assert!(
            cyclic
                .diagnostics
                .iter()
                .any(|finding| finding.code.as_str() == "roff.include-cycle")
        );

        let missing = Parser::default()
            .parse(Source::new(&root, b".so missing.roff\n"))
            .unwrap();
        assert_eq!(missing.document.node_count(), 1);
        assert!(
            missing
                .diagnostics
                .iter()
                .any(|finding| finding.code.as_str() == "roff.include-unavailable")
        );
    }

    #[test]
    fn m3_include_graph_limits_stop_before_source_map_mutation() {
        let root = SourceName::new("root.roff").unwrap();
        let limits = Limits {
            max_sources: 1,
            ..Limits::default()
        };
        let mut bundle = SourceBundle::new(limits.clone());
        bundle.insert("part.roff", b"child\n".to_vec()).unwrap();
        let report = Parser::new(ParserConfig {
            limits,
            ..ParserConfig::default()
        })
        .parse_with_resolver(Source::new(&root, b".so part.roff\n"), &mut bundle)
        .unwrap();
        assert_eq!(report.document.source_count(), 1);
        assert!(report.statistics.truncated);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|finding| finding.code.as_str() == "limits.sources")
        );
    }

    #[test]
    fn m3_include_depth_and_child_source_bounds_are_diagnostic_not_fatal() {
        let root = SourceName::new("root.roff").unwrap();
        let mut bundle = SourceBundle::default();
        bundle
            .insert("first.roff", b".so second.roff\n".to_vec())
            .unwrap();
        bundle.insert("second.roff", b"second\n".to_vec()).unwrap();
        let depth_limited = Parser::new(ParserConfig {
            limits: Limits {
                max_include_depth: 1,
                ..Limits::default()
            },
            ..ParserConfig::default()
        })
        .parse_with_resolver(Source::new(&root, b".so first.roff\n"), &mut bundle)
        .unwrap();
        assert_eq!(depth_limited.document.source_count(), 2);
        assert!(
            depth_limited
                .diagnostics
                .iter()
                .any(|finding| finding.code.as_str() == "limits.include-depth")
        );

        let mut bytes_bundle = SourceBundle::default();
        bytes_bundle
            .insert("large.roff", b"this child is too large\n".to_vec())
            .unwrap();
        let byte_limited = Parser::new(ParserConfig {
            limits: Limits {
                max_root_source_bytes: 16,
                max_total_source_bytes: 64,
                ..Limits::default()
            },
            ..ParserConfig::default()
        })
        .parse_with_resolver(Source::new(&root, b".so large.roff\n"), &mut bytes_bundle)
        .unwrap();
        assert_eq!(byte_limited.document.source_count(), 1);
        assert!(
            byte_limited
                .diagnostics
                .iter()
                .any(|finding| finding.code.as_str() == "limits.source-bytes")
        );
    }

    #[test]
    fn m3_include_diagnostics_share_the_session_budget() {
        let root = SourceName::new("root.roff").unwrap();
        let limits = Limits {
            max_diagnostics: 1,
            ..Limits::default()
        };
        let mut bundle = SourceBundle::new(limits.clone());
        bundle
            .insert("part.roff", b".so missing-a\n.so missing-b\n".to_vec())
            .unwrap();
        let report = Parser::new(ParserConfig {
            limits,
            ..ParserConfig::default()
        })
        .parse_with_resolver(Source::new(&root, b".so part.roff\n"), &mut bundle)
        .unwrap();
        assert_eq!(report.document.source_count(), 2);
        assert_eq!(report.diagnostics.len(), 1);
        assert!(report.statistics.truncated);
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            "roff.include-unavailable"
        );
    }

    #[test]
    fn m3_while_rechecks_register_conditions_and_updates_session_state() {
        let name = SourceName::new("while.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr count 0\n.while \\n[count]<3 .nr count +1\ncount \\n[count]\n",
            ))
            .unwrap();
        let nodes = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .collect::<Vec<_>>();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].text(), Some("count 3"));
        assert!(report.diagnostics.is_empty());
        assert!(!report.statistics.truncated);
    }

    #[test]
    fn m3_while_executes_a_copy_mode_macro_body_on_each_iteration() {
        let name = SourceName::new("while-macro.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr count 2 1\n.de decrement\n\\\\n-[count]\n..\n.while \\n[count] .decrement\ncount \\n[count]\n",
            ))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(text, ["1", "0", "count 0"]);
        assert!(report.diagnostics.is_empty());
        assert!(!report.statistics.truncated);
    }

    #[test]
    fn m3_active_inline_conditionals_execute_environment_requests() {
        let name = SourceName::new("conditional-request.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".if 1 .ds selected yes\n.if 0 .ds selected no\n\\*[selected]\n",
            ))
            .unwrap();
        let nodes = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .collect::<Vec<_>>();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].text(), Some("yes"));
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_while_aggregate_limit_stops_environment_updates() {
        let name = SourceName::new("while-limit.roff").unwrap();
        let limits = Limits {
            max_loop_iterations: 2,
            max_total_loop_iterations: 3,
            ..Limits::default()
        };
        let report = Parser::new(ParserConfig {
            limits,
            ..ParserConfig::default()
        })
        .parse(Source::new(
            &name,
            b".nr first 0\n.while \\n[first]<2 .nr first +1\n.nr second 0\n.while \\n[second]<2 .nr second +1\n",
        ))
        .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert!(text.is_empty());
        assert!(report.statistics.truncated);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|finding| finding.code.as_str() == "limits.total-loop-iterations")
        );
    }

    #[test]
    fn m3_while_per_loop_limit_returns_the_generated_prefix() {
        let name = SourceName::new("while-per-loop-limit.roff").unwrap();
        let limits = Limits {
            max_loop_iterations: 2,
            max_total_loop_iterations: 3,
            ..Limits::default()
        };
        let report = Parser::new(ParserConfig {
            limits,
            ..ParserConfig::default()
        })
        .parse(Source::new(&name, b".while 1 repeated\n"))
        .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(text, ["repeated", "repeated"]);
        assert!(report.statistics.truncated);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|finding| finding.code.as_str() == "limits.loop-iterations")
        );
    }

    #[test]
    fn m3_numeric_and_nroff_conditionals_choose_only_the_active_inline_branch() {
        let name = SourceName::new("conditionals.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr count 7\n.if 1 visible\n.if 0 hidden\n.if !0 inverted\n.if n nroff\n.if t troff\n.if \\n[count]>=7 registered\n.if \\n[count]!=7 wrong\n.ie 0 first\n.el second\n",
            ))
            .unwrap();
        let nodes = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            nodes,
            ["visible", "inverted", "nroff", "registered", "second"]
        );
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn number_registers_accept_whitespace_inside_parenthesized_values() {
        let name = SourceName::new("register-parenthesized-space.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr value 18\n.nr value ( 25 - 6 )\n\\n[value]\n",
            ))
            .unwrap();
        let text = report
            .document
            .preorder()
            .find_map(|node| node.text().map(str::to_owned));
        assert_eq!(text.as_deref(), Some("19"));
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn number_register_division_by_zero_recovers_to_zero_and_reports_the_request() {
        let name = SourceName::new("division-by-zero.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr quotient 1/0\n.nr remainder 1%0\n\\n[quotient] \\n[remainder]\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    (
                        diagnostic.code.as_str(),
                        diagnostic.severity,
                        diagnostic.message.as_ref(),
                    )
                })
                .collect::<Vec<_>>(),
            [
                (
                    DiagnosticCode::ROFF_DIVISION_BY_ZERO,
                    Severity::Error,
                    "divide by zero: 1/0",
                ),
                (
                    DiagnosticCode::ROFF_DIVISION_BY_ZERO,
                    Severity::Error,
                    "divide by zero: 1%0",
                ),
            ]
        );
        let positions = report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                let position = report
                    .document
                    .source_position(diagnostic.primary.as_ref().unwrap())
                    .unwrap();
                (position.line, position.column)
            })
            .collect::<Vec<_>>();
        assert_eq!(positions, [(1, 4), (2, 4)]);
        assert_eq!(
            report
                .document
                .node(report.document.root())
                .unwrap()
                .children()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            ["0 0"]
        );
        assert!(!report.statistics.truncated);
    }

    #[test]
    fn ignore_blocks_report_excess_markers_unmatched_ends_and_eof() {
        let name = SourceName::new("ignore-blocks.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ig end excess\nignored\n.end\n..\n.ig\nignored\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    (
                        diagnostic.code.as_str(),
                        diagnostic.severity,
                        diagnostic.message.as_ref(),
                    )
                })
                .collect::<Vec<_>>(),
            [
                (
                    DiagnosticCode::ROFF_EXCESS_ARGUMENTS,
                    Severity::Error,
                    "skipping excess arguments: .ig ... excess",
                ),
                (
                    DiagnosticCode::ROFF_UNMATCHED_END,
                    Severity::Error,
                    "skipping end of block that is not open: ..",
                ),
                (
                    DiagnosticCode::ROFF_UNCLOSED_IGNORE,
                    Severity::Error,
                    "appending missing end of block: ig",
                ),
            ]
        );
        let positions = report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                let position = report
                    .document
                    .source_position(diagnostic.primary.as_ref().unwrap())
                    .unwrap();
                (position.line, position.column)
            })
            .collect::<Vec<_>>();
        assert_eq!(positions, [(1, 5), (4, 2), (5, 2)]);
        assert!(
            report
                .document
                .preorder()
                .all(|node| node.text() != Some("ignored"))
        );
    }

    #[test]
    fn input_traps_require_a_numeric_prefix_without_replacing_the_existing_trap() {
        let name = SourceName::new("input-trap-arguments.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de trap\ntrapped\n..\n.it 2 trap\n.it trap\nfirst\nsecond\n.it\nthird\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    (
                        diagnostic.code.as_str(),
                        diagnostic.severity,
                        diagnostic.message.as_ref(),
                    )
                })
                .collect::<Vec<_>>(),
            [
                (
                    DiagnosticCode::ROFF_NON_NUMERIC_ARGUMENT,
                    Severity::Error,
                    "skipping request without numeric argument: it trap",
                ),
                (
                    DiagnosticCode::ROFF_NON_NUMERIC_ARGUMENT,
                    Severity::Error,
                    "skipping request without numeric argument: it",
                ),
            ]
        );
        let positions = report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                let position = report
                    .document
                    .source_position(diagnostic.primary.as_ref().unwrap())
                    .unwrap();
                (position.line, position.column)
            })
            .collect::<Vec<_>>();
        assert_eq!(positions, [(5, 2), (8, 2)]);
        assert_eq!(
            report
                .document
                .preorder()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            ["first", "second", "trapped", "third"]
        );
    }

    #[test]
    fn macro_rename_requires_a_space_after_the_old_name() {
        let name = SourceName::new("rename-tab.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de old\nold body\n..\n.rn old\tnew\n.new\n.old\n.rn old new\tignored\n.old\n.new\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .document
                .preorder()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            ["old body", "old body"]
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "skipping unknown macro: .new",
                "skipping unknown macro: .old",
            ]
        );
    }

    #[test]
    fn removed_user_macro_is_reported_without_hiding_removed_string_references() {
        let name = SourceName::new("remove-macro.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de old\nold body\n..\n.ds value text\n.rm old value\n.old\n\\*[value]\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "skipping unknown macro: .old",
                "undefined string, using \"\": value",
            ]
        );
        let positions = report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                let position = report
                    .document
                    .source_position(diagnostic.primary.as_ref().unwrap())
                    .unwrap();
                (position.line, position.column)
            })
            .collect::<Vec<_>>();
        assert_eq!(positions, [(6, 2), (7, 1)]);
        assert!(
            !report
                .document
                .preorder()
                .any(|node| node.macro_name() == Some("old"))
        );
    }

    #[test]
    fn user_macro_tabs_preserve_argument_prefixes_and_defer_validation() {
        let name = SourceName::new("macro-tabs.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH MACRO-TABS 1\n.SH DESCRIPTION\n.de show end ignored\nvalue \\\\$1;\\\\$2\n.end\n.show\t\ttwo\n.show\t\t\t three\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>(),
            [
                DiagnosticCode::ROFF_ALL_ARGUMENTS,
                DiagnosticCode::INPUT_TAB_IN_FILLED_TEXT,
                DiagnosticCode::INPUT_TAB_IN_FILLED_TEXT,
            ]
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter_map(|diagnostic| diagnostic.primary.as_ref())
                .filter_map(|span| report.document.source_position(span))
                .map(|position| (position.line, position.column))
                .collect::<Vec<_>>(),
            [(3, 5), (6, 8), (7, 8)]
        );
        let text = report
            .document
            .preorder()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert!(text.contains(&"value \ttwo;"));
        assert!(text.contains(&"value \t;three"));
    }

    #[test]
    fn register_request_names_reject_escaped_characters_but_keep_literal_backslashes() {
        let name = SourceName::new("register-escaped-name.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr first\\\\second 1\n.nr first\\esecond 2\n.rr first\\esecond\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                (
                    DiagnosticCode::ROFF_ESCAPED_NAME,
                    "escaped character not allowed in a name: first\\e",
                ),
                (
                    DiagnosticCode::ROFF_ESCAPED_NAME,
                    "escaped character not allowed in a name: first\\e",
                ),
            ]
        );
        let positions = report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                let position = report
                    .document
                    .source_position(diagnostic.primary.as_ref().unwrap())
                    .unwrap();
                (position.line, position.column)
            })
            .collect::<Vec<_>>();
        assert_eq!(positions, [(2, 5), (3, 5)]);
    }

    #[test]
    fn macro_names_recover_literal_and_prohibited_escapes_consistently() {
        let name = SourceName::new("macro-escaped-name.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de second\nsecond\n..\n.de first\\\\second\nliteral\n..\n.de first\\esecond\nfirst\n..\n.first\n.second\n.first\\\\second\n.rm first\\\\second first\\esecond second\n.first\n.second\n.first\\\\second\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .document
                .preorder()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            ["first", "second", "literal", "second"]
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                (
                    DiagnosticCode::ROFF_ESCAPED_NAME,
                    "escaped character not allowed in a name: first\\e",
                ),
                (
                    DiagnosticCode::ROFF_ESCAPED_NAME,
                    "escaped character not allowed in a name: first\\e",
                ),
                (
                    DiagnosticCode::ROFF_UNKNOWN_MACRO,
                    "skipping unknown macro: .first"
                ),
                (
                    DiagnosticCode::ROFF_UNKNOWN_MACRO,
                    "skipping unknown macro: .first\\\\second",
                ),
            ]
        );
    }

    #[test]
    fn unterminated_bracketed_register_reference_keeps_legacy_diagnostics() {
        let name = SourceName::new("register-unterminated.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH REGISTER 1\n.SH DESCRIPTION\nincomplete: \\n[second\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                (
                    DiagnosticCode::ESCAPE_INVALID,
                    "invalid escape sequence: \\n[second",
                ),
                (
                    DiagnosticCode::INPUT_TRAILING_WHITESPACE,
                    "whitespace at end of input line",
                ),
            ]
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter_map(|diagnostic| diagnostic.primary.as_ref())
                .filter_map(|span| report.document.source_position(span))
                .map(|position| (position.line, position.column))
                .collect::<Vec<_>>(),
            [(3, 13), (3, 12)]
        );
        assert!(
            report
                .document
                .preorder()
                .filter_map(crate::NodeRef::text)
                .any(|text| text == "incomplete:")
        );
    }

    #[test]
    fn unterminated_delimited_escape_keeps_the_authored_diagnostic_spelling() {
        let name = SourceName::new("unterminated-width.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH WIDTH 1\n.SH DESCRIPTION\nunterminated: \\w'foo\n",
            ))
            .unwrap();
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            (
                report.diagnostics[0].code.as_str(),
                report.diagnostics[0].message.as_ref(),
            ),
            (
                DiagnosticCode::ESCAPE_UNTERMINATED,
                "invalid escape sequence: \\w'foo",
            )
        );
    }

    #[test]
    fn ignored_escape_forms_keep_only_the_legacy_invalid_diagnostics() {
        let name = SourceName::new("ignored-escapes.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                br".TH ESC-IGNORE 1
.SH NAME
esc-ignore \- ignored roff escape sequences
.SH DESCRIPTION
.nf
closing parenthesis: a\)b\[)]c
comma: a\,b\[,]c
slash: a\/b\[/]c
multiform: a\kxb\k(xyc\k[xyz]d
quoted: a\R'myreg 0'b\R'myreg \A'y'0'c
sizes: a\s0b\s(12c\s[123]d\s'123'e\s'1\w'xy'2'f
signed sizes: a\s-0b\s-(12c\s-[123]d\s-'123'e\s-'1\w'xy'2'f\s-
",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "invalid escape sequence: \\[)]",
                "invalid escape sequence: \\[,]",
                "invalid escape sequence: \\[/]",
                "invalid escape sequence: \\s-",
            ]
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter_map(|diagnostic| diagnostic.primary.as_ref())
                .filter_map(|span| report.document.source_position(span))
                .map(|position| (position.line, position.column))
                .collect::<Vec<_>>(),
            [(6, 26), (7, 12), (8, 12), (12, 60)]
        );
    }

    #[test]
    fn invalid_bracket_escapes_are_reported_before_their_raw_form() {
        let name = SourceName::new("invalid-escapes.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                br".TH ESC-INVALID 1
.SH NAME
esc-invalid \- invalid roff escape sequences
.SH DESCRIPTION
.nf
plus: a\+b\[+]c
unicode: a\Ub\[U]c
",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "invalid escape sequence: \\[+]",
                "undefined escape, printing literally: \\+",
                "invalid escape sequence: \\[U]",
                "undefined escape, printing literally: \\U",
            ]
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter_map(|diagnostic| diagnostic.primary.as_ref())
                .filter_map(|span| report.document.source_position(span))
                .map(|position| (position.line, position.column))
                .collect::<Vec<_>>(),
            [(6, 11), (6, 8), (7, 14), (7, 11)]
        );
    }

    #[test]
    fn m3_inline_conditional_body_keeps_its_authored_provenance_and_offset() {
        let name = SourceName::new("conditional-location.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(&name, b".ie 1 body\n"))
            .unwrap();
        let node = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .next()
            .unwrap();
        assert_eq!(node.text(), Some("body"));
        assert!(!node.flags().generated);
        let position = report
            .document
            .source_position(node.location().unwrap())
            .unwrap();
        assert_eq!((position.line, position.column), (1, 7));
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_register_defined_conditionals_track_rr_without_creating_registers() {
        let name = SourceName::new("register-condition.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ie rstate unexpected\n.el absent\n.nr state 1\n.ie rstate present\n.el unexpected\n.rr state\n.ie rstate unexpected\n.el removed\n",
            ))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(text, ["absent", "present", "removed"]);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn roff_register_conditionals_keep_the_legacy_name_and_tab_diagnostics() {
        let name = SourceName::new("register-condition-diagnostics.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH REGISTER 1\n.SH DESCRIPTION\n.ie rknown\tvisible\n.el hidden\n.nr known 0\n.ie rknown\\(enignored\n.el hidden\n",
            ))
            .unwrap();
        assert_eq!(report.diagnostics.len(), 2, "{:#?}", report.diagnostics);
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            DiagnosticCode::ROFF_ESCAPED_NAME
        );
        assert_eq!(
            report.diagnostics[0].message.as_ref(),
            "escaped character not allowed in a name: known\\("
        );
        assert_eq!(
            report.diagnostics[1].code.as_str(),
            DiagnosticCode::INPUT_TAB_IN_FILLED_TEXT
        );
        assert_eq!(report.diagnostics[1].message.as_ref(), "tab in filled text");
        let positions = report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                let position = report
                    .document
                    .source_position(diagnostic.primary.as_ref().unwrap())
                    .unwrap();
                (position.line, position.column)
            })
            .collect::<Vec<_>>();
        assert_eq!(positions, [(6, 6), (3, 11)]);
        let text = report
            .document
            .preorder()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert!(text.contains(&"hidden"), "{text:#?}");
        assert!(text.contains(&"\\(enignored"), "{text:#?}");
    }

    #[test]
    fn roff_renamed_man_macro_remains_defined_for_a_d_condition() {
        let name = SourceName::new("renamed-man-macro-condition.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH RENAMED 1\n.SH DESCRIPTION\n.rn SM renamed\n.ie drenamed visible\n.el hidden\n",
            ))
            .unwrap();
        let text = report
            .document
            .preorder()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert!(text.contains(&"visible"), "{text:#?}");
        assert!(!text.contains(&"hidden"), "{text:#?}");
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn undefined_string_and_conditioned_macro_recover_as_roff_state() {
        let name = SourceName::new("undefined-name-state.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH UNDEFINED-NAME-STATE 1 \"August 26, 2026\"\n.SH NAME\nundefined-name-state - roff state\n.SH DESCRIPTION\nfirst: \"\\*[missing]\"\n.ie dmissing string-defined\n.el string-undefined\n.ie dunknown macro-defined\n.el macro-undefined\n.unknown\n.ie dunknown macro-defined-after\n.el macro-undefined-after\n.rn BR newBR\n.newBR works\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.severity, diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                (Severity::Warning, "undefined string, using \"\": missing"),
                (Severity::Error, "skipping unknown macro: .unknown"),
            ]
        );
        let text = report
            .document
            .preorder()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert!(text.contains(&"string-defined"), "{text:#?}");
        assert!(text.contains(&"macro-undefined"), "{text:#?}");
        assert!(text.contains(&"macro-defined-after"), "{text:#?}");
        let renamed_argument = report
            .document
            .preorder()
            .find(|node| node.text() == Some("works"))
            .unwrap();
        let position = report
            .document
            .source_position(renamed_argument.location().unwrap())
            .unwrap();
        assert_eq!((position.line, position.column), (14, 5));
    }

    #[test]
    fn man_unknown_roff_font_is_removed_at_the_request_and_reports_its_macro() {
        let name = SourceName::new("unknown-man-font.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH FONT 1\n.SH DESCRIPTION\n.ft foo\nvisible\n",
            ))
            .unwrap();
        assert_eq!(report.diagnostics.len(), 1, "{:#?}", report.diagnostics);
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            DiagnosticCode::ROFF_UNKNOWN_FONT
        );
        assert_eq!(
            report.diagnostics[0].message.as_ref(),
            "unknown font, skipping request: ft foo"
        );
        let position = report
            .document
            .source_position(report.diagnostics[0].primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((position.line, position.column), (3, 2));
        assert!(
            !report
                .document
                .preorder()
                .any(|node| node.macro_name() == Some("ft"))
        );
    }

    #[test]
    fn man_roff_font_request_keeps_only_its_first_selector() {
        let name = SourceName::new("man-font-selector.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH FONT 1\n.SH DESCRIPTION\n.ft I surplus\n.ft\nvisible\n",
            ))
            .unwrap();
        let font = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("ft"))
            .unwrap();
        assert_eq!(
            font.children()
                .map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            [Some("I")]
        );
        assert_eq!(report.diagnostics.len(), 1, "{:#?}", report.diagnostics);
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            DiagnosticCode::ROFF_EXCESS_ARGUMENTS
        );
        let default_font = report
            .document
            .preorder()
            .filter(|node| node.macro_name() == Some("ft"))
            .nth(1)
            .unwrap();
        assert_eq!(
            default_font
                .children()
                .map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            [Some("P")]
        );
    }

    #[test]
    fn m3_string_and_macro_defined_conditionals_accept_the_two_token_form() {
        let name = SourceName::new("defined-condition.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ie d phrase unexpected\n.el absent\n.ds phrase value\n.ie d phrase string\n.el unexpected\n.if !d phrase unexpected\n.de macro\nbody\n..\n.ie d macro macro\n.el unexpected\n.ie d PP builtin\n.el unexpected\n",
            ))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(text, ["absent", "string", "macro", "builtin"]);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_delimited_string_conditions_handle_match_mismatch_and_malformed_input() {
        let name = SourceName::new("string-compare.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ie \"\"\" empty\n.el unexpected\n.ie xabcxabcx equal\n.el unexpected\n.ie xabcxabdx unexpected\n.el mismatch\n.ie xabc unexpected\n.el malformed\n.ie !xabcxabcx unexpected\n.el negated\n",
            ))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(text, ["empty", "equal", "mismatch", "malformed", "negated"]);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_numeric_conditions_compare_physical_units_and_boolean_operators() {
        let name = SourceName::new("numeric-condition.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ie 42 positive\n.el unexpected\n.ie !42 unexpected\n.el negated\n.ie -42 unexpected\n.el negative\n.ie !-42 negated-negative\n.el unexpected\n.ie 42=bad unexpected\n.el incomplete\n.ie 1&1 both\n.el unexpected\n.ie 1&0 unexpected\n.el and-false\n.ie 0:1 either\n.el unexpected\n.ie 1i>2c physical\n.el unexpected\n.ie 1i-6P unexpected\n.el zero\n.ie ( unexpected\n.el bare-open\n.ie !( unexpected\n.el negated-bare-open\n.ie (1 open\n.el unexpected\n.ie !(0 negated-open\n.el unexpected\n",
            ))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            text,
            [
                "positive",
                "negated",
                "negative",
                "negated-negative",
                "incomplete",
                "both",
                "and-false",
                "either",
                "physical",
                "zero",
                "bare-open",
                "negated-bare-open",
                "open",
                "negated-open",
            ]
        );
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_multiline_conditional_scopes_use_the_explicit_execution_stack() {
        let name = SourceName::new("conditional-scope.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".if n \\{\\\nouter\n.if t \\{\\\nhidden\n.\\}\n.if n \\{\\\ninner\n.\\}\n.\\}\n.if t \\{\\\nskipped\n.\\}\n.ie n \\{\\\ntrue-branch\n.\\}\n.el \\{\\\nwrong-branch\n.\\}\n.ie t \\{\\\nwrong-branch\n.\\}\n.el \\{\\\nelse-branch\n.\\}\n",
            ))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(text, ["outer", "inner", "true-branch", "else-branch"]);
        let outer = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .next()
            .unwrap();
        let position = report
            .document
            .source_position(outer.location().unwrap())
            .unwrap();
        assert_eq!((position.line, position.column), (2, 9));
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_multiline_while_scope_preserves_its_opener_column() {
        let name = SourceName::new("while-scope.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr count 1\n.while \\n[count] \\{\\\nbody\n.nr count 0\n.\\}\n",
            ))
            .unwrap();
        let node = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .next()
            .unwrap();
        assert_eq!(node.text(), Some("body"));
        let position = report
            .document
            .source_position(node.location().unwrap())
            .unwrap();
        assert_eq!((position.line, position.column), (3, 20));
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_continue_skips_to_the_nearest_explicit_loop_frame() {
        let name = SourceName::new("continue.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr remaining 3\n.while \\n[remaining] \\{\\\n.nr remaining -1\n.if \\n[remaining]=1 \\{\\\n.continue\n.\\}\nkept \\n[remaining]\n.\\}\n",
            ))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(text, ["kept 2", "kept 0"]);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_empty_ie_predicates_consume_their_next_line_before_selecting_else() {
        let name = SourceName::new("empty-ie.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ie\ntext-after-empty\n.el empty-else\n.ie !\ntext-after-negated-empty\n.el negated-empty-else\n",
            ))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(text, ["empty-else", "negated-empty-else"]);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_bare_ie_leaves_an_immediate_else_as_its_paired_branch() {
        let name = SourceName::new("bare-ie-else.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(&name, b".ie 0\n.el selected\n"))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(text, ["selected"]);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_conditional_text_preserves_literal_escape_before_a_brace() {
        let name = SourceName::new("ie-literal-brace.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ie n If \\&.el\\e{ works, nothing follows here:\n.el\\{dummy\nBOOHOO\\}\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["If .el\\{ works, nothing follows here:"]);
    }

    #[test]
    fn m3_conditional_scope_closes_after_a_control_request() {
        let name = SourceName::new("ie-control-closer.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ie n \\{\\\nactive branch\n.br\\}\n.el \\{\\\ninactive branch\n.br\\}\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["active branch"]);
    }

    #[cfg(feature = "render")]
    #[test]
    fn conditional_scope_closer_suffix_keeps_terminal_inline_provenance() {
        let name = SourceName::new("conditional-scope-suffix.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CONDITIONAL 1\n.SH DESCRIPTION\npreceding words\n.if n \\{text line block end\n\\} with additional words\nfollowing words\n",
            ))
            .unwrap();
        let suffix = report
            .document
            .preorder()
            .find(|node| {
                node.text()
                    .is_some_and(|text| text.contains("additional words"))
            })
            .expect("scope-closer suffix must remain visible");
        assert!(suffix.terminal_inline_conditional());
    }

    #[test]
    fn m3_control_scope_closer_discards_following_text() {
        let name = SourceName::new("control-scope-closer-suffix.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CONDITIONAL 1\n.SH DESCRIPTION\n.if n \\{\\\nfirst line\n.\\}suffix must not print\n",
            ))
            .unwrap();
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["DESCRIPTION", "first line"]);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_nested_text_closers_remain_in_the_active_inner_scope() {
        let name = SourceName::new("nested-text-closers.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CONDITIONAL 1\n.SH DESCRIPTION\n.if n \\{outer\n.if n \\{inner\non\\} the\\} same\nafter\n",
            ))
            .unwrap();
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(
            visible,
            [
                "DESCRIPTION",
                "outer",
                "inner",
                "on\\& the\\& same",
                "after"
            ]
        );
    }

    #[test]
    fn m3_attached_font_scope_closers_keep_font_arguments_and_diagnostic() {
        let name = SourceName::new("attached-font-closers.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CONDITIONAL 1\n.SH DESCRIPTION\n.if n \\{outer\n.if n \\{inner\n.BR\\}on\\}the same\nafter\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .first()
                .map(|diagnostic| (diagnostic.severity, diagnostic.message.as_ref())),
            Some((
                Severity::Error,
                "escaped character not allowed in a name: BR\\&"
            ))
        );
        let macro_node = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("BR"))
            .unwrap();
        assert_eq!(
            macro_node
                .children()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            ["on\\&the", "same"]
        );
    }

    #[test]
    fn m3_unterminated_conditional_scope_reports_its_opener_and_executes_prefix() {
        let name = SourceName::new("unterminated-condition.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CONDITIONAL 1\n.SH DESCRIPTION\n.if n \\{\nstill open\n",
            ))
            .unwrap();
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code.as_str() == DiagnosticCode::ROFF_UNTERMINATED_SCOPE)
            .unwrap();
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(
            diagnostic.message.as_ref(),
            "appending missing end of block: if"
        );
        assert!(
            report
                .document
                .preorder()
                .any(|node| node.text() == Some("still open"))
        );
    }

    #[test]
    fn m3_nonstandard_brace_scopes_retain_the_same_line_body_at_every_depth() {
        let name = SourceName::new("nonstandard-brace-scopes.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".if 1 \\{\\\nouter\n.if 1 \\{inner\n\\}\n.\\}\n.nr count 1\n.while \\n[count] \\{first\n.nr count -1\n\\}\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["outer", "inner", "first"]);
    }

    #[test]
    fn m3_nested_scope_closers_share_a_control_line_without_leaking_frames() {
        let name = SourceName::new("nested-control-closers.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".if 1 \\{outer\n.if 1 \\{inner\n.\\}middle\\}end\nafter\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["outer", "inner", "after"]);
    }

    #[test]
    fn m3_nested_ie_else_scopes_keep_the_eligible_branch_in_the_same_frame() {
        let name = SourceName::new("nested-ie-else-scopes.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".if 1 \\{\\\n.ie 0 \\{\\\ninactive\n.\\}\n.el \\{\\\nactive\n.\\}\n.\\}\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["active"]);
    }

    #[test]
    fn m3_collected_scopes_define_direct_and_indirect_copy_mode_macros() {
        let name = SourceName::new("scope-copy-mode-definition.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ds indirect appended\n.de direct\nfirst\n..\n.if 1 \\{\\\n.am direct\nsecond\n..\n.dei indirect\nthird\n..\n.de custom finish\ncustom marker\n.finish\n.\\}\n.direct\n.appended\n.custom\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["first", "second", "third", "custom marker"]);
    }

    #[test]
    fn m3_conditional_macro_definitions_discard_terminator_tails_and_inactive_definitions() {
        let name = SourceName::new("conditional-definition-tails.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CONDITIONAL-DEFINITION 1\n.SH DESCRIPTION\n.if n \\{.de first\nfirst content\n.. \\}\n.if n \\{.de second\nsecond content\n.. \\}ignored\n.if t \\{.de suppressed\nnot visible\n.. \\}ignored\ninitial text\n.first\n.second\n.suppressed\nfinal text\n",
            ))
            .unwrap();
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(
            visible,
            [
                "DESCRIPTION",
                "initial text",
                "first content",
                "second content",
                "final text"
            ]
        );
        assert_eq!(report.diagnostics.len(), 2, "{:#?}", report.diagnostics);
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            DiagnosticCode::ROFF_ALL_ARGUMENTS
        );
        assert_eq!(
            report.diagnostics[1].code.as_str(),
            DiagnosticCode::ROFF_UNKNOWN_MACRO
        );
    }

    #[test]
    fn m3_collected_scope_definitions_preserve_nested_ie_else_copy_mode() {
        let name = SourceName::new("scope-copy-mode-nested-ie.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".if 1 \\{\\\n.de emit\n.ie 0 \\{\\\nskipped\n.\\}\n.el \\{\\\nselected\n.\\}\n..\n.\\}\n.emit\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["selected"]);
    }

    #[test]
    fn m3_inline_ie_else_inside_a_loop_scope_selects_only_the_eligible_body() {
        let name = SourceName::new("inline-ie-else-in-loop.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr count 1\n.while \\n[count] \\{\\\n.ie 0 skipped\n.el kept\n.nr count -1\n.\\}\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["kept"]);
    }

    #[test]
    fn m3_inline_if_inside_a_loop_scope_dispatches_a_macro_body() {
        let name = SourceName::new("inline-if-macro-in-loop.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de emit\nfrom macro\n..\n.nr count 1\n.while \\n[count] \\{\\\n.if 1 .emit\n.nr count -1\n.\\}\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["from macro"]);
    }

    #[test]
    fn m3_top_level_inline_if_dispatches_a_macro_body() {
        let name = SourceName::new("inline-if-macro.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de emit\nfrom macro: \\$1\n..\n.if n .emit argument\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["from macro: argument"]);
    }

    #[test]
    fn m3_inline_if_inside_a_loop_scope_dispatches_translation_requests() {
        let name = SourceName::new("inline-if-translation-in-loop.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr count 1\n.while \\n[count] \\{\\\n.if 1 .tr xy\nx\n.nr count -1\n.\\}\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["y"]);
    }

    #[test]
    fn m3_collected_scopes_reclassify_requests_after_a_dynamic_control_change() {
        let name = SourceName::new("scope-dynamic-control.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".if 1 \\{\\\n.cc !\n!ds word dynamic\n!cc .\n\\*[word]\n.\\}\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["dynamic"]);
    }

    #[test]
    fn m3_inactive_collected_scopes_do_not_leak_dynamic_control_changes() {
        let name = SourceName::new("inactive-scope-dynamic-control.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".if 0 \\{\\\n.cc !\n!ds word hidden\n.\\}\n.ds word outside\n\\*[word]\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["outside"]);
    }

    #[test]
    fn m3_collected_scopes_close_with_a_delayed_escape_character() {
        let name = SourceName::new("scope-dynamic-escape.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".if 1 \\{\\\n.ec @\n@}\n.ds word after\n@*[word]\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["after"]);
    }

    #[test]
    fn m3_scope_macros_execute_their_own_while_brace_frames() {
        let name = SourceName::new("scope-macro-while.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de emit\n.nr count 1\n.while \\n[count] \\{\\\ninside\n.nr count -1\n.\\}\n..\n.if 1 \\{\\\n.emit\n.\\}\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["inside"]);
    }

    #[test]
    fn m3_scope_macro_while_frames_share_the_session_loop_budget() {
        let name = SourceName::new("scope-macro-while-limit.roff").unwrap();
        let report = Parser::new(ParserConfig {
            limits: Limits {
                max_loop_iterations: 2,
                max_total_loop_iterations: 2,
                ..Limits::default()
            },
            ..ParserConfig::default()
        })
        .parse(Source::new(
            &name,
            b".de emit\n.nr count 3\n.while \\n[count] \\{\\\ninside\n.nr count -1\n.\\}\n..\n.if 1 \\{\\\n.emit\n.\\}\n",
        ))
        .unwrap();
        assert!(report.statistics.truncated);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|finding| finding.code.as_str() == "limits.loop-iterations")
        );
        let visible = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(visible, ["inside", "inside"]);
    }

    #[test]
    fn m3_parallel_sessions_do_not_share_delayed_environment_definitions() {
        let workers = ["alpha", "beta", "gamma", "delta"]
            .into_iter()
            .map(|word| {
                std::thread::spawn(move || {
                    let name = SourceName::new(format!("{word}.roff")).unwrap();
                    let source = format!(".ds word {word}\n\\*[word]\n");
                    let report = Parser::default()
                        .parse(Source::new(&name, source.as_bytes()))
                        .unwrap();
                    report
                        .document
                        .preorder()
                        .filter(|node| node.kind() == NodeKind::Text)
                        .filter_map(crate::NodeRef::text)
                        .map(ToOwned::to_owned)
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        let observed = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            observed,
            [
                vec!["alpha".to_owned()],
                vec!["beta".to_owned()],
                vec!["gamma".to_owned()],
                vec!["delta".to_owned()]
            ]
        );
    }

    #[test]
    fn m3_tr_translates_visible_text_without_rewriting_escape_spellings() {
        let name = SourceName::new("translation.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".tr xy\nx \\(em\n.tr z\nz\n.tr \\(emw\n\\(em\n",
            ))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(text, ["y —", " ", "w"]);
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [(
                DiagnosticCode::ROFF_ODD_TRANSLATION,
                "odd number of characters in request: tr z"
            )]
        );
    }

    #[test]
    fn m3_tr_inside_a_loop_scope_affects_later_scope_text() {
        let name = SourceName::new("scope-translation.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".while 1 \\{\\\n.tr xy\nx\n.break\n.\\}\n",
            ))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(text, ["y"]);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_tr_inside_a_scope_macro_affects_later_macro_text() {
        let name = SourceName::new("macro-translation.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de translate\n.tr xy\nx\n..\n.while 1 \\{\\\n.translate\n.break\n.\\}\n",
            ))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(text, ["y"]);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_tr_inside_a_top_level_macro_affects_later_macro_text() {
        let name = SourceName::new("top-level-macro-translation.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de translate\n.tr xy\nx\n..\n.translate\n",
            ))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(text, ["y"]);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_macro_control_accepts_horizontal_space_after_the_control_character() {
        let name = SourceName::new("macro-control-space.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr count 0\n.de increment\n.  nr count +1\n..\n.increment\n.if \\n[count]=1 updated\n",
            ))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(text, ["updated"]);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn m3_macro_opened_while_consumes_and_replays_following_physical_scope() {
        let name = SourceName::new("macro-opened-while.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr count 2\n.de loop\n. while \\\\n[count] \\{\\\n..\n.loop\nvalue \\n[count]\n. nr count -1\n.\\}\n",
            ))
            .unwrap();
        let text = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(text, ["value 2"]);
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>(),
            [
                DiagnosticCode::ROFF_WHILE_OUT_OF_SCOPE,
                DiagnosticCode::ROFF_WHILE_CANNOT_CONTINUE,
            ]
        );
    }

    #[test]
    fn retained_comments_are_not_visible_line_start_nodes() {
        let name = SourceName::new("comment-flags.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(&name, b".\\\" source comment\nvisible text\n"))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let comment = nodes
            .iter()
            .find(|node| node.kind() == NodeKind::Comment)
            .unwrap();
        assert!(!comment.flags().no_print);
        assert!(!comment.flags().line_start);
        let text = nodes
            .iter()
            .find(|node| node.kind() == NodeKind::Text)
            .unwrap();
        assert!(text.flags().line_start);
    }

    #[test]
    fn escaped_comment_control_is_skipped_with_a_style_diagnostic() {
        let name = SourceName::new("escaped-comment-control.roff").unwrap();
        let report = Parser::default()
            .parse(Source::new(&name, b"\\.\"\n"))
            .unwrap();
        assert!(
            report
                .document
                .preorder()
                .all(|node| node.kind() != NodeKind::Text)
        );
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code.as_str() == DiagnosticCode::INPUT_BAD_COMMENT_STYLE)
            .unwrap();
        assert_eq!(diagnostic.severity, Severity::Style);
        let position = diagnostic
            .primary
            .as_ref()
            .and_then(|span| report.document.source_position(span))
            .unwrap();
        assert_eq!((position.line, position.column), (1, 3));
    }

    #[test]
    fn physical_line_continuation_keeps_quoted_control_arguments_together() {
        let name = SourceName::new("continued-ip.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CONTINUED 1\n.SH DESCRIPTION\n.IP \"a long \\\ncontinued \\\nterm\" 4n\nbody\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty());
        let head = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("IP"))
            .unwrap();
        assert_eq!(
            head.children().next().and_then(crate::NodeRef::text),
            Some("a long continued term")
        );
    }

    #[test]
    fn terminal_package_macro_continuation_retains_completed_arguments() {
        let name = SourceName::new("terminal-continued.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH TERMINAL 1\n.SH DESCRIPTION\n.IB one two\\",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let element = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("IB"))
            .unwrap();
        assert_eq!(
            element
                .children()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            ["one", "two"]
        );
    }

    #[test]
    fn mdoc_package_quote_recovery_keeps_the_argument_and_orders_its_tail_warning() {
        let name = SourceName::new("mdoc-quote-recovery.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt QUOTE 1\n.Os\n.Sh NAME\n.Nm quote\n.Nd recovery\n.Fl \"one \n",
            ))
            .unwrap();
        let element = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("Fl"))
            .unwrap();
        assert_eq!(
            element
                .children()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            ["one "]
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>(),
            [
                DiagnosticCode::ARGUMENT_UNTERMINATED_QUOTE,
                DiagnosticCode::INPUT_TRAILING_WHITESPACE,
            ]
        );
    }

    #[test]
    fn man_next_line_conditions_materialize_a_vertical_boundary() {
        let name = SourceName::new("man-condition-boundaries.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CONDITIONAL 1\n.SH DESCRIPTION\n.if n First sentence.\n.if n\nSecond sentence.\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let first = nodes
            .iter()
            .position(|node| node.text() == Some("First sentence."))
            .unwrap();
        assert_eq!(nodes[first + 1].macro_name(), Some("sp"));
        assert_eq!(nodes[first + 2].text(), Some("Second sentence."));
    }

    #[test]
    fn escaped_deferred_references_do_not_become_public_warnings() {
        let name = SourceName::new("deferred-reference.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH DEFERRED 1\n.SH DESCRIPTION\n.ds value used\n.IB prefix ##\\\\*[value]## suffix\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn legacy_unicode_escape_uses_the_legacy_public_diagnostic_message() {
        let name = SourceName::new("legacy-unicode.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH LEGACY-UNICODE 1\n.SH DESCRIPTION\naccent: e\\U'0301'\n",
            ))
            .unwrap();
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            "escape.unsupported-unicode"
        );
        assert_eq!(
            report.diagnostics[0].message.as_ref(),
            "undefined escape, printing literally: \\U"
        );
    }

    #[test]
    fn bracketed_accent_spelling_preserves_legacy_invalid_escape_findings() {
        let name = SourceName::new("invalid-bracket-accent.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH INVALID-BRACKET-ACCENT 1\n.SH DESCRIPTION\nacute e\\[']e\ngrave e\\[`]e\n",
            ))
            .unwrap();
        assert_eq!(report.diagnostics.len(), 2);
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "invalid escape sequence: \\[']",
                "invalid escape sequence: \\[`]"
            ]
        );
    }

    #[test]
    fn bracketed_whitespace_controls_keep_legacy_invalid_escape_findings() {
        let name = SourceName::new("invalid-bracket-whitespace.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH INVALID-BRACKET-WHITESPACE 1\n.SH DESCRIPTION\nblank a\\[ hy]b\npercent a\\[%]b\nampersand a\\[&]b\ncolon a\\[:]b\ncaret a\\[^]b\nunderline a\\[_]b\npipe a\\[|]b\ntilde a\\[~]b\ndigit a\\[0]b\n",
            ))
            .unwrap();
        assert_eq!(report.diagnostics.len(), 9);
        assert!(
            report
                .diagnostics
                .iter()
                .all(|diagnostic| { diagnostic.code.as_str() == DiagnosticCode::ESCAPE_INVALID })
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "invalid escape sequence: \\[",
                "invalid escape sequence: \\[%]",
                "invalid escape sequence: \\[&]",
                "invalid escape sequence: \\[:]",
                "invalid escape sequence: \\[^]",
                "invalid escape sequence: \\[_]",
                "invalid escape sequence: \\[|]",
                "invalid escape sequence: \\[~]",
                "invalid escape sequence: \\[0]",
            ]
        );
    }

    #[test]
    fn invalid_bracketed_unicode_scalar_keeps_the_authored_spelling() {
        let name = SourceName::new("invalid-unicode-scalar.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH INVALID-UNICODE-SCALAR 1\n.SH DESCRIPTION\ntext \\[uD800]\n",
            ))
            .unwrap();
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            DiagnosticCode::ESCAPE_UNSUPPORTED_UNICODE
        );
        assert_eq!(
            report.diagnostics[0].message.as_ref(),
            "invalid escape sequence: \\[uD800]"
        );
    }

    #[test]
    fn malformed_unicode_escape_diagnostics_use_legacy_order_and_position() {
        let name = SourceName::new("invalid-unicode-shape.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH INVALID-UNICODE-SHAPE 1\n.SH DESCRIPTION\ntext \\[u2B].\\[u02B]\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "invalid escape sequence: \\[u02B]",
                "invalid escape sequence: \\[u2B]",
            ]
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter_map(|diagnostic| diagnostic.primary.as_ref())
                .filter_map(|span| report.document.source_position(span))
                .map(|position| position.column)
                .collect::<Vec<_>>(),
            [13, 6]
        );
    }

    #[test]
    fn zero_width_escape_retains_its_following_no_space_escape_in_package_ast() {
        let name = SourceName::new("zero-width-escape.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH ZERO 1\n.SH DESCRIPTION\nzero width: \\z\\c\nfollowing line\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty());
        let text = report
            .document
            .preorder()
            .find(|node| node.text() == Some("zero width: \\z\\c"))
            .unwrap();
        assert!(text.flags().line_continuation);
    }
}
