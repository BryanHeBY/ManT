//! Parser configuration and the M1 byte-safe session boundary.

use crate::ast::DocumentBuilder;
use crate::{
    Diagnostic, DiagnosticCode, IncludeRequest, Limits, MacroSet, NodeFlags, NodeId, NodeKind,
    Severity, Source, SourcePosition, SourceResolver, SourceSpan,
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
        let mut session = ParseSession::new(
            &self.config,
            &mut builder,
            &mut environment,
            &mut active_sources,
            resolver,
        );
        let mut outcome = SourceMachine::new(
            source,
            DocumentBuilder::root_source(),
            0,
            &mut session,
            ScanOutcome::root(source.bytes.len(), root_source_has_mdoc_os),
        )
        .run();
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

mod condition;
mod diagnostics;
mod driver;
mod emit;
mod event;
mod execution;
mod report;
mod request;
mod runtime;
mod session;
use condition::{
    BranchOutcome, condition_body_source_start_from_offset, condition_body_template,
    condition_body_template_from_offset, condition_parts, emit_escaped_condition_name,
    emit_escaped_request_name, evaluate_condition, lex_condition_arguments,
    macro_conditional_body_origin, macro_scope_body_origin, split_escaped_condition_body,
};
use diagnostics::{
    apply_man_structure_outcome, apply_mdoc_structure_outcome, apply_preprocess_outcome,
    apply_tree_depth_limit, reorder_deferred_post_validation_diagnostics,
};
use emit::{
    append_node, append_text_node, append_textual_node, contains_valid_utf8_non_ascii,
    emit_bad_comment_style, emit_escape_issues, emit_filled_macro_argument_tabs,
    emit_filled_text_tabs, emit_font_request_diagnostics, emit_invalid_input_bytes,
    emit_man_alternating_font_trailing_whitespace, emit_mdoc_control_trailing_whitespace,
    emit_mdoc_empty_display, emit_mdoc_implicit_trailing_delimiter_spacing,
    emit_trailing_whitespace, emit_translation_request_diagnostics,
    emit_unterminated_quoted_argument, emit_user_macro_leading_tabs,
    has_physical_line_continuation, is_bad_comment_style, is_builtin_package_macro,
    is_legacy_roff_font_selector, is_man_visible_argument_macro, legacy_table_input_text,
    normalize_document_escapes, recover_unterminated_quoted_arguments,
    retain_user_macro_tab_argument_prefix, update_fill_mode,
};
use event::{ControlEvent, RequestKind, SourceEvent};
use execution::{
    collect::{
        collect_pending_macro_scope, collect_scope, definition_scope_remainder_line,
        record_suppressed_scope_definitions,
    },
    replay::{execute_scope_line, execute_scope_lines, execute_scope_macro_lines},
};
pub use report::{FatalError, FatalErrorKind, ParseReport, ParseStatistics};
use request::{
    apply_environment_request, apply_string_request, consume_ignore_block, copy_mode_reparse,
    has_protected_tabulation_escape, ignore_marker, is_definition_terminator,
    is_environment_request, is_ignore_terminator, is_macro_comment_request,
    is_scope_ignore_terminator, macro_argument_copy_mode_reparse, macro_body_control_column,
    macro_definition_directly_invokes, normalize_roff_name_prefix, recover_attached_control_name,
    register_division_by_zero, roff_escape_name_width, split_macro_control, trim_horizontal_space,
};
use runtime::{
    InputTrap, ManIndentState, arm_input_trap, diagnostic, emit_declared_character_escape_warnings,
    emit_long_input_line, emit_outside_macro_argument_escapes,
    emit_unterminated_register_reference_escapes, emit_unterminated_string_reference_escapes,
    environment_error_diagnostic, expand_copy_mode_definition, expand_declared_character_escapes,
    expand_environment, invalid_input_byte_offsets, normalize_character_request_arguments,
    normalize_macro_argument_number_escapes, push_diagnostic, record_expansion_steps,
    strip_outside_macro_argument_escapes, trailing_whitespace_start, translate_visible,
    update_man_example_fill_presentation, update_man_indent_register, update_preprocessor_depth,
    update_table_preprocessor_depth, validate_character_request, visible_bytes,
};
use session::{ParseSession, ScanOutcome, SourceMachine};

struct DenyResolver;

impl SourceResolver for DenyResolver {
    fn resolve(
        &mut self,
        _request: IncludeRequest<'_>,
    ) -> Result<Option<crate::ResolvedSource>, crate::ResolveError> {
        Ok(None)
    }
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
        previous_conditional: Option<BranchOutcome>,
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
mod tests;
