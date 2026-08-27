//! First structural pass for the semantic mdoc(7) macro package.
//!
//! The roff executor owns source order and macro expansion. This pass gives
//! the initial M5 macro families their `Block`/`Head`/`Body` shape, records
//! metadata and normalized list/display/font attributes, and leaves unhandled
//! macros as ordinary elements for later incremental validation.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    AuthorMode, DisplayKind, MacroSet, NodeFlags, NodeId, NodeKind, NormalizedEnclosure,
    NormalizedFont, NormalizedListKind, SourceSpan,
    ast::{DocumentBuilder, MdocListMarker},
};

/// Bounded semantic-restructuring result consumed by the parser boundary.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct StructureOutcome {
    /// First source location whose `Head`/`Body` pair could not fit.
    pub(crate) node_limit_location: Option<SourceSpan>,
    /// Recoverable mdoc scope findings retained in source order.
    pub(crate) recoveries: Vec<Recovery>,
}

/// One mdoc scope recovery classified by the parser boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Recovery {
    /// A closing macro did not correspond to the active semantic block.
    UnmatchedClose {
        /// Closing macro spelling.
        macro_name: &'static str,
        /// Source location of the closer, when retained by the scanner.
        location: Option<SourceSpan>,
    },
    /// An open semantic block reached end of input without its closer.
    UnclosedBlock {
        /// Opening macro spelling.
        macro_name: &'static str,
        /// Source location of the opener, when retained by the scanner.
        location: Option<SourceSpan>,
    },
    /// A full mdoc block closed without any Body children.
    EmptyBlock {
        /// Opening macro spelling.
        macro_name: &'static str,
        /// Source location of the opener.
        location: Option<SourceSpan>,
    },
    /// A completed or broken list item has no retained body content.
    EmptyListItem {
        /// Public list selector without its leading dash.
        list_type: &'static str,
        /// Location of the empty item request.
        location: Option<SourceSpan>,
    },
    /// A list type requiring a visible term received an item with no Head.
    EmptyListItemHead {
        /// Public list selector without its leading dash.
        list_type: &'static str,
        /// Location of the empty item request.
        location: Option<SourceSpan>,
    },
    /// Content preceding the first item was moved out of a list.
    ContentOutsideList {
        /// Public node spelling, or `text` for ordinary prose.
        content: Box<str>,
        /// Location of the moved node.
        location: Option<SourceSpan>,
    },
    /// An inline mdoc macro requiring a visible argument was discarded.
    EmptyMacro {
        /// Source spelling of the discarded macro.
        macro_name: &'static str,
        /// Source location of the macro request.
        location: Option<SourceSpan>,
    },
    /// A cross-reference retained a name but no manual section argument.
    MissingReferenceSection {
        /// Visible cross-reference name, including any attached delimiter.
        name: Box<str>,
        /// Source location of the `.Xr` request.
        location: Option<SourceSpan>,
    },
    /// A manual target spelling contains whitespace or a roff escape.
    InvalidTag {
        /// Complete authored tag spelling used in the legacy diagnostic.
        tag: Box<str>,
        /// Location of the first invalid byte.
        location: Option<SourceSpan>,
    },
    /// A parsed inline macro received the spelling of a known mdoc macro
    /// that the package does not permit as a nested call.
    NonCallableMacro {
        /// Source spelling of the rejected nested macro.
        macro_name: Box<str>,
        /// Location of the rejected spelling.
        location: Option<SourceSpan>,
    },
    /// A no-space macro had no eligible preceding/following presentation
    /// boundary at its source position.
    NoSpaceMacro {
        /// Source location of the `.Ns` request.
        location: Option<SourceSpan>,
    },
    /// An mdoc boolean control did not receive an accepted argument.
    InvalidBooleanArgument {
        /// Source spelling of the control macro.
        macro_name: &'static str,
        /// Rejected first argument.
        argument: Box<str>,
        /// Source location of the rejected argument.
        location: Option<SourceSpan>,
    },
    /// A direct child of an `Rs` block was neither a bibliographic field nor
    /// an allowed structural boundary.
    ReferenceContent {
        /// Upstream-visible child spelling (`text` for ordinary prose).
        content: Box<str>,
        /// Source location of the invalid child.
        location: Option<SourceSpan>,
    },
    /// An `Rs` block closed without any direct child.
    EmptyReferenceBlock {
        /// Source location of the `Rs` opener.
        location: Option<SourceSpan>,
    },
    /// A roff `.ft` request selected a font outside mandoc's finite catalogue.
    UnknownRoffFont {
        /// Authored font selector.
        font: Box<str>,
        /// Source location of the `.ft` request.
        location: Option<SourceSpan>,
    },
    /// A post-validation style finding for an attached trailing delimiter.
    TrailingDelimiterSpacing {
        /// Source spelling of the validated macro.
        macro_name: &'static str,
        /// Displayed final argument and delimiter.
        display: Box<str>,
        /// Location of the attached delimiter.
        location: Option<SourceSpan>,
    },
    /// A macro that requires outer delimiter flow retained an attached
    /// delimiter in its final text phrase.
    TrailingDelimiter {
        /// Source spelling of the validated macro.
        macro_name: &'static str,
        /// Complete visible phrase used by the legacy diagnostic.
        display: Box<str>,
        /// Location of the attached delimiter.
        location: Option<SourceSpan>,
    },
    /// A description-line macro occurred outside the `NAME` section.
    DescriptionOutsideName {
        /// Source location of the `.Nd` request.
        location: Option<SourceSpan>,
    },
    /// A direct child of a `NAME` section was neither `.Nm` nor `.Nd`.
    BadNameSectionContent {
        /// Public spelling of the invalid child, or `text` for prose.
        content: Box<str>,
        /// Location of the invalid child.
        location: Option<SourceSpan>,
    },
    /// A second direct `.Nm` in `NAME` was not preceded by a comma.
    NameSectionMissingComma {
        /// Visible arguments of the name macro.
        name: Box<str>,
        /// Location of the name macro.
        location: Option<SourceSpan>,
    },
    /// A `NAME` section did not contain a direct `.Nm` child.
    NameSectionMissingName {
        /// Location of the `NAME` section opener.
        location: Option<SourceSpan>,
    },
    /// A `NAME` section did not contain a direct `.Nd` child.
    NameSectionMissingDescription {
        /// Location of the `NAME` section opener.
        location: Option<SourceSpan>,
    },
    /// A direct `.Nd` in `NAME` was followed by another direct child.
    DescriptionNotAtEndOfName {
        /// Location of the `.Nd` request.
        location: Option<SourceSpan>,
    },
    /// A description-line macro did not receive any visible description.
    MissingDescription {
        /// Source location of the `.Nd` request.
        location: Option<SourceSpan>,
    },
    /// An `AUTHORS` section did not contain a populated `.An` macro.
    AuthorsSectionWithoutAuthor {
        /// Source location of the `AUTHORS` section opener.
        location: Option<SourceSpan>,
    },
    /// A function name retained a parenthesis requiring semantic syntax.
    FunctionNameParenthesis {
        /// Complete authored function-name spelling.
        name: Box<str>,
        /// Source location of the first unexpected parenthesis.
        location: Option<SourceSpan>,
    },
    /// A function argument retained a comma before a callback/array suffix.
    FunctionArgumentComma {
        /// Complete authored argument spelling.
        argument: Box<str>,
        /// Source location of the comma.
        location: Option<SourceSpan>,
    },
    /// A library name did not resolve through the mdoc library catalogue.
    UnknownLibrary {
        /// First authored library-name argument.
        library: Box<str>,
        /// Source location of that first argument.
        location: Option<SourceSpan>,
    },
    /// A standard selector did not resolve through the mdoc standard catalogue.
    UnknownStandard {
        /// Rejected standard selector.
        standard: Box<str>,
        /// Source location of that selector.
        location: Option<SourceSpan>,
    },
    /// A repeated mdoc option was discarded after its first occurrence won.
    DuplicateArgument {
        /// Source spelling of the macro.
        macro_name: &'static str,
        /// Rejected option spelling.
        argument: Box<str>,
        /// Source location of the rejected option.
        location: Option<SourceSpan>,
    },
    /// A list layout option did not provide its required width.
    EmptyListLayoutArgument {
        /// Missing-value option spelling without its leading dash.
        option: &'static str,
        /// Source location of the option.
        location: Option<SourceSpan>,
    },
    /// A list selected a second list type after its first type had won.
    DuplicateListType {
        /// Rejected list type spelling including its leading dash.
        argument: &'static str,
        /// Source location of the list request.
        location: Option<SourceSpan>,
    },
    /// A list layout option was repeated after its first value had won.
    DuplicateListArgument {
        /// Rejected option spelling, including its value when present.
        argument: Box<str>,
        /// Source location of the repeated option.
        location: Option<SourceSpan>,
    },
    /// A list type followed one or more layout options.
    ListTypeNotFirst {
        /// First list option that preceded the type.
        argument: Box<str>,
        /// Source location of the list request.
        location: Option<SourceSpan>,
    },
    /// A list omitted its type and recovered as `-item`.
    MissingListType {
        /// Source location of the list request.
        location: Option<SourceSpan>,
    },
    /// A width was supplied to a list that recovered as `-item`.
    SkippedListWidth {
        /// Selected list type without its leading dash.
        list_type: &'static str,
        /// Source location of the width option.
        location: Option<SourceSpan>,
    },
    /// A `-tag` list omitted its width and recovered to `6n`.
    MissingTagListWidth {
        /// Source location of the list request.
        location: Option<SourceSpan>,
    },
    /// A column-list item did not contain the permitted number of cells.
    WrongNumberOfColumnCells {
        /// Number of declared column headings.
        columns: usize,
        /// Number of retained item cells.
        cells: usize,
        /// Source location of the item request.
        location: Option<SourceSpan>,
    },
    /// A zero-argument column item acquired its first cell from the next line.
    ColumnItemUsesNextLine {
        /// Source location of the item request.
        location: Option<SourceSpan>,
    },
    /// A column-cell separator began its own physical input line.
    ColumnFirstMacro {
        /// Source location of the `Ta` request.
        location: Option<SourceSpan>,
    },
    /// An `.At` compatibility selector did not name a known AT&T UNIX version.
    UnknownAtVersion {
        /// Rejected AT&T UNIX version spelling.
        argument: Box<str>,
        /// Source location of the rejected first argument.
        location: Option<SourceSpan>,
    },
    /// A display option required a value but received none.
    EmptyDisplayOffset {
        /// Location of the `-offset` option.
        location: Option<SourceSpan>,
    },
    /// A display option repeated a prior option whose first occurrence won.
    DuplicateDisplayArgument {
        /// Rejected option spelling, including its value when applicable.
        argument: Box<str>,
        /// Source location of the repeated option.
        location: Option<SourceSpan>,
    },
    /// A display selected more than one type; the later type was discarded.
    DuplicateDisplayType {
        /// Rejected display type option.
        argument: &'static str,
        /// Source location of the display request.
        location: Option<SourceSpan>,
    },
    /// A display supplied no recognized display type and fell back to ragged.
    MissingDisplayType {
        /// Source location of the display request.
        location: Option<SourceSpan>,
    },
    /// A display requested the unsupported file-backed form.
    UnsupportedDisplayFile {
        /// Source location of the display request.
        location: Option<SourceSpan>,
    },
    /// A display control had no options and was removed from the public tree.
    DisplayWithoutArguments {
        /// Source location of the display request.
        location: Option<SourceSpan>,
    },
    /// A display block appeared inside another display block.
    NestedDisplay {
        /// Source location of the nested display request.
        location: Option<SourceSpan>,
    },
    /// A new structural block forcibly closed an open mdoc block.
    BrokenBlock {
        /// Incoming block macro that broke the open block.
        breaker: &'static str,
        /// Open block macro being implicitly closed.
        macro_name: &'static str,
        /// Source location of the incoming breaker.
        location: Option<SourceSpan>,
    },
    /// A font block did not select a font type.
    MissingFontType {
        /// Source location of the font request.
        location: Option<SourceSpan>,
    },
    /// A font block supplied an unknown legacy font name.
    UnknownFontType {
        /// Rejected font name.
        argument: Box<str>,
        /// Source location of the rejected first argument.
        location: Option<SourceSpan>,
    },
    /// A paragraph or layout request supplied arguments that mandoc discards.
    InvalidArguments {
        /// Upstream-compatible explanation of the discarded arguments.
        message: Box<str>,
        /// First discarded argument, or the macro itself for `.Pp`.
        location: Option<SourceSpan>,
    },
    /// An obsolete but supported mdoc macro was used.
    Obsolete {
        /// Source spelling of the compatibility macro.
        macro_name: &'static str,
        /// Location of the macro request.
        location: Option<SourceSpan>,
    },
    /// An mdoc prologue macro repeated an earlier instance.
    DuplicatePrologue {
        /// Repeated prologue macro name.
        macro_name: &'static str,
        /// Source location of the repeated request.
        location: Option<SourceSpan>,
    },
    /// An `.Os` request explicitly named an operating system with legacy
    /// OpenBSD/NetBSD style validation enabled.
    OperatingSystemExplicit {
        /// Authored operating-system phrase.
        operating_system: Box<str>,
        /// Legacy validation flavour selected by the first `.Os` request.
        flavour: &'static str,
        /// First operating-system argument.
        location: Option<SourceSpan>,
    },
    /// A NetBSD-style `.Os` request found an `$Mdocdate` prologue.
    MdocDateFound {
        /// Authored date phrase.
        date: Box<str>,
        /// First `.Dd` argument.
        location: Option<SourceSpan>,
    },
    /// A recognized operating-system flavour had no matching RCS id comment.
    RcsIdMissing {
        /// Legacy validation flavour selected by `.Os`.
        flavour: &'static str,
    },
    /// An `.Os` request appeared after visible mdoc body content.
    LateOperatingSystem {
        /// Source location of the late request.
        location: Option<SourceSpan>,
    },
    /// The document ended without any `.Os` prologue request.
    MissingOperatingSystem,
    /// A `.Pf` prefix has no following same-line presentation token.
    PrefixWithoutFollowing {
        /// Displayed source spelling of the incomplete prefix phrase.
        display: Box<str>,
        /// Source location of the `.Pf` request.
        location: Option<SourceSpan>,
    },
    /// A compatibility macro has no semantic effect in the public AST.
    UselessMacro {
        /// Source spelling of the compatibility macro.
        macro_name: &'static str,
        /// Source location of the macro request.
        location: Option<SourceSpan>,
    },
    /// A title prologue request occurred after visible body parsing began.
    LateTitle {
        /// Source location of the late request.
        location: Option<SourceSpan>,
    },
    /// An accepted `.Dt` title contains a lower-case ASCII letter.
    TitleNotUppercase {
        /// Source spelling of the title argument.
        title: Box<str>,
        /// Source location of the first lower-case character.
        location: Option<SourceSpan>,
    },
    /// An accepted `.Dt` has an unknown manual section identifier.
    UnknownTitleSection {
        /// Source spelling of the section argument.
        section: Box<str>,
        /// Source location of the section argument.
        location: Option<SourceSpan>,
    },
    /// An accepted `.Dt` omitted its title argument.
    MissingTitleArgument {
        /// Location of the title request.
        location: Option<SourceSpan>,
    },
    /// An accepted `.Dt` omitted its manual section argument.
    MissingTitleSection {
        /// Recovered document title.
        title: Box<str>,
        /// Location of the title request.
        location: Option<SourceSpan>,
    },
    /// A title request appeared after the operating-system prologue request.
    TitleAfterOperatingSystem {
        /// Location of the title request.
        location: Option<SourceSpan>,
    },
    /// The document ended without a visible body.
    NoDocumentBody,
    /// A `.Dd` request had no date argument.
    DateMissing {
        /// Location of the date request.
        location: Option<SourceSpan>,
    },
    /// A `.Dd` date could not use an accepted mdoc(7) date spelling.
    DateUnparseable {
        /// Authored date retained in metadata.
        date: Box<str>,
        /// Location of the date argument.
        location: Option<SourceSpan>,
    },
    /// A `.Dd` date uses the legacy ISO man(7) spelling.
    LegacyDate {
        /// Authored date retained in metadata.
        date: Box<str>,
        /// Location of the date argument.
        location: Option<SourceSpan>,
    },
    /// A date prologue request appeared after visible body parsing began.
    LateDate {
        /// Location of the date request.
        location: Option<SourceSpan>,
    },
    /// A date prologue request appeared after an accepted title request.
    DateAfterTitle {
        /// Location of the date request.
        location: Option<SourceSpan>,
    },
    /// End of input arrived without an accepted mdoc title prologue.
    MissingTitle,
    /// An empty `.Nm` could not use a previously declared manual name.
    MissingName {
        /// Source location of the request.
        location: Option<SourceSpan>,
    },
    /// A `.Fo` declaration omitted its required function name.
    MissingFunctionName {
        /// Source location of the request.
        location: Option<SourceSpan>,
    },
    /// A standard `.Ex` expansion had no manual name to substitute.
    MissingExitName {
        /// Source location of the request.
        location: Option<SourceSpan>,
    },
    /// A standard Ex/Rv macro omitted its required `-std` selector.
    MissingStandardSelector {
        /// Source spelling of the macro.
        macro_name: &'static str,
        /// Source location of the request.
        location: Option<SourceSpan>,
    },
    /// The first non-prologue root node was not a section header.
    ContentBeforeFirstSection {
        /// Spelling of the first root-level content node.
        content: Box<str>,
        /// Location of that node.
        location: Option<SourceSpan>,
    },
    /// A conventional section title is restricted to other manual sections.
    UnexpectedSection {
        /// Authored conventional section title.
        section: Box<str>,
        /// Comma-separated manual sections permitted by the convention.
        allowed_sections: &'static str,
        /// Location of the section request.
        location: Option<SourceSpan>,
    },
    /// A conventional section repeats the preceding named section.
    DuplicateSection {
        /// Canonical conventional section title.
        section: &'static str,
        /// Location of the section request.
        location: Option<SourceSpan>,
    },
    /// The document's first `.Sh` did not name the conventional NAME section.
    FirstSectionNotName {
        /// Visible section title.
        section: Box<str>,
        /// Source location of the section request.
        location: Option<SourceSpan>,
    },
    /// A conventional section occurs before the preceding named section.
    SectionOutOfOrder {
        /// Canonical conventional section title.
        section: &'static str,
        /// Location of the section request.
        location: Option<SourceSpan>,
    },
    /// A literal tab appeared in a filled mdoc section title.
    FilledTextTab {
        /// Location of the literal tab.
        location: Option<SourceSpan>,
    },
    /// A physical blank input line in filled mode was normalized to `.sp`.
    FilledBlankLine {
        /// Location of the physical blank line.
        location: Option<SourceSpan>,
    },
    /// A paragraph-layout control was redundant at a block boundary.
    ParagraphBoundary {
        /// Source spelling of the paragraph macro.
        macro_name: &'static str,
        /// Relative placement in the upstream diagnostic message.
        placement: &'static str,
        /// Source spelling of the structural block.
        blocker: &'static str,
        /// Location of the discarded paragraph.
        location: Option<SourceSpan>,
    },
    /// A trailing paragraph control was relinked out of its final list item.
    ParagraphMovedOutOfList {
        /// Source spelling of the relinked paragraph control.
        macro_name: &'static str,
        /// Location of the relinked control.
        location: Option<SourceSpan>,
    },
    /// A full-block closer crossed an open explicit partial block.
    BadlyNestedBlock {
        /// Full block being closed.
        breaker: &'static str,
        /// Explicit partial block interrupted by the closer.
        interrupted: &'static str,
        /// Location of the full-block closer.
        location: Option<SourceSpan>,
    },
    /// An item request occurred outside an active list and became a line break.
    ItemOutsideList {
        /// Displayed item arguments, if any.
        arguments: Box<str>,
        /// Location of the item request.
        location: Option<SourceSpan>,
    },
    /// A tabular-column request occurred outside a column list.
    ColumnOutsideColumnList {
        /// Location of the column request.
        location: Option<SourceSpan>,
    },
}

#[derive(Clone, Copy)]
enum ArgumentPlacement {
    Head,
    Body,
    /// Keep scanner-level token boundaries for a body that subsequently
    /// applies mdoc's nested inline-call grammar.
    BodyTokens,
    Drop,
}

#[allow(clippy::struct_excessive_bools)] // Each flag retains a distinct terminal list/display provenance.
#[derive(Clone, Default)]
struct BlockAttributes {
    list_kind: Option<NormalizedListKind>,
    /// Exact selected marker, retained only for native renderer output.
    list_marker: Option<MdocListMarker>,
    /// `Bl -hang` shares the public Definition projection with `-tag`, but
    /// retains a distinct terminal first-line field rule.
    terminal_hanging_list: bool,
    /// `Bl -ohang` shares the public Definition projection with `-tag`, but
    /// renders its Head and Body as separate equally indented terminal lines.
    terminal_overhanging_list: bool,
    /// `Bl -inset` shares the public Definition projection with `-tag`, but
    /// begins terminal Body content directly after its term.
    terminal_inset_list: bool,
    /// `Bl -diag` shares the public Definition projection with `-tag`, but
    /// uses a bold terminal term and a two-cell Body gap.
    terminal_diagnostic_list: bool,
    /// Selected mdoc list selector without its leading dash.
    list_type: &'static str,
    /// Number of declaration phrases following `Bl -column`.
    column_count: Option<usize>,
    /// Declaration phrases retained only for native terminal column layout.
    ///
    /// They are deliberately absent from the normalized public list node:
    /// libmandoc's owned AST exposes the `Column` behavior, not its discarded
    /// input labels.  The terminal device still needs their display widths.
    column_widths: Vec<String>,
    display_kind: Option<DisplayKind>,
    /// `-literal` and `-unfilled` share the public normalized display kind,
    /// but the terminal device assigns their tabs differently.
    literal_display: bool,
    /// `-centered` is publicly a filled display, but its terminal device
    /// field centers each completed visual line.
    centered_display: bool,
    font: Option<NormalizedFont>,
    compact: bool,
    offset: Option<String>,
    width: Option<String>,
}

/// Width retention is more specific than the public normalized list kind:
/// several mdoc list forms lower to `Definition` but have distinct layout
/// validation rules.
#[derive(Clone, Copy)]
enum ListWidthRule {
    /// Discard an authored width and provide no default.
    Drop,
    /// Retain an authored width, but provide no default.
    Retain,
    /// Warn that a `-tag` list uses the formatter's private `6n` default.
    DefaultSix,
    /// Use `2n` when no width was authored.
    DefaultTwo,
    /// Use `3n` when no width was authored.
    DefaultThree,
}

#[derive(Clone, Copy)]
struct ScopeFrame {
    close: &'static str,
    open: NodeId,
    body: NodeId,
    /// Whether this scope materializes a third `Tail` child only when its
    /// matching closer arrives.  Unclosed Eo blocks intentionally retain only
    /// Head and Body, as in the legacy owned AST.
    tail_on_close: bool,
    /// A function block accepts only its first transparent `.Tg` as an
    /// automatic destination.
    transparent_target_taken: bool,
    /// A cross-line explicit opener created in the tail of an already-crossed
    /// block is structurally nested, but its own closer is not another break
    /// of that historical implicit ancestor.
    suppress_implicit_ancestor_break: bool,
    resume_active: NodeId,
    resume_flow: NodeId,
}

/// Restructure the initial M5 mdoc macro families in a bounded arena.
#[allow(clippy::too_many_lines)] // One source-order state machine keeps scope ownership auditable.
pub(crate) fn structure(
    builder: &mut DocumentBuilder,
    max_nodes: usize,
    mut saw_operating_system_request: bool,
) -> StructureOutcome {
    let mut outcome = StructureOutcome::default();
    if builder.macro_set() != MacroSet::Mdoc {
        return outcome;
    }
    let root = DocumentBuilder::root();
    let Some(flat) = builder.children(root).map(<[NodeId]>::to_vec) else {
        return outcome;
    };
    let synopsis_events = builder.take_mdoc_synopsis_events();
    let mut synopsis_event_cursor = 0_usize;

    // mdoc's ordinary source text is tokenized before the package pass, so a
    // terminal sentence marker belongs only to direct flat text events here.
    // Do not apply this fallback to macro arguments: their post-validation
    // punctuation semantics are macro-specific.
    for node in &flat {
        if builder.node_kind(*node) == Some(NodeKind::Text) {
            mark_sentence_end(builder, *node);
        }
    }
    apply_presentation_flags(builder, &flat);
    trim_mdoc_filled_text_trailing_whitespace(builder, &flat);
    for node in &flat {
        let macro_name = builder.node_macro_name(*node);
        if matches!(macro_name, Some("Fd" | "Fl" | "Sy" | "Ar" | "Em" | "Sq")) {
            rebase_expanded_argument_locations(builder, *node);
        }
    }
    let c_blank_followers = suppress_filled_c_blank_lines(builder, &flat);
    let mut filled_blank_line_recoveries =
        normalize_filled_blank_lines(builder, &flat, &c_blank_followers);

    let mut root_children = Vec::new();
    let mut section_parent = root;
    let mut has_section = false;
    // `post_nd()` validates both the completed description phrase and its
    // section after the block has received following physical text.  Retain
    // the current named section separately from the structural Body node so
    // that an `.Nd` can be checked when a later block boundary closes it.
    let mut in_name_section = false;
    let mut pending_nd_delimiter_bodies = Vec::new();
    // `post_sh_name()` validates only direct children of a completed NAME
    // section.  Retain its Body until the next `.Sh` (or EOF) so nested
    // `.Nm`/`.Nd` entries remain intentionally insufficient.
    let mut pending_name_section_body = None;
    let mut pending_authors_body = None;
    // Only named conventional sections participate in the mdoc ordering
    // convention. Custom section titles leave this cursor untouched.
    let mut last_named_section = None::<u8>;
    let mut flow_parent = root;
    let mut active_body = root;
    let mut scopes = Vec::<ScopeFrame>::new();
    // `Bl -column` consumes a variable number of declaration phrases before
    // the next option.  The public normalized list kind intentionally omits
    // that parser-only count, so retain it by the list Body while items are
    // structured below.
    let mut column_counts = BTreeMap::<NodeId, usize>::new();
    // Normalized definition lists coalesce several authored selectors.  Keep
    // this private discriminator for validators with `-diag`-only behavior.
    let mut list_types = BTreeMap::<NodeId, &'static str>::new();
    // A syntactically empty `.It` requires delayed validation because the
    // following physical input line may become its first column body.
    let mut pending_empty_column_items = BTreeSet::<NodeId>::new();
    // A short column row may acquire further cells from a later physical
    // `.Ta` request, so defer its count validation until the next structural
    // boundary rather than diagnosing the provisional prefix immediately.
    let mut pending_short_column_items = BTreeMap::<NodeId, (usize, usize)>::new();
    // A marker-list item beginning with an invalid `Ta` already queues its
    // ignored-argument recovery for the post-validation phase.  Its normal
    // item-boundary validation must not report the same Head a second time.
    let mut deferred_fixed_head_argument_items = BTreeSet::<NodeId>::new();
    let mut target_heads = Vec::new();
    let mut synopsis_bodies = Vec::new();
    // `tag_put()` applies function-name destinations globally: repeated
    // automatic function spellings do not retain a per-node tag.  Defer the
    // tag string while retaining every selected target flag immediately.
    // The bool records whether the macro kind exposes the globally unique
    // spelling as a public tag.  `Fn` does; `Fo` merely owns/surrenders a
    // destination bit in that namespace.
    let mut automatic_function_targets = Vec::<(NodeId, String, bool)>::new();
    let mut automatic_function_tag_occurrences = Vec::<String>::new();
    // mandoc validates Pp after it has completed the containing body, while
    // roff layout requests such as br and sp are validated immediately.
    // Keep the two queues separate to preserve observable finding order.
    let mut deferred_paragraph_argument_recoveries = Vec::new();
    // Delimiter spacing is validated after the syntax pass. Keep those
    // findings distinct so a later source-level recovery is reported first.
    let mut deferred_post_validation_recoveries = Vec::new();
    // A list closer that crosses a partial scope opened from an item header
    // leaves the list and partial block unclosed.  mandoc reports the
    // resulting item validation only after those EOF closers, so retain these
    // narrow recovery events separately from ordinary source-order findings.
    let mut deferred_broken_item_recoveries = Vec::new();
    // List-content relocation is a post-validation action: all item-break
    // errors are observable before the warnings for material moved out of its
    // enclosing list.
    let mut deferred_list_content_recoveries = Vec::new();
    // A callable explicit closer encountered while an implicit partial block
    // is parsed is a syntax-stage finding in libmandoc.  Emit it before the
    // later section/list post-validation findings, despite the implicit
    // block's public AST node being assembled only after those tokens.
    let mut syntax_stage_recoveries = Vec::new();
    // A validated `.Tg` registers an explicit manual tag for the immediately
    // following mdoc node.  The general tag priority table comes later; this
    // state only covers the source-order relationship needed to preserve the
    // public tree for paragraph anchors.
    let mut pending_manual_tag = None::<(NodeId, String)>;
    // An empty `Fl` can make a following Tg transparent: the preceding
    // paragraph owns the destination while the next inline macro receives
    // only the matching permalink.
    let mut pending_transparent_permalink = None::<String>;
    let mut pending_paragraph_href = None::<String>;
    // Function names validate a preceding paragraph as their destination.
    // Keep only the immediately eligible paragraph so ordinary prose cannot
    // acquire a target just because a later Fn happens to occur.
    let mut pending_function_paragraph = None::<NodeId>;
    // Only the first function destination in one paragraph/flow region gains
    // the automatic target.  Later `.Fn` nodes remain plain syntax until a
    // new paragraph starts or an explicit `.Tg` assigns a destination.
    let mut function_target_taken = false;
    let mut enclosure = None::<NormalizedEnclosure>;
    let mut implicitly_closed = Vec::<&'static str>::new();
    let mut in_synopsis = false;
    // `Sh SYNOPSIS` and the private roff `nS` register both enter the same
    // structural context, but libmandoc's generated `.Nm` fallback keeps a
    // distinct presentation bit for the latter execution-driven form.
    let mut synopsis_from_register = false;
    let mut synopsis_name_body = None::<NodeId>;
    // `Bk ... Ek` ends a SYNOPSIS name flow before a following paragraph,
    // whereas an ordinary in-flow Pp remains inside that name block.
    let mut synopsis_keep_boundary = false;
    // `.Sm` changes how the mdoc validator groups otherwise adjacent source
    // words in later partial blocks.  It is stateful: a bare request toggles
    // the current setting, rather than resetting it to the default.
    let mut spacing_enabled = true;
    let mut preserve_leading_comments = true;
    // libmandoc retains the last authored prologue metadata but reports each
    // repeated request, even when the later request appears after the body.
    let mut saw_date_prologue = false;
    let mut saw_title_prologue = false;
    let mut saw_operating_system_prologue = false;
    let mut first_date_prologue = None::<(Box<str>, Option<SourceSpan>)>;
    let mut operating_system_flavour = None::<&'static str>;
    let mut netbsd_operating_system_validation = false;
    let mut saw_netbsd_rcs_id = false;

    for (flat_index, node) in flat.into_iter().enumerate() {
        while let Some((boundary, state)) = synopsis_events.get(synopsis_event_cursor)
            && *boundary <= flat_index
        {
            synopsis_event_cursor += 1;
            if in_synopsis == *state {
                continue;
            }
            // A disabled `nS` finishes a surrounding synopsis-name flow, but
            // must not tear down an explicit partial enclosure that remains
            // open across the state request.  Its later closer still owns
            // the resumed non-synopsis text.
            if !*state && scopes.is_empty() {
                active_body = section_parent;
                flow_parent = section_parent;
            }
            synopsis_name_body = None;
            synopsis_keep_boundary = false;
            in_synopsis = *state;
            synopsis_from_register = *state;
        }
        if c_blank_followers.contains(&node) {
            continue;
        }
        if let Some(recovery) = filled_blank_line_recoveries.remove(&node) {
            outcome.recoveries.push(recovery);
        }
        if builder.node_kind(node) == Some(NodeKind::Comment) {
            // Like man(7), mdoc retains only the source preamble comments in
            // the public syntax tree.  Comments encountered after the first
            // parsed document event are validator input, not rendered or
            // consumer-visible content.
            if preserve_leading_comments {
                root_children.push(node);
                if builder
                    .node_text(node)
                    .is_some_and(|text| text.contains("$NetBSD:"))
                {
                    saw_netbsd_rcs_id = true;
                }
            }
            continue;
        }
        preserve_leading_comments = false;
        // A tbl range inside `Bl -column` has no mdoc control line that can
        // introduce its row.  mandoc therefore materializes one implicit,
        // empty-headed `It` and keeps consecutive table rows in that body's
        // source order.  Do this before generic dispatch: a Table has no
        // macro name and would otherwise remain a list-body sibling.
        if active_column_list(builder, active_body)
            && builder.node_kind(node) == Some(NodeKind::Table)
            && (append_implicit_column_table_row(builder, active_body, node)
                || structure_implicit_column_table_item(
                    builder,
                    active_body,
                    node,
                    max_nodes,
                    &mut outcome,
                ))
        {
            continue;
        }
        // Bk -words retains its parsed inline words as separately owned AST
        // children.  This is presentation-independent semantic grouping, so
        // derive it from the active explicit scope rather than mutating the
        // document-wide `.Sm` state.
        let inline_spacing_enabled =
            spacing_enabled && !scopes.iter().any(|frame| frame.close == "Ek");
        // Vt has a distinct partial-block form in SYNOPSIS.  Delay its
        // inline splitting until that context has selected its final parent.
        if in_synopsis {
            // Preserve SYNOPSIS state before splitting: No's join rule needs
            // the package context while it still owns raw scanner words.
            mark_synopsis_pretty(builder, node);
        }
        let mut inline_column_ta_tail = take_inline_column_ta_tail(builder, node, active_body);
        let inline_events = if builder.node_macro_name(node) == Some("Vt") {
            vec![node]
        } else {
            split_inline_macro_events(
                builder,
                node,
                inline_spacing_enabled,
                max_nodes,
                &mut outcome,
            )
        };
        // A SYNOPSIS Nm is a full block, but inline events split from the
        // same physical request line remain part of its Head.  When no
        // partial scope takes over, restore ordinary flow to the Body after
        // the complete line has been consumed.
        let mut synopsis_name_inline_restore = None::<(NodeId, NodeId)>;
        // Some fixed-argument macros delete a private punctuation event
        // together with their empty source element. The scanner has already
        // split that punctuation into this line's event stream, so defer its
        // suppression until the ordinary source-order loop reaches it.
        let mut suppressed_inline_events = BTreeSet::new();
        for (event_index, node) in inline_events.iter().copied().enumerate() {
            if suppressed_inline_events.contains(&node) {
                continue;
            }
            let macro_name = builder.node_macro_name(node).map(str::to_owned);
            if macro_name.as_deref() == Some("ft") {
                let children = builder.children(node).unwrap_or_default().to_vec();
                if let Some(font) = children.first().and_then(|child| builder.node_text(*child)) {
                    if !is_legacy_roff_font(font.as_bytes()) {
                        outcome.recoveries.push(Recovery::UnknownRoffFont {
                            font: font.into(),
                            location: builder.node_location(node),
                        });
                        continue;
                    }
                    // roff_valid_ft() retains only the first selector.  The
                    // scanner has already reported any surplus source word.
                    let _ = builder.replace_children(node, &children[..1]);
                } else if builder.node_count() < max_nodes {
                    if let Some(default_font) = builder.push(node, NodeKind::Text) {
                        let _ = builder.text(default_font, "P");
                        let _ =
                            builder.set_node_location(default_font, builder.node_location(node));
                    }
                } else if outcome.node_limit_location.is_none() {
                    outcome.node_limit_location = builder.node_location(node);
                }
                append_to_parent(builder, root, &mut root_children, active_body, node);
                continue;
            }
            if scopes.last().is_some_and(|scope| scope.close == "Re")
                // libmandoc's `post_rs()` begins with the second direct
                // child: its first child is retained without a content
                // warning, even when it is a transparent `Tg` node.
                && !builder
                    .children(active_body)
                    .is_some_and(<[NodeId]>::is_empty)
            {
                let reference_content = match macro_name.as_deref() {
                    Some(name) if is_reference_field_macro(name) || name == "Re" => None,
                    Some(name) => Some(name.into()),
                    None if builder.node_kind(node) == Some(NodeKind::Text) => Some("text".into()),
                    _ => None,
                };
                if let Some(content) = reference_content {
                    outcome.recoveries.push(Recovery::ReferenceContent {
                        content,
                        location: builder.node_location(node),
                    });
                }
            }
            if macro_name.as_deref() == Some("Ns")
                && no_space_macro_requires_warning(builder, node, &inline_events[event_index + 1..])
            {
                outcome.recoveries.push(Recovery::NoSpaceMacro {
                    location: builder.node_location(node),
                });
            }
            if macro_name.as_deref().is_some_and(|name| {
                is_inline_mdoc_macro(name) && mdoc_inline_argument_limit(name).is_none()
            }) {
                // `lookup()` diagnoses a known macro spelling when the
                // enclosing in-line macro has MDOC_PARSED but the nested
                // macro lacks MDOC_CALLABLE.  Fixed-argument macros consume
                // their prefix literally, so only the unbounded family gets
                // this lookup pass. Escaped spellings retain their `\\&`
                // projection in the package AST and intentionally do not
                // compare equal to a macro name here.
                for child in builder.children(node).unwrap_or_default() {
                    let Some(name) = builder.node_text(*child) else {
                        continue;
                    };
                    if is_mdoc_noncallable_macro(name) {
                        outcome.recoveries.push(Recovery::NonCallableMacro {
                            macro_name: name.into(),
                            location: builder.node_location(*child),
                        });
                    }
                }
            }
            if macro_name.as_deref() == Some("Ad")
                && builder.children(node).is_none_or(<[NodeId]>::is_empty)
            {
                outcome.recoveries.push(Recovery::EmptyMacro {
                    macro_name: "Ad",
                    location: builder.node_location(node),
                });
                continue;
            }
            if macro_name.as_deref() == Some("Fd")
                && builder.children(node).is_none_or(<[NodeId]>::is_empty)
            {
                // `post_fd()` discards an empty preprocessor directive after
                // recording its warning.  In particular it must not become
                // an empty SYNOPSIS child that affects following flow.
                outcome.recoveries.push(Recovery::EmptyMacro {
                    macro_name: "Fd",
                    location: builder.node_location(node),
                });
                continue;
            }
            let empty_function_macro = match macro_name.as_deref() {
                Some("Fa") => Some("Fa"),
                Some("Fn") => Some("Fn"),
                Some("Ft") => Some("Ft"),
                _ => None,
            };
            if let Some(macro_name) = empty_function_macro
                && builder.children(node).is_none_or(<[NodeId]>::is_empty)
            {
                // Function declarations use the same post-validation rule
                // for their empty field, name, and argument macros: retain
                // the finding but remove the syntax-only element before it
                // can alter a surrounding Fo declaration's public flow.
                outcome.recoveries.push(Recovery::EmptyMacro {
                    macro_name,
                    location: builder.node_location(node),
                });
                continue;
            }
            if macro_name.as_deref() == Some("No")
                && builder.children(node).is_none_or(<[NodeId]>::is_empty)
                && scopes
                    .iter()
                    .rev()
                    .find(|frame| frame.close == "El")
                    .and_then(|list| list_types.get(&list.body))
                    .is_some_and(|list_type| *list_type == "diag")
            {
                // `No` is an inline spacing control only when it owns visible
                // content.  An empty request is discarded by post-validation
                // before it can become a column-list body child.
                if let Some(next) = inline_events.get(event_index + 1).copied()
                    && let Some(mut flags) = builder.node_flags(next)
                {
                    // The discarded request does not own a public node, so a
                    // following callable spelling becomes the first event of
                    // this physical list-body line.
                    flags.line_start = true;
                    let _ = builder.set_node_flags(next, flags);
                }
                outcome.recoveries.push(Recovery::EmptyMacro {
                    macro_name: "No",
                    location: builder.node_location(node),
                });
                continue;
            }
            if macro_name.as_deref() == Some("Ad") {
                let arguments = builder
                    .children(node)
                    .map(<[NodeId]>::to_vec)
                    .unwrap_or_default();
                if let Some(last) = arguments.last().copied()
                    && let Some(text) = builder.node_text(last)
                    && let Some((&delimiter, prefix)) = text.as_bytes().split_last()
                    && matches!(
                        delimiter,
                        b',' | b'.' | b';' | b':' | b'!' | b'?' | b')' | b']'
                    )
                    && prefix
                        .last()
                        .is_some_and(|byte| !byte.is_ascii_whitespace())
                    && let Some(location) = builder.node_location(last).and_then(|span| {
                        span.end
                            .checked_sub(1)
                            .and_then(|start| SourceSpan::new(span.source, start, span.end).ok())
                    })
                {
                    let display = if arguments.len() == 1 {
                        text.to_owned()
                    } else {
                        format!("... {text}")
                    };
                    deferred_post_validation_recoveries.push(Recovery::TrailingDelimiterSpacing {
                        macro_name: "Ad",
                        display: display.into(),
                        location: Some(location),
                    });
                }
            }
            if macro_name.as_deref() == Some("An")
                && builder.children(node).is_none_or(<[NodeId]>::is_empty)
                && let Some(delimiter) = inline_events.get(event_index + 1).copied()
                && builder
                    .node_text(delimiter)
                    .is_some_and(is_mdoc_closing_delimiter)
                && let Some(mut flags) = builder.node_flags(delimiter)
            {
                // With no author argument, punctuation is plain following
                // text rather than a closing delimiter for an An element.
                flags.delimiter_close = false;
                let _ = builder.set_node_flags(delimiter, flags);
            }
            let direct_partial_close = matches!(macro_name.as_deref(), Some("Fl" | "No"))
                .then(|| take_explicit_partial_close_argument(builder, node, &scopes))
                .flatten();
            let paragraph_href = pending_paragraph_href.take();
            let list_open = scopes.iter().rev().any(|frame| frame.close == "El");
            let list_item_follower = macro_name.as_deref() == Some("It") && list_open;
            if builder.node_kind(node) == Some(NodeKind::Text) {
                if let Some((tag_node, tag)) = pending_manual_tag.take() {
                    if let Some(item) = active_column_item(builder, active_body) {
                        // A `.Tg` followed by more text in the current column
                        // row targets that row without turning its syntax node
                        // into a permalink.
                        if !tag.is_empty() {
                            mark_manual_target(builder, item, &tag);
                            mark_no_print(builder, tag_node);
                        }
                    } else if !tag.is_empty()
                        && let Some(paragraph) =
                            preceding_manual_tag_paragraph(builder, active_body, tag_node)
                    {
                        // `post_tg()` gives a tag before ordinary paragraph
                        // text to the preceding Pp rather than publishing a
                        // second text-owned destination.
                        mark_manual_target(builder, paragraph, &tag);
                        mark_no_print(builder, tag_node);
                    }
                }
            } else if !accepts_pending_manual_tag(macro_name.as_deref()) && !list_item_follower {
                // Other manual-target forms are deliberately left to their own
                // semantic families rather than allowing a pending Pp tag to
                // leak across an unrelated source event.
                pending_manual_tag = None;
            }
            if builder.node_kind(node) != Some(NodeKind::Text)
                // Bk only groups the following words for layout.  It does not
                // break the preceding-paragraph relation that lets a nested
                // Fn become that paragraph's automatic destination.
                && !matches!(
                    macro_name.as_deref(),
                    Some("Pp" | "Lp" | "Tg" | "Bk" | "Fn" | "Fo" | "br")
                )
            {
                pending_function_paragraph = None;
            }
            if !matches!(macro_name.as_deref(), Some("Dd" | "Dt" | "Os"))
                && builder.node_kind(node) != Some(NodeKind::Comment)
            {
                builder.metadata_mut().has_body = true;
            }
            // libmandoc carries the current synopsis presentation bit into
            // the next control line before that line's package validator can
            // switch the section state.  Mark the scanner-owned source node
            // first; newly constructed bodies below select their own state.
            if in_synopsis {
                mark_synopsis_pretty(builder, node);
            }
            if active_column_list(builder, active_body)
                && is_implicit_column_row_macro(macro_name.as_deref())
                && builder
                    .node_flags(node)
                    .is_some_and(|flags| flags.line_start)
                && structure_implicit_column_item(
                    builder,
                    active_body,
                    node,
                    spacing_enabled,
                    max_nodes,
                    &mut outcome,
                    &mut scopes,
                )
            {
                continue;
            }
            if macro_name.as_deref() == Some("Ta")
                && let Some(item) = active_column_item(builder, active_body)
            {
                let tokens = builder
                    .children(node)
                    .map(<[NodeId]>::to_vec)
                    .unwrap_or_default();
                let location = builder.node_location(node);
                if let Some(body) = append_column_ta_cell(
                    builder,
                    active_body,
                    location.clone(),
                    &tokens,
                    spacing_enabled,
                    max_nodes,
                    &mut outcome,
                    &mut scopes,
                ) {
                    if let Some(mut flags) = builder.node_flags(body) {
                        flags.line_start = builder
                            .node_flags(node)
                            .is_some_and(|node_flags| node_flags.line_start);
                        let _ = builder.set_node_flags(body, flags);
                    }
                    outcome
                        .recoveries
                        .push(Recovery::ColumnFirstMacro { location });
                    extend_pending_short_column_item(&mut pending_short_column_items, item);
                    active_body = body;
                    flow_parent = body;
                    continue;
                }
            }
            match macro_name.as_deref() {
                Some("Dd") => {
                    if saw_date_prologue {
                        outcome.recoveries.push(Recovery::DuplicatePrologue {
                            macro_name: "Dd",
                            location: builder.node_location(node),
                        });
                    } else if active_body != root {
                        outcome.recoveries.push(Recovery::LateDate {
                            location: builder.node_location(node),
                        });
                    } else if saw_title_prologue {
                        outcome.recoveries.push(Recovery::DateAfterTitle {
                            location: builder.node_location(node),
                        });
                    }
                    saw_date_prologue = true;
                    if first_date_prologue.is_none() {
                        first_date_prologue = Some((
                            node_arguments(builder, node).join(" ").into_boxed_str(),
                            builder
                                .children(node)
                                .and_then(|children| children.first().copied())
                                .and_then(|argument| builder.node_location(argument)),
                        ));
                    }
                    record_date(builder, node, &mut outcome);
                    coalesce_text_children(builder, node);
                    mark_no_print(builder, node);
                    // A late date request is still no-printing metadata, but
                    // its source node remains in the active body rather than
                    // being hoisted back into the document prologue.
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Dt") => {
                    if active_body == root {
                        if saw_title_prologue {
                            outcome.recoveries.push(Recovery::DuplicatePrologue {
                                macro_name: "Dt",
                                location: builder.node_location(node),
                            });
                        } else if saw_operating_system_prologue {
                            outcome
                                .recoveries
                                .push(Recovery::TitleAfterOperatingSystem {
                                    location: builder.node_location(node),
                                });
                        }
                        saw_title_prologue = true;
                        record_title(builder, node, &mut outcome);
                    } else {
                        outcome.recoveries.push(Recovery::LateTitle {
                            location: builder.node_location(node),
                        });
                    }
                    mark_no_print(builder, node);
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Os") => {
                    if saw_operating_system_prologue {
                        outcome.recoveries.push(Recovery::DuplicatePrologue {
                            macro_name: "Os",
                            location: builder.node_location(node),
                        });
                    } else if active_body != root {
                        outcome.recoveries.push(Recovery::LateOperatingSystem {
                            location: builder.node_location(node),
                        });
                    }
                    saw_operating_system_request = true;
                    let values = node_arguments(builder, node);
                    let operating_system = values.join(" ");
                    if operating_system_flavour.is_none() && !operating_system.is_empty() {
                        operating_system_flavour =
                            Some(mdoc_operating_system_flavour(&operating_system));
                        netbsd_operating_system_validation = operating_system == "NetBSD";
                    }
                    saw_operating_system_prologue = true;
                    record_operating_system(builder, node);
                    if let (Some(flavour), Some(argument)) = (
                        operating_system_flavour,
                        builder
                            .children(node)
                            .and_then(|children| children.first().copied()),
                    ) {
                        if !operating_system.is_empty() {
                            outcome.recoveries.push(Recovery::OperatingSystemExplicit {
                                operating_system: operating_system.clone().into_boxed_str(),
                                flavour,
                                location: builder.node_location(argument),
                            });
                        }
                        if active_body == root
                            && netbsd_operating_system_validation
                            && let Some((date, location)) = &first_date_prologue
                            && date.starts_with("$Mdocdate")
                        {
                            outcome.recoveries.push(Recovery::MdocDateFound {
                                date: date.clone(),
                                location: location.clone(),
                            });
                        }
                    }
                    mark_no_print(builder, node);
                    if active_body == root {
                        root_children.push(node);
                    } else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                    }
                }
                Some("Sh") => {
                    if builder.children(node).is_none_or(<[NodeId]>::is_empty) {
                        outcome.recoveries.push(Recovery::EmptyMacro {
                            macro_name: "Sh",
                            location: builder.node_location(node),
                        });
                        continue;
                    }
                    flush_pending_nd_delimiters(
                        builder,
                        &mut pending_nd_delimiter_bodies,
                        &mut outcome.recoveries,
                    );
                    flush_pending_name_section(
                        builder,
                        &mut pending_name_section_body,
                        &mut outcome.recoveries,
                    );
                    flush_pending_authors_section(
                        builder,
                        &mut pending_authors_body,
                        &mut outcome.recoveries,
                    );
                    let raw_section_title = node_arguments(builder, node).join(" ");
                    outcome
                        .recoveries
                        .extend(mdoc_heading_tab_recoveries(builder, node));
                    let breaks_explicit_partial = scopes
                        .iter()
                        .any(|frame| is_explicit_partial_close(frame.close) || frame.close == "Xc");
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        "Sh",
                        ArgumentPlacement::Head,
                        max_nodes,
                        &mut outcome,
                    ) else {
                        root_children.push(node);
                        continue;
                    };
                    if breaks_explicit_partial && let Some(mut flags) = builder.node_flags(node) {
                        // A section that interrupts a cross-line delimiter
                        // block is retained as that block's continuation,
                        // not as an independent line-start event.
                        flags.line_start = false;
                        let _ = builder.set_node_flags(node, flags);
                    }
                    // A section title is one semantic end-of-line phrase;
                    // scanner words and callable macros remain separate only
                    // until this mdoc structural boundary.
                    let heading_events = split_mdoc_inline_children(
                        builder,
                        head,
                        spacing_enabled,
                        max_nodes,
                        &mut outcome,
                    );
                    let _ = builder.replace_children(head, &heading_events);
                    let heading_scopes = structure_unclosed_explicit_partial_blocks(
                        builder,
                        head,
                        spacing_enabled,
                        max_nodes,
                        &mut outcome,
                    );
                    if !heading_scopes.is_empty()
                        && let Some(mut flags) = builder.node_flags(body)
                    {
                        // `blk_full()` opens the section Body while the
                        // header's cross-line partial is still active.  The
                        // generated Body consequently retains the request's
                        // line-start bit, even though no source text has been
                        // attached to it yet.
                        flags.line_start = true;
                        let _ = builder.set_node_flags(body, flags);
                    }
                    coalesce_adjacent_text_children(builder, head);
                    // `post_sh_head()` validates the rendered title text,
                    // not scanner spellings.  Thus `.Sh SEE Em ALSO` is the
                    // conventional SEE ALSO heading after its callable Em
                    // element has been formed.
                    let section_title =
                        visible_head_text(builder, head).unwrap_or(raw_section_title);
                    in_name_section = section_title.eq_ignore_ascii_case("NAME");
                    if !has_section && !in_name_section {
                        outcome.recoveries.push(Recovery::FirstSectionNotName {
                            section: section_title.clone().into(),
                            location: builder.node_location(node),
                        });
                    }
                    has_section = true;
                    let manual_section = builder.metadata_mut().section.clone();
                    if let Some((rank, canonical_title)) = named_mdoc_section(&section_title) {
                        if last_named_section == Some(rank) {
                            outcome.recoveries.push(Recovery::DuplicateSection {
                                section: canonical_title,
                                location: builder.node_location(node),
                            });
                        } else if last_named_section.is_some_and(|last| rank < last) {
                            outcome.recoveries.push(Recovery::SectionOutOfOrder {
                                section: canonical_title,
                                location: builder.node_location(node),
                            });
                        }
                        last_named_section = Some(rank);
                        if let Some(allowed_sections) =
                            unexpected_section_manuals(canonical_title, manual_section.as_deref())
                        {
                            outcome.recoveries.push(Recovery::UnexpectedSection {
                                section: canonical_title.into(),
                                allowed_sections,
                                location: builder.node_location(node),
                            });
                        }
                    }
                    let next_synopsis = section_title.eq_ignore_ascii_case("SYNOPSIS");
                    target_heads.push(head);
                    if let Some((tag_node, tag)) = pending_manual_tag.take() {
                        mark_target(builder, head, Some(&tag));
                        mark_no_print(builder, tag_node);
                    }
                    for frame in std::mem::take(&mut scopes) {
                        let macro_name = open_name(frame.close);
                        outcome.recoveries.push(Recovery::BrokenBlock {
                            breaker: "Sh",
                            macro_name,
                            location: builder.node_location(node),
                        });
                        if frame.close == "Ek"
                            && builder
                                .children(frame.body)
                                .is_some_and(<[NodeId]>::is_empty)
                        {
                            deferred_post_validation_recoveries.push(Recovery::EmptyBlock {
                                macro_name: "Bk",
                                location: builder.node_location(frame.open),
                            });
                            discard_empty_block(
                                builder,
                                root,
                                &mut root_children,
                                frame.resume_flow,
                                frame.open,
                            );
                        }
                        implicitly_closed.push(frame.close);
                    }
                    section_parent = body;
                    if section_title.eq_ignore_ascii_case("NAME") {
                        pending_name_section_body = Some(body);
                    }
                    if section_title.eq_ignore_ascii_case("AUTHORS") {
                        pending_authors_body = Some(body);
                    }
                    flow_parent = body;
                    active_body = body;
                    if let Some(scope) = heading_scopes.last().copied() {
                        // A partial opener at the end of a section title owns
                        // following physical flow until its closer or the
                        // next section request.  Its resume point is the
                        // public section Head, not the new section Body.
                        flow_parent = scope.body;
                        active_body = scope.body;
                    }
                    scopes.extend(heading_scopes);
                    if in_synopsis {
                        // The just-created Head is still part of the current
                        // control line and inherits the old state.  The Body
                        // starts after `Sh` validation and therefore does not.
                        mark_synopsis_pretty(builder, head);
                    }
                    if next_synopsis {
                        // mandoc's `MDOC_SYNOPSIS` state begins at the section
                        // body, not at the Sh block or its heading.
                        mark_synopsis_pretty(builder, body);
                        synopsis_bodies.push(body);
                    }
                    in_synopsis = next_synopsis;
                    synopsis_from_register = false;
                    synopsis_name_body = None;
                    root_children.push(node);
                }
                Some("Ss") => {
                    if builder.children(node).is_none_or(<[NodeId]>::is_empty) {
                        outcome.recoveries.push(Recovery::EmptyMacro {
                            macro_name: "Ss",
                            location: builder.node_location(node),
                        });
                        continue;
                    }
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        "Ss",
                        ArgumentPlacement::Head,
                        max_nodes,
                        &mut outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    coalesce_adjacent_text_children(builder, head);
                    target_heads.push(head);
                    if let Some((tag_node, tag)) = pending_manual_tag.take() {
                        mark_target(builder, head, Some(&tag));
                        mark_no_print(builder, tag_node);
                    }
                    append_to_parent(builder, root, &mut root_children, section_parent, node);
                    flow_parent = body;
                    active_body = body;
                    synopsis_name_body = None;
                    scopes.clear();
                }
                Some("Nm") if in_synopsis => {
                    // `ctx_synopsis()` dispatches Nm through `blk_full()`.
                    // A top-level name implicitly finishes the preceding
                    // name block, while a name inside an open Fo/Oo/... scope
                    // remains owned by that scope's active Body.
                    //
                    // An authored synopsis name is also the fallback document
                    // name when a malformed NAME section did not contribute
                    // one.  The ordinary Nm branch records this before its
                    // structural work; keep the full-block synopsis path
                    // equivalent rather than relying only on generated empty
                    // Nm expansion below.
                    record_name(builder, node);
                    let nested_scope = !scopes.is_empty();
                    let function_scope = scopes.iter().any(|frame| frame.close == "Fc");
                    if !nested_scope && synopsis_name_body.is_some() {
                        active_body = section_parent;
                        flow_parent = section_parent;
                    }
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        "Nm",
                        ArgumentPlacement::Head,
                        max_nodes,
                        &mut outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    if builder.children(head).is_some_and(<[NodeId]>::is_empty)
                        && !insert_generated_nm_name(builder, node, head, max_nodes)
                        && outcome.node_limit_location.is_none()
                    {
                        outcome.node_limit_location = builder.node_location(node);
                    }
                    mark_synopsis_pretty(builder, node);
                    mark_synopsis_pretty(builder, head);
                    if synopsis_from_register {
                        clear_generated_synopsis_pretty_children(builder, head);
                    }
                    let parent = if nested_scope {
                        active_body
                    } else {
                        section_parent
                    };
                    append_to_parent(builder, root, &mut root_children, parent, node);
                    if function_scope {
                        // `Fc` closes the surrounding Fo on the same source
                        // line, so libmandoc retains the embedded Nm's Block
                        // and Head but no provisional Body.
                        let _ = builder.replace_children(node, &[head]);
                    } else {
                        if inline_events.get(event_index + 1).is_some() {
                            active_body = head;
                            flow_parent = head;
                            synopsis_name_inline_restore = Some((head, body));
                        } else {
                            active_body = body;
                            flow_parent = body;
                        }
                        if !nested_scope {
                            synopsis_name_body = Some(body);
                            synopsis_keep_boundary = false;
                        }
                    }
                }
                Some("Vt") if in_synopsis => {
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        "Vt",
                        ArgumentPlacement::BodyTokens,
                        max_nodes,
                        &mut outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    let children = split_mdoc_inline_children(
                        builder,
                        body,
                        spacing_enabled,
                        max_nodes,
                        &mut outcome,
                    );
                    let _ = builder.replace_children(body, &children);
                    mark_synopsis_pretty(builder, node);
                    mark_synopsis_pretty(builder, head);
                    mark_synopsis_pretty(builder, body);
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Vt") => {
                    let events = split_inline_macro_events(
                        builder,
                        node,
                        spacing_enabled,
                        max_nodes,
                        &mut outcome,
                    );
                    for (event_index, event) in events.into_iter().enumerate() {
                        // A Vt request can split into nested callable macro
                        // events and released punctuation. Only Vt Elements
                        // receive Vt's post-validation; the other events
                        // remain ordinary siblings in source order.
                        if builder.node_macro_name(event) != Some("Vt") {
                            append_to_parent(builder, root, &mut root_children, active_body, event);
                            continue;
                        }
                        if builder.children(event).is_none_or(<[NodeId]>::is_empty) {
                            // Outside SYNOPSIS, Vt is an ordinary inline
                            // element. `post_delim_nb()` reports then deletes
                            // its source-spelled empty form, unlike the
                            // partial block form above, which retains its
                            // Body. Later empty events are temporary
                            // delimiter-split restarts and stay private.
                            if event_index == 0 {
                                outcome.recoveries.push(Recovery::EmptyMacro {
                                    macro_name: "Vt",
                                    location: builder.node_location(event),
                                });
                            }
                            continue;
                        }
                        validate_no_break_trailing_delimiter(
                            builder,
                            event,
                            "Vt",
                            &mut deferred_post_validation_recoveries,
                        );
                        append_to_parent(builder, root, &mut root_children, active_body, event);
                    }
                }
                Some("Eo") => {
                    // Eo is the exceptional explicit partial block: its first
                    // argument belongs to Head and an Ec later supplies a
                    // third Tail child only when Ec actually arrives.  An
                    // unclosed Eo retains its observable Head/Body prefix.
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        "Eo",
                        ArgumentPlacement::Head,
                        max_nodes,
                        &mut outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    scopes.push(ScopeFrame {
                        close: "Ec",
                        open: node,
                        body,
                        tail_on_close: true,
                        transparent_target_taken: false,
                        suppress_implicit_ancestor_break: false,
                        resume_active: active_body,
                        resume_flow: flow_parent,
                    });
                    // Keep `head` bound in the branch: it is deliberately the
                    // parser-owned holder of Eo's one opening argument.
                    let _ = head;
                    flow_parent = body;
                    active_body = body;
                }
                Some(name) if explicit_partial_block_close(name).is_some() => {
                    let close = explicit_partial_block_close(name)
                        .expect("the guard checked this explicit partial block");
                    let closes_on_line = builder.children(node).is_some_and(|children| {
                        matching_explicit_partial_close_index(builder, children, close).is_some()
                    });
                    let tail = split_explicit_partial_block_tail(builder, node, close);
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        name,
                        ArgumentPlacement::BodyTokens,
                        max_nodes,
                        &mut outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    if in_synopsis {
                        // The scanner marks the authored opener before this
                        // branch manufactures its structural Head/Body.
                        // Those generated containers inherit the current
                        // synopsis state even if a following `.nr nS 0`
                        // appears before the explicit closer.
                        mark_synopsis_pretty(builder, head);
                        mark_synopsis_pretty(builder, body);
                    }
                    structure_matched_explicit_partial_blocks(
                        builder,
                        body,
                        spacing_enabled,
                        max_nodes,
                        &mut outcome,
                    );
                    let nested_scopes = if closes_on_line {
                        Vec::new()
                    } else {
                        structure_unclosed_explicit_partial_blocks(
                            builder,
                            body,
                            spacing_enabled,
                            max_nodes,
                            &mut outcome,
                        )
                    };
                    let children = split_mdoc_inline_children(
                        builder,
                        body,
                        spacing_enabled,
                        max_nodes,
                        &mut outcome,
                    );
                    let _ = builder.replace_children(body, &children);
                    clear_leading_explicit_partial_punctuation(builder, body);
                    move_explicit_leading_open_delimiter(builder, node, head, body);
                    structure_nested_implicit_partial_blocks(
                        builder,
                        body,
                        max_nodes,
                        &mut outcome,
                        spacing_enabled,
                    );
                    if matches!(name, "Bo" | "Do" | "Po") {
                        // Scanner control arguments begin as separate lexical
                        // children. An ordinary `Bo in brackets` body is one
                        // mdoc phrase in the legacy owned AST, including when
                        // a later `Bc` is extended with `.am`.
                        coalesce_adjacent_text_children(builder, body);
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    if !closes_on_line {
                        scopes.push(ScopeFrame {
                            close,
                            open: node,
                            body,
                            tail_on_close: false,
                            transparent_target_taken: false,
                            suppress_implicit_ancestor_break: false,
                            resume_active: active_body,
                            resume_flow: flow_parent,
                        });
                        flow_parent = body;
                        active_body = body;
                        for nested_scope in nested_scopes {
                            active_body = nested_scope.body;
                            flow_parent = nested_scope.body;
                            scopes.push(nested_scope);
                        }
                    }
                    append_explicit_partial_tail(
                        builder,
                        root,
                        &mut root_children,
                        &mut scopes,
                        &mut implicitly_closed,
                        &mut active_body,
                        &mut flow_parent,
                        node,
                        &tail,
                        false,
                        spacing_enabled,
                        max_nodes,
                        &mut outcome,
                    );
                }
                Some(name) if is_implicit_partial_block_macro(name) => {
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        name,
                        ArgumentPlacement::BodyTokens,
                        max_nodes,
                        &mut outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    // An explicit partial closer in an implicit partial
                    // request is still callable syntax.  `mdoc_macro.c`
                    // splits the surrounding Body around it, inserts the
                    // closed explicit block's empty Body at the call site,
                    // and resumes parsing the remaining implicit argument.
                    // Keeping the source token as plain text would lose both
                    // the public boundary and the `Bo breaks Pq` recovery.
                    let raw_children = builder
                        .children(body)
                        .map(<[NodeId]>::to_vec)
                        .unwrap_or_default();
                    // An otherwise empty argument produced solely by a
                    // failed interpolation is parser-private for implicit
                    // partial blocks.  Keep authored `""` arguments (whose
                    // width delta is zero), but remove the placeholder before
                    // the block body is projected so `.Sq \\*[missing] .`
                    // has the legacy empty Body rather than a visible empty
                    // Text child.
                    let raw_children = raw_children
                        .into_iter()
                        .filter(|token| {
                            !(builder.node_text(*token) == Some("")
                                && builder.node_argument_expansion_width_delta(*token) < 0)
                        })
                        .collect::<Vec<_>>();
                    let mut enclosed_explicit_closes = Vec::new();
                    let mut pending_tokens = Vec::new();
                    let mut children = Vec::new();
                    for token in raw_children {
                        let close = builder
                            .node_text(token)
                            .filter(|close| is_explicit_partial_close(close))
                            .filter(|close| scopes.iter().any(|frame| frame.close == *close))
                            .map(str::to_owned);
                        let Some(close) = close else {
                            pending_tokens.push(token);
                            continue;
                        };
                        children.extend(split_mdoc_inline_tokens(
                            builder,
                            body,
                            &pending_tokens,
                            spacing_enabled,
                            max_nodes,
                            &mut outcome,
                        ));
                        pending_tokens.clear();
                        let location = builder.node_location(token);
                        let Some(closed_body) = builder.push(body, NodeKind::Body) else {
                            if outcome.node_limit_location.is_none() {
                                outcome.node_limit_location = location;
                            }
                            pending_tokens.push(token);
                            continue;
                        };
                        if !builder.macro_name(closed_body, open_name(&close))
                            || !builder.set_node_location(closed_body, location.clone())
                        {
                            pending_tokens.push(token);
                            continue;
                        }
                        children.push(closed_body);
                        enclosed_explicit_closes.push((close, location, closed_body));
                    }
                    children.extend(split_mdoc_inline_tokens(
                        builder,
                        body,
                        &pending_tokens,
                        spacing_enabled,
                        max_nodes,
                        &mut outcome,
                    ));
                    let mut children =
                        expand_fl_elements(builder, body, children, max_nodes, &mut outcome);
                    insert_generated_system_names(builder, &children, max_nodes, &mut outcome);
                    let tail = take_implicit_partial_tail(builder, &mut children);
                    let _ = builder.replace_children(body, &children);
                    move_leading_open_delimiters(builder, node, head, body);
                    clear_initial_implicit_body_delimiter_flags(builder, body);
                    clear_terminal_implicit_body_opening_flags(builder, body);
                    mark_implicit_partial_tail_sentence_ends(builder, &tail);
                    if spacing_enabled && name != "Op" {
                        coalesce_implicit_partial_body_text(builder, body);
                    }
                    structure_nested_implicit_partial_blocks(
                        builder,
                        body,
                        max_nodes,
                        &mut outcome,
                        spacing_enabled,
                    );
                    // A direct explicit opener is an element of this
                    // implicit request (`.Op … Do …`), while an opener inside
                    // a nested implicit block is only discovered after that
                    // block has been projected.  Both must enter the physical
                    // closer stack: a later `.Dc` then reports that the
                    // enclosing `.Op` broke the `.Do` rather than becoming an
                    // inert text control.
                    let mut nested_implicit_scopes = structure_unclosed_explicit_partial_blocks(
                        builder,
                        body,
                        spacing_enabled,
                        max_nodes,
                        &mut outcome,
                    );
                    nested_implicit_scopes.extend(structure_nested_implicit_explicit_scopes(
                        builder,
                        body,
                        max_nodes,
                        &mut outcome,
                        spacing_enabled,
                    ));
                    if spacing_enabled && name == "Op" {
                        // `Op` keeps the lexical argument boundaries that
                        // precede a crossed explicit closer, but prose resumed
                        // after that closer is once again an ordinary phrase.
                        // A nested implicit partial owns the closer when it
                        // is the immediately preceding construct.
                        for (_, _, closed_body) in &enclosed_explicit_closes {
                            let parent = relocate_crossed_closer_to_nested_implicit_body(
                                builder,
                                body,
                                *closed_body,
                            )
                            .unwrap_or(body);
                            coalesce_text_children_after(builder, parent, *closed_body);
                        }
                    }
                    if !tail.is_empty() {
                        let mut block_children = vec![head, body];
                        block_children.extend(tail);
                        let _ = builder.replace_children(node, &block_children);
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    for (close, location, _) in enclosed_explicit_closes {
                        if scopes.iter().any(|frame| frame.close == close) {
                            syntax_stage_recoveries.push(Recovery::BadlyNestedBlock {
                                breaker: open_name(&close),
                                interrupted: implicit_partial_block_name(name),
                                location,
                            });
                        }
                        close_explicit_partial_scope(
                            &mut scopes,
                            &mut implicitly_closed,
                            &mut active_body,
                            &mut flow_parent,
                            &close,
                        );
                    }
                    for scope in &mut nested_implicit_scopes {
                        // The surrounding implicit blocks close at the end
                        // of their source request.  A later explicit closer
                        // therefore resumes this request's outer flow, not
                        // an implicit ancestor that has no cross-line scope.
                        scope.resume_active = active_body;
                        scope.resume_flow = flow_parent;
                    }
                    for scope in nested_implicit_scopes {
                        active_body = scope.body;
                        flow_parent = scope.body;
                        scopes.push(scope);
                    }
                }
                Some("Sm") => {
                    let arguments = node_arguments(builder, node);
                    let children = builder.children(node).unwrap_or_default().to_vec();
                    let mut relocated_arguments = Vec::new();
                    match arguments.first().map(String::as_str) {
                        Some("off") => {
                            spacing_enabled = false;
                            relocated_arguments.extend_from_slice(&children[1..]);
                            let _ = builder.replace_children(node, &children[..1]);
                        }
                        Some("on") => {
                            spacing_enabled = true;
                            relocated_arguments.extend_from_slice(&children[1..]);
                            let _ = builder.replace_children(node, &children[..1]);
                        }
                        None => spacing_enabled = !spacing_enabled,
                        Some(argument) => {
                            outcome.recoveries.push(Recovery::InvalidBooleanArgument {
                                macro_name: "Sm",
                                argument: argument.into(),
                                location: argument_location(builder, node, 0),
                            });
                            // `post_sm()` detaches only an invalid first
                            // argument and its remaining source siblings,
                            // relinking them immediately after the control
                            // node.  Keeping that source-order flow makes
                            // later inline validation observe the same
                            // boundary as libmandoc.
                            relocated_arguments.extend_from_slice(&children);
                            let _ = builder.replace_children(node, &[]);
                        }
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    for argument in relocated_arguments {
                        append_to_parent(builder, root, &mut root_children, active_body, argument);
                    }
                }
                Some(name) if is_reference_field_macro(name) => {
                    // Bibliographic fields inside an Rs block use the
                    // end-of-line argument grammar.  Only the fields marked
                    // MDOC_JOIN by the package coalesce ordinary source
                    // words; the numeric/page/URL fields retain individual
                    // text nodes.
                    if reference_field_joins_arguments(name) {
                        coalesce_adjacent_text_children(builder, node);
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Tg") => {
                    if let Some((tag_node, tag)) = pending_manual_tag.take()
                        && !tag.is_empty()
                    {
                        // Consecutive manual destinations do not make the
                        // earlier tag transparent.  With no later eligible
                        // semantic target, `post_tg()` keeps it as its own
                        // deep-link destination.
                        mark_destination(builder, tag_node);
                    }
                    let reference_transparent =
                        scopes.last().is_some_and(|frame| frame.close == "Re");
                    let arguments = builder
                        .children(node)
                        .map(<[NodeId]>::to_vec)
                        .unwrap_or_default();
                    let xo_preceding_target = scopes
                        .last()
                        .filter(|frame| frame.close == "Xc")
                        .and_then(|frame| {
                            builder.children(frame.resume_active).and_then(|children| {
                                children
                                    .iter()
                                    .position(|child| *child == frame.open)
                                    .and_then(|index| {
                                        children[..index].iter().rev().copied().find(|candidate| {
                                            matches!(
                                                builder.node_macro_name(*candidate),
                                                Some("Pp" | "Lp")
                                            )
                                        })
                                    })
                            })
                        });
                    let fl_preceding_target = builder.children(active_body).and_then(|children| {
                        let last = children.last().copied()?;
                        (builder.node_macro_name(last) == Some("Fl")
                            && builder.children(last).is_some_and(<[NodeId]>::is_empty))
                        .then(|| {
                            children[..children.len().saturating_sub(1)]
                                .iter()
                                .rev()
                                .copied()
                                .find(|candidate| {
                                    matches!(builder.node_macro_name(*candidate), Some("Pp" | "Lp"))
                                })
                        })
                        .flatten()
                    });
                    if scopes.last().is_some_and(|frame| {
                        frame.close == "Fc" && (!in_synopsis || !frame.transparent_target_taken)
                    }) {
                        // Function argument validation treats an in-body Tg
                        // as a transparent destination node.  It remains
                        // visible syntax and does not expose its argument as
                        // the public tag string.
                        mark_destination(builder, node);
                        if in_synopsis {
                            scopes
                                .last_mut()
                                .expect("the matching Fo scope was just checked")
                                .transparent_target_taken = true;
                        }
                    } else if scopes.last().is_some_and(|frame| frame.close == "Fc") {
                        // Later transparent anchors in the same function
                        // block are validation-only syntax.
                        mark_no_print(builder, node);
                    }
                    if reference_transparent {
                        // Reference lists retain transparent tags as direct
                        // destinations, independent of their invalid-content
                        // recovery in `post_rs()`.
                        mark_destination(builder, node);
                    }
                    let first_tag = arguments.first().and_then(|argument| {
                        builder
                            .node_text(*argument)
                            .map(|tag| (*argument, tag.to_owned()))
                    });
                    if let Some((argument, tag)) = first_tag {
                        if tag.is_empty() {
                            outcome.recoveries.push(Recovery::EmptyMacro {
                                macro_name: "Tg",
                                location: builder.node_location(node),
                            });
                            if arguments.len() > 1 {
                                let excess = builder.node_text(arguments[1]).unwrap_or_default();
                                outcome.recoveries.push(Recovery::InvalidArguments {
                                    message: format!("skipping excess arguments: Tg ... {excess}")
                                        .into(),
                                    location: argument_location(builder, node, 1),
                                });
                            }
                            continue;
                        }
                        if let Some(offset) = tag
                            .bytes()
                            .position(|byte| byte.is_ascii_whitespace() || byte == b'\\')
                        {
                            outcome.recoveries.push(Recovery::InvalidTag {
                                tag: tag.into(),
                                location: text_offset_location(builder, argument, offset)
                                    .or_else(|| builder.node_location(argument)),
                            });
                            continue;
                        }
                        if arguments.len() > 1 {
                            let excess = arguments[1..]
                                .iter()
                                .filter_map(|argument| builder.node_text(*argument))
                                .collect::<Vec<_>>()
                                .join(" ");
                            outcome.recoveries.push(Recovery::InvalidArguments {
                                message: format!("skipping excess arguments: Tg ... {excess}")
                                    .into(),
                                location: argument_location(builder, node, 1),
                            });
                            let _ = builder.replace_children(node, &arguments[..1]);
                        }
                        if let Some(target) = xo_preceding_target {
                            // A Tg nested in a cross-line Xo is transparent
                            // syntax. `post_tg()` returns its destination to
                            // the preceding outer-flow node rather than
                            // carrying it through Xc to later source flow.
                            mark_manual_target(builder, target, &tag);
                            mark_no_print(builder, node);
                        } else if let Some(target) = fl_preceding_target {
                            mark_manual_target(builder, target, &tag);
                            mark_no_print(builder, node);
                            pending_transparent_permalink = Some(tag);
                        } else if reference_transparent {
                            // The direct Tg remains the destination; it does
                            // not carry a public tag string forward.
                        } else {
                            pending_manual_tag = Some((node, tag));
                        }
                    } else if arguments.is_empty() && !reference_transparent {
                        // `.Tg` may borrow the first text child of the next
                        // node as its manual destination spelling.  Preserve
                        // that unresolved form until a supported follower can
                        // supply the text, rather than treating an empty Tg as
                        // an empty public tag.
                        pending_manual_tag = Some((node, String::new()));
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Pp" | "Lp") => {
                    // mandoc parses Pp as an in-line, end-of-line macro rather
                    // than a Head/Body block.  Its arguments remain observable
                    // children (and validation decides whether to diagnose them),
                    // while following source lines stay in the surrounding flow.
                    // Lp is an obsolete spelling that validation normalizes to Pp.
                    if macro_name.as_deref() == Some("Lp") {
                        let _ = builder.macro_name(node, "Pp");
                    }
                    if let Some(argument) = node_arguments(builder, node).first() {
                        deferred_paragraph_argument_recoveries.push(Recovery::InvalidArguments {
                            message: format!("skipping all arguments: Pp {argument}").into(),
                            location: builder.node_location(node),
                        });
                    }
                    if let Some((tag_node, tag)) = pending_manual_tag.take() {
                        mark_manual_target(builder, node, &tag);
                        mark_no_print(builder, tag_node);
                        pending_paragraph_href = Some(tag);
                    }
                    function_target_taken = false;
                    pending_function_paragraph = Some(node);
                    if in_synopsis && synopsis_keep_boundary {
                        // A paragraph boundary ends the current SYNOPSIS Nm
                        // block.  The Pp itself is a section-body sibling;
                        // keeping it in the Nm Body loses the observable
                        // input-line boundary before the next synopsis name.
                        flow_parent = section_parent;
                        synopsis_name_body = None;
                        synopsis_keep_boundary = false;
                    }
                    append_to_parent(builder, root, &mut root_children, flow_parent, node);
                    active_body = flow_parent;
                }
                Some("br") => {
                    let arguments = node_arguments(builder, node);
                    if !arguments.is_empty() {
                        outcome.recoveries.push(Recovery::InvalidArguments {
                            message: format!("skipping all arguments: br {}", arguments.join(" "))
                                .into(),
                            location: argument_location(builder, node, 0),
                        });
                        let _ = builder.replace_children(node, &[]);
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("sp") => {
                    let arguments = builder
                        .children(node)
                        .map(<[NodeId]>::to_vec)
                        .unwrap_or_default();
                    if arguments.len() > 1 {
                        let excess = arguments[1..]
                            .iter()
                            .filter_map(|argument| builder.node_text(*argument))
                            .collect::<Vec<_>>()
                            .join(" ");
                        outcome.recoveries.push(Recovery::InvalidArguments {
                            message: format!("skipping excess arguments: sp ... {excess}").into(),
                            location: argument_location(builder, node, 1),
                        });
                        let _ = builder.replace_children(node, &arguments[..1]);
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Nd") => {
                    // A successive description request finishes the prior
                    // one before the new Body becomes active.  This mirrors
                    // libmandoc's post-order validation rather than checking
                    // only the control-line argument.
                    flush_pending_nd_delimiters(
                        builder,
                        &mut pending_nd_delimiter_bodies,
                        &mut outcome.recoveries,
                    );
                    let Some((_, body)) = make_block(
                        builder,
                        node,
                        "Nd",
                        ArgumentPlacement::Body,
                        max_nodes,
                        &mut outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    if !in_name_section {
                        outcome.recoveries.push(Recovery::DescriptionOutsideName {
                            location: builder.node_location(node),
                        });
                    }
                    pending_nd_delimiter_bodies.push(body);
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    active_body = body;
                    flow_parent = body;
                }
                Some("Nm") => {
                    record_name(builder, node);
                    if builder.children(node).is_some_and(<[NodeId]>::is_empty) {
                        if builder.metadata_mut().name.is_none() {
                            outcome.recoveries.push(Recovery::MissingName {
                                location: builder.node_location(node),
                            });
                        } else if !insert_generated_nm_name(builder, node, node, max_nodes)
                            && outcome.node_limit_location.is_none()
                        {
                            outcome.node_limit_location = builder.node_location(node);
                        }
                    }
                    // `Nm` follows the no-break trailing-delimiter validator,
                    // rather than the generic tag validator: its one or more
                    // name words remain the element's complete phrase.
                    validate_no_break_trailing_delimiter(
                        builder,
                        node,
                        "Nm",
                        &mut deferred_post_validation_recoveries,
                    );
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    if !matches!(macro_name.as_deref(), Some("Fl" | "No"))
                        && let Some(close) = scopes.last().map(|frame| frame.close)
                        && node_arguments(builder, node)
                            .iter()
                            .any(|argument| argument == close)
                    {
                        let frame = scopes.pop().expect("last scope was checked");
                        active_body = frame.resume_active;
                        flow_parent = frame.resume_flow;
                    }
                }
                Some("Fl") => {
                    let elements =
                        expand_fl_elements(builder, root, vec![node], max_nodes, &mut outcome);
                    for element in &elements {
                        validate_tag(
                            builder,
                            *element,
                            "Fl",
                            &mut deferred_post_validation_recoveries,
                        );
                    }
                    if let Some((tag_node, tag)) = pending_manual_tag.take()
                        && let Some(element) = elements.first()
                    {
                        if tag.is_empty() {
                            if builder
                                .children(*element)
                                .and_then(|children| children.first())
                                .and_then(|child| builder.node_text(*child))
                                .is_some_and(|text| !text.is_empty())
                            {
                                mark_target(builder, *element, None);
                                mark_no_print(builder, tag_node);
                            }
                        } else {
                            mark_target(builder, *element, Some(&tag));
                            mark_no_print(builder, tag_node);
                        }
                    }
                    for element in elements {
                        append_to_parent(builder, root, &mut root_children, active_body, element);
                    }
                }
                Some("Ar") => {
                    // The argument macro has a semantic default rather than
                    // an empty rendering: mandoc synthesizes `file ...` as
                    // two generated words, including in SYNOPSIS.
                    if builder.children(node).is_some_and(<[NodeId]>::is_empty)
                        && !insert_generated_ar_default(builder, node, node, max_nodes)
                        && outcome.node_limit_location.is_none()
                    {
                        outcome.node_limit_location = builder.node_location(node);
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Fn") => {
                    validate_tag(
                        builder,
                        node,
                        "Fn",
                        &mut deferred_post_validation_recoveries,
                    );
                    validate_function_name(builder, node, &mut outcome.recoveries);
                    validate_function_argument_commas(builder, node, &mut outcome.recoveries);
                    let function_name = node_arguments(builder, node).first().cloned();
                    // mdoc's automatic function destination is the first
                    // space-delimited component of its first parsed argument
                    // (including when that argument came from a quoted
                    // prototype phrase), not the whole display spelling.
                    let function_tag = function_name
                        .as_deref()
                        .and_then(automatic_mdoc_function_tag);
                    if let Some(function_tag) = function_tag {
                        automatic_function_tag_occurrences.push(function_tag.to_owned());
                    }
                    let paragraph = pending_function_paragraph.take();
                    if let Some((tag_node, tag)) = pending_manual_tag.take() {
                        if let Some(paragraph) = paragraph {
                            mark_manual_target(builder, paragraph, &tag);
                            mark_permalink(builder, node, Some(&tag));
                        } else {
                            mark_target(builder, node, Some(&tag));
                        }
                        mark_no_print(builder, tag_node);
                        function_target_taken = true;
                    } else if !in_synopsis
                        && let (Some(paragraph), Some(function_name)) =
                            (paragraph, function_name.as_deref())
                    {
                        mark_manual_target(builder, paragraph, function_name);
                        mark_permalink(builder, node, None);
                        function_target_taken = true;
                    } else if !in_synopsis && !function_target_taken {
                        // A standalone function declaration owns its normal
                        // destination spelling, unlike the paragraph-target
                        // form above where the function is only a permalink.
                        mark_target(builder, node, None);
                        if let Some(function_tag) = function_tag {
                            automatic_function_targets.push((node, function_tag.to_owned(), true));
                        }
                        function_target_taken = true;
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Ft") => {
                    validate_tag(
                        builder,
                        node,
                        "Ft",
                        &mut deferred_post_validation_recoveries,
                    );
                    // A function-type declaration starts a new declaration
                    // context.  This differs from ordinary paragraph prose:
                    // each following standalone `.Fn` is eligible for its
                    // own automatic destination.
                    function_target_taken = false;
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Fa") => {
                    validate_tag(
                        builder,
                        node,
                        "Fa",
                        &mut deferred_post_validation_recoveries,
                    );
                    validate_function_argument_commas(builder, node, &mut outcome.recoveries);
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Lk") => {
                    // Unlike the ordinary tag macros, `Lk` keeps all source
                    // punctuation inside its element. A truly empty request
                    // has no public node; attached delimiters are checked by
                    // the same delayed validator used by the legacy parser.
                    if builder.children(node).is_none_or(<[NodeId]>::is_empty) {
                        outcome.recoveries.push(Recovery::EmptyMacro {
                            macro_name: "Lk",
                            location: builder.node_location(node),
                        });
                        continue;
                    }
                    mark_link_terminal_delimiter(builder, node);
                    validate_tag(
                        builder,
                        node,
                        "Lk",
                        &mut deferred_post_validation_recoveries,
                    );
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Mt") => {
                    if builder.children(node).is_none_or(<[NodeId]>::is_empty)
                        && !insert_generated_nonbreaking_default(builder, node, max_nodes)
                        && outcome.node_limit_location.is_none()
                    {
                        outcome.node_limit_location = builder.node_location(node);
                    }
                    validate_tag(
                        builder,
                        node,
                        "Mt",
                        &mut deferred_post_validation_recoveries,
                    );
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Ot") => {
                    // `Ot` is an obsolete spelling of `Ft`: the diagnostic
                    // retains the authored name, while public AST consumers
                    // receive the normalized contemporary macro.
                    outcome.recoveries.push(Recovery::Obsolete {
                        macro_name: "Ot",
                        location: builder.node_location(node),
                    });
                    let _ = builder.macro_name(node, "Ft");
                    function_target_taken = false;
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Fr") => {
                    // Unlike `Ot`, obsolete `Fr` retains its original public
                    // element identity after validation.
                    outcome.recoveries.push(Recovery::Obsolete {
                        macro_name: "Fr",
                        location: builder.node_location(node),
                    });
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("An") => {
                    validate_an(builder, node, &mut outcome);
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("At") => {
                    let siblings =
                        validate_at(builder, node, spacing_enabled, max_nodes, &mut outcome);
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    for sibling in siblings {
                        append_to_parent(builder, root, &mut root_children, active_body, sibling);
                    }
                }
                Some("St") => {
                    let Some(selector) = builder
                        .children(node)
                        .and_then(|children| children.first())
                        .copied()
                    else {
                        outcome.recoveries.push(Recovery::EmptyMacro {
                            macro_name: "St",
                            location: builder.node_location(node),
                        });
                        continue;
                    };
                    let Some(selector_text) = builder.node_text(selector).map(str::to_owned) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    let Some(expanded) = standard_description(&selector_text) else {
                        // post_st() runs during the validator walk, after an
                        // empty later St has been diagnosed. Keep the error
                        // in that post-validation queue rather than exposing
                        // scanner source order as a compatibility difference.
                        deferred_post_validation_recoveries.push(Recovery::UnknownStandard {
                            standard: selector_text.into(),
                            location: builder.node_location(selector),
                        });
                        continue;
                    };
                    if builder.node_count() >= max_nodes {
                        if outcome.node_limit_location.is_none() {
                            outcome.node_limit_location = builder.node_location(node);
                        }
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    }
                    let Some(expansion) = push_generated_text_at(
                        builder,
                        node,
                        expanded,
                        false,
                        builder.node_location(selector),
                    ) else {
                        if outcome.node_limit_location.is_none() {
                            outcome.node_limit_location = builder.node_location(node);
                        }
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    mark_no_print(builder, selector);
                    // `post_st()` inserts its source-less expansion ahead of
                    // the now-hidden authored selector in the public tree.
                    let _ = builder.replace_children(node, &[expansion, selector]);
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Sx") => {
                    if builder.children(node).is_none_or(<[NodeId]>::is_empty) {
                        outcome.recoveries.push(Recovery::EmptyMacro {
                            macro_name: "Sx",
                            location: builder.node_location(node),
                        });
                        continue;
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Ta")
                    if !scopes
                        .iter()
                        .rev()
                        .find(|frame| frame.close == "El")
                        .is_some_and(|list| {
                            builder.node_list_kind(list.body) == Some(NormalizedListKind::Column)
                        }) =>
                {
                    outcome.recoveries.push(Recovery::ColumnOutsideColumnList {
                        location: builder.node_location(node),
                    });
                }
                Some("Cd") => {
                    // Cd is an in-line callable macro with MDOC_JOIN: its
                    // ordinary direct arguments form one configuration
                    // phrase, while trailing punctuation remains outer flow.
                    if builder.children(node).is_none_or(<[NodeId]>::is_empty) {
                        // Inline splitting may have detached leading closing
                        // punctuation and a later ordinary word from Cd. The
                        // empty element is still private syntax, but mandoc
                        // warns only when no non-delimiter flow remains.
                        let has_non_delimiter_follower = inline_events[event_index + 1..]
                            .iter()
                            .copied()
                            .any(|follower| {
                                builder
                                    .node_text(follower)
                                    .is_none_or(|text| !is_mdoc_closing_delimiter(text))
                            });
                        if !has_non_delimiter_follower {
                            outcome.recoveries.push(Recovery::EmptyMacro {
                                macro_name: "Cd",
                                location: builder.node_location(node),
                            });
                        }
                    } else {
                        coalesce_adjacent_text_children(builder, node);
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                    }
                }
                Some("In") => {
                    // `In` is a one-argument inline request.  Validation
                    // removes a truly empty request, while a populated final
                    // argument uses the normal no-break delimiter rule.
                    if builder.children(node).is_none_or(<[NodeId]>::is_empty) {
                        outcome.recoveries.push(Recovery::EmptyMacro {
                            macro_name: "In",
                            location: builder.node_location(node),
                        });
                        continue;
                    }
                    validate_tag(
                        builder,
                        node,
                        "In",
                        &mut deferred_post_validation_recoveries,
                    );
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Xr") => {
                    let arguments = node_arguments(builder, node);
                    if arguments.is_empty() {
                        // `in_line_argn()` deletes a source-spelled empty
                        // cross reference. Any detached punctuation remains
                        // normal outer flow unless it was the sole closing
                        // delimiter owned by this empty request.
                        outcome.recoveries.push(Recovery::EmptyMacro {
                            macro_name: "Xr",
                            location: builder.node_location(node),
                        });
                        if let Some(delimiter) =
                            inline_events
                                .get(event_index + 1)
                                .copied()
                                .filter(|candidate| {
                                    builder
                                        .node_text(*candidate)
                                        .is_some_and(is_mdoc_closing_delimiter)
                                })
                        {
                            suppressed_inline_events.insert(delimiter);
                        }
                        continue;
                    }
                    if arguments.len() == 1 {
                        // post_xr() runs during the validator sweep, after
                        // later source-line empty requests have been seen.
                        // Keep this alongside delimiter styles to preserve
                        // the legacy document-order post-validation sequence.
                        deferred_post_validation_recoveries.push(
                            Recovery::MissingReferenceSection {
                                name: arguments[0].clone().into_boxed_str(),
                                location: builder.node_location(node),
                            },
                        );
                    }
                    validate_no_break_trailing_delimiter(
                        builder,
                        node,
                        "Xr",
                        &mut deferred_post_validation_recoveries,
                    );
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Lb") => {
                    let mut outer_delimiters = Vec::new();
                    if !validate_library(
                        builder,
                        node,
                        max_nodes,
                        &mut outcome,
                        &mut deferred_post_validation_recoveries,
                        &mut outer_delimiters,
                    ) {
                        continue;
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    for delimiter in outer_delimiters {
                        append_to_parent(builder, root, &mut root_children, active_body, delimiter);
                    }
                }
                Some("Ex") => {
                    // `Ex -std` is semantic syntax rather than a renderer
                    // abbreviation: mdoc expands it into generated prose and
                    // generated Nm elements around the selected utilities.
                    // Keep non-standard invocations intact until the broader
                    // argument-validation family is implemented.
                    if !expand_standard_exit_status(builder, node, max_nodes, &mut outcome)
                        && outcome.node_limit_location.is_none()
                    {
                        outcome.node_limit_location = builder.node_location(node);
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Rv") => {
                    // `Rv -std` shares Ex's validated name-list grammar but
                    // expands into the standard return-value sentence.
                    if !expand_standard_return_value(builder, node, max_nodes, &mut outcome)
                        && outcome.node_limit_location.is_none()
                    {
                        outcome.node_limit_location = builder.node_location(node);
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Bx") => {
                    if !insert_generated_system_name(builder, node, "Bx", max_nodes)
                        && outcome.node_limit_location.is_none()
                    {
                        outcome.node_limit_location = builder.node_location(node);
                    }
                    // `mdoc_args()` retains an outer quote on a standalone
                    // delimiter. `append_delims()` preserves its delimiter
                    // role but does not mark it as an end of sentence (the
                    // Bx regression fixture uses `.Bx 4.4 "."`).
                    clear_quoted_bx_trailing_delimiter_sentence_end(
                        builder,
                        inline_events.get(event_index + 1).copied(),
                    );
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Db") => {
                    // `Db` remains a visible, end-of-line compatibility
                    // request; validation only marks each use obsolete.
                    outcome.recoveries.push(Recovery::Obsolete {
                        macro_name: "Db",
                        location: builder.node_location(node),
                    });
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some(name) if generated_system_name(name).is_some() => {
                    // These mdoc system-name macros have an AST-visible
                    // default word.  It must be allocated before the source
                    // punctuation is attached to the surrounding flow: the
                    // source parser gives Ux no arguments and gives the
                    // other variants at most their documented version/name
                    // prefix.
                    if !insert_generated_system_name(builder, node, name, max_nodes)
                        && outcome.node_limit_location.is_none()
                    {
                        outcome.node_limit_location = builder.node_location(node);
                    }
                    validate_no_break_trailing_delimiter(
                        builder,
                        node,
                        system_macro_name(name),
                        &mut deferred_post_validation_recoveries,
                    );
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Pf") => {
                    let prior_instance = inline_events[..event_index].iter().any(|previous| {
                        builder.node_macro_name(*previous) == Some("Pf")
                            && builder.node_location(*previous) == builder.node_location(node)
                    });
                    if !prior_instance {
                        validate_prefix_following(
                            builder,
                            node,
                            &inline_events[event_index + 1..],
                            &mut deferred_post_validation_recoveries,
                        );
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Pa") => {
                    // Like `.Mt`, an empty path macro has a semantic
                    // nonbreaking-space default. Delimiter splitting can
                    // leave the element empty before its punctuation is
                    // published into surrounding flow.
                    if builder.children(node).is_none_or(<[NodeId]>::is_empty)
                        && !insert_generated_nonbreaking_default(builder, node, max_nodes)
                        && outcome.node_limit_location.is_none()
                    {
                        outcome.node_limit_location = builder.node_location(node);
                    }
                    validate_no_break_trailing_delimiter(
                        builder,
                        node,
                        "Pa",
                        &mut deferred_post_validation_recoveries,
                    );
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Tn") => {
                    if builder.children(node).is_none_or(<[NodeId]>::is_empty) {
                        outcome.recoveries.push(Recovery::EmptyMacro {
                            macro_name: "Tn",
                            location: builder.node_location(node),
                        });
                        continue;
                    }
                    deferred_post_validation_recoveries.push(Recovery::UselessMacro {
                        macro_name: "Tn",
                        location: builder.node_location(node),
                    });
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Ud" | "Bt") => {
                    let (macro_name, generated_sentence) = match macro_name.as_deref() {
                        Some("Ud") => ("Ud", "currently under development."),
                        Some("Bt") => ("Bt", "is currently in beta test."),
                        _ => unreachable!("match arm fixes the compatibility macro spelling"),
                    };
                    outcome.recoveries.push(Recovery::UselessMacro {
                        macro_name,
                        location: builder.node_location(node),
                    });
                    let arguments = node_arguments(builder, node);
                    if let Some(first_argument) = arguments.first() {
                        outcome.recoveries.push(Recovery::InvalidArguments {
                            // These obsolete macros discard their whole
                            // tail, while mandoc's diagnostic prints only the
                            // first argument as the representative spelling.
                            message: format!(
                                "skipping all arguments: {macro_name} {first_argument}"
                            )
                            .into(),
                            location: builder.node_location(node),
                        });
                    }
                    // These obsolete forms remain public Elements, but their
                    // complete visible effect is a generated sibling
                    // sentence.  Their authored argument nodes are private
                    // validator input and must not survive under the Element.
                    let _ = builder.replace_children(node, &[]);
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    if builder.node_count() >= max_nodes {
                        if outcome.node_limit_location.is_none() {
                            outcome.node_limit_location = builder.node_location(node);
                        }
                    } else if let Some(sentence) = push_generated_text_at(
                        builder,
                        active_body,
                        generated_sentence,
                        true,
                        builder.node_location(node),
                    ) {
                        // Root children are staged until the closing semantic
                        // pass; nested parents can retain the arena edge made
                        // by `push_generated_text_at` directly.
                        if active_body == root {
                            root_children.push(sentence);
                        }
                    } else if outcome.node_limit_location.is_none() {
                        outcome.node_limit_location = builder.node_location(node);
                    }
                }
                Some(
                    "Cm" | "Dv" | "Em" | "Er" | "Ev" | "Ic" | "Li" | "Ms" | "No" | "Sy" | "Va",
                ) => {
                    let is_empty = builder.children(node).is_none_or(<[NodeId]>::is_empty);
                    if builder.node_macro_name(node) == Some("Cm") && is_empty {
                        // `post_tag()` removes an empty Cm. A leading
                        // delimiter can reopen the same source request; that
                        // populated successor is the sole non-warning form.
                        if tag_empty_macro_requires_warning(
                            builder,
                            "Cm",
                            &inline_events[event_index + 1..],
                        ) {
                            outcome.recoveries.push(Recovery::EmptyMacro {
                                macro_name: "Cm",
                                location: builder.node_location(node),
                            });
                        }
                        continue;
                    }
                    if builder.node_macro_name(node) == Some("No") && is_empty {
                        // No keeps its empty compatibility Element in the
                        // public tree, but post-validation still reports a
                        // source-spelled empty request. A leading delimiter
                        // at the start of a request is private only when a
                        // populated No restart follows it; inline and
                        // isolated forms remain warnings.
                        let line_start = builder
                            .node_flags(node)
                            .is_some_and(|flags| flags.line_start);
                        let explicit_inline = builder
                            .node_source_position(node)
                            .is_some_and(|position| position.column > 2);
                        let reopened_by_later_name = line_start
                            && !tag_empty_macro_requires_warning(
                                builder,
                                "No",
                                &inline_events[event_index + 1..],
                            );
                        if explicit_inline || (line_start && !reopened_by_later_name) {
                            outcome.recoveries.push(Recovery::EmptyMacro {
                                macro_name: "No",
                                // Inline event splitting retains the logical
                                // start of the reclassified source spelling.
                                // Preserve that span directly: libmandoc
                                // reports the `N` in the inner `No`.
                                location: builder.node_location(node),
                            });
                        }
                        // `post_tag()` removes every empty `No` request. A
                        // delimiter that had been its only quoted argument
                        // remains in the surrounding flow, and a leading
                        // delimiter can reopen a later visible `No`.
                        continue;
                    }
                    if is_empty
                        && let Some(macro_name) = empty_tag_macro_name(macro_name.as_deref())
                    {
                        // The generic tag-style inline macros have no public
                        // zero-argument form.  libmandoc removes the element
                        // in post-validation, leaving only its warning.
                        let line_start = builder
                            .node_flags(node)
                            .is_some_and(|flags| flags.line_start);
                        let explicit_inline = builder
                            .node_source_position(node)
                            .is_some_and(|position| position.column > 2);
                        let preceded_by_opening_delimiter = macro_name == "Em"
                            && inline_events[..event_index]
                                .last()
                                .and_then(|previous| builder.node_text(*previous))
                                .is_some_and(|text| matches!(text, "(" | "["));
                        // A source-spelled inline tag macro is distinguishable
                        // from an internal empty element synthesized by
                        // delimiter splitting: it has its own later source
                        // column. It remains a warning even when its
                        // following delimiter is retained.
                        let report_empty = if macro_name == "Em" {
                            !preceded_by_opening_delimiter
                                && (explicit_inline
                                    || (line_start
                                        && tag_empty_macro_requires_warning(
                                            builder,
                                            macro_name,
                                            &inline_events[event_index + 1..],
                                        )))
                        } else {
                            // `in_line()` emits the empty-macro finding when
                            // the source request produced no element, even
                            // though `append_delims()` may have published one
                            // or more trailing punctuation nodes after it.
                            // Only a delimiter-separated, populated restart
                            // of the same macro makes this first element a
                            // private parser transient.
                            explicit_inline
                                || (line_start
                                    && tag_empty_macro_requires_warning(
                                        builder,
                                        macro_name,
                                        &inline_events[event_index + 1..],
                                    ))
                        };
                        if report_empty {
                            outcome.recoveries.push(Recovery::EmptyMacro {
                                macro_name,
                                location: builder.node_location(node),
                            });
                        }
                        // An empty tag element is always parser-private. A
                        // delimiter can leave it silent when a populated
                        // restart follows, while a true empty source request
                        // contributes the warning above; neither form owns a
                        // public AST node.
                        continue;
                    }
                    if let Some(macro_name) = tag_macro_name(macro_name.as_deref()) {
                        // libmandoc emits delimiter style findings during its
                        // post-validation sweep, after later empty-macro
                        // recoveries in the same document have been seen.
                        validate_tag(
                            builder,
                            node,
                            macro_name,
                            &mut deferred_post_validation_recoveries,
                        );
                    }
                    if let Some(tag) = pending_transparent_permalink.take() {
                        mark_permalink(builder, node, Some(&tag));
                    } else if let Some((tag_node, tag)) = pending_manual_tag.take() {
                        if tag.is_empty() {
                            if builder
                                .children(node)
                                .and_then(|children| children.first())
                                .and_then(|child| builder.node_text(*child))
                                .is_some_and(|text| !text.is_empty())
                            {
                                // The tag text is this node's own child, so
                                // `tag_put()` sets NODE_ID without allocating
                                // a redundant public tag string.
                                mark_target(builder, node, None);
                                mark_no_print(builder, tag_node);
                            }
                        } else {
                            mark_target(builder, node, Some(&tag));
                            mark_no_print(builder, tag_node);
                        }
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    if let Some((tail, location)) = inline_column_ta_tail.take()
                        && let Some(item) = active_column_item(builder, active_body)
                        && let Some(body) = append_column_ta_cell(
                            builder,
                            active_body,
                            location,
                            &tail,
                            spacing_enabled,
                            max_nodes,
                            &mut outcome,
                            &mut scopes,
                        )
                    {
                        extend_pending_short_column_item(&mut pending_short_column_items, item);
                        active_body = body;
                        flow_parent = body;
                    }
                }
                Some("Es") => {
                    outcome.recoveries.push(Recovery::Obsolete {
                        macro_name: "Es",
                        location: builder.node_location(node),
                    });
                    let children = builder
                        .children(node)
                        .map(<[NodeId]>::to_vec)
                        .unwrap_or_default();
                    let values = children
                        .iter()
                        .filter_map(|child| builder.node_text(*child))
                        .map(str::to_owned)
                        .collect::<Vec<_>>();
                    enclosure = values.first().map(|opening| NormalizedEnclosure {
                        opening: opening.clone().into_boxed_str(),
                        closing: values
                            .get(1)
                            .map(|closing| closing.clone().into_boxed_str()),
                    });
                    // Es accepts only the opening/closing delimiter pair.
                    // Later words resume normal source flow instead of
                    // becoming hidden Es arguments.
                    let kept = children.len().min(2);
                    let siblings = split_mdoc_inline_tokens(
                        builder,
                        node,
                        &children[kept..],
                        spacing_enabled,
                        max_nodes,
                        &mut outcome,
                    );
                    let _ = builder.replace_children(node, &children[..kept]);
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    for sibling in siblings {
                        append_to_parent(builder, root, &mut root_children, active_body, sibling);
                    }
                }
                Some("En") => {
                    outcome.recoveries.push(Recovery::Obsolete {
                        macro_name: "En",
                        location: builder.node_location(node),
                    });
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        "En",
                        ArgumentPlacement::BodyTokens,
                        max_nodes,
                        &mut outcome,
                    ) else {
                        let _ = builder.set_node_enclosure(node, enclosure.clone());
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    let children = split_mdoc_inline_children(
                        builder,
                        body,
                        spacing_enabled,
                        max_nodes,
                        &mut outcome,
                    );
                    let _ = builder.replace_children(body, &children);
                    move_leading_open_delimiter(builder, node, head, body);
                    coalesce_adjacent_text_children(builder, body);
                    for part in [node, head, body] {
                        let _ = builder.set_node_enclosure(part, enclosure.clone());
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Bl") => {
                    let attributes =
                        list_attributes(builder, node, &mut deferred_post_validation_recoveries);
                    if !attributes.compact
                        && let Some(previous) = discard_previous_paragraph_control(
                            builder,
                            root,
                            &mut root_children,
                            flow_parent,
                        )
                    {
                        let macro_name = match builder.node_macro_name(previous) {
                            Some("Pp") => "Pp",
                            Some("br") => "br",
                            _ => unreachable!(
                                "the paragraph-control predicate checked the macro name"
                            ),
                        };
                        deferred_post_validation_recoveries.push(Recovery::ParagraphBoundary {
                            macro_name,
                            placement: "before",
                            blocker: "Bl",
                            location: builder.node_location(previous),
                        });
                    }
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        "Bl",
                        ArgumentPlacement::Drop,
                        max_nodes,
                        &mut outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    apply_attributes(builder, &[node, head, body], &attributes);
                    list_types.insert(body, attributes.list_type);
                    if let Some(column_count) = attributes.column_count {
                        column_counts.insert(body, column_count);
                    }
                    if let Some((tag_node, tag)) = pending_manual_tag.take()
                        && !tag.is_empty()
                    {
                        // `post_tg()` transfers an explicit tag before a
                        // list to its Body.  In particular, a column list
                        // has no independent first visible node that could
                        // own a paragraph-style permalink.
                        mark_manual_target(builder, body, &tag);
                        mark_no_print(builder, tag_node);
                    }
                    append_to_parent(builder, root, &mut root_children, flow_parent, node);
                    scopes.push(ScopeFrame {
                        close: "El",
                        open: node,
                        body,
                        tail_on_close: false,
                        transparent_target_taken: false,
                        suppress_implicit_ancestor_break: false,
                        resume_active: active_body,
                        resume_flow: flow_parent,
                    });
                    flow_parent = body;
                    active_body = body;
                }
                Some("It") => {
                    if let Some(list_index) = scopes.iter().rposition(|frame| frame.close == "El")
                        && scopes
                            .get(list_index + 1)
                            .is_some_and(|frame| matches!(frame.close, "Ac" | "Ed"))
                    {
                        // A new item is a full structural boundary.  It
                        // closes an outstanding explicit delimiter or display
                        // scope inside the list before opening the next row;
                        // waiting for `.El` would instead misclassify this as
                        // list-on-block bad nesting.
                        let list = scopes[list_index];
                        let list_body = list.body;
                        deferred_list_content_recoveries.extend(move_initial_list_content_out(
                            builder,
                            root,
                            &mut root_children,
                            list,
                        ));
                        let interrupted = scopes.split_off(list_index + 1);
                        for frame in interrupted.iter().rev() {
                            outcome.recoveries.push(Recovery::BrokenBlock {
                                breaker: "It",
                                macro_name: open_name(frame.close),
                                location: builder.node_location(node),
                            });
                            implicitly_closed.push(frame.close);
                        }
                        flow_parent = list_body;
                        active_body = list_body;
                    }
                    let Some(list) = scopes
                        .iter()
                        .rev()
                        .find(|frame| frame.close == "El")
                        .copied()
                    else {
                        let arguments = node_arguments(builder, node).join(" ");
                        outcome.recoveries.push(Recovery::ItemOutsideList {
                            arguments: arguments.into_boxed_str(),
                            location: builder.node_location(node),
                        });
                        let _ = builder.macro_name(node, "br");
                        let _ = builder.replace_children(node, &[]);
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    // `post_bl()` moves every direct prefix out of a list
                    // before its first item, including a nested block.  The
                    // nested block remains structurally intact; only its
                    // ownership changes to the surrounding flow.
                    outcome.recoveries.extend(move_initial_list_content_out(
                        builder,
                        root,
                        &mut root_children,
                        list,
                    ));
                    let list_body = list.body;
                    let list_is_innermost = scopes
                        .iter()
                        .rposition(|frame| frame.close == "El")
                        .is_some_and(|index| index + 1 == scopes.len());
                    let column_count = column_counts.get(&list_body).copied();
                    if column_count.is_some() {
                        finalize_last_empty_column_item(
                            builder,
                            list_body,
                            &mut pending_empty_column_items,
                            &mut outcome,
                        );
                        finalize_short_column_items(
                            builder,
                            list_body,
                            &mut pending_short_column_items,
                            &mut outcome,
                        );
                    }
                    if list_is_innermost
                        && let Some(list_type) = list_types.get(&list_body).copied()
                        && fixed_head_list_type(list_type)
                    {
                        finalize_last_fixed_head_list_item(
                            builder,
                            list_body,
                            list_type,
                            &deferred_fixed_head_argument_items,
                            &mut outcome,
                        );
                    }
                    if builder.node_list_kind(list_body) != Some(NormalizedListKind::Column) {
                        let arguments = builder
                            .children(node)
                            .map(<[NodeId]>::to_vec)
                            .unwrap_or_default();
                        if arguments
                            .first()
                            .and_then(|argument| builder.node_text(*argument))
                            == Some("Ta")
                        {
                            let ta_location = arguments
                                .first()
                                .and_then(|argument| builder.node_location(*argument));
                            let retained = &arguments[1..];
                            let retained_text = retained
                                .iter()
                                .filter_map(|argument| builder.node_text(*argument))
                                .collect::<Vec<_>>()
                                .join(" ");
                            let _ = builder.replace_children(node, retained);
                            outcome.recoveries.push(Recovery::ColumnOutsideColumnList {
                                location: ta_location,
                            });
                            deferred_post_validation_recoveries.push(Recovery::InvalidArguments {
                                message: format!("skipping all arguments: It {retained_text}")
                                    .into(),
                                location: builder.node_location(node),
                            });
                            if list_types
                                .get(&list_body)
                                .is_some_and(|list_type| fixed_head_list_type(list_type))
                            {
                                deferred_fixed_head_argument_items.insert(node);
                            }
                        }
                    }
                    let diag_list = list_types.get(&list_body) == Some(&"diag");
                    let opens_xo = !diag_list
                        && matches!(node_arguments(builder, node).as_slice(), [value] if value == "Xo");
                    let empty_column_item = column_count.is_some()
                        && !opens_xo
                        && builder.children(node).is_none_or(<[NodeId]>::is_empty);
                    if empty_column_item {
                        pending_empty_column_items.insert(node);
                    }
                    let column_cell_count = column_count
                        .filter(|_| !opens_xo)
                        .map(|_| column_item_cell_count(builder, node));
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        "It",
                        ArgumentPlacement::Head,
                        max_nodes,
                        &mut outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    if matches!(
                        list_types.get(&list_body),
                        Some(&"hang" | &"ohang" | &"inset" | &"diag" | &"tag")
                    ) && builder.children(head).is_none_or(<[NodeId]>::is_empty)
                    {
                        outcome.recoveries.push(Recovery::EmptyListItemHead {
                            list_type: list_types
                                .get(&list_body)
                                .copied()
                                .expect("matched list type must exist"),
                            location: builder.node_location(node),
                        });
                    }
                    let column_list =
                        builder.node_list_kind(list_body) == Some(NormalizedListKind::Column);
                    if column_list
                        && !opens_xo
                        && let Some(column_cell_count) = column_cell_count
                        && let Some(bodies) = split_column_item_cells(
                            builder,
                            node,
                            head,
                            body,
                            spacing_enabled,
                            max_nodes,
                            &mut outcome,
                            &mut scopes,
                        )
                    {
                        if let Some((tag_node, tag)) = pending_manual_tag.take()
                            && !tag.is_empty()
                        {
                            // `post_tg()` leaves an explicit tag before the
                            // next column row on the It block itself.
                            mark_target(builder, node, Some(&tag));
                            mark_no_print(builder, tag_node);
                        }
                        let _ = builder.append_existing_child(list_body, node);
                        if let Some(columns) = column_count.filter(|_| !empty_column_item) {
                            let cells = column_cell_count;
                            if cells < columns {
                                pending_short_column_items.insert(node, (columns, cells));
                            } else if cells > columns.saturating_add(1) {
                                outcome.recoveries.push(Recovery::WrongNumberOfColumnCells {
                                    columns,
                                    cells,
                                    location: builder.node_location(node),
                                });
                            }
                        }
                        let cell = *bodies.last().expect("column items have one body");
                        if let Some(scope) = scopes
                            .last()
                            .copied()
                            .filter(|scope| scope.resume_active == cell)
                        {
                            // A partial explicit opener occurred inside this
                            // column cell. Until its ordinary closer arrives,
                            // physical follow-up input belongs to that nested
                            // Body rather than the cell itself.
                            flow_parent = scope.body;
                            active_body = scope.body;
                        } else {
                            flow_parent = cell;
                            active_body = cell;
                        }
                        continue;
                    }
                    // `Bl -diag` owns its item header as literal prose.  In
                    // particular, `Nx`, `Fl`, and an authored `Xo` spelling
                    // must not enter the callable-macro or partial-block
                    // paths that definition-list terms use.
                    let parsed_head = if diag_list {
                        builder
                            .children(head)
                            .map(<[NodeId]>::to_vec)
                            .unwrap_or_default()
                    } else {
                        let parsed = split_mdoc_inline_children(
                            builder,
                            head,
                            spacing_enabled,
                            max_nodes,
                            &mut outcome,
                        );
                        collapse_long_option_prefixes(builder, &parsed)
                    };
                    let _ = builder.replace_children(head, &parsed_head);
                    // Definition-list terms use the same parsed mdoc
                    // argument grammar as ordinary flow.  In particular an
                    // implicit partial such as `.It Bq Er ENOENT` is a
                    // nested Block/Head/Body before the later tag pass sees
                    // the public term tree; leaving it as an Element loses
                    // both that structure and the nested callable macro.
                    structure_nested_implicit_partial_blocks(
                        builder,
                        head,
                        max_nodes,
                        &mut outcome,
                        spacing_enabled,
                    );
                    if opens_xo {
                        if let Some(mut flags) = builder.node_flags(body) {
                            // `mdoc_macro.c` opens the item body while the
                            // `.It Xo` control line is still active.
                            flags.line_start = true;
                            let _ = builder.set_node_flags(body, flags);
                        }
                        let location = parsed_head
                            .first()
                            .and_then(|opening| builder.node_location(*opening));
                        let Some((xo, _, xo_body)) = make_synthetic_block(
                            builder,
                            head,
                            "Xo",
                            location,
                            max_nodes,
                            &mut outcome,
                        ) else {
                            let _ = builder.append_existing_child(list_body, node);
                            flow_parent = body;
                            active_body = body;
                            continue;
                        };
                        let _ = builder.replace_children(head, &[xo]);
                        let _ = builder.append_existing_child(list_body, node);
                        scopes.push(ScopeFrame {
                            close: "Xc",
                            open: xo,
                            body: xo_body,
                            tail_on_close: false,
                            transparent_target_taken: false,
                            suppress_implicit_ancestor_break: false,
                            resume_active: body,
                            resume_flow: body,
                        });
                        flow_parent = xo_body;
                        active_body = xo_body;
                    } else {
                        if let Some((tag_node, tag)) = pending_manual_tag.take()
                            && !tag.is_empty()
                        {
                            // `post_tg()` chooses a definition-list term but
                            // the content body for ordinary list rows.  The
                            // source-order pass knows that same list kind
                            // before following physical text is attached.
                            let target = if builder.node_list_kind(list_body)
                                == Some(NormalizedListKind::Definition)
                            {
                                head
                            } else {
                                body
                            };
                            mark_target(builder, target, Some(&tag));
                            mark_no_print(builder, tag_node);
                        }
                        mark_definition_item_head_targets(builder, list_body, head, &parsed_head);
                        // A non-column list item head is one semantic phrase
                        // in mandoc's public tree.  The scanner keeps words
                        // separate for roff execution, but those adjacent
                        // plain tokens have no remaining structural meaning.
                        // Column lists are different: their item arguments
                        // delimit cells and must remain independently owned.
                        if !column_list {
                            coalesce_adjacent_text_children(builder, head);
                        }
                        let _ = builder.append_existing_child(list_body, node);
                        if let Some(nested_scope) = structure_item_head_explicit_partial(
                            builder,
                            head,
                            body,
                            max_nodes,
                            &mut outcome,
                        ) {
                            flow_parent = nested_scope.body;
                            active_body = nested_scope.body;
                            scopes.push(nested_scope);
                        } else {
                            flow_parent = body;
                            active_body = body;
                        }
                    }
                }
                Some("Bd") => {
                    if scopes
                        .iter()
                        .any(|frame| builder.node_macro_name(frame.open) == Some("Bd"))
                    {
                        outcome.recoveries.push(Recovery::NestedDisplay {
                            location: builder.node_location(node),
                        });
                    }
                    if builder.children(node).is_some_and(<[NodeId]>::is_empty) {
                        // mandoc deletes a completely argument-less display
                        // and relinks its Body into the surrounding flow.  Its
                        // matching closer remains syntactically consumed.
                        deferred_post_validation_recoveries.push(
                            Recovery::DisplayWithoutArguments {
                                location: builder.node_location(node),
                            },
                        );
                        implicitly_closed.push("Ed");
                        continue;
                    }
                    let mut immediate_display_recoveries = Vec::new();
                    let attributes = display_attributes(
                        builder,
                        node,
                        &mut immediate_display_recoveries,
                        &mut deferred_post_validation_recoveries,
                    );
                    outcome.recoveries.extend(immediate_display_recoveries);
                    if !attributes.compact
                        && let Some(previous) = discard_previous_paragraph_control(
                            builder,
                            root,
                            &mut root_children,
                            flow_parent,
                        )
                    {
                        let macro_name = match builder.node_macro_name(previous) {
                            Some("Pp") => "Pp",
                            Some("br") => "br",
                            _ => unreachable!(
                                "the paragraph-control predicate checked the macro name"
                            ),
                        };
                        outcome.recoveries.push(Recovery::ParagraphBoundary {
                            macro_name,
                            placement: "before",
                            blocker: "Bd",
                            location: builder.node_location(previous),
                        });
                    }
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        "Bd",
                        ArgumentPlacement::Drop,
                        max_nodes,
                        &mut outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    apply_attributes(builder, &[node, head, body], &attributes);
                    if let Some((tag_node, tag)) = pending_manual_tag.take()
                        && !tag.is_empty()
                    {
                        // `post_tg()` attaches an explicit manual tag before
                        // a display to its body; the following visible text
                        // receives the matching permalink below through the
                        // same source-order path as a tagged paragraph.
                        mark_manual_target(builder, body, &tag);
                        mark_no_print(builder, tag_node);
                        pending_paragraph_href = Some(tag);
                    }
                    append_to_parent(builder, root, &mut root_children, flow_parent, node);
                    scopes.push(ScopeFrame {
                        close: "Ed",
                        open: node,
                        body,
                        tail_on_close: false,
                        transparent_target_taken: false,
                        suppress_implicit_ancestor_break: false,
                        resume_active: active_body,
                        resume_flow: flow_parent,
                    });
                    flow_parent = body;
                    active_body = body;
                }
                Some("D1" | "Dl") => {
                    // D1 and Dl are one-line implicit display blocks.  They
                    // have the same observable Block/empty Head/Body shape
                    // as a multi-line Bd, but their body is completed from
                    // this request's argument phrases rather than a later
                    // `.Ed` scope.
                    let name = macro_name.as_deref().expect("matched display macro");
                    let Some((_head, body)) = make_block(
                        builder,
                        node,
                        name,
                        ArgumentPlacement::BodyTokens,
                        max_nodes,
                        &mut outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    let children = split_mdoc_inline_children(
                        builder,
                        body,
                        spacing_enabled,
                        max_nodes,
                        &mut outcome,
                    );
                    let _ = builder.replace_children(body, &children);
                    coalesce_mdoc_display_phrases(builder, body);
                    if let Some((tag_node, tag)) = pending_manual_tag.take()
                        && !tag.is_empty()
                    {
                        // Unlike Bd, this display's visible body is supplied
                        // on the same control line.  Transfer the matching
                        // permalink immediately instead of waiting for the
                        // next source event.
                        mark_manual_target(builder, body, &tag);
                        mark_first_visible_permalink(builder, body, &tag);
                        mark_no_print(builder, tag_node);
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Bf") => {
                    let font_arguments = builder
                        .children(node)
                        .map(<[NodeId]>::to_vec)
                        .unwrap_or_default();
                    let attributes =
                        font_attributes(builder, node, &mut deferred_post_validation_recoveries);
                    let uses_option_form = font_arguments
                        .first()
                        .and_then(|argument| builder.node_text(*argument))
                        .is_some_and(is_bf_option);
                    let option_tail = uses_option_form.then(|| {
                        font_arguments[1..]
                            .iter()
                            .copied()
                            .filter(|argument| {
                                !builder.node_text(*argument).is_some_and(is_bf_option)
                            })
                            .collect::<Vec<_>>()
                    });
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        "Bf",
                        ArgumentPlacement::Head,
                        max_nodes,
                        &mut outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    if uses_option_form {
                        let _ = builder.replace_children(head, &option_tail.unwrap_or_default());
                    }
                    apply_attributes(builder, &[node, head, body], &attributes);
                    append_to_parent(builder, root, &mut root_children, flow_parent, node);
                    scopes.push(ScopeFrame {
                        close: "Ef",
                        open: node,
                        body,
                        tail_on_close: false,
                        transparent_target_taken: false,
                        suppress_implicit_ancestor_break: false,
                        resume_active: active_body,
                        resume_flow: flow_parent,
                    });
                    flow_parent = body;
                    active_body = body;
                }
                Some("Bk") => {
                    let arguments = builder
                        .children(node)
                        .map(<[NodeId]>::to_vec)
                        .unwrap_or_default();
                    let invalid_index = arguments
                        .iter()
                        .position(|argument| builder.node_text(*argument) != Some("-words"));
                    if let Some(argument) = invalid_index.and_then(|index| arguments.get(index)) {
                        outcome.recoveries.push(Recovery::InvalidArguments {
                            message: format!(
                                "skipping excess arguments: Bk ... {}",
                                builder.node_text(*argument).unwrap_or_default()
                            )
                            .into(),
                            location: builder.node_location(*argument),
                        });
                    }
                    let retained_head = invalid_index.map_or_else(Vec::new, |index| {
                        arguments[index.saturating_add(1)..]
                            .iter()
                            .copied()
                            .filter(|argument| {
                                !builder
                                    .node_text(*argument)
                                    .is_some_and(|value| value.starts_with('-'))
                            })
                            .collect::<Vec<_>>()
                    });
                    // Bk is a full explicit block whose `-words` control
                    // argument is validator-only.  The public tree exposes
                    // an empty Head and keeps all following source flow in
                    // Body until Ek consumes the scope.
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        "Bk",
                        ArgumentPlacement::Head,
                        max_nodes,
                        &mut outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    let _ = builder.replace_children(head, &retained_head);
                    append_to_parent(builder, root, &mut root_children, flow_parent, node);
                    synopsis_keep_boundary = in_synopsis && synopsis_name_body.is_some();
                    scopes.push(ScopeFrame {
                        close: "Ek",
                        open: node,
                        body,
                        tail_on_close: false,
                        transparent_target_taken: false,
                        suppress_implicit_ancestor_break: false,
                        resume_active: active_body,
                        resume_flow: flow_parent,
                    });
                    flow_parent = body;
                    active_body = body;
                }
                Some("Fo" | "Rs") => {
                    let name = macro_name.as_deref().expect("matched mdoc block macro");
                    let close = if name == "Fo" { "Fc" } else { "Re" };
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        name,
                        ArgumentPlacement::Head,
                        max_nodes,
                        &mut outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    if name == "Rs" {
                        // A reference list has no public Head arguments.  The
                        // validator reports only the leading selector (which
                        // may itself be an inline macro), then discards the
                        // entire scanner argument subtree before publication.
                        if let Some((tag_node, _)) = pending_manual_tag.take() {
                            // `post_tg()` keeps a preceding transparent tag
                            // as its own destination when the following full
                            // block is an Rs reference list.
                            mark_destination(builder, tag_node);
                        }
                        if let Some(argument) = node_arguments(builder, head).first() {
                            outcome.recoveries.push(Recovery::InvalidArguments {
                                message: format!("skipping all arguments: Rs {argument}").into(),
                                location: argument_location(builder, head, 0),
                            });
                            let _ = builder.replace_children(head, &[]);
                        }
                    } else if name == "Fo" {
                        let arguments = builder
                            .children(head)
                            .map(<[NodeId]>::to_vec)
                            .unwrap_or_default();
                        let has_excess_arguments = arguments.len() > 1;
                        if arguments.is_empty() {
                            outcome.recoveries.push(Recovery::MissingFunctionName {
                                location: builder.node_location(node),
                            });
                        } else if let Some(first) = arguments.first().copied()
                            && let Some(excess) = arguments.get(1).copied()
                        {
                            deferred_post_validation_recoveries.push(Recovery::InvalidArguments {
                                message: format!(
                                    "skipping excess arguments: Fo ... {}",
                                    builder.node_text(excess).unwrap_or_default()
                                )
                                .into(),
                                location: builder.node_location(excess),
                            });
                            let _ = builder.replace_children(head, &[first]);
                        }
                        if !arguments.is_empty() && !has_excess_arguments {
                            validate_function_name(builder, head, &mut outcome.recoveries);
                        }
                    }
                    if in_synopsis {
                        mark_synopsis_pretty(builder, head);
                        mark_synopsis_pretty(builder, body);
                    }
                    if name == "Fo" && !in_synopsis {
                        if let Some((tag_node, tag)) = pending_manual_tag.take() {
                            mark_target(builder, head, Some(&tag));
                            mark_no_print(builder, tag_node);
                        } else if let (Some(paragraph), Some(function_name)) = (
                            pending_function_paragraph.take(),
                            node_arguments(builder, head).first().cloned(),
                        ) {
                            // A preceding Pp owns Fo's automatic function
                            // destination.  Fo's head remains the permalink
                            // source, except that an inline font escape leaves
                            // the normalized leading function word visible as
                            // its public tag as well.
                            if let Some(function_tag) =
                                automatic_mdoc_function_tag(function_name.as_str())
                            {
                                mark_manual_target(builder, paragraph, function_tag);
                                let permalink_tag =
                                    (function_tag != function_name).then_some(function_tag);
                                mark_permalink(builder, head, permalink_tag);
                            } else {
                                mark_target(builder, head, None);
                            }
                            function_target_taken = true;
                        } else if let Some(function_tag) = node_arguments(builder, head)
                            .first()
                            .and_then(|name| automatic_mdoc_function_tag(name))
                        {
                            mark_target(builder, head, None);
                            let function_tag = function_tag.to_owned();
                            automatic_function_tag_occurrences.push(function_tag.clone());
                            automatic_function_targets.push((head, function_tag, false));
                        }
                    }
                    append_to_parent(builder, root, &mut root_children, flow_parent, node);
                    scopes.push(ScopeFrame {
                        close,
                        open: node,
                        body,
                        tail_on_close: false,
                        transparent_target_taken: false,
                        suppress_implicit_ancestor_break: false,
                        resume_active: active_body,
                        resume_flow: flow_parent,
                    });
                    flow_parent = body;
                    active_body = body;
                }
                Some(
                    "Ac" | "Bc" | "Brc" | "Dc" | "Ec" | "Ek" | "El" | "Ed" | "Ef" | "Fc" | "Oc"
                    | "Pc" | "Qc" | "Re" | "Sc" | "Xc",
                ) => {
                    let close = macro_name.as_deref().expect("matched mdoc closer");
                    if close == "Ed" {
                        let arguments = node_arguments(builder, node);
                        if !arguments.is_empty() {
                            outcome.recoveries.push(Recovery::InvalidArguments {
                                message: format!(
                                    "skipping all arguments: Ed {}",
                                    arguments.join(" ")
                                )
                                .into(),
                                location: builder.node_location(node),
                            });
                        }
                    }
                    if close == "Ef" {
                        let arguments = node_arguments(builder, node);
                        if !arguments.is_empty() {
                            outcome.recoveries.push(Recovery::InvalidArguments {
                                message: format!(
                                    "skipping all arguments: Ef {}",
                                    arguments.join(" ")
                                )
                                .into(),
                                location: builder.node_location(node),
                            });
                        }
                    }
                    if close == "El" {
                        let arguments = node_arguments(builder, node);
                        if !arguments.is_empty() {
                            outcome.recoveries.push(Recovery::InvalidArguments {
                                message: format!(
                                    "skipping all arguments: El {}",
                                    arguments.join(" ")
                                )
                                .into(),
                                location: builder.node_location(node),
                            });
                        }
                    }
                    if close == "Ek" {
                        let arguments = node_arguments(builder, node);
                        if !arguments.is_empty() {
                            outcome.recoveries.push(Recovery::InvalidArguments {
                                message: format!(
                                    "skipping all arguments: Ek {}",
                                    arguments.join(" ")
                                )
                                .into(),
                                location: builder.node_location(node),
                            });
                        }
                    }
                    if is_explicit_partial_close(close) {
                        let children = builder
                            .children(node)
                            .map(<[NodeId]>::to_vec)
                            .unwrap_or_default();
                        // Eo is exceptional among explicit partial blocks:
                        // an outer ordinary partial closer crossing its
                        // still-open Tail-owning scope is recoverable, but
                        // not ordinary nesting.  mandoc reports the authored
                        // close (`.Bo … .Eo … .Bc`) while retaining Eo's
                        // pending scope.  Other partial-pair repairs have
                        // distinct broken-body rules and are handled by their
                        // dedicated paths below.
                        if let Some(index) = scopes.iter().rposition(|frame| frame.close == close)
                            && let Some(interrupted) = scopes[index + 1..]
                                .iter()
                                .copied()
                                .find(|frame| frame.tail_on_close)
                        {
                            outcome.recoveries.push(Recovery::BadlyNestedBlock {
                                breaker: open_name(close),
                                interrupted: open_name(interrupted.close),
                                location: builder.node_location(node),
                            });
                        }
                        if let Some(index) = scopes.iter().rposition(|frame| frame.close == close)
                            && scopes[index + 1..].iter().any(|frame| frame.tail_on_close)
                        {
                            // An outer ordinary partial closer crossing Eo
                            // does not close Eo.  Its closer becomes an empty
                            // Body inside Eo's active Body; Eo remains live
                            // until Ec supplies its real Tail.
                            let frame = scopes[index];
                            let _ = append_broken_full_block_body(
                                builder,
                                active_body,
                                close,
                                frame,
                                node,
                                max_nodes,
                                &mut outcome,
                            );
                            let mut surviving_scopes = scopes.split_off(index + 1);
                            scopes.truncate(index);
                            let first = surviving_scopes
                                .first_mut()
                                .expect("the crossed Eo scope was just selected");
                            first.resume_active = frame.resume_active;
                            first.resume_flow = frame.resume_flow;
                            scopes.extend(surviving_scopes);
                            continue;
                        }
                        if let Some(index) = scopes.iter().rposition(|frame| frame.close == close)
                            && let Some(interrupted) = scopes[index + 1..]
                                .iter()
                                .copied()
                                .find(|frame| frame.close == "Ek")
                        {
                            // A word-keep block is validation-closed by an
                            // outer explicit partial closer, but unlike a
                            // display/list it retains its existing public
                            // Body topology until the authored `.Ek` arrives.
                            // Only the crossed-block recovery is observable.
                            let frame = scopes[index];
                            let _ = append_broken_full_block_body(
                                builder,
                                active_body,
                                close,
                                frame,
                                node,
                                max_nodes,
                                &mut outcome,
                            );
                            outcome.recoveries.push(Recovery::BadlyNestedBlock {
                                breaker: open_name(close),
                                interrupted: open_name(interrupted.close),
                                location: builder.node_location(node),
                            });
                        }
                        let close_is_open = scopes.iter().any(|frame| frame.close == close)
                            || implicitly_closed.contains(&close);
                        let has_other_open_partial = scopes.iter().any(|frame| {
                            is_explicit_partial_close(frame.close) || frame.close == "Xc"
                        });
                        let reports_not_open = !close_is_open
                            && has_other_open_partial
                            && implicitly_closed.is_empty();
                        if reports_not_open {
                            // A bare partial closer remains inert, but one
                            // that conflicts with an active explicit partial
                            // must surface mandoc's not-open recovery without
                            // disturbing the still-active scope.
                            outcome.recoveries.push(Recovery::UnmatchedClose {
                                macro_name: close_name(close),
                                location: builder.node_location(node),
                            });
                        }
                        if let Some(index) = scopes.iter().rposition(|frame| frame.close == close)
                            && let Some(interrupted) =
                                scopes[index + 1..].iter().rev().copied().find(|inner| {
                                    is_explicit_partial_scope(inner)
                                        || matches!(inner.close, "Ed" | "Ef" | "El")
                                })
                        {
                            let frame = scopes[index];
                            let crossed_partial = is_explicit_partial_scope(&interrupted);
                            if crossed_partial {
                                coalesce_adjacent_text_children(builder, active_body);
                            }
                            if !interrupted.suppress_implicit_ancestor_break {
                                // A closer can cross an explicit child that
                                // itself sits inside an implicit partial
                                // parent.  libmandoc reports that parent at
                                // its authored opener before reporting the
                                // immediately crossed explicit scope (for
                                // example `.Aq … Bo … Bro` followed by
                                // `.Bc`).  The public tree is already
                                // complete; this is a distinct recovery edge.
                                for implicit in
                                    implicit_partial_ancestor_blocks(builder, interrupted.open)
                                {
                                    let Some(name) = builder.node_macro_name(implicit) else {
                                        continue;
                                    };
                                    let breaker = implicit_partial_block_name(name);
                                    let _ = append_broken_implicit_block_body(
                                        builder,
                                        active_body,
                                        implicit,
                                        max_nodes,
                                        &mut outcome,
                                    );
                                    outcome.recoveries.push(Recovery::BadlyNestedBlock {
                                        breaker,
                                        interrupted: open_name(interrupted.close),
                                        location: builder.node_location(implicit),
                                    });
                                }
                            }
                            append_broken_full_block_body(
                                builder,
                                active_body,
                                close,
                                frame,
                                node,
                                max_nodes,
                                &mut outcome,
                            );
                            outcome.recoveries.push(Recovery::BadlyNestedBlock {
                                breaker: open_name(close),
                                interrupted: open_name(interrupted.close),
                                location: builder.node_location(node),
                            });
                            let mut surviving_scopes = scopes.split_off(index + 1);
                            scopes.truncate(index);
                            let first = surviving_scopes
                                .first_mut()
                                .expect("the interrupted scope was just selected");
                            first.resume_active = frame.resume_active;
                            first.resume_flow = frame.resume_flow;
                            scopes.extend(surviving_scopes);
                            append_explicit_partial_tail(
                                builder,
                                root,
                                &mut root_children,
                                &mut scopes,
                                &mut implicitly_closed,
                                &mut active_body,
                                &mut flow_parent,
                                node,
                                &children,
                                !crossed_partial,
                                spacing_enabled,
                                max_nodes,
                                &mut outcome,
                            );
                            continue;
                        }
                        let crossed_parent_body = scopes
                            .last()
                            .filter(|frame| frame.close == close)
                            .and_then(|frame| builder.node_parent(frame.open))
                            .filter(|parent| {
                                matches!(
                                    builder.node_kind(*parent),
                                    Some(NodeKind::Body | NodeKind::Head)
                                )
                            });
                        if let Some(index) = scopes.iter().rposition(|frame| frame.close == close)
                            && index + 1 == scopes.len()
                        {
                            let frame = scopes[index];
                            if !frame.suppress_implicit_ancestor_break {
                                let implicit_ancestors =
                                    implicit_partial_ancestor_blocks(builder, frame.open);
                                let trailing_text =
                                    take_trailing_line_start_text_children(builder, active_body);
                                for implicit in implicit_ancestors {
                                    let Some(name) = builder.node_macro_name(implicit) else {
                                        continue;
                                    };
                                    let breaker = implicit_partial_block_name(name);
                                    let _ = append_broken_implicit_block_body(
                                        builder,
                                        active_body,
                                        implicit,
                                        max_nodes,
                                        &mut outcome,
                                    );
                                    outcome.recoveries.push(Recovery::BadlyNestedBlock {
                                        breaker,
                                        interrupted: open_name(close),
                                        location: builder.node_location(implicit),
                                    });
                                }
                                for text in trailing_text {
                                    let _ = builder.append_existing_child(active_body, text);
                                }
                            }
                        }
                        close_explicit_partial_scope(
                            &mut scopes,
                            &mut implicitly_closed,
                            &mut active_body,
                            &mut flow_parent,
                            close,
                        );
                        if let Some(parent) =
                            crossed_parent_body.filter(|parent| *parent != active_body)
                        {
                            if !children.is_empty()
                                && builder.node_kind(parent) == Some(NodeKind::Head)
                                && let Some(mut flags) = builder.node_flags(active_body)
                            {
                                // The following item body no longer begins at
                                // the extended `.It` header once its partial
                                // closer supplied a head-owned tail.
                                flags.line_start = false;
                                let _ = builder.set_node_flags(active_body, flags);
                            }
                            // A crossed outer partial is no longer on the
                            // active scope stack, but the tail of its child's
                            // authored closer remains in that structural
                            // parent (`Ao … Bo … Ac … Bc tail`, or an `.It`
                            // header).  Do not retain it as general flow
                            // after the tail unless that tail itself opens
                            // another cross-line partial.
                            let scope_count = scopes.len();
                            let mut tail_active = parent;
                            let mut tail_flow = parent;
                            append_explicit_partial_tail(
                                builder,
                                root,
                                &mut root_children,
                                &mut scopes,
                                &mut implicitly_closed,
                                &mut tail_active,
                                &mut tail_flow,
                                node,
                                &children,
                                true,
                                spacing_enabled,
                                max_nodes,
                                &mut outcome,
                            );
                            if scopes.len() > scope_count {
                                // The tail was attached to an already
                                // crossed parent only for its local AST
                                // ownership.  A scope opened by that tail
                                // must resume the ordinary parser flow after
                                // its own closer, rather than trapping later
                                // physical lines in the historical parent.
                                for scope in &mut scopes[scope_count..] {
                                    scope.suppress_implicit_ancestor_break = true;
                                    scope.resume_active = active_body;
                                    scope.resume_flow = flow_parent;
                                }
                                active_body = tail_active;
                                flow_parent = tail_flow;
                            }
                        } else {
                            append_explicit_partial_tail(
                                builder,
                                root,
                                &mut root_children,
                                &mut scopes,
                                &mut implicitly_closed,
                                &mut active_body,
                                &mut flow_parent,
                                node,
                                &children,
                                true,
                                spacing_enabled,
                                max_nodes,
                                &mut outcome,
                            );
                        }
                        if reports_not_open && builder.node_macro_name(active_body) == Some("Bo") {
                            // The skipped closer's ordinary tail remains in
                            // the active bracket body and extends its one
                            // semantic phrase across the control-line
                            // boundary (`.Bo bo` followed by `.Pc bc`).
                            coalesce_adjacent_text_children(builder, active_body);
                        }
                        continue;
                    }
                    if let Some(index) = scopes.iter().rposition(|frame| frame.close == close) {
                        // mdoc permits a list/display closer to break a nested
                        // compatible block.  This is not a malformed stack: the
                        // matching frame resumes the outer flow, and the popped
                        // inner frames are validation-closed by that request.
                        let frame = scopes[index];
                        if close == "Re" {
                            normalize_reference_field_order(builder, frame.body);
                        }
                        // `.Ec` is the sole explicit partial closer that is
                        // not in `is_explicit_partial_close()`: Eo owns a
                        // closer-created Tail.  Its close still diagnoses an
                        // intervening explicit partial block exactly like the
                        // ordinary Ac/Bc/… family does.
                        if frame.tail_on_close
                            && let Some(interrupted) = scopes[index + 1..]
                                .iter()
                                .copied()
                                .find(is_explicit_partial_scope)
                        {
                            outcome.recoveries.push(Recovery::BadlyNestedBlock {
                                breaker: open_name(close),
                                interrupted: open_name(interrupted.close),
                                location: builder.node_location(node),
                            });
                        }
                        if frame.tail_on_close
                            && scopes[index + 1..].iter().any(is_explicit_partial_scope)
                        {
                            // Ec crossing an inner ordinary partial block is
                            // represented by a closer-owned Eo Body *inside*
                            // that block. The inner scope keeps the following
                            // source flow through its own closer; the crossed
                            // Eo frame is consumed without a Tail child.
                            let tail_remainder = append_broken_full_block_body(
                                builder,
                                active_body,
                                close,
                                frame,
                                node,
                                max_nodes,
                                &mut outcome,
                            )
                            .map(|body| {
                                complete_explicit_tail(
                                    builder,
                                    body,
                                    node,
                                    spacing_enabled,
                                    max_nodes,
                                    &mut outcome,
                                )
                            })
                            .unwrap_or_default();
                            let mut surviving_scopes = scopes.split_off(index + 1);
                            scopes.truncate(index);
                            let first = surviving_scopes
                                .first_mut()
                                .expect("the crossed partial scope was just selected");
                            first.resume_active = frame.resume_active;
                            first.resume_flow = frame.resume_flow;
                            scopes.extend(surviving_scopes);
                            for remainder in tail_remainder {
                                append_to_parent(
                                    builder,
                                    root,
                                    &mut root_children,
                                    active_body,
                                    remainder,
                                );
                            }
                            continue;
                        }
                        if close == "El" && index + 1 == scopes.len() {
                            // A list with no `.It` can only establish that its
                            // leading content was invalid when it closes. Move
                            // it first so the following empty-list recovery
                            // observes the retained public topology.
                            outcome.recoveries.extend(move_initial_list_content_out(
                                builder,
                                root,
                                &mut root_children,
                                frame,
                            ));
                        }
                        if close == "El" && column_counts.contains_key(&frame.body) {
                            finalize_last_empty_column_item(
                                builder,
                                frame.body,
                                &mut pending_empty_column_items,
                                &mut outcome,
                            );
                            finalize_short_column_items(
                                builder,
                                frame.body,
                                &mut pending_short_column_items,
                                &mut outcome,
                            );
                        }
                        if close == "El"
                            && index + 1 == scopes.len()
                            && let Some(list_type) = list_types.get(&frame.body).copied()
                            && fixed_head_list_type(list_type)
                        {
                            finalize_last_fixed_head_list_item(
                                builder,
                                frame.body,
                                list_type,
                                &deferred_fixed_head_argument_items,
                                &mut outcome,
                            );
                        }
                        if close == "El"
                            && let Some(item) = item_header_partial_scope(builder, &scopes, index)
                        {
                            // `post_bl()` does not close an enum list through
                            // a partial block embedded in an item's Head.  It
                            // retains a closer-owned list Body inside that
                            // partial Body, then reports both unclosed scopes
                            // at EOF.  In particular, the ordinary deferred
                            // Item Body is absent from the public AST.
                            append_broken_full_block_body(
                                builder,
                                active_body,
                                close,
                                frame,
                                node,
                                max_nodes,
                                &mut outcome,
                            );
                            outcome.recoveries.push(Recovery::BadlyNestedBlock {
                                breaker: open_name(close),
                                interrupted: open_name(scopes[index + 1].close),
                                location: builder.node_location(node),
                            });
                            discard_item_body(builder, item);
                            deferred_broken_item_recoveries
                                .extend(broken_item_recoveries(builder, frame, item));
                            continue;
                        }
                        if let Some(interrupted) = scopes
                            .get(index + 1)
                            .copied()
                            .filter(|_inner| matches!(close, "Ed" | "Ef" | "El"))
                        {
                            append_broken_full_block_body(
                                builder,
                                active_body,
                                close,
                                frame,
                                node,
                                max_nodes,
                                &mut outcome,
                            );
                            outcome.recoveries.push(Recovery::BadlyNestedBlock {
                                breaker: open_name(close),
                                interrupted: open_name(interrupted.close),
                                location: builder.node_location(node),
                            });
                            let mut surviving_scopes = scopes.split_off(index + 1);
                            scopes.truncate(index);
                            let first = surviving_scopes
                                .first_mut()
                                .expect("the interrupted scope was just selected");
                            first.resume_active = frame.resume_active;
                            first.resume_flow = frame.resume_flow;
                            scopes.extend(surviving_scopes);
                            continue;
                        }
                        let mut tail_remainder = Vec::new();
                        // Fc is a full-block closer, but closing punctuation
                        // on its control line resumes surrounding flow rather
                        // than becoming hidden close-macro syntax.  Other
                        // closers retain their existing dedicated recovery or
                        // validation paths until their argument grammars are
                        // implemented.
                        let close_remainder = if close == "Fc" {
                            let children = builder
                                .children(node)
                                .map(<[NodeId]>::to_vec)
                                .unwrap_or_default();
                            let remainder = split_mdoc_inline_tokens(
                                builder,
                                node,
                                &children,
                                spacing_enabled,
                                max_nodes,
                                &mut outcome,
                            );
                            if let Some(first) = remainder.first()
                                // A callable macro quoted after Fc continues
                                // the same physical control line.  Only a
                                // literal tail token is promoted into the
                                // resumed flow's first line-start event.
                                && builder.node_macro_name(*first).is_none()
                                && !in_synopsis
                                && let Some(mut flags) = builder.node_flags(*first)
                            {
                                // Once Fc has closed the block, its trailing
                                // token begins a fresh flow event even though
                                // the scanner originally held it as a control
                                // argument at a later physical column.
                                flags.line_start = true;
                                let _ = builder.set_node_flags(*first, flags);
                            }
                            remainder
                        } else if close == "Xc" {
                            let children = builder
                                .children(node)
                                .map(<[NodeId]>::to_vec)
                                .unwrap_or_default();
                            let remainder = explicit_partial_tail_events(
                                builder,
                                node,
                                &children,
                                spacing_enabled,
                                max_nodes,
                                &mut outcome,
                            );
                            mark_explicit_partial_close_tail_line_start(builder, &remainder);
                            remainder
                        } else {
                            Vec::new()
                        };
                        let empty_bk = close == "Ek"
                            && builder
                                .children(frame.body)
                                .is_some_and(<[NodeId]>::is_empty);
                        if empty_bk {
                            outcome.recoveries.push(Recovery::EmptyBlock {
                                macro_name: "Bk",
                                location: builder.node_location(frame.open),
                            });
                            discard_empty_block(
                                builder,
                                root,
                                &mut root_children,
                                frame.resume_flow,
                                frame.open,
                            );
                        }
                        if close == "Re"
                            && builder
                                .children(frame.body)
                                .is_some_and(<[NodeId]>::is_empty)
                        {
                            outcome.recoveries.push(Recovery::EmptyReferenceBlock {
                                location: builder.node_location(frame.open),
                            });
                        }
                        if close == "Ed"
                            && builder
                                .children(frame.body)
                                .is_some_and(<[NodeId]>::is_empty)
                        {
                            outcome.recoveries.push(Recovery::EmptyBlock {
                                macro_name: "Bd",
                                location: builder.node_location(frame.open),
                            });
                        }
                        if close == "El"
                            && builder
                                .children(frame.body)
                                .is_some_and(<[NodeId]>::is_empty)
                        {
                            // Unlike an empty Bk, an empty list remains a
                            // visible Block/Head/Body topology.  Validation
                            // reports it at its opener after the closer is
                            // consumed, independent of the selected list
                            // display kind.
                            outcome.recoveries.push(Recovery::EmptyBlock {
                                macro_name: "Bl",
                                location: builder.node_location(frame.open),
                            });
                        }
                        if frame.tail_on_close {
                            if builder.node_count() >= max_nodes {
                                if outcome.node_limit_location.is_none() {
                                    outcome.node_limit_location = builder.node_location(node);
                                }
                            } else if let Some(tail) = builder.push(frame.open, NodeKind::Tail) {
                                let _ = builder.macro_name(tail, "Eo");
                                tail_remainder = complete_explicit_tail(
                                    builder,
                                    tail,
                                    node,
                                    spacing_enabled,
                                    max_nodes,
                                    &mut outcome,
                                );
                            }
                        }
                        implicitly_closed
                            .extend(scopes[index + 1..].iter().map(|frame| frame.close));
                        scopes.truncate(index);
                        active_body = frame.resume_active;
                        flow_parent = frame.resume_flow;
                        for remainder in tail_remainder {
                            append_to_parent(
                                builder,
                                root,
                                &mut root_children,
                                flow_parent,
                                remainder,
                            );
                        }
                        for remainder in close_remainder {
                            append_to_parent(
                                builder,
                                root,
                                &mut root_children,
                                flow_parent,
                                remainder,
                            );
                        }
                    } else if let Some(index) = implicitly_closed
                        .iter()
                        .rposition(|implicit| *implicit == close)
                    {
                        implicitly_closed.remove(index);
                    } else if close == "Xc" {
                        // Xc is also callable syntax inside an ordinary
                        // inline `.Xo … Xc` partial block.  Only consume it
                        // here when `.It Xo` established the cross-line scope.
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                    } else {
                        if close == "Ec" {
                            recover_unmatched_ec(
                                builder,
                                root,
                                &mut root_children,
                                active_body,
                                node,
                                spacing_enabled,
                                max_nodes,
                                &mut outcome,
                            );
                        }
                        outcome.recoveries.push(Recovery::UnmatchedClose {
                            macro_name: close_name(close),
                            location: builder.node_location(node),
                        });
                    }
                }
                _ => {
                    if let Some(close) = scopes.last().map(|frame| frame.close)
                        && is_explicit_partial_close(close)
                        && builder.node_text(node) == Some(close)
                    {
                        // A closer following a callable explicit opener can
                        // remain a bare inline token rather than becoming a
                        // control-line macro event.  It still restores the
                        // surrounding cross-line scope before the following
                        // token is attached (`.No a Oc Oo b Oc Oc Pq`).
                        close_explicit_partial_scope(
                            &mut scopes,
                            &mut implicitly_closed,
                            &mut active_body,
                            &mut flow_parent,
                            close,
                        );
                        continue;
                    }
                    if active_column_list(builder, active_body)
                        && builder
                            .node_flags(node)
                            .is_some_and(|flags| flags.line_start)
                        && structure_implicit_column_item(
                            builder,
                            active_body,
                            node,
                            spacing_enabled,
                            max_nodes,
                            &mut outcome,
                            &mut scopes,
                        )
                    {
                        // A `Bl -column` list does not require explicit
                        // `.It` controls.  At a physical line boundary,
                        // mandoc turns ordinary mdoc macros and literal text
                        // into an implicit row before it processes `Ta` and
                        // tabs as cell boundaries.  The list body remains
                        // active for the following source line.
                        continue;
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    if let Some(tag) = paragraph_href
                        && builder.node_kind(node) == Some(NodeKind::Text)
                    {
                        move_paragraph_permalink(
                            builder,
                            node,
                            active_body,
                            &tag,
                            max_nodes,
                            &mut outcome,
                        );
                    }
                    if let Some(close) = scopes.last().map(|frame| frame.close)
                        && node_arguments(builder, node)
                            .iter()
                            .any(|argument| argument == close)
                    {
                        let frame = scopes.pop().expect("last scope was checked");
                        active_body = frame.resume_active;
                        flow_parent = frame.resume_flow;
                    }
                }
            }
            if let Some((close, tail)) = direct_partial_close {
                close_explicit_partial_scope(
                    &mut scopes,
                    &mut implicitly_closed,
                    &mut active_body,
                    &mut flow_parent,
                    close,
                );
                append_explicit_partial_tail(
                    builder,
                    root,
                    &mut root_children,
                    &mut scopes,
                    &mut implicitly_closed,
                    &mut active_body,
                    &mut flow_parent,
                    node,
                    &tail,
                    false,
                    spacing_enabled,
                    max_nodes,
                    &mut outcome,
                );
            }
        }
        if let Some((head, body)) = synopsis_name_inline_restore {
            if scopes.iter().any(|frame| frame.resume_active == head) {
                // A partial block opened from an Nm Head takes over the next
                // physical line.  libmandoc leaves the otherwise empty Nm
                // Body as the structural boundary and marks that delayed
                // flow transition as line-start.
                if let Some(mut flags) = builder.node_flags(body) {
                    flags.line_start = true;
                    let _ = builder.set_node_flags(body, flags);
                }
            } else if active_body == head && flow_parent == head {
                active_body = body;
                flow_parent = body;
            }
        }
    }

    if let Some((tag_node, tag)) = pending_manual_tag.take()
        && tag.is_empty()
    {
        outcome.recoveries.push(Recovery::EmptyMacro {
            macro_name: "Tg",
            location: builder.node_location(tag_node),
        });
        discard_node_from_parent(builder, root, &mut root_children, tag_node);
    }

    for frame in &scopes {
        if frame.close == "Re" {
            normalize_reference_field_order(builder, frame.body);
            if builder
                .children(frame.body)
                .is_some_and(<[NodeId]>::is_empty)
            {
                outcome.recoveries.push(Recovery::EmptyReferenceBlock {
                    location: builder.node_location(frame.open),
                });
            }
        }
        if frame.close == "El" && column_counts.contains_key(&frame.body) {
            finalize_last_empty_column_item(
                builder,
                frame.body,
                &mut pending_empty_column_items,
                &mut outcome,
            );
            finalize_short_column_items(
                builder,
                frame.body,
                &mut pending_short_column_items,
                &mut outcome,
            );
        }
    }

    // EOF closes the innermost retained semantic scope first, just as the
    // legacy post-validation walk does for a list held open inside a partial
    // block.
    for frame in scopes.into_iter().rev() {
        outcome.recoveries.push(Recovery::UnclosedBlock {
            macro_name: open_name(frame.close),
            location: builder.node_location(frame.open),
        });
    }
    flush_pending_nd_delimiters(
        builder,
        &mut pending_nd_delimiter_bodies,
        &mut outcome.recoveries,
    );
    flush_pending_name_section(
        builder,
        &mut pending_name_section_body,
        &mut outcome.recoveries,
    );
    flush_pending_authors_section(builder, &mut pending_authors_body, &mut outcome.recoveries);
    outcome.recoveries.extend(deferred_broken_item_recoveries);
    outcome.recoveries.extend(deferred_list_content_recoveries);
    outcome
        .recoveries
        .extend(deferred_paragraph_argument_recoveries);
    outcome
        .recoveries
        .extend(deferred_post_validation_recoveries);
    // A crossed closer found while recursively structuring an implicit block
    // is syntactic rather than a late post-validation finding.  It used to be
    // appended only after every validator finding, which was observable for a
    // nested `.Aq` / `.Bq` pair.  Merge just those delayed crossings among the
    // already source-ordered ordinary crossings.  Do not sort the complete
    // recovery vector: its trailing validation findings intentionally retain
    // libmandoc's post-validation order.
    for recovery in syntax_stage_recoveries {
        let line = match &recovery {
            Recovery::BadlyNestedBlock { location, .. } => location
                .as_ref()
                .and_then(|span| builder.source_position(span))
                .map_or(u32::MAX, |position| position.line),
            _ => unreachable!("syntax-stage findings are crossed blocks"),
        };
        // Ordinary crossed blocks are emitted while the line is structured;
        // the recursive crossings above are discovered only afterwards.  Put
        // a delayed finding before the next ordinary crossing, or immediately
        // after the last one when it is later in the source.  If there is no
        // ordinary crossing (the `Nd/broken` shape), it must precede the
        // deferred semantic validators altogether.
        let index = outcome
            .recoveries
            .iter()
            .enumerate()
            .find_map(|(index, existing)| match existing {
                Recovery::BadlyNestedBlock { location, .. }
                    if location
                        .as_ref()
                        .and_then(|span| builder.source_position(span))
                        .is_some_and(|position| position.line > line) =>
                {
                    Some(index)
                }
                _ => None,
            })
            .unwrap_or_else(|| {
                outcome
                    .recoveries
                    .iter()
                    .rposition(|existing| matches!(existing, Recovery::BadlyNestedBlock { .. }))
                    .map_or(0, |index| index + 1)
            });
        outcome.recoveries.insert(index, recovery);
    }
    // Freeze the final root topology before root-level validation. In
    // particular, `post_prevpar()` may discard a top-level Pp immediately
    // before a section, which must not count as content before that section.
    let _ = builder.replace_children(root, &root_children);
    normalize_trailing_no_space_in_implicit_blocks(builder, root);
    let mut paragraph_layout_recoveries = Vec::new();
    normalize_list_trailing_paragraph_controls(builder, root, &mut paragraph_layout_recoveries);
    normalize_inline_paragraph_controls(builder, root, &mut paragraph_layout_recoveries);
    paragraph_layout_recoveries.sort_by_key(paragraph_layout_recovery_offset);
    outcome.recoveries.extend(paragraph_layout_recoveries);
    // Em/Sy fallback can assign the destination of a preceding paragraph.
    // Run it before section-boundary cleanup so that cleanup does not mistake
    // that paragraph for an empty redundant layout control.
    let emphasis_elements = emphasis_fallback_elements(builder);
    mark_emphasis_targets(builder, &emphasis_elements);
    let mut section_paragraph_recoveries = Vec::new();
    normalize_section_paragraph_boundaries(builder, root, &mut section_paragraph_recoveries);

    // The legacy wrapper defines `has_body` as the presence of any retained
    // non-comment root child, including no-printing prologue nodes.  Keep
    // that observable metadata separate from the structural no-body warning
    // below, which is based on visible document content.
    builder.metadata_mut().has_body = root_children
        .iter()
        .copied()
        .any(|node| builder.node_kind(node) != Some(NodeKind::Comment));
    if !saw_title_prologue {
        let metadata = builder.metadata_mut();
        metadata.title = Some("UNTITLED".into());
        if metadata.volume.is_none() {
            metadata.volume = Some("LOCAL".into());
        }
        outcome.recoveries.push(Recovery::MissingTitle);
    }
    let final_root_children = builder.children(root).unwrap_or_default();
    match first_mdoc_content_node(builder, final_root_children) {
        Some(node) if builder.node_macro_name(node) != Some("Sh") => {
            let content = builder
                .node_macro_name(node)
                .unwrap_or_else(|| node_kind_name(builder.node_kind(node)))
                .into();
            outcome
                .recoveries
                .push(Recovery::ContentBeforeFirstSection {
                    content,
                    location: builder.node_location(node),
                });
        }
        None => outcome.recoveries.push(Recovery::NoDocumentBody),
        Some(_) => {}
    }

    rebase_option_expansion_locations(builder, root);
    // Structural lowering may allocate descendants after their scanner event
    // was marked. The synopsis presentation state belongs to the entire
    // section Body, so complete the projection once every retained child has
    // reached its final parent.
    for body in synopsis_bodies {
        mark_synopsis_pretty(builder, body);
    }
    mark_definition_item_xo_head_targets(builder);
    mark_section_targets(builder, &target_heads);
    mark_unique_function_targets(
        builder,
        &automatic_function_targets,
        &automatic_function_tag_occurrences,
    );
    outcome.recoveries.extend(section_paragraph_recoveries);
    if !saw_operating_system_request && (saw_date_prologue || saw_title_prologue) {
        outcome.recoveries.push(Recovery::MissingOperatingSystem);
        // An absent request is distinguishable from bare `.Os`: preserve the
        // legacy empty metadata value and prevent ParserConfig's bare-`.Os`
        // transport fallback from manufacturing a host/session value.
        builder.operating_system("");
    }
    if netbsd_operating_system_validation && !saw_netbsd_rcs_id {
        outcome
            .recoveries
            .push(Recovery::RcsIdMissing { flavour: "NetBSD" });
    }
    outcome
}

fn close_name(value: &str) -> &'static str {
    match value {
        "Ac" => "Ac",
        "Bc" => "Bc",
        "Brc" => "Brc",
        "Dc" => "Dc",
        "Ec" => "Ec",
        "Ek" => "Ek",
        "El" => "El",
        "Ed" => "Ed",
        "Ef" => "Ef",
        "Fc" => "Fc",
        "Oc" => "Oc",
        "Pc" => "Pc",
        "Qc" => "Qc",
        "Re" => "Re",
        "Sc" => "Sc",
        "Xc" => "Xc",
        _ => unreachable!("only known mdoc closers reach this helper"),
    }
}

fn is_explicit_partial_close(value: &str) -> bool {
    matches!(
        value,
        "Ac" | "Bc" | "Brc" | "Dc" | "Oc" | "Pc" | "Qc" | "Sc"
    )
}

/// Whether a retained scope is an explicit partial block.  Eo is exceptional:
/// its `.Ec` materializes a Tail and therefore is not listed with the simple
/// Ac/Bc/… close macros, but it follows the same broken-nesting recovery.
fn is_explicit_partial_scope(frame: &ScopeFrame) -> bool {
    frame.tail_on_close || is_explicit_partial_close(frame.close)
}

/// Return the conventional manual-section restriction for a named `.Sh`.
/// This is the deliberately finite `post_sh_head()` subset whose condition is
/// independent of section ordering and body validation. Like libmandoc, the
/// first byte of composite manual sections (for example `3p`) controls it.
fn unexpected_section_manuals(section: &str, manual_section: Option<&str>) -> Option<&'static str> {
    let manual = manual_section?.as_bytes().first().copied()?;
    if section == "ERRORS" {
        return (!matches!(manual, b'2' | b'3' | b'4' | b'9')).then_some("2, 3, 4, 9");
    }
    if matches!(section, "RETURN VALUES" | "LIBRARY") {
        return (!matches!(manual, b'2' | b'3' | b'9')).then_some("2, 3, 9");
    }
    (section == "CONTEXT" && manual != b'9').then_some("9")
}

/// Return conventional mdoc section rank and canonical spelling. This order
/// is deliberately the upstream `enum roff_sec` order used by `post_sh_head`.
fn named_mdoc_section(section: &str) -> Option<(u8, &'static str)> {
    match section {
        "NAME" => Some((1, "NAME")),
        "LIBRARY" => Some((2, "LIBRARY")),
        "SYNOPSIS" => Some((3, "SYNOPSIS")),
        "DESCRIPTION" => Some((4, "DESCRIPTION")),
        "CONTEXT" => Some((5, "CONTEXT")),
        "IMPLEMENTATION NOTES" => Some((6, "IMPLEMENTATION NOTES")),
        "RETURN VALUES" => Some((7, "RETURN VALUES")),
        "ENVIRONMENT" => Some((8, "ENVIRONMENT")),
        "FILES" => Some((9, "FILES")),
        "EXIT STATUS" => Some((10, "EXIT STATUS")),
        "EXAMPLES" => Some((11, "EXAMPLES")),
        "DIAGNOSTICS" => Some((12, "DIAGNOSTICS")),
        "COMPATIBILITY" => Some((13, "COMPATIBILITY")),
        "ERRORS" => Some((14, "ERRORS")),
        "SEE ALSO" => Some((15, "SEE ALSO")),
        "STANDARDS" => Some((16, "STANDARDS")),
        "HISTORY" => Some((17, "HISTORY")),
        "AUTHORS" => Some((18, "AUTHORS")),
        "CAVEATS" => Some((19, "CAVEATS")),
        "BUGS" => Some((20, "BUGS")),
        "SECURITY CONSIDERATIONS" => Some((21, "SECURITY CONSIDERATIONS")),
        _ => None,
    }
}

/// Recover literal tabs in filled `.Sh` arguments after earlier section
/// validation. Scanner diagnostics normally precede structural lowering, but
/// libmandoc reports the preceding duplicate/order finding before tabs in a
/// later section heading; retaining this source-local recovery preserves that
/// public ordering.
fn mdoc_heading_tab_recoveries(builder: &DocumentBuilder, node: NodeId) -> Vec<Recovery> {
    builder
        .children(node)
        .into_iter()
        .flatten()
        .flat_map(|argument| {
            let Some(location) = builder.node_location(*argument) else {
                return Vec::new();
            };
            builder
                .node_text(*argument)
                .into_iter()
                .flat_map(|text| {
                    text.bytes()
                        .enumerate()
                        .filter(|(_, byte)| *byte == b'\t')
                        .filter_map(|(offset, _)| {
                            let offset = u32::try_from(offset).ok()?;
                            let start = location.start.checked_add(offset)?;
                            let location =
                                SourceSpan::new(location.source, start, start.saturating_add(1))
                                    .ok();
                            Some(Recovery::FilledTextTab { location })
                        })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn close_explicit_partial_scope(
    scopes: &mut Vec<ScopeFrame>,
    implicitly_closed: &mut Vec<&'static str>,
    active_body: &mut NodeId,
    flow_parent: &mut NodeId,
    close: &str,
) {
    if let Some(index) = scopes.iter().rposition(|frame| frame.close == close) {
        let frame = scopes[index];
        implicitly_closed.extend(scopes[index + 1..].iter().map(|frame| frame.close));
        scopes.truncate(index);
        *active_body = frame.resume_active;
        *flow_parent = frame.resume_flow;
    } else if let Some(index) = implicitly_closed
        .iter()
        .rposition(|implicit| *implicit == close)
    {
        implicitly_closed.remove(index);
    } else {
        // Unlike the full-block closer family, an explicit partial closer
        // can be consumed by the surrounding parsed macro without opening a
        // public scope (for example `Bc` following a column `It`).  The
        // legacy parser leaves that inert syntax diagnostic-free.
    }
}

/// A tail on an authored closer request starts a fresh source-line event.
/// Same-line tails of an opener and inline `No`/`Fl` close arguments retain
/// their existing source position instead.
fn mark_explicit_partial_close_tail_line_start(builder: &mut DocumentBuilder, events: &[NodeId]) {
    let Some(first) = events.first().copied() else {
        return;
    };
    if builder.node_macro_name(first).is_some() {
        return;
    }
    if let Some(mut flags) = builder.node_flags(first) {
        flags.line_start = true;
        let _ = builder.set_node_flags(first, flags);
    }
}

/// Remove a surrounding explicit-partial closer that a callable macro holds
/// in its direct argument stream.  The caller attaches the retained prefix
/// first, then restores the enclosing flow through `close_explicit_partial_scope`.
fn take_explicit_partial_close_argument(
    builder: &mut DocumentBuilder,
    node: NodeId,
    scopes: &[ScopeFrame],
) -> Option<(&'static str, Vec<NodeId>)> {
    let close = scopes
        .last()
        .map(|frame| frame.close)
        .filter(|close| is_explicit_partial_close(close))?;
    let arguments = builder.children(node)?.to_vec();
    if let Some(index) = arguments
        .iter()
        .position(|argument| builder.node_text(*argument) == Some(close))
    {
        let _ = builder.replace_children(node, &arguments[..index]);
        return Some((close, arguments[index + 1..].to_vec()));
    }

    // `No` coalesces adjacent source words before the parent scope machine
    // sees them.  Preserve the retained phrase but split an exact final close
    // word back out of that one semantic text node.
    let last = *arguments.last()?;
    let text = builder.node_text(last)?.to_owned();
    let prefix = text.strip_suffix(close)?.trim_end_matches([' ', '\t']);
    if prefix.len().saturating_add(close.len()) == text.len() {
        return None;
    }
    if prefix.is_empty() {
        let _ = builder.replace_children(node, &arguments[..arguments.len().saturating_sub(1)]);
    } else {
        let _ = builder.text(last, prefix.to_owned());
    }
    Some((close, Vec::new()))
}

/// Macros currently implementing `post_tg()`'s explicit-destination cases.
/// Keep this gate narrow: a pending `.Tg` must never silently cross an
/// unrelated source event while additional structural families are staged.
fn accepts_pending_manual_tag(macro_name: Option<&str>) -> bool {
    matches!(
        macro_name,
        Some(
            "Pp" | "Lp"
                | "Tg"
                | "Bl"
                | "Bd"
                | "D1"
                | "Dl"
                | "Fn"
                | "Fo"
                | "Fc"
                | "Rs"
                | "Sh"
                | "Ss"
                | "Fl"
                | "Cm"
                | "Dv"
                | "Em"
                | "Er"
                | "Ev"
                | "Ic"
                | "Li"
                | "Ms"
                | "No"
                | "Sy"
        )
    )
}

/// Return the immediately preceding paragraph boundary eligible for a
/// following standalone `.Tg` destination.  Do not let a tag cross visible
/// text or an unrelated semantic element: `post_tg()` only borrows the
/// paragraph it directly follows.
fn preceding_manual_tag_paragraph(
    builder: &DocumentBuilder,
    parent: NodeId,
    tag_node: NodeId,
) -> Option<NodeId> {
    let children = builder.children(parent)?;
    let position = children.iter().position(|node| *node == tag_node)?;
    for node in children[..position].iter().rev() {
        match builder.node_macro_name(*node) {
            Some("Pp" | "Lp") => return Some(*node),
            Some("Tg") => {}
            _ => return None,
        }
    }
    None
}

/// Bibliographic fields accepted inside an mdoc `Rs` reference block.
///
/// The scanner retains each control-line word independently for roff
/// execution, but the mdoc end-of-line validator exposes every direct text
/// run as one phrase in the public AST.
fn is_reference_field_macro(name: &str) -> bool {
    reference_field_order(name).is_some()
}

/// Return the validator's stable ordering for a direct `Rs` child.
///
/// Invalid children have no order and therefore sort before bibliography
/// fields; equal entries retain authored order.  This mirrors libmandoc's
/// insertion sort in `post_rs()`.
fn reference_field_order(name: &str) -> Option<u8> {
    Some(match name {
        "%A" => 1,
        "%T" => 2,
        "%B" => 3,
        "%I" => 4,
        "%J" => 5,
        "%R" => 6,
        "%N" => 7,
        "%V" => 8,
        "%U" => 9,
        "%P" => 10,
        "%Q" => 11,
        "%C" => 12,
        "%D" => 13,
        "%O" => 14,
        _ => return None,
    })
}

/// Whether the `in_line_eoln` grammar for this bibliography field has the
/// upstream `MDOC_JOIN` flag.
fn reference_field_joins_arguments(name: &str) -> bool {
    matches!(
        name,
        "%A" | "%B" | "%C" | "%D" | "%I" | "%J" | "%O" | "%Q" | "%R" | "%T"
    )
}

/// Apply mdoc's `post_rs()` direct-child order after a reference scope ends.
fn normalize_reference_field_order(builder: &mut DocumentBuilder, body: NodeId) {
    let Some(children) = builder.children(body) else {
        return;
    };
    let mut ordered = children.to_vec();
    ordered.sort_by_key(|node| {
        builder
            .node_macro_name(*node)
            .and_then(reference_field_order)
            .unwrap_or_default()
    });
    if ordered != children {
        let _ = builder.replace_children(body, &ordered);
    }
}

fn open_name(value: &str) -> &'static str {
    match value {
        "Ac" => "Ao",
        "Bc" => "Bo",
        "Brc" => "Bro",
        "Dc" => "Do",
        "Ec" => "Eo",
        "Ek" => "Bk",
        "El" => "Bl",
        "Ed" => "Bd",
        "Ef" => "Bf",
        "Fc" => "Fo",
        "Oc" => "Oo",
        "Pc" => "Po",
        "Qc" => "Qo",
        "Re" => "Rs",
        "Sc" => "So",
        "Xc" => "Xo",
        _ => unreachable!("only known mdoc scope closers are retained"),
    }
}

/// Return the live `It` row when the current source-flow body belongs to a
/// `Bl -column` list.  The parser keeps this relationship in the arena rather
/// than a global mutable row pointer, so nested scopes cannot leak a target to
/// an unrelated list.
fn active_column_item(builder: &DocumentBuilder, active_body: NodeId) -> Option<NodeId> {
    if builder.node_kind(active_body) != Some(NodeKind::Body)
        || builder.node_macro_name(active_body) != Some("It")
    {
        return None;
    }
    let item = builder.node_parent(active_body)?;
    if builder.node_kind(item) != Some(NodeKind::Block)
        || builder.node_macro_name(item) != Some("It")
    {
        return None;
    }
    let list_body = builder.node_parent(item)?;
    if builder.node_kind(list_body) == Some(NodeKind::Body)
        && builder.node_macro_name(list_body) == Some("Bl")
        && builder.node_list_kind(list_body) == Some(NormalizedListKind::Column)
    {
        Some(item)
    } else {
        None
    }
}

/// Return the `It` block when the innermost scope crossed by a list closer is
/// an explicit partial block opened from that item's Head.
fn item_header_partial_scope(
    builder: &DocumentBuilder,
    scopes: &[ScopeFrame],
    list_index: usize,
) -> Option<NodeId> {
    if builder.node_list_kind(scopes.get(list_index)?.body) != Some(NormalizedListKind::Ordered) {
        return None;
    }
    let partial = scopes.get(list_index + 1)?;
    if !is_explicit_partial_close(partial.close) {
        return None;
    }
    let head = builder.node_parent(partial.open)?;
    if builder.node_kind(head) != Some(NodeKind::Head)
        || builder.node_macro_name(head) != Some("It")
    {
        return None;
    }
    let item = builder.node_parent(head)?;
    (builder.node_kind(item) == Some(NodeKind::Block)
        && builder.node_macro_name(item) == Some("It"))
    .then_some(item)
}

/// Remove the deferred body of an `It` whose header was left open across a
/// malformed list close.  Its visible header and nested partial block remain
/// attached to the list, matching mandoc's finite recovery tree.
fn discard_item_body(builder: &mut DocumentBuilder, item: NodeId) {
    let Some(children) = builder.children(item).map(<[NodeId]>::to_vec) else {
        return;
    };
    let retained = children
        .into_iter()
        .filter(|child| {
            !(builder.node_kind(*child) == Some(NodeKind::Body)
                && builder.node_macro_name(*child) == Some("It"))
        })
        .collect::<Vec<_>>();
    let _ = builder.replace_children(item, &retained);
}

/// Build the delayed item findings for the one post-`El` malformed shape
/// where mandoc leaves an ordered list and its header partial scope open.
fn broken_item_recoveries(
    builder: &DocumentBuilder,
    list: ScopeFrame,
    item: NodeId,
) -> Vec<Recovery> {
    if builder.node_list_kind(list.body) != Some(NormalizedListKind::Ordered) {
        return Vec::new();
    }
    let Some(head) = builder.children(item).and_then(|children| {
        children.iter().copied().find(|child| {
            builder.node_kind(*child) == Some(NodeKind::Head)
                && builder.node_macro_name(*child) == Some("It")
        })
    }) else {
        return Vec::new();
    };
    let arguments = node_arguments(builder, head).join(" ");
    let location = builder.node_location(item);
    let mut recoveries = vec![Recovery::EmptyListItem {
        list_type: "enum",
        location: location.clone(),
    }];
    if !arguments.is_empty() {
        recoveries.push(Recovery::InvalidArguments {
            message: format!("skipping all arguments: It {arguments}").into(),
            location,
        });
    }
    recoveries
}

/// Move direct list content preceding the first item into the surrounding
/// flow, immediately before the list block.  mdoc performs this recovery when
/// that first `.It` interrupts an active nested scope.
fn move_initial_list_content_out(
    builder: &mut DocumentBuilder,
    root: NodeId,
    root_children: &mut Vec<NodeId>,
    list: ScopeFrame,
) -> Vec<Recovery> {
    let Some(list_children) = builder.children(list.body).map(<[NodeId]>::to_vec) else {
        return Vec::new();
    };
    if list_children
        .iter()
        .any(|child| builder.node_macro_name(*child) == Some("It"))
    {
        return Vec::new();
    }
    if list_children.is_empty() {
        return Vec::new();
    }
    // A trailing `Sm on`/`Sm off` or explicit `Tg` controls the first item's
    // spacing/destination and stays in the list. Other direct content,
    // including an earlier spacing change that belongs to malformed prose,
    // moves before the block.
    let retained_start = list_children
        .iter()
        .rposition(|child| !list_content_stays_with_first_item(builder, *child))
        .map_or(0, |index| index + 1);
    let (moved, retained) = list_children.split_at(retained_start);
    let moved = moved.to_vec();
    let retained = retained.to_vec();
    if moved.is_empty() {
        return Vec::new();
    }
    if !builder.replace_children(list.body, &retained) {
        return Vec::new();
    }

    if list.resume_flow == root {
        let Some(index) = root_children.iter().position(|child| *child == list.open) else {
            return Vec::new();
        };
        root_children.splice(index..index, moved.iter().copied());
    } else {
        let Some(parent_children) = builder.children(list.resume_flow).map(<[NodeId]>::to_vec)
        else {
            return Vec::new();
        };
        let Some(index) = parent_children.iter().position(|child| *child == list.open) else {
            return Vec::new();
        };
        let mut reordered = parent_children;
        reordered.splice(index..index, moved.iter().copied());
        if !builder.replace_children(list.resume_flow, &reordered) {
            return Vec::new();
        }
    }
    list_content_recoveries(builder, &moved)
}

/// Trailing item controls belong to the following item flow rather than to
/// the malformed prefix of a list. This mirrors mandoc's `post_bl()` recovery
/// ordering.
fn list_content_stays_with_first_item(builder: &DocumentBuilder, child: NodeId) -> bool {
    match builder.node_macro_name(child) {
        Some("Tg") => true,
        Some("Sm") => builder.children(child).is_some_and(|children| {
            children.len() == 1 && matches!(builder.node_text(children[0]), Some("on" | "off"))
        }),
        _ => false,
    }
}

/// Collect the delayed warnings for direct list content that mdoc moves back
/// into surrounding flow when the first `.It` breaks an open nested block.
fn list_content_recoveries(builder: &DocumentBuilder, children: &[NodeId]) -> Vec<Recovery> {
    children
        .iter()
        .copied()
        .filter_map(|child| {
            let content = if builder.node_kind(child) == Some(NodeKind::Text) {
                Some("text".to_owned())
            } else {
                builder.node_macro_name(child).map(str::to_owned)
            }?;
            Some(Recovery::ContentOutsideList {
                content: content.into_boxed_str(),
                location: builder.node_location(child),
            })
        })
        .collect()
}

/// Whether the current source-flow parent is the body of a `Bl -column`
/// list, before that input has established an explicit or implicit item row.
fn active_column_list(builder: &DocumentBuilder, active_body: NodeId) -> bool {
    builder.node_kind(active_body) == Some(NodeKind::Body)
        && builder.node_macro_name(active_body) == Some("Bl")
        && builder.node_list_kind(active_body) == Some(NormalizedListKind::Column)
}

/// Attach a consecutive tbl row to the synthetic column-list item created for
/// the preceding tbl row.  The empty head distinguishes this form from a
/// normal `.It` header; limiting the body's children to tables prevents a
/// later ordinary source line from being swallowed into the same row.
fn append_implicit_column_table_row(
    builder: &mut DocumentBuilder,
    list_body: NodeId,
    table: NodeId,
) -> bool {
    let Some(item) = builder
        .children(list_body)
        .and_then(<[NodeId]>::last)
        .copied()
    else {
        return false;
    };
    if builder.node_kind(item) != Some(NodeKind::Block)
        || builder.node_macro_name(item) != Some("It")
    {
        return false;
    }
    let Some(children) = builder.children(item) else {
        return false;
    };
    let Some((head, body)) = children
        .split_first()
        .and_then(|(head, rest)| rest.first().map(|body| (*head, *body)))
    else {
        return false;
    };
    if builder.node_kind(head) != Some(NodeKind::Head)
        || builder.node_macro_name(head) != Some("It")
        || builder
            .children(head)
            .is_none_or(|children| !children.is_empty())
        || builder.node_kind(body) != Some(NodeKind::Body)
        || builder.node_macro_name(body) != Some("It")
    {
        return false;
    }
    let Some(body_children) = builder.children(body) else {
        return false;
    };
    if body_children.is_empty()
        || !body_children
            .iter()
            .all(|child| builder.node_kind(*child) == Some(NodeKind::Table))
    {
        return false;
    }
    builder.append_existing_child(body, table)
}

/// Materialize the first tbl row in a `Bl -column` body as the implicit `It`
/// that mandoc exposes in its owned tree.  The table already carries its
/// source location and presentation flags, while the synthetic item, head,
/// and body inherit only structural location information.
fn structure_implicit_column_table_item(
    builder: &mut DocumentBuilder,
    list_body: NodeId,
    table: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> bool {
    let location = builder.node_location(table);
    if builder.node_count().saturating_add(3) > max_nodes {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = location;
        }
        return false;
    }
    let Some(item) = builder.push(list_body, NodeKind::Block) else {
        return false;
    };
    let Some(head) = builder.push(item, NodeKind::Head) else {
        return false;
    };
    let Some(body) = builder.push(item, NodeKind::Body) else {
        return false;
    };
    if !builder.macro_name(item, "It")
        || !builder.macro_name(head, "It")
        || !builder.macro_name(body, "It")
        || !builder.set_node_location(item, location.clone())
        || !builder.set_node_location(head, location.clone())
        || !builder.set_node_location(body, location)
        || !builder.replace_children(item, &[head, body])
        || !builder.replace_children(body, &[table])
    {
        return false;
    }
    true
}

/// Inline mdoc forms accepted as a row when a `Bl -column` source omits
/// `.It`. Structural controls retain their ordinary dispatch rather than
/// becoming accidental cells.
fn is_implicit_column_row_macro(name: Option<&str>) -> bool {
    matches!(
        name,
        Some("Cm" | "Dv" | "Em" | "Er" | "Ev" | "Fl" | "Ic" | "Li" | "Ms" | "No" | "Sy" | "Va")
    )
}

/// Materialize one `Bl -column` row whose source omitted the usual `.It`.
///
/// The first cell retains an authored mdoc element or literal text node.  The
/// remaining cells begin at in-line `Ta` controls or literal tab boundaries,
/// exactly like the explicit-item splitter above.  This deliberately runs
/// before the global Em/Sy fallback tag pass, allowing the implicit `It` to
/// own the destination while the inline element keeps its permalink.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn structure_implicit_column_item(
    builder: &mut DocumentBuilder,
    list_body: NodeId,
    node: NodeId,
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
    scopes: &mut Vec<ScopeFrame>,
) -> bool {
    let location = builder.node_location(node);
    let source_flags = builder.node_flags(node).unwrap_or_default();
    let inline_ta_text_count = (builder.node_kind(node) != Some(NodeKind::Text))
        .then(|| builder.children(node))
        .flatten()
        .map_or(0, |children| {
            children
                .iter()
                .filter_map(|child| builder.node_text(*child))
                .map(inline_column_ta_count)
                .sum::<usize>()
        });
    let element_cells = (builder.node_kind(node) != Some(NodeKind::Text))
        .then(|| builder.children(node))
        .flatten()
        .map(|children| {
            1 + children
                .iter()
                .filter(|child| builder.node_text(**child) == Some("Ta"))
                .count()
                + inline_ta_text_count
        });
    let text_cells = builder
        .node_text(node)
        .filter(|text| text.contains('\t'))
        .map(|text| text.split('\t').count());
    let Some(cell_count) = element_cells.or(text_cells) else {
        return false;
    };
    // Block + Head + one Body per cell. Literal text additionally needs one
    // new node for every cell after the first.
    let additional_nodes = 2_usize
        .saturating_add(cell_count)
        .saturating_add(text_cells.unwrap_or(1).saturating_sub(1))
        .saturating_add(inline_ta_text_count);
    if builder.node_count().saturating_add(additional_nodes) > max_nodes {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = location;
        }
        return false;
    }

    let Some(item) = builder.push(list_body, NodeKind::Block) else {
        return false;
    };
    let Some(head) = builder.push(item, NodeKind::Head) else {
        return false;
    };
    let Some(first_body) = builder.push(item, NodeKind::Body) else {
        return false;
    };
    if !builder.macro_name(item, "It")
        || !builder.macro_name(head, "It")
        || !builder.macro_name(first_body, "It")
        || !builder.set_node_location(item, location.clone())
        || !builder.set_node_location(head, location.clone())
        || !builder.set_node_location(first_body, location.clone())
        || !builder.replace_children(item, &[head, first_body])
    {
        return false;
    }
    let mut item_flags = source_flags;
    item_flags.deep_link_target = false;
    item_flags.permalink = false;
    let _ = builder.set_node_flags(item, item_flags);
    let mut child_flags = source_flags;
    child_flags.line_start = false;
    child_flags.deep_link_target = false;
    child_flags.permalink = false;
    let _ = builder.set_node_flags(node, child_flags);

    if builder.node_kind(node) != Some(NodeKind::Text)
        && let Some(tokens) = builder.children(node).map(<[NodeId]>::to_vec)
    {
        let mut cells = vec![Vec::new()];
        let mut body_locations = vec![location.clone()];
        for token in tokens {
            if builder.node_text(token) == Some("Ta") {
                cells.push(Vec::new());
                body_locations.push(builder.node_location(token));
            } else if let Some((prefix, suffix, separator_end)) = builder
                .node_text(token)
                .and_then(split_inline_column_ta_argument)
                .map(|(prefix, suffix, separator_end)| {
                    (prefix.to_owned(), suffix.to_owned(), separator_end)
                })
            {
                let token_location = builder.node_location(token);
                let prefix_length = prefix.len();
                let _ = builder.set_node_text(token, prefix);
                cells
                    .last_mut()
                    .expect("implicit column row has one cell")
                    .push(token);
                cells.push(Vec::new());
                let separator_location = token_location.as_ref().and_then(|span| {
                    let start = span
                        .start
                        .checked_add(u32::try_from(prefix_length + 1).ok()?)?;
                    SourceSpan::new(span.source, start, span.end).ok()
                });
                body_locations.push(separator_location);
                let Some(tail) = builder.push(node, NodeKind::Text) else {
                    return false;
                };
                let tail_location = token_location.and_then(|span| {
                    let start = span.start.checked_add(u32::try_from(separator_end).ok()?)?;
                    SourceSpan::new(span.source, start, span.end).ok()
                });
                if !builder.text(tail, suffix)
                    || !builder.set_node_location(tail, tail_location)
                    || !builder.set_node_flags(tail, NodeFlags::default())
                {
                    return false;
                }
                cells
                    .last_mut()
                    .expect("implicit column row has one cell")
                    .push(tail);
            } else {
                cells
                    .last_mut()
                    .expect("implicit column row has one cell")
                    .push(token);
            }
        }
        let first = cells.remove(0);
        if !builder.replace_children(node, &first) || !builder.replace_children(first_body, &[node])
        {
            return false;
        }
        let mut bodies = vec![first_body];
        for (tokens, cell_location) in cells.into_iter().zip(body_locations.into_iter().skip(1)) {
            let Some(body) = builder.push(item, NodeKind::Body) else {
                return false;
            };
            if !builder.macro_name(body, "It")
                || !builder.set_node_location(body, cell_location)
                || !builder.set_node_flags(body, NodeFlags::default())
            {
                return false;
            }
            let events = split_mdoc_inline_tokens(
                builder,
                body,
                &tokens,
                spacing_enabled,
                max_nodes,
                outcome,
            );
            let _ = builder.replace_children(body, &events);
            structure_nested_implicit_partial_blocks(
                builder,
                body,
                max_nodes,
                outcome,
                spacing_enabled,
            );
            structure_column_cell_explicit_partials(
                builder,
                body,
                max_nodes,
                outcome,
                spacing_enabled,
                scopes,
            );
            bodies.push(body);
        }
        let mut children = vec![head];
        children.extend(bodies);
        let _ = builder.replace_children(item, &children);
        if matches!(builder.node_macro_name(node), Some("Em" | "Sy"))
            && let Some((tag, explicit)) = inline_target_name(builder, node)
        {
            mark_manual_target(builder, item, &tag);
            mark_permalink(builder, node, explicit.then_some(tag.as_str()));
        }
        return true;
    }

    let Some(text) = builder.node_text(node).map(str::to_owned) else {
        return false;
    };
    let Some(text_location) = location else {
        return false;
    };
    let mut bodies = vec![first_body];
    let mut offset = 0_usize;
    for (index, value) in text.split('\t').enumerate() {
        let start = text_location
            .start
            .saturating_add(u32::try_from(offset).unwrap_or(u32::MAX));
        let end = start.saturating_add(u32::try_from(value.len()).unwrap_or(u32::MAX));
        if index == 0 {
            let _ = builder.set_node_text(node, value);
            let _ = builder
                .set_node_location(node, SourceSpan::new(text_location.source, start, end).ok());
            let _ = builder.replace_children(first_body, &[node]);
        } else {
            let Some(body) = builder.push(item, NodeKind::Body) else {
                return false;
            };
            let Some(cell) = builder.push(body, NodeKind::Text) else {
                return false;
            };
            if !builder.macro_name(body, "It")
                || !builder.set_node_location(body, Some(text_location.clone()))
                || !builder.text(cell, value)
                || !builder
                    .set_node_location(cell, SourceSpan::new(text_location.source, start, end).ok())
                || !builder.set_node_flags(cell, NodeFlags::default())
            {
                return false;
            }
            bodies.push(body);
        }
        offset = offset.saturating_add(value.len()).saturating_add(1);
    }
    let mut children = vec![head];
    children.extend(bodies);
    let _ = builder.replace_children(item, &children);
    true
}

/// Count unescaped, whole-word `Ta` spellings that mdoc's inline parser has
/// already coalesced into one argument phrase. A standalone token is handled
/// by the main splitter, so this only counts embedded forms such as `c Ta d`.
fn inline_column_ta_count(text: &str) -> usize {
    usize::from(split_inline_column_ta_argument(text).is_some())
}

/// Split the first embedded ` Ta ` phrase separator while preserving the
/// exact byte offset used to source-locate the following cell.
fn split_inline_column_ta_argument(text: &str) -> Option<(&str, &str, usize)> {
    let (prefix, suffix) = text.split_once(" Ta ")?;
    (!prefix.is_empty() && !suffix.is_empty()).then_some((prefix, suffix, prefix.len() + 4))
}

fn make_block(
    builder: &mut DocumentBuilder,
    block: NodeId,
    macro_name: &str,
    placement: ArgumentPlacement,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Option<(NodeId, NodeId)> {
    if builder.node_kind(block) != Some(NodeKind::Element) {
        return None;
    }
    if builder.node_count().saturating_add(2) > max_nodes {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = builder.node_location(block);
        }
        return None;
    }
    if matches!(placement, ArgumentPlacement::Body) {
        coalesce_text_children(builder, block);
    }
    let arguments = builder.children(block)?.to_vec();
    let location = builder.node_location(block);
    let head = builder.push(block, NodeKind::Head)?;
    let body = builder.push(block, NodeKind::Body)?;
    if !builder.set_node_kind(block, NodeKind::Block)
        || !builder.macro_name(block, macro_name)
        || !builder.macro_name(head, macro_name)
        || !builder.macro_name(body, macro_name)
        || !builder.set_node_location(head, location.clone())
        || !builder.set_node_location(body, location)
        || !builder.replace_children(block, &[head, body])
    {
        return None;
    }
    match placement {
        ArgumentPlacement::Head => {
            let _ = builder.replace_children(head, &arguments);
        }
        ArgumentPlacement::Body | ArgumentPlacement::BodyTokens => {
            let _ = builder.replace_children(body, &arguments);
        }
        ArgumentPlacement::Drop => {}
    }
    Some((head, body))
}

/// Rebuild one `Bl -column` item as the legacy sequence of `It` bodies.
///
/// Column-list arguments are phrases rather than a normal `It` head.  `Ta`
/// is an in-line request separating phrases, and a tab ends the current
/// phrase even though generic argument lexing otherwise treats it like a
/// space.  The scanner records that delimiter privately so the public arena
/// can remain source-agnostic after this package pass finishes.
#[allow(
    clippy::naive_bytecount,
    clippy::too_many_arguments,
    clippy::too_many_lines
)] // Ordered column-cell recovery mirrors libmandoc's stateful parser without exposing scanner provenance publicly.
fn split_column_item_cells(
    builder: &mut DocumentBuilder,
    item: NodeId,
    head: NodeId,
    first_body: NodeId,
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
    scopes: &mut Vec<ScopeFrame>,
) -> Option<Vec<NodeId>> {
    let tokens = builder.children(head)?.to_vec();
    let item_location = builder.node_location(item);
    let additional_text_nodes = tokens
        .iter()
        .filter_map(|token| builder.node_text(*token))
        .map(|text| text.bytes().filter(|byte| *byte == b'\t').count())
        .sum::<usize>();
    let mut cells = vec![Vec::new()];
    let mut suppress_first_tab_column_system_name = vec![false];
    let mut leading_tab_padding = vec![false];
    let mut terminal_tab_cell_padding = vec![false];
    let mut body_locations = vec![item_location.clone()];
    let token_count = tokens.len();
    for (token_index, token) in tokens.into_iter().enumerate() {
        if builder.node_text(token) == Some("Ta") {
            cells.push(Vec::new());
            suppress_first_tab_column_system_name.push(false);
            leading_tab_padding.push(false);
            terminal_tab_cell_padding.push(false);
            body_locations.push(builder.node_location(token));
            continue;
        }
        let tab_segments = split_column_tab_token(builder, item, token)?;
        for (index, segment) in tab_segments.into_iter().enumerate() {
            if index > 0 {
                let has_leading_space = builder
                    .node_text(segment)
                    .is_some_and(|text| text.starts_with(' '));
                cells.push(Vec::new());
                suppress_first_tab_column_system_name.push(!has_leading_space);
                leading_tab_padding.push(has_leading_space);
                terminal_tab_cell_padding.push(false);
                // A phrase begun by an in-token tab uses the original `.It`
                // position just like one begun by an ordinary tab separator.
                body_locations.push(item_location.clone());
            }
            cells
                .last_mut()
                .expect("column items always have a first cell")
                .push(segment);
        }
        if builder.node_separator_contains_tab(token) {
            let has_leading_tab_padding = builder.node_separator_after(token) == Some(b'\t')
                && builder.node_separator_width(token) > 1;
            cells.push(Vec::new());
            suppress_first_tab_column_system_name.push(
                builder.node_separator_after(token) == Some(b'\t')
                    && builder.node_separator_width(token) == 1,
            );
            leading_tab_padding.push(has_leading_tab_padding);
            terminal_tab_cell_padding.push(token_index + 1 == token_count);
            // A phrase begun by a tab uses the original `.It` position;
            // `Ta`, in contrast, has its own in-line source position.
            body_locations.push(item_location.clone());
        }
    }

    let additional_bodies = cells.len().saturating_sub(1);
    let additional_tab_padding_nodes = leading_tab_padding.iter().filter(|value| **value).count();
    let additional_terminal_tab_nodes = terminal_tab_cell_padding
        .iter()
        .filter(|value| **value)
        .count();
    if builder
        .node_count()
        .saturating_add(additional_bodies)
        .saturating_add(additional_text_nodes)
        .saturating_add(additional_tab_padding_nodes)
        .saturating_add(additional_terminal_tab_nodes)
        > max_nodes
    {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = item_location;
        }
        return None;
    }

    let mut bodies = vec![first_body];
    for location in body_locations.into_iter().skip(1) {
        let body = builder.push(item, NodeKind::Body)?;
        if !builder.macro_name(body, "It")
            || !builder.set_node_location(body, location)
            || !builder.set_node_flags(body, NodeFlags::default())
        {
            return None;
        }
        bodies.push(body);
    }
    let mut item_children = Vec::with_capacity(bodies.len().saturating_add(1));
    item_children.push(head);
    item_children.extend(bodies.iter().copied());
    if !builder.replace_children(head, &[]) || !builder.replace_children(item, &item_children) {
        return None;
    }

    for (
        (((body, tokens), suppress_first_tab_column_system_name), leading_tab_padding),
        terminal_tab_cell_padding,
    ) in bodies
        .iter()
        .copied()
        .zip(cells)
        .zip(suppress_first_tab_column_system_name)
        .zip(leading_tab_padding)
        .zip(terminal_tab_cell_padding)
    {
        let mut inline_tokens = Vec::with_capacity(
            tokens.len()
                + usize::from(leading_tab_padding)
                + usize::from(terminal_tab_cell_padding),
        );
        if leading_tab_padding {
            let padding = builder.push(body, NodeKind::Text)?;
            let location = tokens
                .first()
                .and_then(|token| builder.node_location(*token))
                .and_then(|span| {
                    let start = span.start.checked_sub(1)?;
                    SourceSpan::new(span.source, start, span.start).ok()
                });
            if !builder.text(padding, String::new())
                || !builder.set_node_location(padding, location)
                || !builder.set_node_flags(padding, NodeFlags::default())
            {
                return None;
            }
            inline_tokens.push(padding);
        }
        if terminal_tab_cell_padding {
            let padding = builder.push(body, NodeKind::Text)?;
            if !builder.text(padding, r"\&".to_owned())
                || !builder.set_node_location(padding, item_location.clone())
                || !builder.set_node_flags(padding, NodeFlags::default())
            {
                return None;
            }
            inline_tokens.push(padding);
        }
        inline_tokens.extend(tokens);
        let events = split_mdoc_inline_tokens_with_options(
            builder,
            body,
            &inline_tokens,
            spacing_enabled,
            max_nodes,
            outcome,
            suppress_first_tab_column_system_name,
        );
        let _ = builder.replace_children(body, &events);
        for event in &events {
            let Some(macro_name) = builder.node_macro_name(*event).map(str::to_owned) else {
                continue;
            };
            if !insert_generated_system_name(builder, *event, &macro_name, max_nodes)
                && outcome.node_limit_location.is_none()
            {
                outcome.node_limit_location = builder.node_location(*event);
            }
        }
        // Column cells own the same parsed inline stream as ordinary list
        // item bodies. In particular an `Aq` nested before `Ta` is an
        // implicit partial Block, not a flat Element merely because the
        // source reached it through the column-cell splitter.
        structure_nested_implicit_partial_blocks(
            builder,
            body,
            max_nodes,
            outcome,
            spacing_enabled,
        );
        structure_column_cell_explicit_partials(
            builder,
            body,
            max_nodes,
            outcome,
            spacing_enabled,
            scopes,
        );
    }
    Some(bodies)
}

/// Detach an inline `Ta` and its following phrase before an ordinary mdoc
/// macro consumes the entire source line.  The prefix remains owned by that
/// macro; the suffix is moved into the next Body of the active column row.
fn take_inline_column_ta_tail(
    builder: &mut DocumentBuilder,
    node: NodeId,
    active_body: NodeId,
) -> Option<(Vec<NodeId>, Option<SourceSpan>)> {
    active_column_item(builder, active_body)?;
    if builder.node_macro_name(node) == Some("It") {
        return None;
    }
    let tokens = builder.children(node)?.to_vec();
    let separator = tokens
        .iter()
        .position(|token| builder.node_text(*token) == Some("Ta"))?;
    let tail = tokens.get(separator + 1..)?.to_vec();
    let location = builder.node_location(tokens[separator]);
    if !builder.replace_children(node, &tokens[..separator]) {
        return None;
    }
    Some((tail, location))
}

/// Append one cell introduced by a physical or inline `Ta` to the active
/// `Bl -column` row.  The separator is syntax only; its source position
/// becomes the new Body's location, matching libmandoc's row projection.
#[allow(clippy::too_many_arguments)]
fn append_column_ta_cell(
    builder: &mut DocumentBuilder,
    active_body: NodeId,
    location: Option<SourceSpan>,
    tokens: &[NodeId],
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
    scopes: &mut Vec<ScopeFrame>,
) -> Option<NodeId> {
    let item = active_column_item(builder, active_body)?;
    if builder.node_count() >= max_nodes {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = location;
        }
        return None;
    }
    let body = builder.push(item, NodeKind::Body)?;
    if !builder.macro_name(body, "It")
        || !builder.set_node_location(body, location)
        || !builder.set_node_flags(body, NodeFlags::default())
    {
        return None;
    }
    // The scanner retains ordinary macro arguments as separate text nodes;
    // a cell introduced by an in-line `Ta` is nevertheless one mdoc phrase.
    // Reuse the normal body coalescer before its inline pass so `after tab`
    // remains one public Text node, as it does through a regular item body.
    if !builder.replace_children(body, tokens) {
        return None;
    }
    coalesce_text_children(builder, body);
    let cell_tokens = builder.children(body)?.to_vec();
    let events = split_mdoc_inline_tokens(
        builder,
        body,
        &cell_tokens,
        spacing_enabled,
        max_nodes,
        outcome,
    );
    if !builder.replace_children(body, &events) {
        return None;
    }
    for event in &events {
        let Some(macro_name) = builder.node_macro_name(*event).map(str::to_owned) else {
            continue;
        };
        if !insert_generated_system_name(builder, *event, &macro_name, max_nodes)
            && outcome.node_limit_location.is_none()
        {
            outcome.node_limit_location = builder.node_location(*event);
        }
    }
    structure_nested_implicit_partial_blocks(builder, body, max_nodes, outcome, spacing_enabled);
    structure_column_cell_explicit_partials(
        builder,
        body,
        max_nodes,
        outcome,
        spacing_enabled,
        scopes,
    );
    Some(body)
}

/// Account for one late `Ta` cell against a row whose initial short prefix
/// was intentionally held pending. Once the declared count is reached, the
/// row no longer needs a deferred wrong-cell finding.
fn extend_pending_short_column_item(
    pending_short_column_items: &mut BTreeMap<NodeId, (usize, usize)>,
    item: NodeId,
) {
    let complete = if let Some((columns, cells)) = pending_short_column_items.get_mut(&item) {
        *cells = cells.saturating_add(1);
        *cells >= *columns
    } else {
        false
    };
    if complete {
        pending_short_column_items.remove(&item);
    }
}

/// Split literal tab bytes retained inside one column argument into individual
/// source phrases.  A quoted phrase still treats its literal tabs as cell
/// boundaries; this differs from generic argument lexing, which correctly
/// keeps the quoted token intact for every other mdoc macro family.
fn split_column_tab_token(
    builder: &mut DocumentBuilder,
    item: NodeId,
    token: NodeId,
) -> Option<Vec<NodeId>> {
    let text = builder.node_text(token)?.to_owned();
    if !text.contains('\t') {
        return Some(vec![token]);
    }
    let flags = builder.node_flags(token).unwrap_or_default();
    let location = builder.node_location(token);
    let quoted = builder.node_argument_quoted(token);
    let mut segments = text.split('\t');
    let first = segments
        .next()
        .expect("contains a tab but always has a prefix");
    if !builder.text(token, first.to_owned()) {
        return None;
    }
    let mut retained = vec![token];
    let mut text_offset = first.len().saturating_add(1);
    for segment in segments {
        let child = builder.push(item, NodeKind::Text)?;
        if !builder.text(child, segment.to_owned()) || !builder.set_node_flags(child, flags) {
            return None;
        }
        if let Some(span) = location.as_ref() {
            let source_offset = text_offset.saturating_add(usize::from(quoted));
            let start = span
                .start
                .saturating_add(u32::try_from(source_offset).unwrap_or(u32::MAX));
            let end = start.saturating_add(u32::try_from(segment.len()).unwrap_or(u32::MAX));
            let location = SourceSpan::new(span.source, start, end).ok()?;
            if !builder.set_node_location(child, Some(location)) {
                return None;
            }
        }
        retained.push(child);
        text_offset = text_offset.saturating_add(segment.len()).saturating_add(1);
    }
    Some(retained)
}

/// Count column phrases from the scanner representation before the package
/// pass turns them into `It` Bodies.  A tab can be embedded in a quoted
/// argument or occur later in an otherwise space-prefixed separator run, and
/// both spellings are semantic cell boundaries in mdoc.
fn column_item_cell_count(builder: &DocumentBuilder, item: NodeId) -> usize {
    let Some(tokens) = builder.children(item) else {
        return 1;
    };
    let mut cells = 1_usize;
    for token in tokens {
        if builder.node_text(*token) == Some("Ta") {
            cells = cells.saturating_add(1);
            continue;
        }
        cells = cells.saturating_add(builder.node_embedded_tab_count(*token) as usize);
        if builder.node_separator_contains_tab(*token) {
            cells = cells.saturating_add(1);
        }
    }
    cells
}

/// Complete the preceding zero-argument column item at its next structural
/// boundary.  libmandoc keeps such an item when its first Body acquires input
/// from the following physical line, but removes it when another item or the
/// list closer arrives first.
fn finalize_last_empty_column_item(
    builder: &mut DocumentBuilder,
    list_body: NodeId,
    pending_empty_column_items: &mut BTreeSet<NodeId>,
    outcome: &mut StructureOutcome,
) {
    let Some(item) = builder
        .children(list_body)
        .and_then(|children| children.last())
        .copied()
        .filter(|item| builder.node_macro_name(*item) == Some("It"))
    else {
        return;
    };
    if !pending_empty_column_items.remove(&item) {
        return;
    }
    let bodies = builder
        .children(item)
        .map(|children| {
            children
                .iter()
                .copied()
                .filter(|child| builder.node_kind(*child) == Some(NodeKind::Body))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if bodies
        .first()
        .is_none_or(|body| builder.children(*body).is_none_or(<[NodeId]>::is_empty))
    {
        if bodies.len() == 1 {
            let mut retained = builder
                .children(list_body)
                .map(<[NodeId]>::to_vec)
                .unwrap_or_default();
            retained.pop();
            let _ = builder.replace_children(list_body, &retained);
            outcome.recoveries.push(Recovery::EmptyMacro {
                macro_name: "It",
                location: builder.node_location(item),
            });
        }
        return;
    }
    outcome.recoveries.push(Recovery::ColumnItemUsesNextLine {
        location: builder.node_location(item),
    });
}

/// Whether an item Head is syntax only for this list selector.  The selector
/// is retained separately from `NormalizedListKind`, whose `Plain` projection
/// deliberately merges several mdoc list families with different validators.
fn fixed_head_list_type(list_type: &str) -> bool {
    matches!(list_type, "bullet" | "dash" | "enum" | "hyphen" | "item")
}

/// Validate the immediately preceding fixed-head item when the next item or
/// the list close gives it a complete Body. This ordering is observable: an
/// empty item warning precedes its ignored Head arguments, while an earlier
/// non-empty row reports its ignored arguments before a later empty row.
fn finalize_last_fixed_head_list_item(
    builder: &DocumentBuilder,
    list_body: NodeId,
    list_type: &'static str,
    deferred_argument_items: &BTreeSet<NodeId>,
    outcome: &mut StructureOutcome,
) {
    let Some(item) = builder
        .children(list_body)
        .and_then(|children| children.last())
        .copied()
        .filter(|item| builder.node_macro_name(*item) == Some("It"))
    else {
        return;
    };
    let Some((head, body)) = builder.children(item).and_then(|children| {
        let head = children.iter().copied().find(|child| {
            builder.node_kind(*child) == Some(NodeKind::Head)
                && builder.node_macro_name(*child) == Some("It")
        })?;
        let body = children.iter().copied().find(|child| {
            builder.node_kind(*child) == Some(NodeKind::Body)
                && builder.node_macro_name(*child) == Some("It")
        })?;
        Some((head, body))
    }) else {
        return;
    };
    let location = builder.node_location(item);
    if list_type != "item" && builder.children(body).is_none_or(<[NodeId]>::is_empty) {
        outcome.recoveries.push(Recovery::EmptyListItem {
            list_type,
            location: location.clone(),
        });
    }
    let arguments = fixed_head_item_arguments(builder, head);
    if !deferred_argument_items.contains(&item) && !arguments.is_empty() {
        outcome.recoveries.push(Recovery::InvalidArguments {
            message: format!("skipping all arguments: It {arguments}").into(),
            location,
        });
    }
}

/// Summarize a marker-style item's ignored Head as mandoc's validator does:
/// ordinary prose remains one phrase, while a callable macro contributes its
/// own selector but none of its private argument subtree.
fn fixed_head_item_arguments(builder: &DocumentBuilder, head: NodeId) -> String {
    builder
        .children(head)
        .into_iter()
        .flatten()
        .filter_map(|child| {
            builder
                .node_text(*child)
                .or_else(|| builder.node_macro_name(*child))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Report all still-short rows in one completed column list.  A physical
/// `.Ta` can add a cell after an `.It` has already been structured, so the
/// row is deliberately not diagnosed until its next item or list boundary.
fn finalize_short_column_items(
    builder: &DocumentBuilder,
    list_body: NodeId,
    pending_short_column_items: &mut BTreeMap<NodeId, (usize, usize)>,
    outcome: &mut StructureOutcome,
) {
    let pending = pending_short_column_items
        .iter()
        .filter_map(|(item, (columns, cells))| {
            (builder.node_parent(*item) == Some(list_body)).then_some((*item, *columns, *cells))
        })
        .collect::<Vec<_>>();
    for (item, columns, cells) in pending {
        pending_short_column_items.remove(&item);
        outcome.recoveries.push(Recovery::WrongNumberOfColumnCells {
            columns,
            cells,
            location: builder.node_location(item),
        });
    }
}

/// Reify explicit partial openers embedded in a column cell and retain their
/// cross-line close state. The ordinary top-level dispatcher cannot see these
/// as standalone source nodes: they began life as `.It` arguments, but a
/// following physical `.Bc`/… still closes the same mdoc scope.
fn structure_column_cell_explicit_partials(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
    spacing_enabled: bool,
    scopes: &mut Vec<ScopeFrame>,
) {
    let Some(children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
        return;
    };
    for node in children {
        let Some(name) = builder.node_macro_name(node).map(str::to_owned) else {
            continue;
        };
        let Some(close) = explicit_partial_block_close(&name) else {
            continue;
        };
        let Some((head, body)) = make_block(
            builder,
            node,
            &name,
            ArgumentPlacement::BodyTokens,
            max_nodes,
            outcome,
        ) else {
            continue;
        };
        let children =
            split_mdoc_inline_children(builder, body, spacing_enabled, max_nodes, outcome);
        let _ = builder.replace_children(body, &children);
        clear_leading_explicit_partial_punctuation(builder, body);
        move_explicit_leading_open_delimiter(builder, node, head, body);
        coalesce_adjacent_text_children(builder, body);
        scopes.push(ScopeFrame {
            close,
            open: node,
            body,
            tail_on_close: false,
            transparent_target_taken: false,
            suppress_implicit_ancestor_break: false,
            resume_active: parent,
            resume_flow: parent,
        });
    }
}

/// Allocate a source-less partial block nested inside a validated parent.
/// `.It Xo` carries its opener as an `It` argument rather than a scanner
/// event, so it needs the same public Block/Head/Body shape without first
/// converting an independent source node via [`make_block`].
fn make_synthetic_block(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    macro_name: &str,
    location: Option<SourceSpan>,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Option<(NodeId, NodeId, NodeId)> {
    if builder.node_count().saturating_add(3) > max_nodes {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = location;
        }
        return None;
    }
    let block = builder.push(parent, NodeKind::Block)?;
    let head = builder.push(block, NodeKind::Head)?;
    let body = builder.push(block, NodeKind::Body)?;
    if !builder.macro_name(block, macro_name)
        || !builder.macro_name(head, macro_name)
        || !builder.macro_name(body, macro_name)
        || !builder.set_node_location(block, location.clone())
        || !builder.set_node_location(head, location.clone())
        || !builder.set_node_location(body, location)
        || !builder.replace_children(block, &[head, body])
    {
        return None;
    }
    Some((block, head, body))
}

/// mdoc synthesizes the document name for an empty `.Nm`. This is an
/// AST-visible generated word, not a renderer convenience.
fn insert_generated_nm_name(
    builder: &mut DocumentBuilder,
    source: NodeId,
    head: NodeId,
    max_nodes: usize,
) -> bool {
    if builder.node_count() >= max_nodes {
        return false;
    }
    let Some(name) = builder.metadata_mut().name.clone() else {
        return true;
    };
    let Some(text) = builder.push(head, NodeKind::Text) else {
        return false;
    };
    if !builder.text(text, name) || !builder.set_node_location(text, builder.node_location(source))
    {
        return false;
    }
    let Some(mut flags) = builder.node_flags(text) else {
        return false;
    };
    flags.generated = true;
    builder.set_node_flags(text, flags)
}

/// An empty `.Ar` exposes mdoc's generated default argument words.  They are
/// separate nodes in the owned AST (rather than one renderer-only string), so
/// canonical consumers can preserve their generated provenance.
fn insert_generated_ar_default(
    builder: &mut DocumentBuilder,
    source: NodeId,
    parent: NodeId,
    max_nodes: usize,
) -> bool {
    const DEFAULT_WORDS: [&str; 2] = ["file", "..."];
    if builder.node_count().saturating_add(DEFAULT_WORDS.len()) > max_nodes {
        return false;
    }
    let synopsis_pretty = builder
        .node_flags(source)
        .is_some_and(|flags| flags.synopsis_pretty);
    let location = builder.node_location(source);
    for word in DEFAULT_WORDS {
        let Some(text) = builder.push(parent, NodeKind::Text) else {
            return false;
        };
        let flags = NodeFlags {
            generated: true,
            synopsis_pretty,
            ..NodeFlags::default()
        };
        if !builder.text(text, word)
            || !builder.set_node_location(text, location.clone())
            || !builder.set_node_flags(text, flags)
        {
            return false;
        }
    }
    true
}

/// Empty `.Mt` and `.Pa` elements use mandoc's generated nonbreaking-space
/// placeholder so following punctuation remains separated from prior prose.
fn insert_generated_nonbreaking_default(
    builder: &mut DocumentBuilder,
    source: NodeId,
    max_nodes: usize,
) -> bool {
    if builder.node_count() >= max_nodes {
        return false;
    }
    let Some(text) = push_generated_text(builder, source, "~", false) else {
        return false;
    };
    if builder
        .node_flags(source)
        .is_some_and(|flags| flags.synopsis_pretty)
        && let Some(mut flags) = builder.node_flags(text)
    {
        flags.synopsis_pretty = true;
        return builder.set_node_flags(text, flags);
    }
    true
}

/// Apply compact-system-name generation to a parsed inline event list.  This
/// is needed after a partial block's Body re-enters inline parsing, which
/// bypasses the top-level source-order dispatcher.
fn insert_generated_system_names(
    builder: &mut DocumentBuilder,
    events: &[NodeId],
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) {
    for event in events {
        let Some(macro_name) = builder.node_macro_name(*event).map(str::to_owned) else {
            continue;
        };
        if !insert_generated_system_name(builder, *event, &macro_name, max_nodes)
            && outcome.node_limit_location.is_none()
        {
            outcome.node_limit_location = builder.node_location(*event);
        }
    }
}

/// Allocate the default operating-system word published by mdoc's compact
/// system-name macros.  The generated child remains distinct from any
/// authored version/name argument, matching the legacy owned AST rather than
/// deferring the spelling to a renderer.
fn insert_generated_system_name(
    builder: &mut DocumentBuilder,
    source: NodeId,
    macro_name: &str,
    max_nodes: usize,
) -> bool {
    if macro_name == "Bx" {
        return insert_generated_bx(builder, source, max_nodes);
    }
    let Some(name) = generated_system_name(macro_name) else {
        return true;
    };
    if builder.node_count() >= max_nodes {
        return false;
    }
    let existing_children = builder
        .children(source)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    let Some(text) = builder.push(source, NodeKind::Text) else {
        return false;
    };
    let flags = NodeFlags {
        generated: true,
        ..NodeFlags::default()
    };
    if !(builder.text(text, name)
        && builder.set_node_location(text, builder.node_location(source))
        && builder.set_node_flags(text, flags))
    {
        return false;
    }
    // `post_xx()` constructs the generated operating-system word before an
    // optional authored version.
    let mut children = Vec::with_capacity(existing_children.len() + 1);
    children.push(text);
    children.extend(existing_children);
    builder.replace_children(source, &children)
}

/// Mirror mdoc's specialised `post_bx()` validation.
///
/// Unlike the other compact system-name macros, `Bx` makes its authored
/// version and the generated `BSD` word adjoining words, and a second
/// argument forms the generated `-` separator plus a title-cased BSD variant.
/// Keep these as distinct generated AST nodes: renderers and canonical
/// differential tests both consume their topology and provenance.
fn insert_generated_bx(builder: &mut DocumentBuilder, source: NodeId, max_nodes: usize) -> bool {
    let existing_children = builder
        .children(source)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    // List-column restructuring may call the common system-name helper after
    // its body has already been normalised. Never publish a second synthetic
    // BSD sequence when that happens.
    if existing_children.iter().any(|child| {
        builder.node_text(*child) == Some("BSD")
            && builder
                .node_flags(*child)
                .is_some_and(|flags| flags.generated)
    }) {
        return true;
    }

    let additional_nodes = match existing_children.len() {
        0 => 1,
        1 => 2,
        _ => 5,
    };
    if builder.node_count().saturating_add(additional_nodes) > max_nodes {
        return false;
    }
    let location = builder.node_location(source);
    let Some(bsd) = push_generated_text_at(builder, source, "BSD", false, location.clone()) else {
        return false;
    };
    if existing_children.is_empty() {
        return builder.replace_children(source, &[bsd]);
    }

    let Some(before_bsd) = push_generated_element(builder, source, "Ns", location.clone()) else {
        return false;
    };
    let mut children = Vec::with_capacity(existing_children.len().saturating_add(5));
    children.push(existing_children[0]);
    children.push(before_bsd);
    children.push(bsd);

    if let Some(second_argument) = existing_children.get(1).copied() {
        let Some(before_dash) = push_generated_element(builder, source, "Ns", location.clone())
        else {
            return false;
        };
        let Some(dash) = push_generated_text_at(builder, source, "-", false, location.clone())
        else {
            return false;
        };
        let Some(before_variant) = push_generated_element(builder, source, "Ns", location) else {
            return false;
        };
        if let Some(value) = builder.node_text(second_argument) {
            let mut title_cased = value.as_bytes().to_vec();
            if let Some(first) = title_cased.first_mut() {
                first.make_ascii_uppercase();
            }
            let Ok(title_cased) = String::from_utf8(title_cased) else {
                return false;
            };
            if !builder.text(second_argument, title_cased) {
                return false;
            }
        }
        children.extend([before_dash, dash, before_variant, second_argument]);
        // The lexer currently exposes at most two Bx arguments, but retaining
        // a future scanner extension's tail is safer than silently dropping
        // user syntax before its own validator can classify it.
        children.extend(existing_children.into_iter().skip(2));
    }
    builder.replace_children(source, &children)
}

/// Match `append_delims()`'s quoted-delimiter EOS suppression after `.Bx`.
fn clear_quoted_bx_trailing_delimiter_sentence_end(
    builder: &mut DocumentBuilder,
    candidate: Option<NodeId>,
) {
    let Some(candidate) = candidate else {
        return;
    };
    if !builder.node_argument_quoted(candidate)
        || !builder
            .node_text(candidate)
            .is_some_and(is_mdoc_closing_delimiter)
    {
        return;
    }
    let Some(mut flags) = builder.node_flags(candidate) else {
        return;
    };
    flags.sentence_end = false;
    let _ = builder.set_node_flags(candidate, flags);
}

/// A run of compact system-name requests remains a run of source elements
/// while its words are separated by ordinary whitespace.  A tab immediately
/// following the next spelling changes the legacy `Bl -column` parser state:
/// that spelling is then the current system macro's optional argument rather
/// than a new request.
fn column_system_name_starts_next_element(
    builder: &DocumentBuilder,
    current: NodeId,
    next: NodeId,
) -> bool {
    builder
        .node_macro_name(current)
        .is_some_and(|name| generated_system_name(name).is_some())
        && builder
            .node_text(next)
            .is_some_and(|text| generated_system_name(text).is_some())
        && builder.node_separator_after(next) != Some(b'\t')
}

/// Expand mdoc's standard exit-status sentence.  A missing `-std` is a
/// recoverable validator omission: mandoc adds it, keeps authored words as
/// utility names, and publishes the normal generated sentence.
fn expand_standard_exit_status(
    builder: &mut DocumentBuilder,
    source: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> bool {
    let Some(arguments) = builder.children(source).map(<[NodeId]>::to_vec) else {
        return true;
    };
    let names = if arguments
        .first()
        .is_some_and(|first| builder.node_text(*first) == Some("-std"))
    {
        &arguments[1..]
    } else {
        outcome.recoveries.push(Recovery::MissingStandardSelector {
            macro_name: "Ex",
            location: builder.node_location(source),
        });
        &arguments[..]
    };
    let generated_name_nodes = if names.is_empty() { 2 } else { names.len() };
    let required_nodes = 3_usize
        .saturating_add(generated_name_nodes)
        .saturating_add(names.len().saturating_sub(1));
    if builder.node_count().saturating_add(required_nodes) > max_nodes {
        return false;
    }

    let mut children = Vec::with_capacity(required_nodes);
    let Some(the) = push_generated_text(builder, source, "The", false) else {
        return false;
    };
    children.push(the);

    if names.is_empty() {
        if let Some(name) = builder.metadata_mut().name.clone() {
            let Some(name_element) = push_generated_element(builder, source, "Nm", None) else {
                return false;
            };
            let Some(name_text) = push_generated_text(builder, name_element, &name, false) else {
                return false;
            };
            if !builder.replace_children(name_element, &[name_text]) {
                return false;
            }
            children.push(name_element);
        } else {
            outcome.recoveries.push(Recovery::MissingExitName {
                location: builder.node_location(source),
            });
        }
    } else {
        for (index, name) in names.iter().enumerate() {
            if index > 0 {
                let separator = if index + 1 == names.len() { "and" } else { "," };
                let separator_location = if index + 1 == names.len() {
                    builder.node_location(*name)
                } else {
                    builder.node_location(names[index - 1])
                };
                let Some(separator) =
                    push_generated_text_at(builder, source, separator, false, separator_location)
                else {
                    return false;
                };
                children.push(separator);
            }
            let Some(name_element) =
                push_generated_element(builder, source, "Nm", builder.node_location(*name))
            else {
                return false;
            };
            if !builder.replace_children(name_element, &[*name]) {
                return false;
            }
            children.push(name_element);
        }
    }

    let utility_word = if names.len() > 1 {
        "utilities exit\\~0"
    } else {
        "utility exits\\~0"
    };
    let Some(result) = push_generated_text(builder, source, utility_word, false) else {
        return false;
    };
    children.push(result);
    let Some(outcome) = push_generated_text(
        builder,
        source,
        "on success, and\\~>0 if an error occurs.",
        true,
    ) else {
        return false;
    };
    children.push(outcome);
    builder.replace_children(source, &children)
}

/// Expand mdoc's standard return-value sentence.  A missing `-std` uses the
/// same recoverable defaulting rule as `Ex`; named entries become generated
/// `Fn` elements and the no-name form keeps its alternate introduction.
#[allow(clippy::too_many_lines)] // The two grammar-selected sentence forms share bounded allocation and source-order rules.
fn expand_standard_return_value(
    builder: &mut DocumentBuilder,
    source: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> bool {
    let Some(arguments) = builder.children(source).map(<[NodeId]>::to_vec) else {
        return true;
    };
    let names = if arguments
        .first()
        .is_some_and(|first| builder.node_text(*first) == Some("-std"))
    {
        &arguments[1..]
    } else {
        outcome.recoveries.push(Recovery::MissingStandardSelector {
            macro_name: "Rv",
            location: builder.node_location(source),
        });
        &arguments[..]
    };
    let required_nodes = if names.is_empty() {
        5
    } else {
        7_usize
            .saturating_add(names.len())
            .saturating_add(names.len().saturating_sub(1))
    };
    if builder.node_count().saturating_add(required_nodes) > max_nodes {
        return false;
    }

    let mut children = Vec::with_capacity(required_nodes.saturating_sub(1));
    if names.is_empty() {
        let Some(success) = push_generated_text(
            builder,
            source,
            "Upon successful completion, the value\\~0 is returned;",
            false,
        ) else {
            return false;
        };
        children.push(success);
    } else {
        let Some(the) = push_generated_text(builder, source, "The", false) else {
            return false;
        };
        children.push(the);
        for (index, name) in names.iter().enumerate() {
            if index > 0 {
                let separator = if index + 1 == names.len() { "and" } else { "," };
                let separator_location = if index + 1 == names.len() {
                    builder.node_location(*name)
                } else {
                    builder.node_location(names[index - 1])
                };
                let Some(separator) =
                    push_generated_text_at(builder, source, separator, false, separator_location)
                else {
                    return false;
                };
                children.push(separator);
            }
            let Some(function) =
                push_generated_element(builder, source, "Fn", builder.node_location(*name))
            else {
                return false;
            };
            if !builder.replace_children(function, &[*name]) {
                return false;
            }
            children.push(function);
        }
        let returns = if names.len() > 1 {
            "functions return"
        } else {
            "function returns"
        };
        let Some(returns) = push_generated_text(builder, source, returns, false) else {
            return false;
        };
        children.push(returns);
        let Some(success) =
            push_generated_text(builder, source, "the value\\~0 if successful;", false)
        else {
            return false;
        };
        children.push(success);
    }

    let Some(otherwise) = push_generated_text(
        builder,
        source,
        "otherwise the value\\~\\-1 is returned and the global variable",
        false,
    ) else {
        return false;
    };
    children.push(otherwise);
    let Some(errno) = push_generated_element(builder, source, "Va", None) else {
        return false;
    };
    let Some(errno_text) = push_generated_text(builder, errno, "errno", false) else {
        return false;
    };
    if !builder.replace_children(errno, &[errno_text]) {
        return false;
    }
    children.push(errno);
    let Some(final_clause) =
        push_generated_text(builder, source, "is set to indicate the error.", true)
    else {
        return false;
    };
    children.push(final_clause);
    builder.replace_children(source, &children)
}

/// Allocate a generated text node at an mdoc macro's source location.
fn push_generated_text(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    value: &str,
    sentence_end: bool,
) -> Option<NodeId> {
    push_generated_text_at(builder, parent, value, sentence_end, None)
}

/// Allocate a generated text node, optionally retaining the source position
/// of an authored list word that selected its generated connector.
fn push_generated_text_at(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    value: &str,
    sentence_end: bool,
    location: Option<SourceSpan>,
) -> Option<NodeId> {
    let text = builder.push(parent, NodeKind::Text)?;
    let flags = NodeFlags {
        generated: true,
        sentence_end,
        ..NodeFlags::default()
    };
    (builder.text(text, value)
        && builder.set_node_location(text, location.or_else(|| builder.node_location(parent)))
        && builder.set_node_flags(text, flags))
    .then_some(text)
}

/// Allocate a generated, source-less-in-meaning element while retaining the
/// legacy source position used by mdoc's generated node projection.
fn push_generated_element(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    macro_name: &str,
    location: Option<SourceSpan>,
) -> Option<NodeId> {
    let element = builder.push(parent, NodeKind::Element)?;
    let flags = NodeFlags {
        generated: true,
        ..NodeFlags::default()
    };
    (builder.macro_name(element, macro_name)
        && builder.set_node_location(element, location.or_else(|| builder.node_location(parent)))
        && builder.set_node_flags(element, flags))
    .then_some(element)
}

fn is_legacy_roff_font(font: &[u8]) -> bool {
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

/// The compact system-name macros whose validator inserts a default word.
/// `Bx` uses the same generated-child rule when no BSD variant is authored;
/// its richer two-argument rewriting is retained as a separate follow-up.
fn generated_system_name(macro_name: &str) -> Option<&'static str> {
    match macro_name {
        "Bsx" => Some("BSD/OS"),
        "Bx" => Some("BSD"),
        "Dx" => Some("DragonFly"),
        "Fx" => Some("FreeBSD"),
        "Nx" => Some("NetBSD"),
        "Ox" => Some("OpenBSD"),
        "Ux" => Some("UNIX"),
        _ => None,
    }
}

/// Reborrow a recognized compact system-name spelling as the static recovery
/// label required by the public diagnostic contract.
fn system_macro_name(macro_name: &str) -> &'static str {
    match macro_name {
        "Bsx" => "Bsx",
        "Bx" => "Bx",
        "Dx" => "Dx",
        "Fx" => "Fx",
        "Nx" => "Nx",
        "Ox" => "Ox",
        "Ux" => "Ux",
        _ => unreachable!("only compact system-name macros reach this helper"),
    }
}

/// Complete the Tail created for an explicit Eo block from its Ec control
/// line.  Ec itself is structural syntax and must not remain in the public
/// tree; its arguments retain their original source positions under Tail.
fn complete_explicit_tail(
    builder: &mut DocumentBuilder,
    tail: NodeId,
    closer: NodeId,
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Vec<NodeId> {
    let children = builder
        .children(closer)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    let events = split_mdoc_inline_tokens(
        builder,
        closer,
        &children,
        spacing_enabled,
        max_nodes,
        outcome,
    );
    // An Ec tail owns the delimiter/text prefix only.  A callable macro after
    // that prefix begins normal source-order flow again (for example
    // `.Ec >> "Sy" bold` puts `>>` in Tail and `Sy bold` after the Eo block).
    let split_at = events
        .iter()
        .position(|event| builder.node_macro_name(*event).is_some())
        .unwrap_or(events.len());
    let _ = builder.set_node_location(tail, builder.node_location(closer));
    if let Some(flags) = builder.node_flags(closer) {
        let _ = builder.set_node_flags(tail, flags);
    }
    let _ = builder.replace_children(tail, &events[..split_at]);
    events[split_at..].to_vec()
}

/// Recover an unmatched `.Ec` exactly as mdoc's line-break fallback: the
/// closing control becomes a visible `br` element and its parsed arguments
/// resume ordinary sibling flow.  Other unmatched closers remain validation
/// syntax and do not have this Eo-specific AST fallback.
#[allow(clippy::too_many_arguments)] // Recovery must retain root attachment, flow parent, and bounded splitter state.
fn recover_unmatched_ec(
    builder: &mut DocumentBuilder,
    root: NodeId,
    root_children: &mut Vec<NodeId>,
    parent: NodeId,
    node: NodeId,
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) {
    let children = builder
        .children(node)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    let siblings = split_mdoc_inline_tokens(
        builder,
        node,
        &children,
        spacing_enabled,
        max_nodes,
        outcome,
    );
    let _ = builder.replace_children(node, &[]);
    let _ = builder.macro_name(node, "br");
    append_to_parent(builder, root, root_children, parent, node);
    for sibling in siblings {
        append_to_parent(builder, root, root_children, parent, sibling);
    }
}

fn append_to_parent(
    builder: &mut DocumentBuilder,
    root: NodeId,
    root_children: &mut Vec<NodeId>,
    parent: NodeId,
    node: NodeId,
) {
    if parent == root {
        root_children.push(node);
    } else {
        let _ = builder.append_existing_child(parent, node);
    }
}

/// Remove a preceding paragraph-layout control when the next block's
/// validator declares it redundant.  The root's children are still staged in
/// a local vector, while nested parents already own provisional arena edges.
fn discard_previous_paragraph_control(
    builder: &mut DocumentBuilder,
    root: NodeId,
    root_children: &mut Vec<NodeId>,
    parent: NodeId,
) -> Option<NodeId> {
    let previous = if parent == root {
        root_children.last().copied()
    } else {
        builder
            .children(parent)
            .and_then(|children| children.last().copied())
    }?;
    if !matches!(builder.node_macro_name(previous), Some("Pp" | "br")) {
        return None;
    }
    if parent == root {
        root_children.pop();
    } else {
        let children = builder.children(parent)?.to_vec();
        let (last, retained) = children.split_last()?;
        debug_assert_eq!(*last, previous);
        let _ = builder.replace_children(parent, retained);
    }
    Some(previous)
}

/// Materialize the closer-owned Body node that mdoc leaves inside an explicit
/// partial scope when a full block is closed through that scope.  The surviving
/// partial frame retains the surrounding flow until its authored closer.
fn append_broken_full_block_body(
    builder: &mut DocumentBuilder,
    active_body: NodeId,
    close: &str,
    frame: ScopeFrame,
    closer: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Option<NodeId> {
    if builder.node_count() >= max_nodes {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = builder.node_location(closer);
        }
        return None;
    }
    let Some(body) = builder.push(active_body, NodeKind::Body) else {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = builder.node_location(closer);
        }
        return None;
    };
    let _ = builder.macro_name(body, open_name(close));
    let _ = builder.copy_node_layout(frame.body, body);
    let _ = builder.set_node_flags(body, builder.node_flags(closer).unwrap_or_default());
    if let Some(location) = builder.node_location(closer) {
        let _ = builder.location(body, location);
    }
    Some(body)
}

/// Collect the nearest-to-farthest implicit partial blocks that contain an
/// explicit partial opener.  Their source request ends before the explicit
/// scope does, so a later physical closer leaves closer-owned empty Bodies in
/// the explicit Body and reports each interrupted implicit block.
fn implicit_partial_ancestor_blocks(builder: &DocumentBuilder, node: NodeId) -> Vec<NodeId> {
    let mut blocks = Vec::new();
    let mut cursor = builder.node_parent(node);
    while let Some(parent) = cursor {
        if builder.node_kind(parent) == Some(NodeKind::Body)
            && let Some(name) = builder.node_macro_name(parent)
            && is_implicit_partial_block_macro(name)
            && let Some(block) = builder.node_parent(parent)
            && builder.node_kind(block) == Some(NodeKind::Block)
            && builder.node_macro_name(block) == Some(name)
        {
            blocks.push(block);
        }
        cursor = builder.node_parent(parent);
    }
    blocks
}

/// Insert the public empty Body retained by a crossed implicit partial block.
/// Unlike a full scope there is no close-token-to-name mapping: the block
/// itself supplies the observable macro identity and source location.
fn append_broken_implicit_block_body(
    builder: &mut DocumentBuilder,
    active_body: NodeId,
    block: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Option<NodeId> {
    if builder.node_count() >= max_nodes {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = builder.node_location(block);
        }
        return None;
    }
    let name = builder.node_macro_name(block)?.to_owned();
    let location = builder.node_location(block);
    let body = builder.push(active_body, NodeKind::Body)?;
    if !builder.macro_name(body, name.as_str())
        || !builder.set_node_location(body, location)
        || !builder.set_node_flags(body, NodeFlags::default())
    {
        return None;
    }
    Some(body)
}

/// Detach the physical-line text that arrived in an explicit partial Body
/// before its later closer.  A crossed implicit ancestor inserts its empty
/// Bodies before this continuation, and the continuation is no longer a new
/// public flow event at that point.
fn take_trailing_line_start_text_children(
    builder: &mut DocumentBuilder,
    parent: NodeId,
) -> Vec<NodeId> {
    let Some(children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
        return Vec::new();
    };
    let split = children
        .iter()
        .rposition(|child| {
            builder.node_kind(*child) != Some(NodeKind::Text)
                || builder
                    .node_flags(*child)
                    .is_none_or(|flags| !flags.line_start)
        })
        .map_or(0, |index| index + 1);
    if split == children.len() {
        return Vec::new();
    }
    let trailing = children[split..].to_vec();
    let _ = builder.replace_children(parent, &children[..split]);
    trailing
}

/// Return the first root child that is neither retained prologue metadata nor
/// a comment.  mdoc validates only this one node when checking that a manual
/// begins with a section header.
fn first_mdoc_content_node(builder: &DocumentBuilder, root_children: &[NodeId]) -> Option<NodeId> {
    root_children.iter().copied().find(|node| {
        builder.node_kind(*node) != Some(NodeKind::Comment)
            && !matches!(builder.node_macro_name(*node), Some("Dd" | "Dt" | "Os"))
    })
}

/// Finalize `blk_part_imp()`'s trailing `.Ns` rule after every structural pass
/// has established ownership. A final no-space Element leaves an implicit
/// block Body and becomes a direct block sibling before any closing tail.
fn normalize_trailing_no_space_in_implicit_blocks(builder: &mut DocumentBuilder, root: NodeId) {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        let children = builder
            .children(node)
            .map(<[NodeId]>::to_vec)
            .unwrap_or_default();
        pending.extend(children.iter().rev().copied());

        if builder.node_kind(node) != Some(NodeKind::Block)
            || !builder
                .node_macro_name(node)
                .is_some_and(is_implicit_partial_block_macro)
        {
            continue;
        }
        let Some((_, body)) = children.iter().copied().enumerate().find(|(_, child)| {
            builder.node_kind(*child) == Some(NodeKind::Body)
                && builder.node_macro_name(*child) == builder.node_macro_name(node)
        }) else {
            continue;
        };
        let Some(mut body_children) = builder.children(body).map(<[NodeId]>::to_vec) else {
            continue;
        };
        let Some(last) = body_children.last().copied() else {
            continue;
        };
        if builder.node_macro_name(last) != Some("Ns") {
            continue;
        }

        let Some(parent) = builder.node_parent(node) else {
            continue;
        };
        let Some(mut parent_children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
            continue;
        };
        let Some(block_index) = parent_children.iter().position(|child| *child == node) else {
            continue;
        };
        body_children.pop();
        let _ = builder.replace_children(body, &body_children);
        parent_children.insert(block_index + 1, last);
        let _ = builder.replace_children(parent, &parent_children);
    }
}

/// Mirror `post_bl_block()` for the paragraph controls at the tail of list
/// items.  A non-final item in a non-compact, non-column list drops a trailing
/// `Pp`/`br` before the next item.  A final item's trailing control is instead
/// relinked directly after the completed list, where ordinary sibling
/// validation can subsequently compare it with following paragraph flow.
fn normalize_list_trailing_paragraph_controls(
    builder: &mut DocumentBuilder,
    root: NodeId,
    recoveries: &mut Vec<Recovery>,
) {
    let mut pending = vec![(root, false)];
    while let Some((node, visited)) = pending.pop() {
        if !visited {
            pending.push((node, true));
            if let Some(children) = builder.children(node) {
                pending.extend(children.iter().rev().copied().map(|child| (child, false)));
            }
            continue;
        }
        if builder.node_kind(node) != Some(NodeKind::Block)
            || builder.node_macro_name(node) != Some("Bl")
        {
            continue;
        }
        let Some(body) = builder.children(node).and_then(|children| {
            children.iter().copied().find(|child| {
                builder.node_kind(*child) == Some(NodeKind::Body)
                    && builder.node_macro_name(*child) == Some("Bl")
            })
        }) else {
            continue;
        };
        let list_children = builder
            .children(body)
            .map(<[NodeId]>::to_vec)
            .unwrap_or_default();
        let compact = builder.node_compact(body).unwrap_or(false);
        let column = builder.node_list_kind(body) == Some(NormalizedListKind::Column);
        let mut moved = Vec::new();

        for (item_index, item) in list_children.iter().copied().enumerate() {
            if builder.node_kind(item) != Some(NodeKind::Block)
                || builder.node_macro_name(item) != Some("It")
            {
                continue;
            }
            let Some(item_body) = builder.children(item).and_then(|children| {
                children.iter().copied().find(|child| {
                    builder.node_kind(*child) == Some(NodeKind::Body)
                        && builder.node_macro_name(*child) == Some("It")
                })
            }) else {
                continue;
            };
            let final_item = item_index + 1 == list_children.len();
            let mut children = builder
                .children(item_body)
                .map(<[NodeId]>::to_vec)
                .unwrap_or_default();
            while let Some(control) = children
                .last()
                .copied()
                .filter(|control| matches!(builder.node_macro_name(*control), Some("Pp" | "br")))
            {
                let macro_name = match builder.node_macro_name(control) {
                    Some("Pp") => "Pp",
                    Some("br") => "br",
                    _ => unreachable!("the list-tail predicate checked the macro name"),
                };
                if final_item {
                    children.pop();
                    recoveries.push(Recovery::ParagraphMovedOutOfList {
                        macro_name,
                        location: builder.node_location(control),
                    });
                    moved.push(control);
                    continue;
                }
                if compact || column {
                    break;
                }
                children.pop();
                recoveries.push(Recovery::ParagraphBoundary {
                    macro_name,
                    placement: "before",
                    blocker: "It",
                    location: builder.node_location(control),
                });
            }
            let _ = builder.replace_children(item_body, &children);
        }

        if moved.is_empty() {
            continue;
        }
        let Some(parent) = builder.node_parent(node) else {
            continue;
        };
        let Some(mut siblings) = builder.children(parent).map(<[NodeId]>::to_vec) else {
            continue;
        };
        let Some(list_index) = siblings.iter().position(|sibling| *sibling == node) else {
            continue;
        };
        // Controls were popped from item tails in reverse source order.
        // Restore their authored order when placing them after the list.
        moved.reverse();
        siblings.splice((list_index + 1)..=list_index, moved);
        let _ = builder.replace_children(parent, &siblings);
    }
}

/// Stable source-order key for the two paragraph-layout postprocessors.  A
/// stable sort deliberately leaves a list relocation before the generic
/// adjacent-control finding at the same source control, matching mandoc's
/// `post_bl_block()` then roff-validation order.
fn paragraph_layout_recovery_offset(recovery: &Recovery) -> u32 {
    match recovery {
        Recovery::ParagraphBoundary { location, .. }
        | Recovery::ParagraphMovedOutOfList { location, .. } => location
            .as_ref()
            .map_or(u32::MAX, |location| location.start),
        _ => u32::MAX,
    }
}

/// Mirror the roff-level paragraph controls that validate while an mdoc
/// document is being built.  These checks deliberately precede section
/// post-validation: upstream first resolves adjacent `br`/`sp`/`Pp` requests
/// in a completed local body, then lets `post_section()` inspect the resulting
/// first and last child.
///
/// The traversal is iterative and post-order so controls inside list items or
/// display bodies are normalized before their enclosing macro gets a chance
/// to apply its own boundary rule.  Only direct sibling relationships matter;
/// transparent nodes remain ordinary siblings and never manufacture a false
/// paragraph predecessor.
#[allow(clippy::too_many_lines)] // Post-order mdoc control recovery requires one source-order pass.
fn normalize_inline_paragraph_controls(
    builder: &mut DocumentBuilder,
    root: NodeId,
    recoveries: &mut Vec<Recovery>,
) {
    let mut pending = vec![(root, false)];
    while let Some((parent, visited)) = pending.pop() {
        if !visited {
            pending.push((parent, true));
            if let Some(children) = builder.children(parent) {
                pending.extend(children.iter().rev().copied().map(|child| (child, false)));
            }
            continue;
        }

        let mut retained = builder
            .children(parent)
            .map(<[NodeId]>::to_vec)
            .unwrap_or_default();
        let mut index = 0;
        while index < retained.len() {
            let node = retained[index];
            let macro_name = builder.node_macro_name(node);
            let previous_control = preceding_paragraph_control(builder, &retained, index);

            match macro_name {
                Some("br") => {
                    if let Some((previous_index, previous, previous_name @ ("br" | "sp" | "Pp"))) =
                        previous_control
                    {
                        recoveries.push(Recovery::ParagraphBoundary {
                            macro_name: "br",
                            placement: "after",
                            blocker: previous_name,
                            location: builder.node_location(node),
                        });
                        preserve_transparent_tag_after_deleted_current_control(
                            builder,
                            &retained,
                            previous_index,
                            index,
                            previous,
                            previous_name,
                        );
                        retained.remove(index);
                        continue;
                    }
                }
                Some("sp") => match previous_control {
                    Some((previous_index, previous, "br")) => {
                        recoveries.push(Recovery::ParagraphBoundary {
                            macro_name: "br",
                            placement: "before",
                            blocker: "sp",
                            location: builder.node_location(previous),
                        });
                        preserve_transparent_tag_after_deleted_previous_control(
                            builder,
                            &retained,
                            previous_index,
                            index,
                            "sp",
                        );
                        retained.remove(previous_index);
                        index = index.saturating_sub(1);
                        continue;
                    }
                    Some((previous_index, previous, "Pp")) => {
                        recoveries.push(Recovery::ParagraphBoundary {
                            macro_name: "sp",
                            placement: "after",
                            blocker: "Pp",
                            location: builder.node_location(node),
                        });
                        preserve_transparent_tag_after_deleted_current_control(
                            builder,
                            &retained,
                            previous_index,
                            index,
                            previous,
                            "Pp",
                        );
                        retained.remove(index);
                        continue;
                    }
                    _ => {}
                },
                Some("Pp") => {
                    if let Some((previous_index, previous, previous_name @ ("br" | "Pp"))) =
                        previous_control
                    {
                        recoveries.push(Recovery::ParagraphBoundary {
                            macro_name: previous_name,
                            placement: "before",
                            blocker: "Pp",
                            location: builder.node_location(previous),
                        });
                        retained.remove(previous_index);
                        index = index.saturating_sub(1);
                        continue;
                    }
                }
                _ => {}
            }
            index += 1;
        }
        normalize_transparent_layout_tag_destinations(builder, &retained);
        let _ = builder.replace_children(parent, &retained);
    }
}

/// `post_tg()` keeps an explicit tag as its own destination when the local
/// paragraph-control run has no surviving `.Pp` owner.  Tags already hidden
/// by a paragraph owner are left untouched; this only covers the direct
/// `br/sp Tg br/sp` and blank-line forms that roff validation treats as
/// transparent layout separators.
fn normalize_transparent_layout_tag_destinations(
    builder: &mut DocumentBuilder,
    children: &[NodeId],
) {
    for (index, node) in children.iter().copied().enumerate() {
        if builder.node_macro_name(node) != Some("Tg")
            || builder
                .node_flags(node)
                .is_none_or(|flags| flags.no_print || flags.deep_link_target)
            || node_arguments(builder, node)
                .first()
                .is_none_or(String::is_empty)
        {
            continue;
        }
        let has_layout_neighbour = index
            .checked_sub(1)
            .and_then(|previous| children.get(previous))
            .copied()
            .into_iter()
            .chain(children.get(index + 1).copied())
            .any(|neighbour| {
                matches!(builder.node_macro_name(neighbour), Some("br" | "sp" | "Pp"))
            });
        if has_layout_neighbour {
            mark_destination(builder, node);
        }
    }
}

/// Preserve the narrow `post_tg()` destination relation when roff validation
/// removes a control that originally followed one or more transparent tags.
/// A surviving preceding `.Pp` owns the tag and hides the tag syntax; other
/// layout controls are not destination owners, so the tag remains a direct
/// destination without publishing an explicit tag string.
fn preserve_transparent_tag_after_deleted_current_control(
    builder: &mut DocumentBuilder,
    children: &[NodeId],
    previous_index: usize,
    current_index: usize,
    previous: NodeId,
    previous_name: &str,
) {
    let tags = transparent_tag_arguments(builder, children, previous_index, current_index);
    if previous_name == "Pp" {
        for (tag_node, tag) in tags {
            mark_manual_target(builder, previous, &tag);
            mark_no_print(builder, tag_node);
        }
    } else {
        for (tag_node, _) in tags {
            mark_destination(builder, tag_node);
        }
    }
}

/// When `.sp` deletes a preceding `.br`, the following control cannot own a
/// `.Tg` target; retain that destination on the transparent tag instead.
fn preserve_transparent_tag_after_deleted_previous_control(
    builder: &mut DocumentBuilder,
    children: &[NodeId],
    previous_index: usize,
    current_index: usize,
    current_name: &str,
) {
    if current_name != "sp" {
        return;
    }
    for (tag_node, _) in transparent_tag_arguments(builder, children, previous_index, current_index)
    {
        mark_destination(builder, tag_node);
    }
}

/// Return valid explicit `.Tg` spellings strictly between two source siblings.
/// The caller has already established that all intervening siblings are
/// transparent tags, so ordinary text or another macro deliberately ends the
/// search instead of accidentally moving a destination across visible flow.
fn transparent_tag_arguments(
    builder: &DocumentBuilder,
    children: &[NodeId],
    previous_index: usize,
    current_index: usize,
) -> Vec<(NodeId, String)> {
    children[previous_index.saturating_add(1)..current_index]
        .iter()
        .copied()
        .map_while(|node| (builder.node_macro_name(node) == Some("Tg")).then_some(node))
        .filter_map(|node| {
            node_arguments(builder, node)
                .first()
                .cloned()
                .filter(|tag| !tag.is_empty())
                .map(|tag| (node, tag))
        })
        .collect()
}

/// Find the preceding layout control recognized by `roff_node_prev()`.  A
/// manual tag is transparent to this particular source-order query: it owns a
/// destination but must not break `br Tg br` or `Pp Tg sp` validation.
fn preceding_paragraph_control(
    builder: &DocumentBuilder,
    children: &[NodeId],
    index: usize,
) -> Option<(usize, NodeId, &'static str)> {
    for previous_index in (0..index).rev() {
        let previous = children[previous_index];
        match builder.node_macro_name(previous) {
            Some("br") => return Some((previous_index, previous, "br")),
            Some("sp") => return Some((previous_index, previous, "sp")),
            Some("Pp") => return Some((previous_index, previous, "Pp")),
            Some("Tg") => {}
            _ => return None,
        }
    }
    None
}

/// Apply the narrow `post_section()` / `post_prevpar()` paragraph checks that
/// libmandoc runs while post-validating `Sh` and `Ss` trees. This is a
/// post-order walk on the final arena topology: validating a nested section
/// before its parent deliberately preserves the legacy diagnostic order.
fn normalize_section_paragraph_boundaries(
    builder: &mut DocumentBuilder,
    root: NodeId,
    recoveries: &mut Vec<Recovery>,
) {
    let mut pending = vec![(root, false)];
    while let Some((node, visited)) = pending.pop() {
        if !visited {
            pending.push((node, true));
            if let Some(children) = builder.children(node) {
                pending.extend(children.iter().rev().copied().map(|child| (child, false)));
            }
            continue;
        }

        let section = matches!(
            (builder.node_kind(node), builder.node_macro_name(node)),
            (Some(NodeKind::Block | NodeKind::Body), Some("Sh" | "Ss"))
        );
        if !section {
            continue;
        }

        if builder.node_kind(node) == Some(NodeKind::Body) {
            normalize_section_body_paragraph_boundaries(builder, node, recoveries);
        } else {
            normalize_section_preceding_paragraph_boundary(builder, node, recoveries);
        }
    }
}

/// Match `post_section()` on a completed section Body. The initial request
/// accepts `Pp`, `br`, and `sp`, while only `Pp` and `br` are redundant at its
/// trailing edge.
fn normalize_section_body_paragraph_boundaries(
    builder: &mut DocumentBuilder,
    body: NodeId,
    recoveries: &mut Vec<Recovery>,
) {
    let blocker = match builder.node_macro_name(body) {
        Some("Sh") => "Sh",
        Some("Ss") => "Ss",
        _ => return,
    };
    let mut children = builder
        .children(body)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    let original = children.clone();

    if let Some(first) = children.first().copied()
        && let Some(macro_name) = paragraph_control_name(builder, first, true)
        // A paragraph control can be syntactically redundant at a section
        // boundary yet still own an explicit or automatic destination.  In
        // that case libmandoc retains it as the tag anchor; dropping it would
        // silently discard the destination transferred from Tg/Fn/Fo/Em/Sy.
        && !builder
            .node_flags(first)
            .is_some_and(|flags| flags.deep_link_target || flags.permalink)
    {
        children.remove(0);
        recoveries.push(Recovery::ParagraphBoundary {
            macro_name,
            placement: "after",
            blocker,
            location: builder.node_location(first),
        });
    }
    if let Some(last) = children.last().copied()
        && let Some(macro_name) = paragraph_control_name(builder, last, false)
    {
        children.pop();
        recoveries.push(Recovery::ParagraphBoundary {
            macro_name,
            placement: "at the end of",
            blocker,
            location: builder.node_location(last),
        });
    }
    if children != original {
        let _ = builder.replace_children(body, &children);
    }
}

/// Match `post_prevpar()` when a completed section Block has a direct
/// preceding `Pp` or `br` sibling.
fn normalize_section_preceding_paragraph_boundary(
    builder: &mut DocumentBuilder,
    block: NodeId,
    recoveries: &mut Vec<Recovery>,
) {
    let Some(parent) = builder.node_parent(block) else {
        return;
    };
    let Some(children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
        return;
    };
    let Some(index) = children.iter().position(|child| *child == block) else {
        return;
    };
    let Some(previous) = index
        .checked_sub(1)
        .and_then(|index| children.get(index))
        .copied()
    else {
        return;
    };
    let Some(macro_name) = paragraph_control_name(builder, previous, false) else {
        return;
    };
    let blocker = match builder.node_macro_name(block) {
        Some("Sh") => "Sh",
        Some("Ss") => "Ss",
        _ => return,
    };

    let mut retained = children;
    retained.remove(index - 1);
    let _ = builder.replace_children(parent, &retained);
    recoveries.push(Recovery::ParagraphBoundary {
        macro_name,
        placement: "before",
        blocker,
        location: builder.node_location(previous),
    });
}

/// Return an mdoc paragraph-layout control accepted at a section boundary.
/// The `sp` request is accepted only immediately after a section starts.
fn paragraph_control_name(
    builder: &DocumentBuilder,
    node: NodeId,
    allow_space: bool,
) -> Option<&'static str> {
    match builder.node_macro_name(node) {
        Some("Pp") => Some("Pp"),
        Some("br") => Some("br"),
        Some("sp") if allow_space => Some("sp"),
        _ => None,
    }
}

/// Use the upstream visible spelling for a root node with no macro name.
fn node_kind_name(kind: Option<NodeKind>) -> &'static str {
    match kind {
        Some(NodeKind::Text) => "text",
        Some(NodeKind::Table) => "TS",
        Some(NodeKind::Equation) => "EQ",
        Some(NodeKind::Comment) => "comment",
        Some(
            NodeKind::Root | NodeKind::Block | NodeKind::Head | NodeKind::Body | NodeKind::Tail,
        ) => "block",
        Some(NodeKind::Element) | None => "unknown",
    }
}

/// Remove one already-published semantic node before the root topology is
/// frozen. Root and nested parents use their respective in-progress edges.
fn discard_node_from_parent(
    builder: &mut DocumentBuilder,
    root: NodeId,
    root_children: &mut Vec<NodeId>,
    node: NodeId,
) {
    let Some(parent) = builder.node_parent(node) else {
        return;
    };
    if parent == root {
        root_children.retain(|child| *child != node);
    } else if let Some(mut children) = builder.children(parent).map(<[NodeId]>::to_vec) {
        children.retain(|child| *child != node);
        let _ = builder.replace_children(parent, &children);
    }
}

/// Remove an empty full block after its closer validates it away. The parser
/// has not frozen root children yet, so root and nested parents use their
/// respective in-progress edge lists.
fn discard_empty_block(
    builder: &mut DocumentBuilder,
    root: NodeId,
    root_children: &mut Vec<NodeId>,
    parent: NodeId,
    block: NodeId,
) {
    if parent == root {
        root_children.retain(|child| *child != block);
        return;
    }
    let Some(children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
        return;
    };
    let retained = children
        .into_iter()
        .filter(|child| *child != block)
        .collect::<Vec<_>>();
    let _ = builder.replace_children(parent, &retained);
}

fn argument_location(builder: &DocumentBuilder, node: NodeId, index: usize) -> Option<SourceSpan> {
    builder
        .children(node)
        .and_then(|children| children.get(index))
        .and_then(|argument| builder.node_location(*argument))
}

/// Split a callable mdoc macro's scanner tokens into the source-order inline
/// macro events that mandoc's `in_line()` parser constructs.  The scanner
/// already owns one `Text` arena record per lexical token, so macro-name
/// tokens can be reclassified in place: no new AST node allocation is needed
/// and every argument keeps its original source location.
#[allow(clippy::too_many_lines)] // Mirrors mdoc's ordered in_line state transitions without hiding macro-boundary cases.
fn split_inline_macro_events(
    builder: &mut DocumentBuilder,
    node: NodeId,
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Vec<NodeId> {
    let Some(name) = builder.node_macro_name(node) else {
        return vec![node];
    };
    if !is_inline_mdoc_macro(name) && name != "Vt" {
        return vec![node];
    }
    let Some(tokens) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return vec![node];
    };

    let mut events = vec![node];
    let mut remaining_arguments = mdoc_inline_argument_limit(name);
    let mut current = (remaining_arguments != Some(0)).then_some(node);
    let mut current_children = Vec::new();
    let mut resume_after_delimiter = None::<NodeId>;
    let mut pending_trailing_opening_delimiters = Vec::<NodeId>::new();
    let mut pending_nested_leading_delimiter_sentence_end = None::<NodeId>;
    let mut reopened_after_middle_delimiter = false;
    // A top-level zero-argument request arrives from the scanner with its
    // following lexical tokens provisionally attached below the control
    // node.  They are source-order siblings, not macro arguments (`.Ux .`
    // is the visible regression); detach them before emitting the event
    // stream.  Reclassified inline tokens already have no children.
    if remaining_arguments == Some(0) {
        let _ = builder.replace_children(node, &[]);
    }
    for (token_index, token) in tokens.iter().copied().enumerate() {
        // A nested source macro can publish a leading closing delimiter that
        // ends a sentence only when it is the final token of its physical
        // request. Any next token resumes or supersedes that private macro
        // state and therefore clears the pending sentence boundary.
        pending_nested_leading_delimiter_sentence_end = None;
        if let Some(current_node) = current
            && let Some(current_name) = builder.node_macro_name(current_node)
            && let Some(close) = explicit_partial_block_close(current_name)
        {
            // A callable explicit partial block owns its raw source stream
            // through its paired closer. Its later structural pass then
            // parses that body, including nested inline macros, as a unit.
            current_children.push(token);
            if builder.node_text(token) == Some(close) {
                finish_inline_element(builder, current_node, &current_children, spacing_enabled);
                current = None;
                remaining_arguments = None;
                current_children.clear();
            }
            continue;
        }
        if let Some(current_node) = current
            && builder
                .node_macro_name(current_node)
                .is_some_and(is_implicit_partial_block_macro)
        {
            // An implicit partial block extends to the rest of its source
            // line. Nested macros are parsed when its Body is constructed,
            // matching the upstream block parser's first-call handoff.
            current_children.push(token);
            continue;
        }
        if let Some(current_node) = current
            && matches!(builder.node_macro_name(current_node), Some("Ar" | "Pa"))
            && builder
                .node_text(token)
                .is_some_and(|text| matches!(text, "(" | "["))
        {
            // `in_line()` publishes an opening delimiter between consecutive
            // Ar and Pa elements. A leading delimiter simply moves before the
            // first element; a later one ends the current element and starts
            // the next.
            mark_opening_delimiter(builder, token, Some("("));
            if current_children.is_empty() {
                events.retain(|event| *event != current_node);
                events.push(token);
                if let Some(mut flags) = builder.node_flags(current_node) {
                    flags.line_start = false;
                    let _ = builder.set_node_flags(current_node, flags);
                }
                if let Some(mut flags) = builder.node_flags(token) {
                    flags.line_start = true;
                    let _ = builder.set_node_flags(token, flags);
                }
                events.push(current_node);
                continue;
            }
            finish_inline_element(builder, current_node, &current_children, spacing_enabled);
            events.push(token);
            let Some(reopened) =
                reopen_inline_element(builder, node, current_node, max_nodes, outcome)
            else {
                current = None;
                remaining_arguments = None;
                current_children.clear();
                continue;
            };
            events.push(reopened);
            current = Some(reopened);
            remaining_arguments = None;
            current_children.clear();
            continue;
        }
        if let Some(current_node) = current
            && builder.node_macro_name(current_node) == Some("Nm")
            && current_children.is_empty()
            && builder
                .node_text(token)
                .is_some_and(|text| matches!(text, "(" | "["))
        {
            // Nm begins a leading-delimiter form by publishing the delimiter
            // outside a temporary empty Element, then reuses that Element for
            // the following literal name.
            let delimiter = builder.node_text(token).map(str::to_owned);
            mark_opening_delimiter(builder, token, delimiter.as_deref());
            events.retain(|event| *event != current_node);
            transfer_line_start(builder, current_node, token);
            if let Some(mut flags) = builder.node_flags(current_node) {
                flags.line_start = false;
                let _ = builder.set_node_flags(current_node, flags);
            }
            events.push(token);
            events.push(current_node);
            continue;
        }
        if let Some(current_node) = current
            && builder.node_macro_name(current_node) == Some("Xr")
            && current_children.is_empty()
            && builder
                .node_text(token)
                .is_some_and(|text| matches!(text, "(" | "["))
        {
            // `in_line_argn()` publishes a leading delimiter before the
            // fixed two-argument Xr element. It owns the request's
            // line-start provenance, leaving the reference itself inline.
            let delimiter = builder.node_text(token).map(str::to_owned);
            mark_opening_delimiter(builder, token, delimiter.as_deref());
            events.retain(|event| *event != current_node);
            transfer_line_start(builder, current_node, token);
            if let Some(mut flags) = builder.node_flags(current_node) {
                flags.line_start = false;
                let _ = builder.set_node_flags(current_node, flags);
            }
            events.push(token);
            events.push(current_node);
            continue;
        }
        if let Some(current_node) = current
            && let Some(remaining) = remaining_arguments
            && remaining > 0
            && (builder.node_macro_name(current_node) == Some("Pf")
                || !builder
                    .node_text(token)
                    .is_some_and(is_mdoc_closing_delimiter))
            && !(builder.node_macro_name(current_node) == Some("St")
                && builder.node_text(token).is_some_and(is_mdoc_callable_macro))
        {
            // A finite-argument macro owns its next token literally, except
            // St's callable first argument: `.St Fl called` is an empty St
            // followed by Fl, matching `in_line_argn()`. Pf also owns a
            // leading closing delimiter literally: it is its prefix, rather
            // than outer punctuation.
            let terminal_pf_prefix = builder.node_macro_name(current_node) == Some("Pf")
                && builder
                    .node_text(token)
                    .is_some_and(is_mdoc_closing_delimiter)
                && tokens[token_index + 1..].is_empty();
            current_children.push(token);
            if remaining == 1 {
                finish_inline_element(builder, current_node, &current_children, spacing_enabled);
                if terminal_pf_prefix {
                    mark_sentence_end(builder, token);
                }
                current = None;
                remaining_arguments = None;
                current_children.clear();
            } else {
                remaining_arguments = Some(remaining - 1);
            }
            continue;
        }
        let token_text = builder.node_text(token).map(str::to_owned);
        if let Some(current_node) = current
            && builder.node_macro_name(current_node) == Some("Lk")
            && !token_text.as_deref().is_some_and(is_mdoc_callable_macro)
        {
            // `in_line()` keeps a link scope open through ordinary source
            // words and punctuation. Only a following callable macro ends
            // the link and starts independent inline flow.
            current_children.push(token);
            continue;
        }
        if let Some(current_node) = current
            && is_tag_style_delimiter_restart_macro(builder.node_macro_name(current_node))
            && token_text
                .as_deref()
                .is_some_and(|text| matches!(text, "(" | "["))
        {
            // `in_line_argn()` keeps opening punctuation outside these
            // tag-style macros,
            // then resumes the macro with the next ordinary word. This also
            // applies when the opening punctuation is the first token: the
            // empty source element stays private so structural validation can
            // retain its empty-macro diagnostic if there is no later word.
            let starts_tag_macro = current_children.is_empty();
            finish_inline_element(builder, current_node, &current_children, spacing_enabled);
            mark_opening_delimiter(builder, token, token_text.as_deref());
            if starts_tag_macro {
                transfer_line_start(builder, current_node, token);
                if builder
                    .node_source_position(current_node)
                    .is_some_and(|position| position.column > 2)
                {
                    // A nested source-spelled tag macro may consist solely
                    // of an isolated delimiter. It is kept as an opening
                    // delimiter only if a subsequent callable macro proves
                    // that the flow actually continues.
                    pending_trailing_opening_delimiters.push(token);
                }
            } else {
                // Whether this is an opening delimiter depends on whether
                // later input actually resumes the source macro. Defer the
                // public flag until the complete request has been consumed.
                pending_trailing_opening_delimiters.push(token);
            }
            events.push(token);
            current = None;
            remaining_arguments = None;
            current_children.clear();
            resume_after_delimiter = Some(current_node);
            reopened_after_middle_delimiter = false;
            continue;
        }
        if let Some(current_node) = current
            && is_tag_style_delimiter_restart_macro(builder.node_macro_name(current_node))
            && current_children.is_empty()
            && token_text.as_deref().is_some_and(is_mdoc_closing_delimiter)
        {
            // A leading closing delimiter is not tag-style macro content.
            // libmandoc publishes it as the line's first literal node without
            // a delimiter-close flag, then lets a later word reopen the same
            // source request. A nested source-spelled tag macro at physical
            // end keeps the punctuation's sentence boundary, but a later
            // token clears it before this splitter returns.
            let nested_source_macro = builder
                .node_source_position(current_node)
                .is_some_and(|position| position.column > 2);
            finish_inline_element(builder, current_node, &[], spacing_enabled);
            transfer_line_start(builder, current_node, token);
            if let Some(mut flags) = builder.node_flags(token) {
                flags.delimiter_close = false;
                flags.sentence_end = false;
                let _ = builder.set_node_flags(token, flags);
            }
            if nested_source_macro {
                pending_nested_leading_delimiter_sentence_end = Some(token);
            }
            events.push(token);
            current = None;
            remaining_arguments = None;
            current_children.clear();
            resume_after_delimiter = Some(current_node);
            reopened_after_middle_delimiter = false;
            continue;
        }
        let inline_name = builder
            .node_text(token)
            .filter(|text| is_mdoc_callable_macro(text))
            .map(str::to_owned);
        if let Some(inline_name) = inline_name {
            let st_source = current.filter(|current_node| {
                builder.node_macro_name(*current_node) == Some("St") && current_children.is_empty()
            });
            resume_after_delimiter = None;
            pending_trailing_opening_delimiters.clear();
            if let Some(current) = current {
                if reopened_after_middle_delimiter && current_children.is_empty() {
                    // A middle delimiter only reopens the preceding macro
                    // for following ordinary words.  A callable macro takes
                    // over the stream directly, so the temporary empty
                    // element is not public (`Ar word | Fl flag`).
                    events.retain(|event| *event != current);
                } else {
                    finish_inline_element(builder, current, &current_children, spacing_enabled);
                }
            }
            let remaining = mdoc_inline_argument_limit(&inline_name);
            if !builder.clear_node_text(token)
                || !builder.set_node_kind(token, NodeKind::Element)
                || !builder.macro_name(token, inline_name)
            {
                return events;
            }
            if let Some(source) = st_source {
                transfer_line_start(builder, source, token);
            }
            events.push(token);
            current = (remaining != Some(0)).then_some(token);
            remaining_arguments = remaining;
            current_children.clear();
            reopened_after_middle_delimiter = false;
        } else if token_text.as_deref().is_some_and(is_mdoc_middle_delimiter) {
            let Some(current_node) = current else {
                events.push(token);
                continue;
            };
            let leading_tag_macro_delimiter =
                is_tag_style_delimiter_restart_macro(builder.node_macro_name(current_node))
                    && current_children.is_empty();
            if is_empty_middle_delimiter_element(builder, current_node, &current_children) {
                events.retain(|event| *event != current_node);
                if let Some(mut flags) = builder.node_flags(token) {
                    // The discarded source macro owns no public node, so its
                    // middle delimiter becomes this input line's first event.
                    flags.line_start = true;
                    let _ = builder.set_node_flags(token, flags);
                }
            } else {
                finish_inline_element(builder, current_node, &current_children, spacing_enabled);
            }
            if leading_tag_macro_delimiter {
                // Leading middle delimiters in tag-style inline macros follow
                // the same private-element rule as leading opening and
                // closing delimiters.
                transfer_line_start(builder, current_node, token);
            }
            events.push(token);
            let Some(reopened) =
                reopen_inline_element(builder, node, current_node, max_nodes, outcome)
            else {
                current = None;
                remaining_arguments = None;
                current_children.clear();
                continue;
            };
            events.push(reopened);
            current = Some(reopened);
            remaining_arguments = None;
            current_children.clear();
            reopened_after_middle_delimiter = true;
        } else if token_text.as_deref().is_some_and(is_mdoc_closing_delimiter) {
            let resume = current.filter(|current| {
                builder.node_macro_name(*current) != Some("Fn")
                    && !(builder.node_macro_name(*current) == Some("Nm")
                        && current_children.is_empty())
            });
            if let Some(current) = current {
                finish_inline_element(builder, current, &current_children, spacing_enabled);
            }
            if let Some(mut flags) = builder.node_flags(token) {
                flags.delimiter_close = true;
                let _ = builder.set_node_flags(token, flags);
            }
            mark_sentence_end(builder, token);
            events.push(token);
            current = None;
            remaining_arguments = None;
            current_children.clear();
            resume_after_delimiter = resume;
            reopened_after_middle_delimiter = false;
        } else if current.is_some() {
            current_children.push(token);
            reopened_after_middle_delimiter = false;
        } else if let Some(source) = resume_after_delimiter
            && is_tag_style_delimiter_restart_macro(builder.node_macro_name(source))
            && token_text
                .as_deref()
                .is_some_and(|text| matches!(text, "(" | "["))
        {
            // A chain of opening delimiters stays outside the private empty
            // restart element (`Em a Em ( [ Em b`).  Do not reopen until a
            // real word arrives; a following callable macro consumes the
            // pending restart without exposing it at all.
            mark_opening_delimiter(builder, token, token_text.as_deref());
            events.push(token);
            if !pending_trailing_opening_delimiters.is_empty() {
                pending_trailing_opening_delimiters.push(token);
            }
        } else if let Some(source) = resume_after_delimiter.take() {
            let Some(reopened) = reopen_inline_element(builder, node, source, max_nodes, outcome)
            else {
                events.push(token);
                continue;
            };
            events.push(reopened);
            current = Some(reopened);
            current_children.push(token);
            pending_trailing_opening_delimiters.clear();
            reopened_after_middle_delimiter = false;
        } else {
            mark_opening_delimiter(builder, token, token_text.as_deref());
            events.push(token);
        }
    }
    if let Some(current) = current {
        if reopened_after_middle_delimiter && current_children.is_empty() {
            // A middle delimiter opens a provisional continuation only for a
            // following word. At physical end of line that continuation is
            // not public; retaining it would turn `.Em a Em |` into a second
            // spurious empty Em recovery.
            events.retain(|event| *event != current);
        } else {
            finish_inline_element(builder, current, &current_children, spacing_enabled);
        }
    }
    if let Some(delimiter) = pending_nested_leading_delimiter_sentence_end {
        mark_sentence_end(builder, delimiter);
    }
    for delimiter in pending_trailing_opening_delimiters {
        if let Some(mut flags) = builder.node_flags(delimiter) {
            flags.delimiter_open = false;
            let _ = builder.set_node_flags(delimiter, flags);
        }
    }
    clear_nonterminal_inline_delimiter_flags(builder, &events);
    events
}

/// Split a scanner-tokenized argument sequence directly beneath `parent`.
///
/// `Vt` in a SYNOPSIS section is an implicit partial block: its Body owns the
/// literal prefix and any nested callable macros, rather than the outer `Vt`
/// element owning all scanner tokens or the nested macros escaping as siblings.
fn split_mdoc_inline_children(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Vec<NodeId> {
    let Some(tokens) = builder.children(parent).map(<[NodeId]>::to_vec) else {
        return Vec::new();
    };
    split_mdoc_inline_tokens(
        builder,
        parent,
        &tokens,
        spacing_enabled,
        max_nodes,
        outcome,
    )
}

/// Classify an already-selected run of scanner tokens as direct text, callable
/// elements, and closing delimiters.  It does not attach the returned nodes;
/// callers can place a block body or a post-closer tail explicitly.
#[allow(clippy::too_many_lines)] // Mirrors mdoc's ordered in_line state transitions without hiding macro-boundary cases.
fn split_mdoc_inline_tokens(
    builder: &mut DocumentBuilder,
    allocation_parent: NodeId,
    tokens: &[NodeId],
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Vec<NodeId> {
    split_mdoc_inline_tokens_with_options(
        builder,
        allocation_parent,
        tokens,
        spacing_enabled,
        max_nodes,
        outcome,
        false,
    )
}

/// Split mdoc inline tokens with the one `Bl -column` provenance distinction
/// that is not part of ordinary inline flow.  A system-name spelling at the
/// very start of a tab-created column remains literal text in libmandoc.
#[allow(clippy::too_many_lines)] // Mirrors mdoc's ordered in_line state transitions without hiding macro-boundary cases.
fn split_mdoc_inline_tokens_with_options(
    builder: &mut DocumentBuilder,
    allocation_parent: NodeId,
    tokens: &[NodeId],
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
    suppress_first_tab_column_system_name: bool,
) -> Vec<NodeId> {
    let mut children = Vec::new();
    let mut current = None::<NodeId>;
    let mut remaining_arguments = None::<usize>;
    let mut current_children = Vec::new();
    let mut resume_after_delimiter = None::<NodeId>;
    let mut reopened_after_middle_delimiter = false;
    for (token_index, &token) in tokens.iter().enumerate() {
        if let Some(current_node) = current
            && matches!(builder.node_macro_name(current_node), Some("Ar" | "Pa"))
            && builder
                .node_text(token)
                .is_some_and(|text| matches!(text, "(" | "["))
        {
            mark_opening_delimiter(builder, token, Some("("));
            if current_children.is_empty() {
                children.push(token);
                if let Some(mut flags) = builder.node_flags(current_node) {
                    flags.line_start = false;
                    let _ = builder.set_node_flags(current_node, flags);
                }
                if let Some(mut flags) = builder.node_flags(token) {
                    flags.line_start = true;
                    let _ = builder.set_node_flags(token, flags);
                }
                children.push(current_node);
                continue;
            }
            finish_inline_element(builder, current_node, &current_children, spacing_enabled);
            children.push(current_node);
            children.push(token);
            let Some(reopened) =
                reopen_inline_element(builder, allocation_parent, current_node, max_nodes, outcome)
            else {
                current = None;
                remaining_arguments = None;
                current_children.clear();
                continue;
            };
            current = Some(reopened);
            remaining_arguments = None;
            current_children.clear();
            continue;
        }
        if let Some(current_node) = current
            && builder
                .node_macro_name(current_node)
                .is_some_and(is_implicit_partial_block_macro)
        {
            // An implicit partial block owns the rest of its parsed argument
            // stream. Defer callable classification until its Body exists,
            // otherwise `.Op Ar argument` escapes as an empty Op followed by
            // a sibling Ar instead of a nested partial block.
            current_children.push(token);
            continue;
        }
        if let Some(current_node) = current
            && builder.node_macro_name(current_node) == Some("Xr")
            && current_children.is_empty()
            && builder
                .node_text(token)
                .is_some_and(|text| matches!(text, "(" | "["))
        {
            let delimiter = builder.node_text(token).map(str::to_owned);
            mark_opening_delimiter(builder, token, delimiter.as_deref());
            transfer_line_start(builder, current_node, token);
            if let Some(mut flags) = builder.node_flags(current_node) {
                flags.line_start = false;
                let _ = builder.set_node_flags(current_node, flags);
            }
            children.push(token);
            children.push(current_node);
            continue;
        }
        if let Some(current_node) = current
            && let Some(remaining) = remaining_arguments
            && remaining > 0
            && (builder.node_macro_name(current_node) == Some("Pf")
                || !builder
                    .node_text(token)
                    .is_some_and(is_mdoc_closing_delimiter))
            && !(builder.node_macro_name(current_node) == Some("St")
                && builder.node_text(token).is_some_and(is_mdoc_callable_macro))
            && !column_system_name_starts_next_element(builder, current_node, token)
        {
            current_children.push(token);
            if remaining == 1 {
                finish_inline_element(builder, current_node, &current_children, spacing_enabled);
                children.push(current_node);
                current = None;
                remaining_arguments = None;
                current_children.clear();
            } else {
                remaining_arguments = Some(remaining - 1);
            }
            continue;
        }
        let token_text = builder.node_text(token).map(str::to_owned);
        if let Some(current_node) = current
            && builder.node_macro_name(current_node) == Some("Lk")
            && !token_text.as_deref().is_some_and(is_mdoc_callable_macro)
        {
            current_children.push(token);
            continue;
        }
        let inline_name = builder
            .node_text(token)
            .filter(|text| {
                is_mdoc_callable_macro(text)
                    && !(suppress_first_tab_column_system_name
                        && token_index == 0
                        && generated_system_name(text).is_some())
            })
            .map(str::to_owned);
        if let Some(inline_name) = inline_name {
            let st_source = current.filter(|current_node| {
                builder.node_macro_name(*current_node) == Some("St") && current_children.is_empty()
            });
            resume_after_delimiter = None;
            if let Some(current) = current
                && !(reopened_after_middle_delimiter && current_children.is_empty())
            {
                finish_inline_element(builder, current, &current_children, spacing_enabled);
                children.push(current);
            }
            let remaining = mdoc_inline_argument_limit(&inline_name);
            if !builder.clear_node_text(token)
                || !builder.set_node_kind(token, NodeKind::Element)
                || !builder.macro_name(token, inline_name)
            {
                children.push(token);
                current = None;
                remaining_arguments = None;
                current_children.clear();
                continue;
            }
            if let Some(source) = st_source {
                transfer_line_start(builder, source, token);
            }
            if builder.node_macro_name(token) == Some("Ns")
                && no_space_macro_requires_warning(builder, token, &tokens[token_index + 1..])
            {
                outcome.recoveries.push(Recovery::NoSpaceMacro {
                    location: builder.node_location(token),
                });
            }
            if remaining == Some(0) {
                children.push(token);
                current = None;
            } else {
                current = Some(token);
            }
            remaining_arguments = remaining;
            current_children.clear();
        } else if token_text.as_deref().is_some_and(is_mdoc_middle_delimiter) {
            let Some(current_node) = current else {
                children.push(token);
                continue;
            };
            if !is_empty_middle_delimiter_element(builder, current_node, &current_children) {
                finish_inline_element(builder, current_node, &current_children, spacing_enabled);
                children.push(current_node);
            } else if let Some(mut flags) = builder.node_flags(token) {
                flags.line_start = true;
                let _ = builder.set_node_flags(token, flags);
            }
            children.push(token);
            let Some(reopened) =
                reopen_inline_element(builder, allocation_parent, current_node, max_nodes, outcome)
            else {
                current = None;
                remaining_arguments = None;
                current_children.clear();
                continue;
            };
            current = Some(reopened);
            remaining_arguments = None;
            current_children.clear();
            reopened_after_middle_delimiter = true;
        } else if token_text.as_deref().is_some_and(is_mdoc_closing_delimiter) {
            let resume = current.filter(|current| builder.node_macro_name(*current) != Some("Fn"));
            if let Some(current) = current {
                finish_inline_element(builder, current, &current_children, spacing_enabled);
                children.push(current);
            }
            if let Some(mut flags) = builder.node_flags(token) {
                flags.delimiter_close = true;
                let _ = builder.set_node_flags(token, flags);
            }
            mark_sentence_end(builder, token);
            children.push(token);
            current = None;
            remaining_arguments = None;
            current_children.clear();
            resume_after_delimiter = resume;
            reopened_after_middle_delimiter = false;
        } else if current.is_some() {
            current_children.push(token);
            reopened_after_middle_delimiter = false;
        } else if let Some(source) = resume_after_delimiter.take() {
            let Some(reopened) =
                reopen_inline_element(builder, allocation_parent, source, max_nodes, outcome)
            else {
                children.push(token);
                continue;
            };
            current = Some(reopened);
            current_children.push(token);
            reopened_after_middle_delimiter = false;
        } else {
            mark_opening_delimiter(builder, token, token_text.as_deref());
            children.push(token);
        }
    }
    if let Some(current) = current {
        finish_inline_element(builder, current, &current_children, spacing_enabled);
        children.push(current);
    }
    clear_nonterminal_inline_delimiter_flags(builder, &children);
    children
}

/// A middle delimiter suppresses an empty default or compatibility element.
/// The delimiter stays in surrounding flow and the following token opens the
/// next element, matching `in_line()`'s empty-first element handling.
fn is_empty_middle_delimiter_element(
    builder: &DocumentBuilder,
    node: NodeId,
    children: &[NodeId],
) -> bool {
    matches!(builder.node_macro_name(node), Some("Ar" | "Nm" | "Pa")) && children.is_empty()
}

/// A delimiter only ends a sentence when it is the final inline event on its
/// source request. Reopened macros after `|` or closing punctuation continue
/// the same input line, so their preceding `.`/`!`/`?` remains nonterminal.
fn clear_nonterminal_inline_delimiter_flags(builder: &mut DocumentBuilder, events: &[NodeId]) {
    for (index, event) in events.iter().copied().enumerate() {
        if events[index + 1..].is_empty()
            || !builder
                .node_text(event)
                .is_some_and(is_mdoc_closing_delimiter)
        {
            continue;
        }
        if let Some(mut flags) = builder.node_flags(event) {
            flags.sentence_end = false;
            let _ = builder.set_node_flags(event, flags);
        }
    }
}

/// Split a physical explicit partial-block invocation at its same-line closer.
/// The closer is structural syntax and therefore does not survive as a public
/// node; the following tokens re-enter the surrounding source-order flow.
fn split_explicit_partial_block_tail(
    builder: &mut DocumentBuilder,
    node: NodeId,
    close: &str,
) -> Vec<NodeId> {
    let Some(tokens) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return Vec::new();
    };
    let Some(close_index) = matching_explicit_partial_close_index(builder, &tokens, close) else {
        return Vec::new();
    };
    let tail = tokens[close_index.saturating_add(1)..].to_vec();
    let _ = builder.replace_children(node, &tokens[..close_index]);
    tail
}

/// Find the closer belonging to an explicit partial opener's current source
/// request.  A physical request may contain nested explicit pairs, so its
/// first syntactic closer is not necessarily the closer for `outer_close`:
/// `.Oo Oo No a Oc Oc` has two distinct `Oc` tokens.
fn matching_explicit_partial_close_index(
    builder: &DocumentBuilder,
    tokens: &[NodeId],
    outer_close: &str,
) -> Option<usize> {
    let mut expected_closes = vec![outer_close];
    for (index, token) in tokens.iter().copied().enumerate() {
        let Some(text) = builder.node_text(token) else {
            continue;
        };
        if let Some(close) = explicit_partial_block_close(text) {
            expected_closes.push(close);
        } else if expected_closes
            .last()
            .is_some_and(|expected| *expected == text)
        {
            expected_closes.pop();
            if expected_closes.is_empty() {
                return Some(index);
            }
        }
    }
    None
}

/// Turn every complete explicit partial pair nested directly below `parent`
/// into its public Block/Head/Body projection before the inline splitter sees
/// it.  The splitter otherwise classifies a nested `.Oo` as an ordinary
/// element and turns its `Oc` tokens into visible prose.
fn structure_matched_explicit_partial_blocks(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) {
    let Some(children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
        return;
    };
    let events = structure_matched_explicit_partial_events(
        builder,
        &children,
        spacing_enabled,
        max_nodes,
        outcome,
    );
    let _ = builder.replace_children(parent, &events);
}

/// The event-level form of `structure_matched_explicit_partial_blocks` is
/// also used for a same-line tail, which has no single public parent until it
/// is attached after any outer scope close has been processed.
fn structure_matched_explicit_partial_events(
    builder: &mut DocumentBuilder,
    children: &[NodeId],
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Vec<NodeId> {
    let mut events = Vec::with_capacity(children.len());
    let mut cursor = 0;
    while cursor < children.len() {
        let opener = children[cursor];
        let Some(name) = builder.node_text(opener).map(str::to_owned) else {
            events.push(opener);
            cursor += 1;
            continue;
        };
        let Some(close) = explicit_partial_block_close(&name) else {
            events.push(opener);
            cursor += 1;
            continue;
        };
        let Some(relative_close) =
            matching_explicit_partial_close_index(builder, &children[cursor + 1..], close)
        else {
            events.push(opener);
            cursor += 1;
            continue;
        };
        let close_index = cursor + relative_close + 1;
        let nested_tokens = children[cursor + 1..close_index].to_vec();
        let inherits_synopsis = builder
            .node_flags(opener)
            .is_some_and(|flags| flags.synopsis_pretty);
        if !builder.clear_node_text(opener)
            || !builder.set_node_kind(opener, NodeKind::Element)
            || !builder.macro_name(opener, name.as_str())
        {
            events.push(opener);
            cursor += 1;
            continue;
        }
        let Some((head, body)) = make_block(
            builder,
            opener,
            &name,
            ArgumentPlacement::BodyTokens,
            max_nodes,
            outcome,
        ) else {
            events.push(opener);
            cursor += 1;
            continue;
        };
        let _ = builder.replace_children(body, &nested_tokens);
        if inherits_synopsis {
            mark_synopsis_pretty(builder, head);
            mark_synopsis_pretty(builder, body);
        }
        let nested_events = structure_matched_explicit_partial_events(
            builder,
            &nested_tokens,
            spacing_enabled,
            max_nodes,
            outcome,
        );
        let _ = builder.replace_children(body, &nested_events);
        let nested_children =
            split_mdoc_inline_children(builder, body, spacing_enabled, max_nodes, outcome);
        let _ = builder.replace_children(body, &nested_children);
        clear_leading_explicit_partial_punctuation(builder, body);
        move_explicit_leading_open_delimiter(builder, opener, head, body);
        if matches!(name.as_str(), "Bo" | "Po") {
            coalesce_adjacent_text_children(builder, body);
        }
        events.push(opener);
        cursor = close_index + 1;
    }
    events
}

/// Project a retained source-line tail after its enclosing explicit opener
/// has closed.  The caller determines any global explicit closers first;
/// each resulting segment can then safely form nested explicit and implicit
/// blocks without leaking close syntax into the public tree.
fn explicit_partial_tail_events(
    builder: &mut DocumentBuilder,
    allocation_parent: NodeId,
    tokens: &[NodeId],
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Vec<NodeId> {
    let events = structure_matched_explicit_partial_events(
        builder,
        tokens,
        spacing_enabled,
        max_nodes,
        outcome,
    );
    let events = split_mdoc_inline_tokens(
        builder,
        allocation_parent,
        &events,
        spacing_enabled,
        max_nodes,
        outcome,
    );
    for event in &events {
        structure_implicit_partial_block(builder, *event, max_nodes, outcome, spacing_enabled);
    }
    events
}

/// Attach the source-order tail of an explicit partial opener.  Local explicit
/// pairs are projected as blocks within a segment; a closer not owned by such
/// a pair instead restores the preceding cross-line scope before the next
/// segment is attached.
#[allow(clippy::too_many_arguments)] // This is the source-order scope hand-off itself.
fn append_explicit_partial_tail(
    builder: &mut DocumentBuilder,
    root: NodeId,
    root_children: &mut Vec<NodeId>,
    scopes: &mut Vec<ScopeFrame>,
    implicitly_closed: &mut Vec<&'static str>,
    active_body: &mut NodeId,
    flow_parent: &mut NodeId,
    allocation_parent: NodeId,
    tail: &[NodeId],
    mark_tail_line_start: bool,
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) {
    let mut segment_start = 0;
    let mut local_closes = Vec::new();
    let mut mark_next_tail_segment = mark_tail_line_start;
    for (index, token) in tail.iter().copied().enumerate() {
        let Some(text) = builder.node_text(token).map(str::to_owned) else {
            continue;
        };
        if let Some(local_close) = explicit_partial_block_close(&text) {
            local_closes.push(local_close);
            continue;
        }
        if local_closes
            .last()
            .is_some_and(|local_close| *local_close == text)
        {
            local_closes.pop();
            continue;
        }
        if !local_closes.is_empty() || !is_explicit_partial_close(&text) {
            continue;
        }
        let events = explicit_partial_tail_events(
            builder,
            allocation_parent,
            &tail[segment_start..index],
            spacing_enabled,
            max_nodes,
            outcome,
        );
        if mark_next_tail_segment && !events.is_empty() {
            mark_explicit_partial_close_tail_line_start(builder, &events);
            mark_next_tail_segment = false;
        }
        for sibling in events {
            append_to_parent(builder, root, root_children, *active_body, sibling);
        }
        close_explicit_partial_scope(scopes, implicitly_closed, active_body, flow_parent, &text);
        segment_start = index + 1;
    }
    let events = explicit_partial_tail_events(
        builder,
        allocation_parent,
        &tail[segment_start..],
        spacing_enabled,
        max_nodes,
        outcome,
    );
    if mark_next_tail_segment && !events.is_empty() {
        mark_explicit_partial_close_tail_line_start(builder, &events);
    }
    let has_cross_line_explicit_opener = events.iter().any(|event| {
        builder.node_kind(*event) == Some(NodeKind::Element)
            && builder
                .node_macro_name(*event)
                .is_some_and(|name| explicit_partial_block_close(name).is_some())
    });
    for sibling in events {
        append_to_parent(builder, root, root_children, *active_body, sibling);
    }
    if has_cross_line_explicit_opener {
        let tail_scopes = structure_unclosed_explicit_partial_blocks(
            builder,
            *active_body,
            spacing_enabled,
            max_nodes,
            outcome,
        );
        for scope in tail_scopes {
            // The remainder of a closer request is one mdoc phrase even
            // when it opens a new cross-line partial (`.Bc Po po pc`).
            // The generic nested-opener path preserves lexical tokens for
            // other contexts, so apply this tightening only to tail scopes.
            coalesce_adjacent_text_children(builder, scope.body);
            *active_body = scope.body;
            *flow_parent = scope.body;
            scopes.push(scope);
        }
    }
}

/// Convert unclosed explicit partial openers on this source request into a
/// nested scope stack.  Complete pairs were removed first by
/// `structure_matched_explicit_partial_blocks`; the first remaining opener
/// owns the request suffix and resumes its parent only when a later physical
/// closer arrives.
fn structure_unclosed_explicit_partial_blocks(
    builder: &mut DocumentBuilder,
    outer_body: NodeId,
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Vec<ScopeFrame> {
    let Some(children) = builder.children(outer_body).map(<[NodeId]>::to_vec) else {
        return Vec::new();
    };
    let Some(opener_index) = children.iter().position(|node| {
        builder
            .node_text(*node)
            .is_some_and(|name| explicit_partial_block_close(name).is_some())
            || (builder.node_kind(*node) == Some(NodeKind::Element)
                && builder
                    .node_macro_name(*node)
                    .is_some_and(|name| explicit_partial_block_close(name).is_some()))
    }) else {
        return Vec::new();
    };
    let opener = children[opener_index];
    let name = builder
        .node_text(opener)
        .or_else(|| builder.node_macro_name(opener))
        .expect("the position predicate required text")
        .to_owned();
    let close = explicit_partial_block_close(&name)
        .expect("the position predicate required an explicit partial opener");
    let mut suffix = builder
        .children(opener)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    suffix.extend_from_slice(&children[opener_index + 1..]);
    let inherits_synopsis = builder
        .node_flags(opener)
        .is_some_and(|flags| flags.synopsis_pretty);
    if (builder.node_text(opener).is_some() && !builder.clear_node_text(opener))
        || !builder.set_node_kind(opener, NodeKind::Element)
        || !builder.macro_name(opener, name.as_str())
    {
        return Vec::new();
    }
    let Some((head, body)) = make_block(
        builder,
        opener,
        &name,
        ArgumentPlacement::BodyTokens,
        max_nodes,
        outcome,
    ) else {
        return Vec::new();
    };
    let _ = builder.replace_children(body, &suffix);
    if inherits_synopsis {
        mark_synopsis_pretty(builder, head);
        mark_synopsis_pretty(builder, body);
    }
    structure_matched_explicit_partial_blocks(builder, body, spacing_enabled, max_nodes, outcome);
    let mut nested_scopes = structure_unclosed_explicit_partial_blocks(
        builder,
        body,
        spacing_enabled,
        max_nodes,
        outcome,
    );
    let nested_children =
        split_mdoc_inline_children(builder, body, spacing_enabled, max_nodes, outcome);
    let _ = builder.replace_children(body, &nested_children);
    clear_leading_explicit_partial_punctuation(builder, body);
    move_explicit_leading_open_delimiter(builder, opener, head, body);
    if matches!(name.as_str(), "Bo" | "Bro" | "Do") {
        coalesce_adjacent_text_children(builder, body);
    }
    let mut retained = children[..opener_index].to_vec();
    retained.push(opener);
    let _ = builder.replace_children(outer_body, &retained);
    let mut scopes = vec![ScopeFrame {
        close,
        open: opener,
        body,
        tail_on_close: false,
        transparent_target_taken: false,
        suppress_implicit_ancestor_break: false,
        resume_active: outer_body,
        resume_flow: outer_body,
    }];
    scopes.append(&mut nested_scopes);
    scopes
}

/// An explicit partial opener at the end of an `.It` header extends that
/// header across following physical macro lines.  It is structurally the same
/// `Ao`/`Bo`/… Block as an opener in ordinary body flow, but its close must
/// resume the item's Body rather than the surrounding list Body.
fn structure_item_head_explicit_partial(
    builder: &mut DocumentBuilder,
    item_head: NodeId,
    item_body: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Option<ScopeFrame> {
    let opener = *builder.children(item_head)?.last()?;
    let name = builder.node_macro_name(opener)?.to_owned();
    let close = explicit_partial_block_close(&name)?;
    let (head, body) = make_block(
        builder,
        opener,
        &name,
        ArgumentPlacement::BodyTokens,
        max_nodes,
        outcome,
    )?;
    if let Some(mut flags) = builder.node_flags(item_body) {
        // The item body is opened while its header extension remains active;
        // mandoc preserves the authored `.It` line-start marker on that
        // deferred body, matching the analogous `.It Xo` transition.
        flags.line_start = true;
        let _ = builder.set_node_flags(item_body, flags);
    }
    move_explicit_leading_open_delimiter(builder, opener, head, body);
    if matches!(name.as_str(), "Ao" | "Bo") {
        // Item-header partials bypass the ordinary top-level branches;
        // retain the one-phrase representation used by their legacy blocks.
        coalesce_adjacent_text_children(builder, body);
    }
    Some(ScopeFrame {
        close,
        open: opener,
        body,
        tail_on_close: false,
        transparent_target_taken: false,
        suppress_implicit_ancestor_break: false,
        resume_active: item_body,
        resume_flow: item_body,
    })
}

/// Initial pure-inline subset of `MDOC_CALLABLE | MDOC_PARSED` macros.
/// Partial/full block macros stay with their dedicated structural state
/// machine until their distinct argument and scope rules are implemented.
fn is_inline_mdoc_macro(name: &str) -> bool {
    matches!(
        name,
        "Ad" | "An"
            | "Ap"
            | "Ar"
            | "Bsx"
            | "Bx"
            | "Cd"
            | "Cm"
            | "Dx"
            | "Dv"
            | "Em"
            | "Er"
            | "Ev"
            | "Fa"
            | "Fl"
            | "Fn"
            | "Fx"
            | "Ft"
            | "Ic"
            | "In"
            | "Lk"
            | "Li"
            | "Ms"
            | "Mt"
            | "Nm"
            | "No"
            | "Ns"
            | "Ot"
            | "Nx"
            | "Ox"
            | "Pa"
            | "Pf"
            | "St"
            | "Sx"
            | "Sy"
            | "Tn"
            | "Ux"
            | "Va"
            | "Xr"
    )
}

/// Callable partial implicit blocks share the same token grammar as in-line
/// macros, but their final public shape is Block/Head/Body.  Keep the set
/// separate from the pure inline family so the source-order dispatcher can
/// build the block after its enclosing macro has yielded it as a sibling.
fn is_implicit_partial_block_macro(name: &str) -> bool {
    matches!(
        name,
        "Aq" | "Bq" | "Brq" | "Dq" | "Op" | "Pq" | "Ql" | "Qq" | "Sq"
    )
}

/// Return the static public spelling for an implicit partial block macro.
/// The source-order dispatcher holds a borrowed spelling, while recoveries
/// deliberately store only vocabulary from this fixed grammar.
fn implicit_partial_block_name(name: &str) -> &'static str {
    match name {
        "Aq" => "Aq",
        "Bq" => "Bq",
        "Brq" => "Brq",
        "Dq" => "Dq",
        "Op" => "Op",
        "Pq" => "Pq",
        "Ql" => "Ql",
        "Qq" => "Qq",
        "Sq" => "Sq",
        _ => unreachable!("caller checked the implicit partial block grammar"),
    }
}

pub(crate) fn is_mdoc_callable_macro(name: &str) -> bool {
    is_inline_mdoc_macro(name)
        || is_implicit_partial_block_macro(name)
        || explicit_partial_block_close(name).is_some()
        // A function closer is callable from another parsed macro's argument
        // list (for example `.Nm name Fc tail`), where it must reach the
        // source-order scope machine instead of remaining literal text.
        || matches!(name, "Ec" | "Eo" | "Fc")
}

/// Known mdoc spellings that `lookup()` recognizes but does not allow as a
/// nested invocation from an `MDOC_PARSED` in-line macro. Keep this separate
/// from `is_mdoc_callable_macro`: the latter intentionally contains only the
/// macro families currently reclassified by the native inline parser.
fn is_mdoc_noncallable_macro(name: &str) -> bool {
    matches!(
        name,
        "Dd" | "Dt"
            | "Os"
            | "Sh"
            | "Ss"
            | "Pp"
            | "D1"
            | "Dl"
            | "Bd"
            | "Ed"
            | "Bl"
            | "El"
            | "It"
            | "Ex"
            | "Fd"
            | "Nd"
            | "Rv"
            | "%A"
            | "%B"
            | "%D"
            | "%I"
            | "%J"
            | "%N"
            | "%O"
            | "%P"
            | "%R"
            | "%T"
            | "%V"
            | "Bf"
            | "Db"
            | "Ef"
            | "Re"
            | "Rs"
            | "Sm"
            | "Bk"
            | "Ek"
            | "Bt"
            | "Hf"
            | "Ud"
            | "Lb"
            | "Lp"
            | "%C"
            | "%Q"
            | "%U"
            | "Tg"
    )
}

/// `None` is variadic.  Finite counts consume their authored tokens before
/// callable-macro classification, preserving macro-specific argument rules.
fn mdoc_inline_argument_limit(name: &str) -> Option<usize> {
    match name {
        "Ap" | "Ns" | "Ux" => Some(0),
        // `in_line_argn()` owns a fixed prefix before ordinary source flow
        // resumes.  Pf shares its one-argument shape but adds separate
        // validation; Xr is the only currently callable two-argument form.
        "Bsx" | "Dx" | "Fx" | "In" | "Nx" | "Ox" | "Pf" | "St" => Some(1),
        "Bx" | "Xr" => Some(2),
        _ => None,
    }
}

/// Explicit partial-block openers and their matching closers.  The initial
/// implementation handles a closer present on the same physical line; the
/// stored pair is deliberately centralized so cross-line scope handling can
/// reuse this taxonomy without widening the public AST contract.
fn explicit_partial_block_close(name: &str) -> Option<&'static str> {
    match name {
        "Ao" => Some("Ac"),
        "Bo" => Some("Bc"),
        "Bro" => Some("Brc"),
        "Do" => Some("Dc"),
        "Eo" => Some("Ec"),
        "Oo" => Some("Oc"),
        "Po" => Some("Pc"),
        "Qo" => Some("Qc"),
        "So" => Some("Sc"),
        "Xo" => Some("Xc"),
        _ => None,
    }
}

fn is_mdoc_closing_delimiter(value: &str) -> bool {
    matches!(value, "," | "." | ";" | ":" | "!" | "?" | ")" | "]")
}

/// `post_ns()` warns when a no-space request is the first semantic event of
/// its physical request or is immediately followed by closing punctuation.
/// Other positions either join the neighboring words or are inert after a
/// closer, so they retain the same public empty Element without a finding.
fn no_space_macro_requires_warning(
    builder: &DocumentBuilder,
    node: NodeId,
    following: &[NodeId],
) -> bool {
    builder
        .node_flags(node)
        .is_some_and(|flags| flags.line_start)
        || following
            .first()
            .and_then(|next| builder.node_text(*next))
            .is_some_and(is_mdoc_closing_delimiter)
}

/// The only mdoc middle delimiter closes the current `in_line()` element but
/// lets the following ordinary word reopen the same macro.  This is distinct
/// from closing punctuation, after which subsequent words resume surrounding
/// source flow。`mandoc` 同时识别包住该分隔符的常见字体复位拼写，并将其保留为可见文本节点。
fn is_mdoc_middle_delimiter(value: &str) -> bool {
    matches!(value, "|" | r"\fR|\fP")
}

/// Allocate the next element after mdoc's middle-delimiter scope rewind.  It
/// inherits the request's source position, but cannot claim a physical line
/// start because the separator occurred in the same parsed argument list.
fn reopen_inline_element(
    builder: &mut DocumentBuilder,
    allocation_parent: NodeId,
    source: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Option<NodeId> {
    if builder.node_count() >= max_nodes {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = builder.node_location(source);
        }
        return None;
    }
    let macro_name = builder.node_macro_name(source)?.to_owned();
    let location = builder.node_location(source);
    let mut flags = builder.node_flags(source).unwrap_or_default();
    flags.line_start = false;
    // `push` needs a concrete private parent, but the caller is about to
    // replace that parent's children with its source-order event list.  Keep
    // a snapshot so this provisional allocation cannot become an accidental
    // child of the preceding element (notably the empty `Fl` before `|`).
    let previous_children = builder
        .children(allocation_parent)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    let reopened = builder.push(allocation_parent, NodeKind::Element)?;
    let configured = builder.macro_name(reopened, macro_name)
        && builder.set_node_location(reopened, location)
        && builder.set_node_flags(reopened, flags);
    let _ = builder.replace_children(allocation_parent, &previous_children);
    if !configured {
        return None;
    }
    Some(reopened)
}

fn mark_opening_delimiter(builder: &mut DocumentBuilder, node: NodeId, text: Option<&str>) {
    if !text.is_some_and(|value| matches!(value, "(" | "[")) {
        return;
    }
    if let Some(mut flags) = builder.node_flags(node) {
        flags.delimiter_open = true;
        let _ = builder.set_node_flags(node, flags);
    }
}

/// Move the source request's first-event provenance onto a delimiter that
/// became the public leading event after the private macro element vanished.
fn transfer_line_start(builder: &mut DocumentBuilder, source: NodeId, target: NodeId) {
    let line_start = builder
        .node_flags(source)
        .is_some_and(|flags| flags.line_start);
    if let Some(mut flags) = builder.node_flags(target) {
        flags.line_start = line_start;
        let _ = builder.set_node_flags(target, flags);
    }
}

/// Return whether an empty tag-style macro survives its source request as a
/// warning.
///
/// A leading delimiter may split a request into a discarded empty first
/// element and a later populated element of the same macro. Any other
/// follower (including another callable macro) leaves the first element
/// genuinely argument-less and therefore warned.
fn tag_empty_macro_requires_warning(
    builder: &DocumentBuilder,
    macro_name: &str,
    following: &[NodeId],
) -> bool {
    let delimiter_count = following
        .iter()
        .take_while(|node| {
            builder.node_text(**node).is_some_and(|text| {
                is_mdoc_closing_delimiter(text)
                    || is_mdoc_middle_delimiter(text)
                    || matches!(text, "(" | "[")
            })
        })
        .count();
    delimiter_count == 0
        || !following.get(delimiter_count).is_some_and(|successor| {
            builder.node_macro_name(*successor) == Some(macro_name)
                && builder
                    .children(*successor)
                    .is_some_and(|children| !children.is_empty())
        })
}

fn finish_inline_element(
    builder: &mut DocumentBuilder,
    node: NodeId,
    children: &[NodeId],
    spacing_enabled: bool,
) {
    let _ = builder.replace_children(node, children);
    if spacing_enabled && matches!(builder.node_macro_name(node), Some("Em" | "Sy")) {
        // Em 与 Sy use MDOC_JOIN for ordinary word sequences, but libmandoc
        // disables that join while `.Sm off` is in effect. Real inline
        // boundaries have already been split before this point.
        coalesce_text_children(builder, node);
    } else if spacing_enabled
        && builder.node_macro_name(node) == Some("No")
        && builder
            .node_flags(node)
            .is_none_or(|flags| !flags.synopsis_pretty)
    {
        coalesce_text_children(builder, node);
    }
}

/// Parsed macro arguments can contain a second implicit partial block.  The
/// scanner first classifies that inner macro as an element; mdoc then applies
/// the same Block/Head/Body construction recursively without making it a
/// source-line sibling (for example `.Op one Op two`).
fn structure_nested_implicit_partial_blocks(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
    spacing_enabled: bool,
) {
    let Some(children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
        return;
    };
    for node in children {
        structure_implicit_partial_block(builder, node, max_nodes, outcome, spacing_enabled);
    }
}

/// Discover explicit partial scopes nested below already-structured implicit
/// blocks.  Such an opener is not a top-level source event, but its physical
/// closer still participates in the main scope stack (for example
/// `.Aq … Bq … Po` followed by `.Pc`).
fn structure_nested_implicit_explicit_scopes(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
    spacing_enabled: bool,
) -> Vec<ScopeFrame> {
    let Some(children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
        return Vec::new();
    };
    let mut scopes = Vec::new();
    for child in children {
        if builder.node_kind(child) != Some(NodeKind::Block)
            || !builder
                .node_macro_name(child)
                .is_some_and(is_implicit_partial_block_macro)
        {
            continue;
        }
        let Some(body) = builder.children(child).and_then(|parts| {
            parts.iter().copied().find(|part| {
                builder.node_kind(*part) == Some(NodeKind::Body)
                    && builder.node_macro_name(*part) == builder.node_macro_name(child)
            })
        }) else {
            continue;
        };
        scopes.extend(structure_nested_implicit_explicit_scopes(
            builder,
            body,
            max_nodes,
            outcome,
            spacing_enabled,
        ));
        scopes.extend(structure_unclosed_explicit_partial_blocks(
            builder,
            body,
            spacing_enabled,
            max_nodes,
            outcome,
        ));
    }
    scopes
}

/// Apply the implicit-partial Block projection to one already-classified
/// callable macro.  Explicit partial openers can yield a same-line tail that
/// never re-enters the top-level source-order dispatcher; keeping this helper
/// separate lets those tail events receive the same projection as ordinary
/// children.
fn structure_implicit_partial_block(
    builder: &mut DocumentBuilder,
    node: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
    spacing_enabled: bool,
) {
    let Some(name) = builder.node_macro_name(node).map(str::to_owned) else {
        return;
    };
    if !is_implicit_partial_block_macro(&name) {
        return;
    }
    let inherits_synopsis = builder
        .node_flags(node)
        .is_some_and(|flags| flags.synopsis_pretty);
    let Some((head, body)) = make_block(
        builder,
        node,
        &name,
        ArgumentPlacement::BodyTokens,
        max_nodes,
        outcome,
    ) else {
        return;
    };
    if inherits_synopsis {
        mark_synopsis_pretty(builder, head);
        mark_synopsis_pretty(builder, body);
    }
    let nested = split_mdoc_inline_children(builder, body, spacing_enabled, max_nodes, outcome);
    let mut nested = expand_fl_elements(builder, body, nested, max_nodes, outcome);
    insert_generated_system_names(builder, &nested, max_nodes, outcome);
    let tail = take_implicit_partial_tail(builder, &mut nested);
    let _ = builder.replace_children(body, &nested);
    move_leading_open_delimiters(builder, node, head, body);
    clear_initial_implicit_body_delimiter_flags(builder, body);
    clear_terminal_implicit_body_opening_flags(builder, body);
    mark_implicit_partial_tail_sentence_ends(builder, &tail);
    if spacing_enabled && name != "Op" {
        coalesce_implicit_partial_body_text(builder, body);
    }
    structure_nested_implicit_partial_blocks(builder, body, max_nodes, outcome, spacing_enabled);
    if !tail.is_empty() {
        let mut block_children = vec![head, body];
        block_children.extend(tail);
        let _ = builder.replace_children(node, &block_children);
    }
}

/// A trailing unescaped closing delimiter is not body prose for an implicit
/// mdoc partial block. `blk_part_imp()` publishes it after the Body, where it
/// carries the terminal sentence state. The inline splitter has already
/// classified only real delimiter tokens, so escaped spellings such as `\\&.`
/// remain ordinary body text.
fn take_implicit_partial_tail(
    builder: &DocumentBuilder,
    children: &mut Vec<NodeId>,
) -> Vec<NodeId> {
    let is_tail = |node: &NodeId| {
        builder
            .node_text(*node)
            .is_some_and(is_mdoc_closing_delimiter)
            && builder
                .node_flags(*node)
                .is_some_and(|flags| flags.delimiter_close)
    };
    let split = children
        .iter()
        .rposition(|node| !is_tail(node))
        .map_or(0, |index| index + 1);
    if split < children.len() {
        children.split_off(split)
    } else {
        Vec::new()
    }
}

/// `blk_part_imp()` keeps a leading opening delimiter between its empty Head
/// and Body instead of treating it as the first body word.  That placement is
/// observable in the owned AST and applies to constructs such as
/// `.Dq "(" user@host)`.
fn move_leading_open_delimiter(
    builder: &mut DocumentBuilder,
    block: NodeId,
    head: NodeId,
    body: NodeId,
) {
    let Some(children) = builder.children(body).map(<[NodeId]>::to_vec) else {
        return;
    };
    let Some((&first, rest)) = children.split_first() else {
        return;
    };
    if !builder
        .node_text(first)
        .is_some_and(|value| matches!(value, "(" | "["))
    {
        return;
    }
    if let Some(mut flags) = builder.node_flags(first) {
        flags.delimiter_open = true;
        let _ = builder.set_node_flags(first, flags);
    }
    let _ = builder.replace_children(body, rest);
    let _ = builder.replace_children(block, &[head, first, body]);
}

/// `blk_part_imp()` publishes every leading opening delimiter between the
/// empty Head and Body.  The single-delimiter form is common, but an input
/// such as `.Op ( (` exposes both authored delimiters as block children.
fn move_leading_open_delimiters(
    builder: &mut DocumentBuilder,
    block: NodeId,
    head: NodeId,
    body: NodeId,
) {
    let Some(children) = builder.children(body).map(<[NodeId]>::to_vec) else {
        return;
    };
    let leading_count = children
        .iter()
        .take_while(|node| {
            builder
                .node_text(**node)
                .is_some_and(|value| matches!(value, "(" | "["))
        })
        .count();
    if leading_count == 0 {
        return;
    }
    let (leading, rest) = children.split_at(leading_count);
    for delimiter in leading {
        if let Some(mut flags) = builder.node_flags(*delimiter) {
            flags.delimiter_open = true;
            let _ = builder.set_node_flags(*delimiter, flags);
        }
    }
    let _ = builder.replace_children(body, rest);
    let mut block_children = Vec::with_capacity(leading.len().saturating_add(2));
    block_children.push(head);
    block_children.extend_from_slice(leading);
    block_children.push(body);
    let _ = builder.replace_children(block, &block_children);
}

/// A leading closing delimiter is literal body content when more source words
/// follow it (`.Op . z`).  The inline tokenizer initially marks every closing
/// delimiter; remove that provisional classification only for this body-first
/// case, after terminal tails have already been selected.
fn clear_initial_implicit_body_delimiter_flags(builder: &mut DocumentBuilder, body: NodeId) {
    let Some(&first) = builder.children(body).and_then(|children| children.first()) else {
        return;
    };
    if !builder
        .node_text(first)
        .is_some_and(is_mdoc_closing_delimiter)
    {
        return;
    }
    if let Some(mut flags) = builder.node_flags(first) {
        flags.delimiter_close = false;
        flags.sentence_end = false;
        let _ = builder.set_node_flags(first, flags);
    }
}

/// A trailing opening delimiter remains literal body content.  It only gains
/// its delimiter-open flag when later body flow makes it an in-line opener
/// (`.Op a ( z` versus `.Op a (`).
fn clear_terminal_implicit_body_opening_flags(builder: &mut DocumentBuilder, body: NodeId) {
    let Some(&last) = builder.children(body).and_then(|children| children.last()) else {
        return;
    };
    if !builder
        .node_text(last)
        .is_some_and(|text| matches!(text, "(" | "["))
    {
        return;
    }
    if let Some(mut flags) = builder.node_flags(last) {
        flags.delimiter_open = false;
        let _ = builder.set_node_flags(last, flags);
    }
}

/// Consecutive terminal delimiters are all public tail nodes.  Re-evaluate
/// each node after splitting so `.Op . .` preserves sentence state on both
/// periods rather than only on the final one.
fn mark_implicit_partial_tail_sentence_ends(builder: &mut DocumentBuilder, tail: &[NodeId]) {
    for delimiter in tail {
        mark_sentence_end(builder, *delimiter);
    }
}

/// `blk_part_exp()` places a leading opening delimiter before the generated
/// Head, unlike `blk_part_imp()` which keeps it after the Head.
fn move_explicit_leading_open_delimiter(
    builder: &mut DocumentBuilder,
    block: NodeId,
    head: NodeId,
    body: NodeId,
) {
    let Some(children) = builder.children(body).map(<[NodeId]>::to_vec) else {
        return;
    };
    let Some((&first, rest)) = children.split_first() else {
        return;
    };
    if !builder
        .node_text(first)
        .is_some_and(|value| matches!(value, "(" | "["))
    {
        return;
    }
    if let Some(mut flags) = builder.node_flags(first) {
        flags.delimiter_open = true;
        let _ = builder.set_node_flags(first, flags);
    }
    let _ = builder.replace_children(body, rest);
    let _ = builder.replace_children(block, &[first, head, body]);
}

/// Punctuation in an explicit partial block remains literal while later body
/// content follows it (`.Oo . word` and `.Oo word . next`).  The shared inline
/// splitter cannot see that body-level continuation, so clear its provisional
/// punctuation flags after the Body is selected.
fn clear_leading_explicit_partial_punctuation(builder: &mut DocumentBuilder, body: NodeId) {
    let Some(children) = builder.children(body).map(<[NodeId]>::to_vec) else {
        return;
    };
    for (index, node) in children.iter().copied().enumerate() {
        if index > 0
            && children[index + 1..].is_empty()
            && builder
                .node_text(node)
                .is_some_and(|text| matches!(text, "(" | "["))
            && let Some(mut flags) = builder.node_flags(node)
        {
            flags.delimiter_open = false;
            let _ = builder.set_node_flags(node, flags);
        }
        if !builder
            .node_text(node)
            .is_some_and(is_mdoc_closing_delimiter)
            || children[index + 1..].is_empty()
        {
            continue;
        }
        if let Some(mut flags) = builder.node_flags(node) {
            if index == 0 {
                flags.delimiter_close = false;
            }
            flags.sentence_end = false;
            let _ = builder.set_node_flags(node, flags);
        }
    }
}

/// Reproduce `tag_move_href()` for the validated `.Tg` followed by `.Pp`
/// path.  The paragraph owns the destination and its immediately following
/// text owns the display permalink.  mandoc splits that text only when its
/// historical five-byte scan stops on a separating space.
fn move_paragraph_permalink(
    builder: &mut DocumentBuilder,
    text_node: NodeId,
    parent: NodeId,
    tag: &str,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) {
    let Some(text) = builder.node_text(text_node).map(str::to_owned) else {
        return;
    };
    if text.is_empty() || text.starts_with(' ') {
        return;
    }

    let split = paragraph_permalink_split(&text);
    if let Some(split) = split
        && builder.node_count() < max_nodes
    {
        let tail = text[split + 1..].to_owned();
        let Some(mut flags) = builder.node_flags(text_node) else {
            return;
        };
        let location = builder.node_location(text_node);
        if !builder.text(text_node, text[..split].to_owned()) {
            return;
        }
        let Some(tail_node) = builder.push(parent, NodeKind::Text) else {
            return;
        };
        flags.line_start = false;
        let _ = builder.text(tail_node, tail);
        let _ = builder.set_node_flags(tail_node, flags);
        if let Some(mut location) = location {
            // mandoc assigns the synthetic word `n->pos + (cp - n->string)`,
            // i.e. the delimiter's column rather than the first byte after
            // it.  Preserve that observable legacy location exactly.
            let split = u32::try_from(split).unwrap_or(u32::MAX);
            location.start = location.start.saturating_add(split);
            let _ = builder.set_node_location(tail_node, Some(location));
        }
        let Some(children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
            return;
        };
        let Some(position) = children.iter().position(|child| *child == text_node) else {
            return;
        };
        let mut reordered = children;
        let Some(created_position) = reordered.iter().position(|child| *child == tail_node) else {
            return;
        };
        reordered.remove(created_position);
        reordered.insert(position.saturating_add(1), tail_node);
        let _ = builder.replace_children(parent, &reordered);
    } else if split.is_some() && outcome.node_limit_location.is_none() {
        outcome.node_limit_location = builder.node_location(text_node);
    }

    let Some(mut flags) = builder.node_flags(text_node) else {
        return;
    };
    flags.permalink = true;
    let _ = builder.set_node_flags(text_node, flags);
    let _ = builder.set_node_tag(text_node, tag);
}

fn paragraph_permalink_split(text: &str) -> Option<usize> {
    let mut search_from = 0;
    let mut space = text[search_from..]
        .find(' ')
        .map(|offset| search_from.saturating_add(offset));
    while space.is_some_and(|offset| offset < 5) {
        search_from = space.expect("space was checked").saturating_add(1);
        space = text[search_from..]
            .find(' ')
            .map(|offset| search_from.saturating_add(offset));
    }
    space.filter(|offset| {
        text.as_bytes()
            .get(offset.saturating_add(1))
            .is_some_and(|byte| *byte != b'\0')
    })
}

fn node_arguments(builder: &DocumentBuilder, node: NodeId) -> Vec<String> {
    builder
        .children(node)
        .into_iter()
        .flatten()
        .filter_map(|argument| builder.node_text(*argument))
        .map(str::to_owned)
        .collect()
}

/// Validate the standalone author macro's compact option surface.  The first
/// `-split`/`-nosplit` option selects the public layout mode; later options
/// are syntax-only and all remaining text is a single author phrase.
fn validate_an(builder: &mut DocumentBuilder, node: NodeId, outcome: &mut StructureOutcome) {
    let Some(arguments) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return;
    };
    let mut option_count = 0;
    let mut author_mode = None;
    for argument in &arguments {
        let Some(option) = builder.node_text(*argument) else {
            break;
        };
        let mode = match option {
            "-split" => AuthorMode::Split,
            "-nosplit" => AuthorMode::NoSplit,
            _ => break,
        };
        if author_mode.is_some() {
            outcome.recoveries.push(Recovery::DuplicateArgument {
                macro_name: "An",
                argument: option.into(),
                location: builder.node_location(*argument),
            });
        } else {
            author_mode = Some(mode);
        }
        option_count += 1;
    }
    let retained = &arguments[option_count..];
    if option_count != 0 {
        let _ = builder.replace_children(node, retained);
    }
    let _ = builder.set_node_author_mode(node, author_mode);

    if author_mode.is_some() {
        if let Some(excess) = retained.first().copied() {
            outcome.recoveries.push(Recovery::InvalidArguments {
                message: format!(
                    "skipping excess arguments: An ... {}",
                    builder.node_text(excess).unwrap_or_default()
                )
                .into(),
                location: builder.node_location(excess),
            });
        }
        return;
    }

    let Some(last) = retained.last().copied() else {
        outcome.recoveries.push(Recovery::EmptyMacro {
            macro_name: "An",
            location: builder.node_location(node),
        });
        return;
    };
    let Some(text) = builder.node_text(last) else {
        return;
    };
    let Some((&delimiter, prefix)) = text.as_bytes().split_last() else {
        return;
    };
    if !matches!(
        delimiter,
        b',' | b'.' | b';' | b':' | b'!' | b'?' | b')' | b']'
    ) || prefix.last().is_none_or(u8::is_ascii_whitespace)
    {
        return;
    }
    let Some(location) = builder.node_location(last).and_then(|span| {
        span.end
            .checked_sub(1)
            .and_then(|start| SourceSpan::new(span.source, start, span.end).ok())
    }) else {
        return;
    };
    let display = if retained.len() == 1 {
        text.to_owned()
    } else {
        format!("... {text}")
    };
    outcome.recoveries.push(Recovery::TrailingDelimiterSpacing {
        macro_name: "An",
        display: display.into(),
        location: Some(location),
    });
}

/// Return the tag-style macros whose empty public elements are deleted by
/// legacy post-validation. `Cm` and `No` have additional context-sensitive
/// rules at their call sites and intentionally stay out of this table.
fn empty_tag_macro_name(macro_name: Option<&str>) -> Option<&'static str> {
    match macro_name {
        Some("Dv") => Some("Dv"),
        Some("Em") => Some("Em"),
        Some("Er") => Some("Er"),
        Some("Ev") => Some("Ev"),
        Some("Ic") => Some("Ic"),
        Some("Li") => Some("Li"),
        Some("Ms") => Some("Ms"),
        Some("Sy") => Some("Sy"),
        Some("Va") => Some("Va"),
        _ => None,
    }
}

/// Return the ordinary `in_line()` tag-style macros whose leading delimiters
/// are published outside the element before an ordinary following word opens
/// a new element of the same kind. This excludes fixed-argument forms such as
/// `In` and `Xr`, plus `Fl`/`Fn`, which have their own argument rules.
fn is_tag_style_delimiter_restart_macro(macro_name: Option<&str>) -> bool {
    matches!(
        macro_name,
        Some("Cd" | "Cm" | "Dv" | "Em" | "Er" | "Ev" | "Ic" | "Li" | "Ms" | "No" | "Sy" | "Va")
    )
}

/// Return the tag-style macros that use `post_delim_nb()` validation.
fn tag_macro_name(macro_name: Option<&str>) -> Option<&'static str> {
    match macro_name {
        Some("Cm") => Some("Cm"),
        Some("Dv") => Some("Dv"),
        Some("Em") => Some("Em"),
        Some("Er") => Some("Er"),
        Some("Ev") => Some("Ev"),
        Some("Ic") => Some("Ic"),
        Some("Li") => Some("Li"),
        Some("Ms") => Some("Ms"),
        Some("Sy") => Some("Sy"),
        Some("Va") => Some("Va"),
        _ => None,
    }
}

/// Preserve the link macro's exceptional punctuation ownership. `Lk` keeps a
/// standalone closing delimiter inside its element, unlike ordinary
/// `in_line()` macros that release it to surrounding flow.
fn mark_link_terminal_delimiter(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(last) = builder
        .children(node)
        .and_then(|children| children.last())
        .copied()
    else {
        return;
    };
    if !builder
        .node_text(last)
        .is_some_and(is_mdoc_closing_delimiter)
    {
        return;
    }
    if let Some(mut flags) = builder.node_flags(last) {
        flags.delimiter_close = true;
        let _ = builder.set_node_flags(last, flags);
    }
    mark_sentence_end(builder, last);
}

/// Apply the `post_tag()` delimiter validation that is otherwise hidden when
/// a tag-style macro is parsed as a callable macro inside another request.
fn validate_tag(
    builder: &DocumentBuilder,
    node: NodeId,
    macro_name: &'static str,
    recoveries: &mut Vec<Recovery>,
) {
    let Some(children) = builder.children(node) else {
        return;
    };
    let Some(last) = children.last().copied() else {
        return;
    };
    let Some(text) = builder.node_text(last) else {
        return;
    };
    let Some((&delimiter, prefix)) = text.as_bytes().split_last() else {
        return;
    };
    if !matches!(
        delimiter,
        b',' | b'.' | b';' | b':' | b'!' | b'?' | b')' | b']'
    ) || prefix.last().is_none_or(u8::is_ascii_whitespace)
    {
        return;
    }
    let Some(location) = builder.node_location(last).and_then(|span| {
        // Parsed source words retain a logical start but may share a physical
        // control-line end after the inline splitter has separated them.
        // The attached ASCII delimiter is therefore relative to that logical
        // word start, never to the shared physical end.
        let base = builder.node_source_position(last)?;
        let offset = u32::try_from(text.len().checked_sub(1)?).ok()?;
        let column = base.column.checked_add(offset)?;
        Some(
            SourceSpan::new(span.source, span.start, span.end)
                .ok()?
                .with_logical_start(crate::SourcePosition {
                    line: base.line,
                    column,
                }),
        )
    }) else {
        return;
    };
    let display = if children.first().copied() == Some(last) {
        text.to_owned()
    } else {
        format!("... {text}")
    };
    recoveries.push(Recovery::TrailingDelimiterSpacing {
        macro_name,
        display: display.into(),
        location: Some(location),
    });
}

/// Apply mdoc's library-catalogue expansion.  A known library hides its
/// selector and prepends its generated description; an unknown library is
/// rendered as `library \(lq<selector>\(rq` while preserving later authored
/// arguments.  This is AST semantics, not a renderer substitution.
fn validate_library(
    builder: &mut DocumentBuilder,
    node: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
    deferred_recoveries: &mut Vec<Recovery>,
    outer_delimiters: &mut Vec<NodeId>,
) -> bool {
    let mut children = builder
        .children(node)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    if children.len() > 1
        && children
            .last()
            .and_then(|child| builder.node_text(*child))
            .is_some_and(is_mdoc_closing_delimiter)
    {
        let delimiter = children.pop().expect("length was checked");
        let Some(mut flags) = builder.node_flags(delimiter) else {
            return false;
        };
        flags.delimiter_close = true;
        flags.sentence_end = builder
            .node_text(delimiter)
            .is_some_and(|text| matches!(text, "." | "!" | "?"));
        if !builder.set_node_flags(delimiter, flags) || !builder.replace_children(node, &children) {
            return false;
        }
        outer_delimiters.push(delimiter);
    }
    let Some(first) = children.first().copied() else {
        outcome.recoveries.push(Recovery::EmptyMacro {
            macro_name: "Lb",
            location: builder.node_location(node),
        });
        return false;
    };
    let Some(library) = builder.node_text(first).map(str::to_owned) else {
        return true;
    };

    validate_no_break_trailing_delimiter(builder, node, "Lb", deferred_recoveries);

    if let Some(description) = mdoc_library_description(&library) {
        if builder.node_count() >= max_nodes {
            if outcome.node_limit_location.is_none() {
                outcome.node_limit_location = builder.node_location(node);
            }
            return false;
        }
        // Catalogue rows are Rust-owned generated text rather than physical
        // roff input.  Their historical table intentionally stores doubled
        // escapes for source readability; expose the one-escape spelling the
        // normal document escape pass expects so `\\-` remains a hyphen and
        // not a visible reverse solidus in the engine projection.
        let description = description.replace(r"\\-", r"\-").replace(r"\\~", r"\~");
        let Some(description_node) = push_generated_text(builder, node, &description, false) else {
            if outcome.node_limit_location.is_none() {
                outcome.node_limit_location = builder.node_location(node);
            }
            return false;
        };
        let Some(mut flags) = builder.node_flags(first) else {
            return false;
        };
        flags.no_print = true;
        if !builder.set_node_flags(first, flags) {
            return false;
        }
        let mut reordered = Vec::with_capacity(children.len().saturating_add(1));
        reordered.push(description_node);
        reordered.extend(children);
        return builder.replace_children(node, &reordered);
    }

    if builder.node_count().saturating_add(3) > max_nodes {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = builder.node_location(node);
        }
        return false;
    }
    deferred_recoveries.push(Recovery::UnknownLibrary {
        library: library.into(),
        location: builder.node_location(first),
    });
    let Some(generic) = push_generated_text(builder, node, "library", false) else {
        return false;
    };
    let Some(opening) = push_generated_text(builder, node, r"\(lq", false) else {
        return false;
    };
    let Some(closing) = push_generated_text(builder, node, r"\(rq", false) else {
        return false;
    };
    let Some(mut flags) = builder.node_flags(opening) else {
        return false;
    };
    flags.delimiter_open = true;
    if !builder.set_node_flags(opening, flags) {
        return false;
    }
    let Some(mut flags) = builder.node_flags(closing) else {
        return false;
    };
    flags.delimiter_close = true;
    if !builder.set_node_flags(closing, flags) {
        return false;
    }

    let mut reordered = Vec::with_capacity(children.len().saturating_add(3));
    reordered.extend([generic, opening, first, closing]);
    reordered.extend(children.into_iter().skip(1));
    builder.replace_children(node, &reordered)
}

/// Match `post_delim_nb()` for the family of macros that keeps punctuation in
/// its own presentation flow.  The current callers are intentionally narrow;
/// retaining its complete historical false-positive filtering here prevents
/// `.Lb` validation from over-reporting compared with libmandoc.
fn validate_no_break_trailing_delimiter(
    builder: &DocumentBuilder,
    node: NodeId,
    macro_name: &'static str,
    recoveries: &mut Vec<Recovery>,
) {
    let Some(children) = builder.children(node) else {
        return;
    };
    let Some(last) = children.last().copied() else {
        return;
    };
    let Some(text) = builder.node_text(last) else {
        return;
    };
    let bytes = text.as_bytes();
    let Some((&delimiter, prefix)) = bytes.split_last() else {
        return;
    };
    if prefix.is_empty()
        || !matches!(
            delimiter,
            b',' | b'.' | b';' | b':' | b'!' | b'?' | b')' | b']' | b'|'
        )
    {
        return;
    }
    let delimiter_index = bytes.len().saturating_sub(1);
    if delimiter_index >= 2
        && matches!(
            bytes.get(delimiter_index - 2..delimiter_index),
            Some(b"\\&" | b"\\e")
        )
    {
        return;
    }
    match delimiter {
        b')' if text.contains('(') => return,
        b'.' if bytes.len() >= 3 && bytes[bytes.len() - 3..] == *b"..." => return,
        // `post_delim_nb()` suppresses the false positive for C-style
        // variable declarations ending in a semicolon.
        b';' if macro_name == "Vt" => return,
        b'?' if prefix.last() == Some(&b'?') => return,
        b']' if text.contains('[') => return,
        b'|' if bytes.len() == 2 && prefix == b"|" => return,
        _ => {}
    }
    if bytes.len() == 2 && !prefix[0].is_ascii_alphanumeric() {
        return;
    }
    let Some(location) = text_offset_location(builder, last, delimiter_index) else {
        return;
    };
    let display = if generated_system_name(macro_name).is_some() {
        // The synthetic operating-system word is public AST structure but
        // not part of `post_delim_nb()`'s authored diagnostic phrase.
        text.to_owned()
    } else if children.len() == 1 {
        text.to_owned()
    } else {
        format!("... {text}")
    };
    recoveries.push(Recovery::TrailingDelimiterSpacing {
        macro_name,
        display: display.into(),
        location: Some(location),
    });
}

/// Match `post_pf()`'s source-line requirement: `.Pf` owns one literal prefix
/// argument, but another visible token must follow it on that same line.
/// Delimiters alone do not satisfy that requirement; a closing `Pc` does,
/// because the punctuation remains owned by its enclosing partial block.
fn validate_prefix_following(
    builder: &DocumentBuilder,
    node: NodeId,
    following: &[NodeId],
    recoveries: &mut Vec<Recovery>,
) {
    let Some(position) = builder.node_source_position(node) else {
        return;
    };
    let same_line = following
        .iter()
        .copied()
        .take_while(|candidate| {
            builder
                .node_source_position(*candidate)
                .is_some_and(|candidate_position| candidate_position.line == position.line)
        })
        .collect::<Vec<_>>();
    if same_line.iter().any(|candidate| {
        builder.node_macro_name(*candidate) == Some("Pc")
            || builder
                .node_macro_name(*candidate)
                .is_some_and(|macro_name| macro_name != "Pc")
            || builder
                .node_text(*candidate)
                .is_some_and(|text| !is_mdoc_closing_delimiter(text))
    }) {
        return;
    }

    let prefix = node_arguments(builder, node).join(" ");
    let display = if prefix.is_empty() {
        same_line
            .iter()
            .find_map(|candidate| builder.node_text(*candidate))
            .map_or_else(|| "Pf at eol".to_owned(), |text| format!("Pf {text}"))
    } else {
        format!("Pf {prefix}")
    };
    recoveries.push(Recovery::PrefixWithoutFollowing {
        display: display.into_boxed_str(),
        location: builder.node_location(node),
    });
}

/// Resolve the stable mdoc library-name catalogue.  It mirrors mandoc 1.14.6
/// plus the wrapper's pinned `libbsd` addition, but is native data rather than
/// a runtime dependency on the former C vendor tree.
#[allow(clippy::too_many_lines)] // The pinned upstream library-name catalogue is intentionally data-local.
fn mdoc_library_description(name: &str) -> Option<&'static str> {
    match name {
        "lib80211" => Some(r"802.11 Wireless Network Management Library (lib80211, \\-l80211)"),
        "libalias" => Some(r"Packet Aliasing Library (libalias, \\-lalias)"),
        "libarchive" => Some(r"Streaming Archive Library (libarchive, \\-larchive)"),
        "libarm" => Some(r"ARM Architecture Library (libarm, \\-larm)"),
        "libarm32" => Some(r"ARM32 Architecture Library (libarm32, \\-larm32)"),
        "libbe" => Some(r"Boot Environment Library (libbe, \\-lbe)"),
        "libbluetooth" => Some(r"Bluetooth Library (libbluetooth, \\-lbluetooth)"),
        "libbsd" => Some(r"Utility functions from BSD systems (libbsd, \\-lbsd)"),
        "libbsdxml" => Some(r"eXpat XML parser library (libbsdxml, \\-lbsdxml)"),
        "libbsm" => Some(r"Basic Security Module Library (libbsm, \\-lbsm)"),
        "libc" => Some(r"Standard C\\~Library (libc, \\-lc)"),
        "libc_r" => Some(r"Reentrant C\\~Library (libc_r, \\-lc_r)"),
        "libcalendar" => Some(r"Calendar Arithmetic Library (libcalendar, \\-lcalendar)"),
        "libcam" => Some(r"Common Access Method User Library (libcam, \\-lcam)"),
        "libcasper" => Some(r"Casper Library (libcasper, \\-lcasper)"),
        "libcdk" => Some(r"Curses Development Kit Library (libcdk, \\-lcdk)"),
        "libcipher" => Some(r"FreeSec Crypt Library (libcipher, \\-lcipher)"),
        "libcompat" => Some(r"Compatibility Library (libcompat, \\-lcompat)"),
        "libcrypt" => Some(r"Crypt Library (libcrypt, \\-lcrypt)"),
        "libcurses" => Some(r"Curses Library (libcurses, \\-lcurses)"),
        "libcuse" => Some(r"Userland Character Device Library (libcuse, \\-lcuse)"),
        "libdevattr" => Some(r"Device attribute and event library (libdevattr, \\-ldevattr)"),
        "libdevctl" => Some(r"Device Control Library (libdevctl, \\-ldevctl)"),
        "libdevinfo" => {
            Some(r"Device and Resource Information Utility Library (libdevinfo, \\-ldevinfo)")
        }
        "libdevstat" => Some(r"Device Statistics Library (libdevstat, \\-ldevstat)"),
        "libdisk" => Some(r"Interface to Slice and Partition Labels Library (libdisk, \\-ldisk)"),
        "libdl" => Some(r"Dynamic Linker Services Filter (libdl, \\-ldl)"),
        "libdm" => Some(r"Device Mapper Library (libdm, \\-ldm)"),
        "libdwarf" => Some(r"DWARF Access Library (libdwarf, \\-ldwarf)"),
        "libedit" => Some(r"Command Line Editor Library (libedit, \\-ledit)"),
        "libefi" => Some(r"EFI Runtime Services Library (libefi, \\-lefi)"),
        "libelf" => Some(r"ELF Access Library (libelf, \\-lelf)"),
        "libevent" => Some(r"Event Notification Library (libevent, \\-levent)"),
        "libexecinfo" => Some(r"Backtrace Information Library (libexecinfo, \\-lexecinfo)"),
        "libfetch" => Some(r"File Transfer Library (libfetch, \\-lfetch)"),
        "libfsid" => Some(r"Filesystem Identification Library (libfsid, \\-lfsid)"),
        "libftpio" => Some(r"FTP Connection Management Library (libftpio, \\-lftpio)"),
        "libform" => Some(r"Curses Form Library (libform, \\-lform)"),
        "libgeom" => Some(r"Userland API Library for Kernel GEOM subsystem (libgeom, \\-lgeom)"),
        "libgpio" => Some(r"General-Purpose Input Output (GPIO) library (libgpio, \\-lgpio)"),
        "libhammer" => Some(r"HAMMER Filesystem Userland Library (libhammer, \\-lhammer)"),
        "libi386" => Some(r"i386 Architecture Library (libi386, \\-li386)"),
        "libintl" => Some(r"Internationalized Message Handling Library (libintl, \\-lintl)"),
        "libipsec" => Some(r"IPsec Policy Control Library (libipsec, \\-lipsec)"),
        "libiscsi" => Some(r"iSCSI protocol library (libiscsi, \\-liscsi)"),
        "libisns" => Some(r"iSNS protocol library (libisns, \\-lisns)"),
        "libjail" => Some(r"Jail Library (libjail, \\-ljail)"),
        "libkcore" => Some(r"Kernel Memory Core Access Library (libkcore, \\-lkcore)"),
        "libkiconv" => Some(r"Kernel-side iconv Library (libkiconv, \\-lkiconv)"),
        "libkse" => Some(r"N:M Threading Library (libkse, \\-lkse)"),
        "libkvm" => Some(r"Kernel Data Access Library (libkvm, \\-lkvm)"),
        "libm" => Some(r"Math Library (libm, \\-lm)"),
        "libm68k" => Some(r"m68k Architecture Library (libm68k, \\-lm68k)"),
        "libmagic" => Some(r"Magic Number Recognition Library (libmagic, \\-lmagic)"),
        "libmandoc" => Some(r"Mandoc Macro Compiler Library (libmandoc, \\-lmandoc)"),
        "libmd" => Some(r"Message Digest (MD4, MD5, etc.) Support Library (libmd, \\-lmd)"),
        "libmemstat" => {
            Some(r"Kernel Memory Allocator Statistics Library (libmemstat, \\-lmemstat)")
        }
        "libmenu" => Some(r"Curses Menu Library (libmenu, \\-lmenu)"),
        "libmj" => Some(r"Minimalist JSON library (libmj, \\-lmj)"),
        "libnetgraph" => Some(r"Netgraph User Library (libnetgraph, \\-lnetgraph)"),
        "libnetpgp" => {
            Some(r"Netpgp Signing, Verification, Encryption and Decryption (libnetpgp, \\-lnetpgp)")
        }
        "libnetpgpverify" => Some(r"Netpgp Verification (libnetpgpverify, \\-lnetpgpverify)"),
        "libnpf" => Some(r"NPF Packet Filter Library (libnpf, \\-lnpf)"),
        "libnv" => Some(r"Name/value pairs library (libnv, \\-lnv)"),
        "libossaudio" => Some(r"OSS Audio Emulation Library (libossaudio, \\-lossaudio)"),
        "libpam" => Some(r"Pluggable Authentication Module Library (libpam, \\-lpam)"),
        "libpanel" => Some(r"Z-order for curses windows (libpanel, \\-lpanel)"),
        "libpcap" => Some(r"Packet capture Library (libpcap, \\-lpcap)"),
        "libpci" => Some(r"PCI Bus Access Library (libpci, \\-lpci)"),
        "libpmc" => Some(r"Performance Counters Library (libpmc, \\-lpmc)"),
        "libppath" => Some(r"Property-List Paths Library (libppath, \\-lppath)"),
        "libposix" => Some(r"POSIX Compatibility Library (libposix, \\-lposix)"),
        "libposix1e" => Some(r"POSIX.1e Security API Library (libposix1e, \\-lposix1e)"),
        "libproc" => Some(r"Processor Monitoring and Analysis Library (libproc, \\-lproc)"),
        "libprocstat" => {
            Some(r"Process and Files Information Retrieval (libprocstat, \\-lprocstat)")
        }
        "libprop" => Some(r"Property Container Object Library (libprop, \\-lprop)"),
        "libpthread" => Some(r"POSIX Threads Library (libpthread, \\-lpthread)"),
        "libpthread_dbg" => Some(r"POSIX Threads Library (libpthread_dbg, \\-lpthread_dbg)"),
        "libpuffs" => Some(r"puffs Convenience Library (libpuffs, \\-lpuffs)"),
        "libquota" => Some(r"Disk Quota Access Library (libquota, \\-lquota)"),
        "libradius" => Some(r"RADIUS Client Library (libradius, \\-lradius)"),
        "librefuse" => {
            Some(r"File System in Userspace Convenience Library (librefuse, \\-lrefuse)")
        }
        "libresolv" => Some(r"DNS Resolver Library (libresolv, \\-lresolv)"),
        "librpcsec_gss" => {
            Some(r"RPC GSS-API Authentication Library (librpcsec_gss, \\-lrpcsec_gss)")
        }
        "librpcsvc" => Some(r"RPC Service Library (librpcsvc, \\-lrpcsvc)"),
        "librt" => Some(r"POSIX Real\\-time Library (librt, \\-lrt)"),
        "librtld_db" => {
            Some(r"Debugging interface to the runtime linker Library (librtld_db, \\-lrtld_db)")
        }
        "librumpclient" => Some(
            r"Clientside Stubs for rump Kernel Remote Protocols (librumpclient, \\-lrumpclient)",
        ),
        "libsaslc" => {
            Some(r"Simple Authentication and Security Layer client library (libsaslc, \\-lsaslc)")
        }
        "libsbuf" => Some(r"Safe String Composition Library (libsbuf, \\-lsbuf)"),
        "libsdp" => Some(r"Bluetooth Service Discovery Protocol User Library (libsdp, \\-lsdp)"),
        "libssp" => Some(r"Buffer Overflow Protection Library (libssp, \\-lssp)"),
        "libstand" => Some(r"Standalone Applications Library (libstand, \\-lstand)"),
        "libstdthreads" => Some(r"C11 Threads Library (libstdthreads, \\-lstdthreads)"),
        "libSystem" => Some(r"System Library (libSystem, \\-lSystem)"),
        "libsysdecode" => Some(r"System Argument Decoding Library (libsysdecode, \\-lsysdecode)"),
        "libtacplus" => Some(r"TACACS+ Client Library (libtacplus, \\-ltacplus)"),
        "libtcplay" => Some(r"TrueCrypt-compatible API library (libtcplay, \\-ltcplay)"),
        "libtermcap" => Some(r"Termcap Access Library (libtermcap, \\-ltermcap)"),
        "libterminfo" => Some(r"Terminfo Access Library (libterminfo, \\-lterminfo)"),
        "libthr" => Some(r"1:1 Threading Library (libthr, \\-lthr)"),
        "libufs" => Some(r"UFS File System Access Library (libufs, \\-lufs)"),
        "libugidfw" => Some(r"Userland Firewall Library (libugidfw, \\-lugidfw)"),
        "libulog" => Some(r"User Login Record Library (libulog, \\-lulog)"),
        "libusbhid" => Some(r"USB Human Interface Devices Library (libusbhid, \\-lusbhid)"),
        "libutil" => Some(r"System Utilities Library (libutil, \\-lutil)"),
        "libvgl" => Some(r"Video Graphics Library (libvgl, \\-lvgl)"),
        "libx86_64" => Some(r"x86_64 Architecture Library (libx86_64, \\-lx86_64)"),
        "libxo" => Some(r"Text, XML, JSON, and HTML Output Emission Library (libxo, \\-lxo)"),
        "libz" => Some(r"Compression Library (libz, \\-lz)"),
        _ => None,
    }
}

/// Mirror `post_delim()` for mdoc constructs whose final phrase must leave
/// punctuation in outer flow.  `Nd` owns one joined Body phrase, so the
/// diagnostic prints that complete phrase rather than the abbreviated
/// `... tail` form used by macros with separately owned arguments.  A closing
/// parenthesis is explicitly accepted by upstream for description text.
fn validate_attached_trailing_delimiter(
    builder: &DocumentBuilder,
    node: NodeId,
    macro_name: &'static str,
    recoveries: &mut Vec<Recovery>,
) {
    let Some(children) = builder.children(node) else {
        return;
    };
    let Some(last) = children.last().copied() else {
        return;
    };
    let Some(text) = builder.node_text(last) else {
        return;
    };
    let Some((delimiter_index, delimiter)) = text.char_indices().last() else {
        return;
    };
    if delimiter == ')' || !is_mdoc_closing_delimiter(&text[delimiter_index..]) {
        return;
    }
    let Some(location) = builder.node_location(last).and_then(|span| {
        span.end
            .checked_sub(u32::try_from(delimiter.len_utf8()).ok()?)
            .and_then(|start| SourceSpan::new(span.source, start, span.end).ok())
    }) else {
        return;
    };
    let display = children
        .iter()
        .filter_map(|child| builder.node_text(*child))
        .collect::<Vec<_>>()
        .join(" ");
    if display.is_empty() {
        return;
    }
    recoveries.push(Recovery::TrailingDelimiter {
        macro_name,
        display: display.into(),
        location: Some(location),
    });
}

/// Complete the delayed `post_nd()` delimiter validation for all descriptions
/// ended by the current structural boundary.  An `.Nd` owns both its control
/// line and following physical prose, so checking it at declaration time
/// misses punctuation attached to the final following text line.
fn flush_pending_nd_delimiters(
    builder: &DocumentBuilder,
    bodies: &mut Vec<NodeId>,
    recoveries: &mut Vec<Recovery>,
) {
    for body in bodies.drain(..) {
        // `post_nd()` treats an empty Body as a recoverable missing
        // description, even though the Block/Head/Body shape remains part of
        // the public tree.  Delay this until the next boundary because
        // following physical prose belongs to the same Body.
        if builder.children(body).is_some_and(<[NodeId]>::is_empty) {
            recoveries.push(Recovery::MissingDescription {
                location: builder.node_location(body),
            });
            continue;
        }
        // A following paragraph stays in an `Nd` Body.  Once it contains a
        // later direct text phrase, `post_delim()` prints only that final
        // phrase with the legacy ellipsis marker instead of the control-line
        // argument preceding it.
        let trailing_text = builder.children(body).and_then(|children| {
            let first_text = children
                .iter()
                .copied()
                .find(|child| builder.node_kind(*child) == Some(NodeKind::Text))?;
            children
                .iter()
                .rev()
                .copied()
                .find(|child| builder.node_kind(*child) == Some(NodeKind::Text))
                .filter(|last_text| *last_text != first_text)
        });
        if let Some(text) = trailing_text {
            validate_nd_following_text_delimiter(builder, text, recoveries);
        } else {
            validate_attached_trailing_delimiter(builder, body, "Nd", recoveries);
        }
    }
}

/// Complete `post_sh_name()` after a `NAME` section has received every direct
/// child.  libmandoc deliberately does not descend through a partial block:
/// an `.Nm` or `.Nd` nested in `.Oo`, for example, remains invalid NAME
/// content rather than satisfying this section-level contract.
fn flush_pending_name_section(
    builder: &DocumentBuilder,
    section_body: &mut Option<NodeId>,
    recoveries: &mut Vec<Recovery>,
) {
    let Some(body) = section_body.take() else {
        return;
    };
    let Some(children) = builder.children(body) else {
        return;
    };
    let mut has_name = false;
    let mut has_description = false;
    let mut index = 0;
    while index < children.len() {
        let child = children[index];
        match builder.node_macro_name(child) {
            Some("Nm") => {
                if has_name {
                    let name = node_arguments(builder, child).join(" ");
                    recoveries.push(Recovery::NameSectionMissingComma {
                        name: name.into_boxed_str(),
                        location: builder.node_location(child),
                    });
                }
                has_name = true;
            }
            Some("Nd") => {
                has_description = true;
                if index + 1 < children.len() {
                    recoveries.push(Recovery::DescriptionNotAtEndOfName {
                        location: builder.node_location(child),
                    });
                }
                break;
            }
            _ if builder.node_kind(child) == Some(NodeKind::Text)
                && builder.node_text(child) == Some(",")
                && children
                    .get(index + 1)
                    .is_some_and(|next| builder.node_macro_name(*next) == Some("Nm")) =>
            {
                // `post_sh_name()` accepts the separating comma itself and
                // then proceeds to the name macro.
                index += 1;
                has_name = true;
            }
            _ => {
                let content = builder
                    .node_macro_name(child)
                    .map_or_else(|| "text".to_owned(), str::to_owned);
                recoveries.push(Recovery::BadNameSectionContent {
                    content: content.into_boxed_str(),
                    location: builder.node_location(child),
                });
            }
        }
        index += 1;
    }
    let location = builder.node_location(body);
    if !has_name {
        recoveries.push(Recovery::NameSectionMissingName {
            location: location.clone(),
        });
    }
    if !has_description {
        recoveries.push(Recovery::NameSectionMissingDescription { location });
    }
}

/// Complete `post_sh_authors()` after an `AUTHORS` section has received all
/// of its descendant blocks.  A nested author entry is sufficient, but an
/// option-only or empty `.An` is not.
fn flush_pending_authors_section(
    builder: &DocumentBuilder,
    authors_body: &mut Option<NodeId>,
    recoveries: &mut Vec<Recovery>,
) {
    let Some(body) = authors_body.take() else {
        return;
    };
    if contains_populated_author(builder, body) {
        return;
    }
    recoveries.push(Recovery::AuthorsSectionWithoutAuthor {
        location: builder.node_location(body),
    });
}

/// Iteratively mirror libmandoc's recursive `child_an()` predicate without
/// exposing parser input depth to the host call stack.
fn contains_populated_author(builder: &DocumentBuilder, root: NodeId) -> bool {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if builder.node_macro_name(node) == Some("An")
            && builder
                .children(node)
                .is_some_and(|children| !children.is_empty())
        {
            return true;
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().copied());
        }
    }
    false
}

/// Mirror `post_fname()` for `Fn` Elements and validated `Fo` Heads.  A name
/// wrapped as one complete parenthesized phrase is accepted; every other
/// parenthesis is a source-precise upstream warning.
fn validate_function_name(builder: &DocumentBuilder, node: NodeId, recoveries: &mut Vec<Recovery>) {
    let Some(name_node) = builder
        .children(node)
        .and_then(|children| children.first())
        .copied()
    else {
        return;
    };
    let Some(name) = builder.node_text(name_node) else {
        return;
    };
    let offset = if name.starts_with('(') {
        if name.ends_with(')') {
            return;
        }
        0
    } else {
        let Some(offset) = name.bytes().position(|byte| matches!(byte, b'(' | b')')) else {
            return;
        };
        offset
    };
    recoveries.push(Recovery::FunctionNameParenthesis {
        name: name.into(),
        location: text_offset_location(builder, name_node, offset),
    });
}

/// Mirror `post_fa()` for standalone arguments and the arguments carried by a
/// function declaration.  A comma after a callback or array opener is part of
/// that type expression; only the first earlier comma in each source phrase
/// is diagnosed.
fn validate_function_argument_commas(
    builder: &DocumentBuilder,
    node: NodeId,
    recoveries: &mut Vec<Recovery>,
) {
    let Some(children) = builder.children(node) else {
        return;
    };
    for child in children {
        let Some(argument) = builder.node_text(*child) else {
            continue;
        };
        let Some(offset) = argument
            .bytes()
            .position(|byte| matches!(byte, b',' | b'(' | b'{'))
        else {
            continue;
        };
        if argument.as_bytes().get(offset) != Some(&b',') {
            continue;
        }
        recoveries.push(Recovery::FunctionArgumentComma {
            argument: argument.into(),
            location: text_offset_location(builder, *child, offset),
        });
    }
}

/// Return a one-byte logical location inside a text node.  Scanner words may
/// share one physical control-line end, so callers must derive positions from
/// their retained logical start rather than from `SourceSpan::end`.
fn text_offset_location(
    builder: &DocumentBuilder,
    node: NodeId,
    offset: usize,
) -> Option<SourceSpan> {
    let span = builder.node_location(node)?;
    let base = builder.node_source_position(node)?;
    let offset = u32::try_from(offset).ok()?;
    let column = base.column.checked_add(offset)?;
    SourceSpan::new(span.source, span.start, span.end)
        .ok()
        .map(|span| {
            span.with_logical_start(crate::SourcePosition {
                line: base.line,
                column,
            })
        })
}

/// Validate the final physical prose phrase owned by a preceding `.Nd`.
fn validate_nd_following_text_delimiter(
    builder: &DocumentBuilder,
    node: NodeId,
    recoveries: &mut Vec<Recovery>,
) {
    let Some(text) = builder.node_text(node) else {
        return;
    };
    let Some((delimiter_index, delimiter)) = text.char_indices().last() else {
        return;
    };
    if delimiter == ')' || !is_mdoc_closing_delimiter(&text[delimiter_index..]) {
        return;
    }
    let Some(location) = builder.node_location(node).and_then(|span| {
        span.end
            .checked_sub(u32::try_from(delimiter.len_utf8()).ok()?)
            .and_then(|start| SourceSpan::new(span.source, start, span.end).ok())
    }) else {
        return;
    };
    recoveries.push(Recovery::TrailingDelimiter {
        macro_name: "Nd",
        display: format!("... {text}").into(),
        location: Some(location),
    });
}

/// Apply the standard AT&T UNIX spelling expansion and resume mdoc's inline
/// grammar after the selector.  `mandoc` retains the authored selector for a
/// known version (but hides it from rendering), while an unknown selector is
/// displayed after the generic generated prefix and reported as a warning.
fn validate_at(
    builder: &mut DocumentBuilder,
    node: NodeId,
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Vec<NodeId> {
    let Some(arguments) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return Vec::new();
    };
    let Some((&first, tail)) = arguments.split_first() else {
        // The no-argument `At` spelling has a public generated default,
        // rather than being an empty formatting request.  Insert it during
        // validation so an earlier `.de At` cannot replace the package result.
        if builder.node_count() < max_nodes {
            let _ = push_generated_text(builder, node, "AT&T UNIX", false);
        }
        return Vec::new();
    };
    let Some(argument) = builder.node_text(first).map(str::to_owned) else {
        return Vec::new();
    };

    let expanded = at_version(&argument);
    let generated = expanded.unwrap_or("AT&T UNIX");
    if expanded.is_none() {
        outcome.recoveries.push(Recovery::UnknownAtVersion {
            argument: argument.into(),
            location: builder.node_location(first),
        });
    }
    if builder.node_count() >= max_nodes {
        return Vec::new();
    }
    let Some(prefix) = push_generated_text_at(
        builder,
        node,
        generated,
        false,
        expanded.and(builder.node_location(first)),
    ) else {
        return Vec::new();
    };
    if expanded.is_some() {
        mark_no_print(builder, first);
    }
    let _ = builder.replace_children(node, &[prefix, first]);
    split_mdoc_inline_tokens(builder, node, tail, spacing_enabled, max_nodes, outcome)
}

/// Standard `.St` selectors and their public expansion text.
///
/// The table is part of mdoc's semantic contract, not a renderer alias: the
/// selector stays in the tree as a hidden authored child and this description
/// is a generated sibling. Keep the spellings pinned to the stable mandoc
/// baseline used by the compatibility corpus.
fn standard_description(selector: &str) -> Option<&'static str> {
    Some(match selector {
        "-p1003.1-88" => "IEEE Std 1003.1-1988 (\\(lqPOSIX.1\\(rq)",
        "-p1003.1-90" => "IEEE Std 1003.1-1990 (\\(lqPOSIX.1\\(rq)",
        "-p1003.1-96" | "-iso9945-1-96" => "ISO/IEC 9945-1:1996 (\\(lqPOSIX.1\\(rq)",
        "-p1003.1-2001" => "IEEE Std 1003.1-2001 (\\(lqPOSIX.1\\(rq)",
        "-p1003.1-2004" => "IEEE Std 1003.1-2004 (\\(lqPOSIX.1\\(rq)",
        "-p1003.1-2008" => "IEEE Std 1003.1-2008 (\\(lqPOSIX.1\\(rq)",
        "-p1003.1-2024" => "IEEE Std 1003.1-2024 (\\(lqPOSIX.1\\(rq)",
        "-p1003.1" => "IEEE Std 1003.1 (\\(lqPOSIX.1\\(rq)",
        "-p1003.1b" => "IEEE Std 1003.1b (\\(lqPOSIX.1b\\(rq)",
        "-p1003.1b-93" => "IEEE Std 1003.1b-1993 (\\(lqPOSIX.1b\\(rq)",
        "-p1003.1c-95" => "IEEE Std 1003.1c-1995 (\\(lqPOSIX.1c\\(rq)",
        "-p1003.1g-2000" => "IEEE Std 1003.1g-2000 (\\(lqPOSIX.1g\\(rq)",
        "-p1003.1i-95" => "IEEE Std 1003.1i-1995 (\\(lqPOSIX.1i\\(rq)",
        "-p1003.2" => "IEEE Std 1003.2 (\\(lqPOSIX.2\\(rq)",
        "-p1003.2-92" => "IEEE Std 1003.2-1992 (\\(lqPOSIX.2\\(rq)",
        "-p1003.2a-92" => "IEEE Std 1003.2a-1992 (\\(lqPOSIX.2a\\(rq)",
        "-isoC" | "-isoC-90" => "ISO/IEC 9899:1990 (\\(lqISO\\~C90\\(rq)",
        "-isoC-amd1" => "ISO/IEC 9899/AMD1:1995 (\\(lqISO\\~C90, Amendment 1\\(rq)",
        "-isoC-tcor1" => "ISO/IEC 9899/TCOR1:1994 (\\(lqISO\\~C90, Technical Corrigendum 1\\(rq)",
        "-isoC-tcor2" => "ISO/IEC 9899/TCOR2:1995 (\\(lqISO\\~C90, Technical Corrigendum 2\\(rq)",
        "-isoC-99" => "ISO/IEC 9899:1999 (\\(lqISO\\~C99\\(rq)",
        "-isoC-2011" => "ISO/IEC 9899:2011 (\\(lqISO\\~C11\\(rq)",
        "-isoC-2023" => "ISO/IEC 9899:2024 (\\(lqISO\\~C23\\(rq)",
        "-iso9945-1-90" => "ISO/IEC 9945-1:1990 (\\(lqPOSIX.1\\(rq)",
        "-iso9945-2-93" => "ISO/IEC 9945-2:1993 (\\(lqPOSIX.2\\(rq)",
        "-ansiC" | "-ansiC-89" => "ANSI X3.159-1989 (\\(lqANSI\\~C89\\(rq)",
        "-ieee754" => "IEEE Std 754-1985",
        "-iso8802-3" => "ISO 8802-3: 1989",
        "-iso8601" => "ISO 8601",
        "-ieee1275-94" => "IEEE Std 1275-1994 (\\(lqOpen Firmware\\(rq)",
        "-xpg3" => "X/Open Portability Guide Issue\\~3 (\\(lqXPG3\\(rq)",
        "-xpg4" => "X/Open Portability Guide Issue\\~4 (\\(lqXPG4\\(rq)",
        "-xpg4.2" => "X/Open Portability Guide Issue\\~4, Version\\~2 (\\(lqXPG4.2\\(rq)",
        "-xbd5" => "X/Open Base Definitions Issue\\~5 (\\(lqXBD5\\(rq)",
        "-xcu5" => "X/Open Commands and Utilities Issue\\~5 (\\(lqXCU5\\(rq)",
        "-xsh4.2" => {
            "X/Open System Interfaces and Headers Issue\\~4, Version\\~2 (\\(lqXSH4.2\\(rq)"
        }
        "-xsh5" => "X/Open System Interfaces and Headers Issue\\~5 (\\(lqXSH5\\(rq)",
        "-xns5" => "X/Open Networking Services Issue\\~5 (\\(lqXNS5\\(rq)",
        "-xns5.2" => "X/Open Networking Services Issue\\~5.2 (\\(lqXNS5.2\\(rq)",
        "-xcurses4.2" => "X/Open Curses Issue\\~4, Version\\~2 (\\(lqXCURSES4.2\\(rq)",
        "-susv1" => "Version\\~1 of the Single UNIX Specification (\\(lqSUSv1\\(rq)",
        "-susv2" => "Version\\~2 of the Single UNIX Specification (\\(lqSUSv2\\(rq)",
        "-susv3" => "Version\\~3 of the Single UNIX Specification (\\(lqSUSv3\\(rq)",
        "-susv4" => "Version\\~4 of the Single UNIX Specification (\\(lqSUSv4\\(rq)",
        "-svid4" => "System\\~V Interface Definition, Fourth Edition (\\(lqSVID4\\(rq)",
        _ => return None,
    })
}

/// Standard `.At` selectors and their public expansion text.
fn at_version(argument: &str) -> Option<&'static str> {
    Some(match argument {
        "v1" => "Version\\~1 AT&T UNIX",
        "v2" => "Version\\~2 AT&T UNIX",
        "v3" => "Version\\~3 AT&T UNIX",
        "v4" => "Version\\~4 AT&T UNIX",
        "v5" => "Version\\~5 AT&T UNIX",
        "v6" => "Version\\~6 AT&T UNIX",
        "v7" => "Version\\~7 AT&T UNIX",
        "32v" => "Version\\~7 AT&T UNIX/32V",
        "III" => "AT&T System\\~III UNIX",
        "V" => "AT&T System\\~V UNIX",
        "V.1" => "AT&T System\\~V Release\\~1 UNIX",
        "V.2" => "AT&T System\\~V Release\\~2 UNIX",
        "V.3" => "AT&T System\\~V Release\\~3 UNIX",
        "V.4" => "AT&T System\\~V Release\\~4 UNIX",
        _ => return None,
    })
}

/// mdoc 的 `.Fd` 及文本型 `.Fl`/`.Sy`/`.Ar`/`.Em` 按展开后的前序参数宽度定位后续参数。
/// 扫描器保留原始跨度，故只在已证实的宏语义中重定位公开 AST。
fn rebase_expanded_argument_locations(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(arguments) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return;
    };
    let mut prior_delta = 0_i32;
    for argument in arguments {
        if prior_delta != 0 && builder.node_location(argument).is_some() {
            rebase_expanded_subtree_locations(builder, argument, prior_delta);
        }
        prior_delta =
            prior_delta.saturating_add(builder.node_argument_expansion_width_delta(argument));
    }
}

/// An expanded control-line argument can contain a nested inline macro.  The
/// nested node and all of its public descendants inherit the preceding
/// expansion width even when only its direct parent owns the escape spelling.
fn rebase_expanded_subtree_locations(builder: &mut DocumentBuilder, root: NodeId, delta: i32) {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if let Some(mut location) = builder.node_location(node) {
            let start = location.start.saturating_add_signed(delta);
            // Public spans remain within authored source bytes: only rebase
            // the logical start when it stays before the lexical end.
            if start <= location.end {
                location.start = start;
                let _ = builder.set_node_location(node, Some(location));
            }
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().copied());
        }
    }
}

/// Apply roff expansion width to the completed `Op` subtree. Option bodies
/// can acquire nested callable macros only during mdoc restructuring, so the
/// scanner-stage argument rebase cannot make their descendants inherit a
/// preceding string expansion.
fn rebase_option_expansion_locations(builder: &mut DocumentBuilder, root: NodeId) {
    let mut option_roots = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node != root
            && builder.node_kind(node) == Some(NodeKind::Block)
            && builder.node_macro_name(node) == Some("Op")
        {
            option_roots.push(node);
            continue;
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().copied());
        }
    }

    for option in option_roots {
        rebase_completed_option_locations(builder, option);
    }
}

fn rebase_completed_option_locations(builder: &mut DocumentBuilder, option: NodeId) {
    let mut entries = Vec::new();
    let mut pending = vec![option];
    while let Some(node) = pending.pop() {
        if let Some(location) = builder.node_location(node)
            && let Ok(physical) = SourceSpan::new(location.source, location.start, location.end)
            && let Some(position) = builder.source_position(&physical)
        {
            entries.push((node, physical, position));
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().copied());
        }
    }
    entries.sort_by_key(|(_, location, _)| (location.source, location.start, location.end));

    let mut deltas = BTreeMap::<(crate::SourceId, u32), i32>::new();
    for (node, location, position) in entries {
        let delta = *deltas.entry((location.source, position.line)).or_default();
        if delta != 0 {
            let column = position.column.saturating_add_signed(delta);
            let _ = builder.set_node_logical_start(
                node,
                crate::SourcePosition {
                    line: position.line,
                    column,
                },
            );
        }
        if builder.node_kind(node) == Some(NodeKind::Text) {
            let entry = deltas.entry((location.source, position.line)).or_default();
            *entry = entry.saturating_add(builder.node_argument_expansion_width_delta(node));
        }
    }
}

/// Recombine a semantic line argument from scanner-level lexical tokens.
///
/// The scanner intentionally tokenizes every control line so that the roff
/// executor can apply expansion safely.  Some mdoc macros (`Dd`, `Nd`) take
/// one complete line argument in libmandoc's public AST. Reusing the first
/// temporary child keeps its source position and bounded arena allocation.
fn coalesce_text_children(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(children) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return;
    };
    let Some(&first) = children.first() else {
        return;
    };
    if children.len() == 1 {
        return;
    }
    let value = children
        .iter()
        .filter_map(|child| builder.node_text(*child))
        .collect::<Vec<_>>()
        .join(" ");
    if builder.text(first, value) {
        let _ = builder.replace_children(node, &[first]);
    }
}

/// Merge adjacent direct text children while preserving macro boundaries.
/// Scanner tokens are deliberately word-sized; partial mdoc blocks expose a
/// complete text run as one owned-AST text node between any callable elements.
fn coalesce_adjacent_text_children(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(children) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return;
    };
    let mut merged = Vec::with_capacity(children.len());
    let mut text_run = None::<NodeId>;
    for child in children {
        if let Some(text) = builder.node_text(child).map(str::to_owned) {
            if let Some(first) = text_run {
                let Some(existing) = builder.node_text(first) else {
                    continue;
                };
                let value = format!("{existing} {text}");
                let _ = builder.text(first, value);
            } else {
                merged.push(child);
                text_run = Some(child);
            }
        } else {
            merged.push(child);
            text_run = None;
        }
    }
    let _ = builder.replace_children(node, &merged);
}

/// When an implicit partial contains another implicit partial, a crossed
/// explicit closer belongs to that innermost construct.  The scanner exposes
/// the closer as a direct child of the outer argument run, so repair the
/// ownership after recursive block construction and before phrase coalescing.
fn relocate_crossed_closer_to_nested_implicit_body(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    closer_body: NodeId,
) -> Option<NodeId> {
    let children = builder.children(parent)?.to_vec();
    let closer_index = children.iter().position(|child| *child == closer_body)?;
    let previous = *children.get(closer_index.checked_sub(1)?)?;
    if builder.node_kind(previous) != Some(NodeKind::Block)
        || !builder
            .node_macro_name(previous)
            .is_some_and(is_implicit_partial_block_macro)
    {
        return None;
    }
    let nested_body = builder.children(previous)?.iter().copied().find(|child| {
        builder.node_kind(*child) == Some(NodeKind::Body)
            && builder.node_macro_name(*child) == builder.node_macro_name(previous)
    })?;
    let mut nested_children = builder.children(nested_body)?.to_vec();
    nested_children.push(closer_body);
    let mut cursor = closer_index + 1;
    while let Some(child) = children.get(cursor) {
        if builder.node_kind(*child) != Some(NodeKind::Text) {
            break;
        }
        nested_children.push(*child);
        cursor += 1;
    }
    let mut retained = children[..closer_index].to_vec();
    retained.extend_from_slice(&children[cursor..]);
    if builder.replace_children(nested_body, &nested_children)
        && builder.replace_children(parent, &retained)
    {
        Some(nested_body)
    } else {
        None
    }
}

/// Merge the direct text run that resumes after a structural recovery marker.
/// The words before the marker retain their scanner-visible boundaries; mdoc
/// uses that distinction for an implicit partial block interrupted by an
/// explicit closer (`.Op first Dc resumed words`).
fn coalesce_text_children_after(builder: &mut DocumentBuilder, node: NodeId, marker: NodeId) {
    let Some(children) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return;
    };
    let Some(marker_index) = children.iter().position(|child| *child == marker) else {
        return;
    };
    let mut merged = children[..=marker_index].to_vec();
    let mut text_run = None::<NodeId>;
    for child in &children[marker_index + 1..] {
        if let Some(text) = builder.node_text(*child).map(str::to_owned) {
            if let Some(first) = text_run {
                let Some(existing) = builder.node_text(first) else {
                    continue;
                };
                let value = format!("{existing} {text}");
                let _ = builder.text(first, value);
            } else {
                merged.push(*child);
                text_run = Some(*child);
            }
        } else {
            merged.push(*child);
            text_run = None;
        }
    }
    let _ = builder.replace_children(node, &merged);
}

/// Merge ordinary implicit-partial body prose without crossing an authored
/// mdoc delimiter.  Those delimiters are independently observable nodes even
/// when they occur within a body (`.Op a | z`), while ordinary word runs keep
/// their phrase representation (`.Op now optional`).
fn coalesce_implicit_partial_body_text(builder: &mut DocumentBuilder, node: NodeId) {
    // SYNOPSIS keeps the scanner's individual argument nodes.  This matters
    // for partial blocks emitted after an explicit closer as well as ordinary
    // source-order implicit blocks (`.Pq one line` remains two text nodes).
    if builder
        .node_flags(node)
        .is_some_and(|flags| flags.synopsis_pretty)
    {
        return;
    }
    let Some(children) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return;
    };
    let mut merged = Vec::with_capacity(children.len());
    let mut text_run = None::<NodeId>;
    for child in children {
        let delimiter = builder.node_text(child).is_some_and(|text| {
            matches!(text, "(" | "[")
                || is_mdoc_middle_delimiter(text)
                || is_mdoc_closing_delimiter(text)
        });
        if delimiter {
            merged.push(child);
            text_run = None;
        } else if let Some(text) = builder.node_text(child).map(str::to_owned) {
            if let Some(first) = text_run {
                let Some(existing) = builder.node_text(first) else {
                    continue;
                };
                let value = format!("{existing} {text}");
                let _ = builder.text(first, value);
            } else {
                merged.push(child);
                text_run = Some(child);
            }
        } else {
            merged.push(child);
            text_run = None;
        }
    }
    let _ = builder.replace_children(node, &merged);
}

/// Reconstruct mdoc's `ARGS_PHRASE` partition for one-line displays.
///
/// `D1` and `Dl` preserve their first doubled separator as a public
/// text-node boundary. The legacy `ARGS_PPHRASE` mode then treats all
/// remaining ordinary arguments as one phrase (including later doubled
/// separators). Inline macro elements deliberately terminate a phrase run
/// rather than being folded through as text.
fn coalesce_mdoc_display_phrases(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(children) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return;
    };
    let mut rebuilt = Vec::with_capacity(children.len());
    let mut phrase = None::<NodeId>;
    let mut previous_text = None::<NodeId>;
    let mut phrase_boundary_seen = false;
    for child in children {
        if let Some(text) = builder.node_text(child).map(str::to_owned) {
            // Display phrases coalesce ordinary words, but the inline splitter
            // has already classified mdoc delimiters.  Folding `(` and `)`
            // into a text phrase erases their no-space flags and turns
            // `.Dl name ( ) command` into `name ( ) command` instead of the
            // legacy `name () command`.
            if matches!(text.as_str(), "(" | "[")
                || is_mdoc_middle_delimiter(&text)
                || is_mdoc_closing_delimiter(&text)
            {
                rebuilt.push(child);
                phrase = None;
                previous_text = None;
                continue;
            }
            let phrase_break = previous_text.is_none_or(|previous| {
                !phrase_boundary_seen && builder.node_separator_width(previous) >= 2
            });
            if phrase_break {
                if let Some(previous) = previous_text
                    && !phrase_boundary_seen
                    && builder.node_separator_width(previous) >= 2
                {
                    // The mdoc argument parser attaches the source location
                    // of the separating whitespace to the second phrase.
                    // Scanner tokens begin at the following word, so repair
                    // that private provenance before freezing the public AST.
                    if let (Some(previous_location), Some(mut location)) = (
                        builder.node_location(previous),
                        builder.node_location(child),
                    ) {
                        let width = builder.node_text(previous).map_or(0, |value| {
                            u32::try_from(value.len())
                                .expect("source text length fits public u32 spans")
                        });
                        location.start = previous_location.start.saturating_add(width);
                        let _ = builder.set_node_location(child, Some(location));
                    }
                    phrase_boundary_seen = true;
                }
                rebuilt.push(child);
                phrase = Some(child);
            } else if let Some(first) = phrase {
                let Some(existing) = builder.node_text(first) else {
                    continue;
                };
                let value = format!("{existing} {text}");
                let _ = builder.text(first, value);
            }
            previous_text = Some(child);
        } else {
            rebuilt.push(child);
            phrase = None;
            previous_text = None;
        }
    }
    let _ = builder.replace_children(node, &rebuilt);
}

/// mdoc's `Fl` is variadic, but each ordinary argument owns a distinct output
/// element (and therefore a distinct rendered dash). Opening delimiters and a
/// vertical-bar argument stay in outer flow rather than becoming flags.
#[allow(clippy::too_many_lines)] // `Fl` preserves several delimiter and recovery edge cases in one pass.
fn expand_fl_elements(
    builder: &mut DocumentBuilder,
    allocation_parent: NodeId,
    nodes: Vec<NodeId>,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Vec<NodeId> {
    let mut expanded = Vec::with_capacity(nodes.len());
    for node in nodes {
        if builder.node_macro_name(node) != Some("Fl") {
            expanded.push(node);
            continue;
        }
        let arguments = builder
            .children(node)
            .map(<[NodeId]>::to_vec)
            .unwrap_or_default();
        if arguments
            .first()
            .is_some_and(|argument| builder.node_text(*argument) == Some("Es"))
        {
            let enclosure = arguments[0];
            let enclosure_arguments = &arguments[1..];
            if !builder.replace_children(node, &[])
                || !builder.clear_node_text(enclosure)
                || !builder.set_node_kind(enclosure, NodeKind::Element)
                || !builder.macro_name(enclosure, "Es")
                || !builder.replace_children(enclosure, enclosure_arguments)
            {
                expanded.push(node);
                continue;
            }
            outcome.recoveries.push(Recovery::Obsolete {
                macro_name: "Es",
                location: builder.node_location(enclosure),
            });
            expanded.push(node);
            expanded.push(enclosure);
            continue;
        }
        let argument_count = arguments
            .iter()
            .filter(|argument| {
                builder
                    .node_text(**argument)
                    .is_none_or(|text| text != "|" && !matches!(text, "(" | "["))
            })
            .count();
        let leading_separator = arguments
            .first()
            .is_some_and(|argument| builder.node_text(*argument) == Some("|"));
        let flag_count = argument_count.saturating_add(usize::from(leading_separator));
        let has_opening_delimiter = arguments.iter().any(|argument| {
            builder
                .node_text(*argument)
                .is_some_and(|text| matches!(text, "(" | "["))
        });
        if flag_count <= 1 && !has_opening_delimiter {
            expanded.push(node);
            continue;
        }
        let additional = flag_count.saturating_sub(1);
        if builder.node_count().saturating_add(additional) > max_nodes {
            if outcome.node_limit_location.is_none() {
                outcome.node_limit_location = builder.node_location(node);
            }
            expanded.push(node);
            continue;
        }

        let location = builder.node_location(node);
        let mut inherited_flags = builder.node_flags(node).unwrap_or_default();
        let mut first = !leading_separator;
        if leading_separator {
            let _ = builder.replace_children(node, &[]);
            expanded.push(node);
        }
        for argument in arguments {
            if builder.node_text(argument) == Some("|") {
                expanded.push(argument);
                continue;
            }
            if builder
                .node_text(argument)
                .is_some_and(|text| matches!(text, "(" | "["))
            {
                let delimiter_text = builder.node_text(argument).map(str::to_owned);
                mark_opening_delimiter(builder, argument, delimiter_text.as_deref());
                if expanded.is_empty() {
                    transfer_line_start(builder, node, argument);
                    if let Some(mut flags) = builder.node_flags(node) {
                        flags.line_start = false;
                        let _ = builder.set_node_flags(node, flags);
                    }
                    inherited_flags.line_start = false;
                }
                expanded.push(argument);
                continue;
            }
            let flag = if first {
                first = false;
                node
            } else {
                let Some(flag) = builder.push(allocation_parent, NodeKind::Element) else {
                    continue;
                };
                inherited_flags.line_start = false;
                let _ = builder.macro_name(flag, "Fl");
                let _ = builder.set_node_location(flag, location.clone());
                let _ = builder.set_node_flags(flag, inherited_flags);
                flag
            };
            let _ = builder.replace_children(flag, &[argument]);
            expanded.push(flag);
        }
    }
    expanded
}

/// An mdoc list-term spelling of `Fl Fl long` is a single long-option
/// element.  The first `Fl` is a semantic prefix, not a separately rendered
/// empty flag; `mdoc_validate.c` therefore retains the second element and
/// gives its word the escaped leading dash.  Keep this deliberately local to
/// already split list heads so ordinary adjacent inline macros retain their
/// own source structure.
fn collapse_long_option_prefixes(builder: &mut DocumentBuilder, nodes: &[NodeId]) -> Vec<NodeId> {
    let mut collapsed = Vec::with_capacity(nodes.len());
    let mut index = 0;
    while index < nodes.len() {
        let Some(next) = nodes.get(index.saturating_add(1)).copied() else {
            collapsed.push(nodes[index]);
            break;
        };
        let current = nodes[index];
        let is_empty_prefix = builder.node_macro_name(current) == Some("Fl")
            && builder.children(current).is_some_and(<[NodeId]>::is_empty);
        let Some(text) = (builder.node_macro_name(next) == Some("Fl"))
            .then(|| {
                builder
                    .children(next)
                    .and_then(|children| children.first().copied())
            })
            .flatten()
            .and_then(|child| builder.node_text(child).map(str::to_owned))
        else {
            collapsed.push(current);
            index = index.saturating_add(1);
            continue;
        };
        if !is_empty_prefix || text.is_empty() {
            collapsed.push(current);
            index = index.saturating_add(1);
            continue;
        }
        if let Some(child) = builder
            .children(next)
            .and_then(|children| children.first().copied())
        {
            let _ = builder.text(child, format!("\\-{text}"));
            collapsed.push(next);
            index = index.saturating_add(2);
        } else {
            collapsed.push(current);
            index = index.saturating_add(1);
        }
    }
    collapsed
}

fn record_date(builder: &mut DocumentBuilder, node: NodeId, outcome: &mut StructureOutcome) {
    let values = node_arguments(builder, node);
    let date = values.join(" ");
    let location = builder
        .children(node)
        .and_then(|children| children.first().copied())
        .and_then(|argument| builder.node_location(argument));
    if date.is_empty() {
        outcome.recoveries.push(Recovery::DateMissing {
            location: builder.node_location(node),
        });
    } else if legacy_man_date(&date) {
        outcome.recoveries.push(Recovery::LegacyDate {
            date: date.clone().into_boxed_str(),
            location,
        });
    } else if !is_mdoc_date(&date) {
        outcome.recoveries.push(Recovery::DateUnparseable {
            date: date.clone().into_boxed_str(),
            location,
        });
    }
    builder.metadata_mut().date = Some(normalize_mdoc_date(&date).into_boxed_str());
}

fn is_mdoc_date(value: &str) -> bool {
    let value = value.trim();
    if value == "$Mdocdate$" {
        return true;
    }
    let value = value
        .strip_prefix("$Mdocdate: ")
        .and_then(|value| value.strip_suffix(" $"))
        .unwrap_or(value);
    let mut fields = value.split_whitespace();
    let Some(month) = fields.next().and_then(normalize_month) else {
        return false;
    };
    let Some(day) = fields
        .next()
        .and_then(|day| day.strip_suffix(',').unwrap_or(day).parse::<u8>().ok())
    else {
        return false;
    };
    let Some(year) = fields.next().and_then(|year| year.parse::<i32>().ok()) else {
        return false;
    };
    fields.next().is_none() && valid_calendar_day(month, day, year)
}

fn legacy_man_date(value: &str) -> bool {
    let mut fields = value.split('-');
    let (Some(year), Some(month), Some(day)) = (fields.next(), fields.next(), fields.next()) else {
        return false;
    };
    if fields.next().is_some() {
        return false;
    }
    if year.len() != 4 || month.len() != 2 || day.len() != 2 {
        return false;
    }
    let (Ok(year), Ok(month), Ok(day)) =
        (year.parse::<i32>(), month.parse::<u8>(), day.parse::<u8>())
    else {
        return false;
    };
    let month = match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => return false,
    };
    valid_calendar_day(month, day, year)
}

/// Normalize the deterministic mdoc(7) date spellings accepted by mandoc.
///
/// `$Mdocdate$` intentionally remains literal: libmandoc expands that form
/// using wall-clock time, while native parsing must not consult host time.
fn normalize_mdoc_date(value: &str) -> String {
    let value = value.trim();
    let value = value
        .strip_prefix("$Mdocdate: ")
        .and_then(|value| value.strip_suffix(" $"))
        .unwrap_or(value);
    let mut fields = value.split_whitespace();
    let Some(month) = fields.next().and_then(normalize_month) else {
        return value.to_owned();
    };
    let Some(day) = fields
        .next()
        .and_then(|day| day.strip_suffix(',').unwrap_or(day).parse::<u8>().ok())
    else {
        return value.to_owned();
    };
    let Some(year) = fields.next().and_then(|year| year.parse::<i32>().ok()) else {
        return value.to_owned();
    };
    if fields.next().is_some() || !valid_calendar_day(month, day, year) {
        return value.to_owned();
    }
    format!("{month} {day}, {year:04}")
}

fn normalize_month(value: &str) -> Option<&'static str> {
    match value.get(..3)?.to_ascii_lowercase().as_str() {
        "jan" => Some("January"),
        "feb" => Some("February"),
        "mar" => Some("March"),
        "apr" => Some("April"),
        "may" => Some("May"),
        "jun" => Some("June"),
        "jul" => Some("July"),
        "aug" => Some("August"),
        "sep" => Some("September"),
        "oct" => Some("October"),
        "nov" => Some("November"),
        "dec" => Some("December"),
        _ => None,
    }
}

fn valid_calendar_day(month: &str, day: u8, year: i32) -> bool {
    let maximum = match month {
        "January" | "March" | "May" | "July" | "August" | "October" | "December" => 31,
        "April" | "June" | "September" | "November" => 30,
        "February" if year.rem_euclid(4) != 0 => 28,
        "February" if year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0 => 29,
        "February" => 28,
        _ => return false,
    };
    (1..=maximum).contains(&day)
}

fn record_title(builder: &mut DocumentBuilder, node: NodeId, outcome: &mut StructureOutcome) {
    let values = node_arguments(builder, node);
    if let Some((title, location)) = title_lowercase(builder, node) {
        outcome.recoveries.push(Recovery::TitleNotUppercase {
            title,
            location: Some(location),
        });
    }
    if let Some(argument) = values.get(3) {
        let location = builder
            .children(node)
            .and_then(|children| children.get(3).copied())
            .and_then(|argument_node| builder.node_location(argument_node));
        outcome.recoveries.push(Recovery::InvalidArguments {
            message: format!("skipping excess arguments: Dt ... {argument}").into(),
            location,
        });
    }
    let location = builder.node_location(node);
    let title = values
        .first()
        .filter(|title| !title.is_empty())
        .cloned()
        .unwrap_or_else(|| {
            outcome.recoveries.push(Recovery::MissingTitleArgument {
                location: location.clone(),
            });
            "UNTITLED".into()
        });
    let section = values.get(1).cloned();
    let volume = match section.as_deref() {
        Some(section) if let Some(volume) = default_volume(section) => volume.into_boxed_str(),
        Some(section) => {
            let location = builder
                .children(node)
                .and_then(|children| children.get(1).copied())
                .and_then(|argument| builder.node_location(argument));
            outcome.recoveries.push(Recovery::UnknownTitleSection {
                section: section.into(),
                location,
            });
            section.into()
        }
        None => {
            outcome.recoveries.push(Recovery::MissingTitleSection {
                title: title.clone().into_boxed_str(),
                location,
            });
            "LOCAL".into()
        }
    };
    let metadata = builder.metadata_mut();
    metadata.title = Some(title.into_boxed_str());
    metadata.section = section.map(String::into_boxed_str);
    metadata.volume = Some(volume);
    metadata.arch = values
        .get(2)
        .map(|value| value.to_ascii_lowercase().into_boxed_str());
}

fn title_lowercase(builder: &DocumentBuilder, title: NodeId) -> Option<(Box<str>, SourceSpan)> {
    let argument = builder.children(title)?.first().copied()?;
    let title = builder.node_text(argument)?;
    let offset = title.bytes().position(|byte| byte.is_ascii_lowercase())?;
    let location = builder.node_location(argument)?;
    let offset = u32::try_from(offset).ok()?;
    let start = location.start.checked_add(offset)?;
    let location = SourceSpan::new(location.source, start, start.saturating_add(1)).ok()?;
    Some((title.to_owned().into_boxed_str(), location))
}

fn record_operating_system(builder: &mut DocumentBuilder, node: NodeId) {
    let values = node_arguments(builder, node);
    if !values.is_empty() {
        builder.operating_system(values.join(" "));
    }
}

fn mdoc_operating_system_flavour(value: &str) -> &'static str {
    if value.contains("OpenBSD") {
        "OpenBSD"
    } else {
        // libmandoc retains the historical NetBSD validation label for an
        // arbitrary explicit `.Os` value, while only literal `NetBSD`
        // activates its Mdocdate/RCS companion checks.
        "NetBSD"
    }
}

fn record_name(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(value) = node_arguments(builder, node).into_iter().next() else {
        return;
    };
    if builder.metadata_mut().name.is_none() {
        // `.Nm` text keeps formatter spelling in the public AST, but document
        // metadata is the normalized lookup name.  In particular, `\\&` is a
        // zero-width no-break control and must not leak into `metadata.name`.
        builder.metadata_mut().name = Some(value.replace("\\&", "").into_boxed_str());
    }
}

fn mark_no_print(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(mut flags) = builder.node_flags(node) else {
        return;
    };
    flags.no_print = true;
    let _ = builder.set_node_flags(node, flags);
}

/// Propagate the formatter's synopsis presentation state without relying on
/// ambient formatter globals.  The structural pass uses an explicit stack so
/// a malformed but bounded tree never consumes the process stack.
fn mark_synopsis_pretty(builder: &mut DocumentBuilder, node: NodeId) {
    let mut pending = vec![node];
    while let Some(node) = pending.pop() {
        if let Some(mut flags) = builder.node_flags(node) {
            flags.synopsis_pretty = true;
            let _ = builder.set_node_flags(node, flags);
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().copied());
        }
    }
}

/// The execution-driven `nS` path differs subtly from `Sh SYNOPSIS`: a
/// generated fallback name remains generated prose rather than synopsis
/// presentation, even though its surrounding Nm block is synopsis-pretty.
fn clear_generated_synopsis_pretty_children(builder: &mut DocumentBuilder, node: NodeId) {
    let children = builder
        .children(node)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    for child in children {
        let Some(mut flags) = builder.node_flags(child) else {
            continue;
        };
        if flags.generated {
            flags.synopsis_pretty = false;
            let _ = builder.set_node_flags(child, flags);
        }
    }
}

fn mark_sentence_end(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(text) = builder.node_text(node) else {
        return;
    };
    let terminal = text.trim_end_matches(['"', '\'', ')', ']', '}']);
    if !terminal.ends_with(['.', '!', '?']) {
        return;
    }
    let Some(mut flags) = builder.node_flags(node) else {
        return;
    };
    flags.sentence_end = true;
    let _ = builder.set_node_flags(node, flags);
}

#[allow(clippy::too_many_lines)] // Mirrors mdoc's ordered list-option validation and recovery.
fn list_attributes(
    builder: &DocumentBuilder,
    node: NodeId,
    post_validation_recoveries: &mut Vec<Recovery>,
) -> BlockAttributes {
    let arguments = builder
        .children(node)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    let mut attributes = BlockAttributes {
        list_kind: Some(NormalizedListKind::Plain),
        list_type: "item",
        ..BlockAttributes::default()
    };
    // A list without an explicit type defaults to `-item`, which has no
    // normalized width.  Later type switches replace this policy just as
    // libmandoc's final list validator does.
    let mut width_rule = ListWidthRule::Drop;
    let mut selected_type = None::<&str>;
    let mut compact_seen = false;
    let mut offset_seen = false;
    let mut width_seen = false;
    let mut first_type_index = None;
    let mut last_width_argument = None;
    let mut index = 0;
    while let Some(argument) = arguments.get(index).copied() {
        let value = builder.node_text(argument).unwrap_or_default();
        let selected_list_type = matches!(
            value,
            "-bullet"
                | "-dash"
                | "-hyphen"
                | "-enum"
                | "-tag"
                | "-hang"
                | "-diag"
                | "-ohang"
                | "-inset"
                | "-column"
                | "-item"
        );
        let duplicate_list_type = selected_list_type && selected_type.is_some();
        if duplicate_list_type {
            post_validation_recoveries.push(Recovery::DuplicateListType {
                argument: match value {
                    "-bullet" => "-bullet",
                    "-dash" => "-dash",
                    "-hyphen" => "-hyphen",
                    "-enum" => "-enum",
                    "-tag" => "-tag",
                    "-hang" => "-hang",
                    "-diag" => "-diag",
                    "-ohang" => "-ohang",
                    "-inset" => "-inset",
                    "-column" => "-column",
                    "-item" => "-item",
                    _ => unreachable!("selected list type was matched above"),
                },
                location: builder.node_location(node),
            });
        }
        if selected_list_type && !duplicate_list_type {
            selected_type = Some(value);
            first_type_index = Some(index);
            attributes.list_type =
                list_type_name(value).expect("selected list type was matched above");
            attributes.terminal_hanging_list = value == "-hang";
            attributes.terminal_overhanging_list = value == "-ohang";
            attributes.terminal_inset_list = value == "-inset";
            attributes.terminal_diagnostic_list = value == "-diag";
            attributes.list_marker = match value {
                "-bullet" => Some(MdocListMarker::Bullet),
                "-dash" => Some(MdocListMarker::Dash),
                "-hyphen" => Some(MdocListMarker::Hyphen),
                "-enum" => Some(MdocListMarker::Enum),
                _ => None,
            };
        }
        attributes.list_kind = if duplicate_list_type {
            attributes.list_kind
        } else {
            match value {
                "-bullet" | "-dash" | "-hyphen" => {
                    width_rule = ListWidthRule::DefaultTwo;
                    Some(NormalizedListKind::Bullet)
                }
                "-enum" => {
                    width_rule = ListWidthRule::DefaultThree;
                    Some(NormalizedListKind::Ordered)
                }
                "-tag" => {
                    width_rule = ListWidthRule::DefaultSix;
                    Some(NormalizedListKind::Definition)
                }
                "-hang" => {
                    width_rule = ListWidthRule::Retain;
                    Some(NormalizedListKind::Definition)
                }
                "-diag" | "-ohang" | "-inset" => {
                    width_rule = ListWidthRule::Drop;
                    Some(NormalizedListKind::Definition)
                }
                "-column" => {
                    width_rule = ListWidthRule::Drop;
                    Some(NormalizedListKind::Column)
                }
                "-item" => {
                    width_rule = ListWidthRule::Drop;
                    Some(NormalizedListKind::Plain)
                }
                "-compact" => {
                    if compact_seen {
                        post_validation_recoveries.push(Recovery::DuplicateListArgument {
                            argument: "-compact".into(),
                            location: builder.node_location(argument),
                        });
                    }
                    compact_seen = true;
                    attributes.compact = true;
                    attributes.list_kind
                }
                "-offset" | "-width" => {
                    let option = value.trim_start_matches('-');
                    let value_argument = arguments.get(index + 1).copied().filter(|next| {
                        builder
                            .node_text(*next)
                            .is_some_and(|next| !is_list_option(next))
                    });
                    let seen = if value == "-offset" {
                        &mut offset_seen
                    } else {
                        &mut width_seen
                    };
                    if *seen {
                        let display = value_argument
                            .and_then(|next| builder.node_text(next))
                            .map_or_else(|| value.to_owned(), |next| format!("{value} {next}"));
                        post_validation_recoveries.push(Recovery::DuplicateListArgument {
                            argument: display.into_boxed_str(),
                            location: builder.node_location(argument),
                        });
                    }
                    *seen = true;
                    if value == "-width" {
                        last_width_argument = Some(argument);
                    }
                    if let Some(value_argument) = value_argument {
                        let normalized = builder
                            .node_text(value_argument)
                            .map(normalize_mdoc_layout_width);
                        if value == "-offset" {
                            attributes.offset = normalized;
                        } else {
                            attributes.width = normalized;
                        }
                    } else {
                        post_validation_recoveries.push(Recovery::EmptyListLayoutArgument {
                            option: if option == "offset" {
                                "offset"
                            } else {
                                "width"
                            },
                            location: builder.node_location(argument),
                        });
                        if value == "-width" {
                            attributes.width = Some("0n".to_owned());
                        }
                    }
                    attributes.list_kind
                }
                _ if value.starts_with('-') => {
                    post_validation_recoveries.push(Recovery::InvalidArguments {
                        message: format!("skipping excess arguments: Bl ... {value}").into(),
                        location: builder.node_location(argument),
                    });
                    attributes.list_kind
                }
                _ => attributes.list_kind,
            }
        };
        if matches!(value, "-offset" | "-width")
            && arguments.get(index + 1).is_some_and(|next| {
                builder
                    .node_text(*next)
                    .is_some_and(|next| !is_list_option(next))
            })
        {
            index += 1;
        }
        index += 1;
    }
    match first_type_index {
        Some(index) if index > 0 => {
            let first = arguments
                .first()
                .and_then(|argument| builder.node_text(*argument))
                .unwrap_or_default();
            post_validation_recoveries.push(Recovery::ListTypeNotFirst {
                argument: first.to_owned().into_boxed_str(),
                location: builder.node_location(node),
            });
        }
        None => {
            post_validation_recoveries.push(Recovery::MissingListType {
                location: builder.node_location(node),
            });
        }
        Some(_) => {}
    }
    match width_rule {
        ListWidthRule::Drop => {
            if attributes.width.is_some()
                && let Some(width) = last_width_argument
            {
                post_validation_recoveries.push(Recovery::SkippedListWidth {
                    list_type: attributes.list_type,
                    location: builder.node_location(width),
                });
            }
            attributes.width = None;
        }
        ListWidthRule::DefaultTwo if attributes.width.is_none() => {
            attributes.width = Some("2n".to_owned());
        }
        ListWidthRule::DefaultThree if attributes.width.is_none() => {
            attributes.width = Some("3n".to_owned());
        }
        ListWidthRule::DefaultSix if attributes.width.is_none() => {
            post_validation_recoveries.push(Recovery::MissingTagListWidth {
                location: builder.node_location(node),
            });
        }
        ListWidthRule::Retain
        | ListWidthRule::DefaultTwo
        | ListWidthRule::DefaultThree
        | ListWidthRule::DefaultSix => {}
    }
    if attributes.list_type == "column" {
        attributes.column_widths = column_declarations(builder, &arguments);
        attributes.column_count = Some(attributes.column_widths.len());
    }
    attributes
}

/// Retain the declaration phrases that mdoc associates with a column list.
///
/// libmandoc accepts further column labels after no-argument list options
/// such as `-compact`; only the single payload of `-width` and `-offset` is
/// excluded from the declaration.  Keeping this separate from generic option
/// validation avoids losing those labels when the public list Head is dropped.
fn column_declarations(builder: &DocumentBuilder, arguments: &[NodeId]) -> Vec<String> {
    let mut declarations = Vec::new();
    let mut index = 0_usize;
    while let Some(argument) = arguments.get(index).copied() {
        let value = builder.node_text(argument).unwrap_or_default();
        if matches!(value, "-width" | "-offset") {
            index += 2;
            continue;
        }
        if !value.starts_with('-') || builder.node_argument_quoted(argument) {
            declarations.push(value.to_owned());
        }
        index += 1;
    }
    declarations
}

fn is_list_option(value: &str) -> bool {
    matches!(
        value,
        "-bullet"
            | "-dash"
            | "-hyphen"
            | "-enum"
            | "-tag"
            | "-hang"
            | "-diag"
            | "-ohang"
            | "-inset"
            | "-column"
            | "-item"
            | "-compact"
            | "-offset"
            | "-width"
    )
}

fn list_type_name(value: &str) -> Option<&'static str> {
    match value {
        "-bullet" => Some("bullet"),
        "-dash" => Some("dash"),
        "-hyphen" => Some("hyphen"),
        "-enum" => Some("enum"),
        "-tag" => Some("tag"),
        "-hang" => Some("hang"),
        "-diag" => Some("diag"),
        "-ohang" => Some("ohang"),
        "-inset" => Some("inset"),
        "-column" => Some("column"),
        "-item" => Some("item"),
        _ => None,
    }
}

/// mdoc normalizes macro names in `-width` and `-offset` to the fixed
/// terminal-cell width assigned by `mdoc_validate.c`; this is a normalized
/// public field, while layout-option tokens are structural input syntax.
fn normalize_mdoc_layout_width(value: &str) -> String {
    let width = match value {
        "Ad" | "Ao" | "An" | "Aq" | "Ar" | "Bo" | "Bq" | "Cd" | "Dq" | "Dv" | "Eo" | "Fa"
        | "No" | "Pf" | "Po" | "Pq" | "Qo" | "So" | "Sq" | "Va" | "Vt" => Some(12),
        "Cm" | "Do" | "Em" | "Fl" | "Ic" | "Nm" | "Oo" | "Tn" | "Xr" => Some(10),
        "Er" => Some(17),
        "Ev" => Some(15),
        "Fo" | "Fn" | "Li" | "Ql" | "Sx" => Some(16),
        "Ds" | "Ms" | "Sy" => Some(6),
        "Op" => Some(14),
        "Pa" => Some(32),
        _ => None,
    };
    width.map_or_else(|| value.to_owned(), |width| format!("{width}n"))
}

fn display_attributes(
    builder: &DocumentBuilder,
    node: NodeId,
    immediate_recoveries: &mut Vec<Recovery>,
    post_validation_recoveries: &mut Vec<Recovery>,
) -> BlockAttributes {
    let arguments = builder
        .children(node)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    let mut attributes = BlockAttributes {
        ..BlockAttributes::default()
    };
    let mut index = 0;
    while let Some(argument) = arguments.get(index).copied() {
        let value = builder.node_text(argument).unwrap_or_default();
        match value {
            "-literal" | "-unfilled" | "-filled" | "-ragged" | "-centered" => {
                let display_kind = match value {
                    "-literal" | "-unfilled" => DisplayKind::Literal,
                    "-filled" | "-ragged" | "-centered" => DisplayKind::Filled,
                    _ => unreachable!("the display option was matched above"),
                };
                if attributes.display_kind.is_some() {
                    post_validation_recoveries.push(Recovery::DuplicateDisplayType {
                        argument: match value {
                            "-literal" => "literal",
                            "-unfilled" => "unfilled",
                            "-filled" => "filled",
                            "-ragged" => "ragged",
                            "-centered" => "centered",
                            _ => unreachable!("the display option was matched above"),
                        },
                        location: builder.node_location(node),
                    });
                } else {
                    attributes.display_kind = Some(display_kind);
                    attributes.literal_display = value == "-literal";
                    attributes.centered_display = value == "-centered";
                }
            }
            "-compact" => {
                if attributes.compact {
                    post_validation_recoveries.push(Recovery::DuplicateDisplayArgument {
                        argument: "-compact".into(),
                        location: builder.node_location(argument),
                    });
                }
                attributes.compact = true;
            }
            "-offset" => {
                let value = arguments
                    .get(index.saturating_add(1))
                    .and_then(|next| builder.node_text(*next))
                    .filter(|next| is_display_offset_value(next));
                let (offset, consumed) = if let Some(value) = value {
                    (Some(normalize_mdoc_layout_width(value)), true)
                } else {
                    post_validation_recoveries.push(Recovery::EmptyDisplayOffset {
                        location: builder.node_location(argument),
                    });
                    (None, false)
                };
                if let Some(offset) = offset {
                    if attributes.offset.is_some() {
                        post_validation_recoveries.push(Recovery::DuplicateDisplayArgument {
                            argument: format!("-offset {offset}").into(),
                            location: builder.node_location(argument),
                        });
                    }
                    attributes.offset = Some(offset);
                }
                index += usize::from(consumed);
            }
            "-file" => {
                post_validation_recoveries.push(Recovery::UnsupportedDisplayFile {
                    location: builder.node_location(node),
                });
                if arguments.get(index.saturating_add(1)).is_some() {
                    index += 1;
                }
            }
            _ if value.starts_with('-') => {
                immediate_recoveries.push(Recovery::InvalidArguments {
                    message: format!("skipping excess arguments: Bd ... {value}").into(),
                    location: builder.node_location(argument),
                });
                break;
            }
            _ => {}
        }
        index += 1;
    }
    if attributes.display_kind.is_none() {
        post_validation_recoveries.push(Recovery::MissingDisplayType {
            location: builder.node_location(node),
        });
        attributes.display_kind = Some(DisplayKind::Filled);
    }
    attributes
}

/// `-offset` accepts ordinary layout widths and signed numeric widths, but a
/// following named display option still starts a fresh option rather than
/// becoming its value.
fn is_display_offset_value(value: &str) -> bool {
    !value.starts_with('-') || value.as_bytes().get(1).is_some_and(u8::is_ascii_digit)
}

fn font_attributes(
    builder: &DocumentBuilder,
    node: NodeId,
    post_validation_recoveries: &mut Vec<Recovery>,
) -> BlockAttributes {
    let Some(first) = builder
        .children(node)
        .and_then(|arguments| arguments.first())
        .copied()
    else {
        post_validation_recoveries.push(Recovery::MissingFontType {
            location: builder.node_location(node),
        });
        return BlockAttributes::default();
    };
    let value = builder.node_text(first).unwrap_or_default();
    let arguments = builder.children(node).unwrap_or_default();
    let option_form = is_bf_option(value);
    let font = match value {
        "-emphasis" | "Em" => Some(NormalizedFont::Emphasis),
        "-literal" | "Li" => Some(NormalizedFont::Literal),
        "-symbolic" | "Sy" => Some(NormalizedFont::Symbolic),
        _ => {
            post_validation_recoveries.push(Recovery::UnknownFontType {
                argument: value.into(),
                location: builder.node_location(first),
            });
            None
        }
    };
    let excess = if option_form {
        arguments[1..]
            .iter()
            .copied()
            .find(|argument| !builder.node_text(*argument).is_some_and(is_bf_option))
    } else {
        arguments.get(1).copied()
    };
    if let Some(excess) = excess {
        post_validation_recoveries.push(Recovery::InvalidArguments {
            message: format!(
                "skipping excess arguments: Bf ... {}",
                builder.node_text(excess).unwrap_or_default()
            )
            .into(),
            location: builder.node_location(excess),
        });
    }
    BlockAttributes {
        font,
        ..BlockAttributes::default()
    }
}

fn is_bf_option(value: &str) -> bool {
    matches!(value, "-emphasis" | "-literal" | "-symbolic")
}

fn apply_attributes(builder: &mut DocumentBuilder, nodes: &[NodeId], attributes: &BlockAttributes) {
    for node in nodes {
        let _ = builder.set_node_list_kind(*node, attributes.list_kind);
        let _ = builder.set_node_list_marker(*node, attributes.list_marker);
        let _ = builder.set_node_column_widths(*node, attributes.column_widths.clone());
        let _ = builder.set_node_terminal_hanging_list(*node, attributes.terminal_hanging_list);
        let _ =
            builder.set_node_terminal_overhanging_list(*node, attributes.terminal_overhanging_list);
        let _ = builder.set_node_terminal_inset_list(*node, attributes.terminal_inset_list);
        let _ =
            builder.set_node_terminal_diagnostic_list(*node, attributes.terminal_diagnostic_list);
        let _ = builder.set_node_display_kind(*node, attributes.display_kind);
        let _ = builder.set_node_literal_display(*node, attributes.literal_display);
        let _ = builder.set_node_centered_display(*node, attributes.centered_display);
        let _ = builder.set_node_font(*node, attributes.font);
        let _ = builder.set_node_compact(*node, attributes.compact);
        if let Some(offset) = &attributes.offset {
            let _ = builder.set_node_offset(*node, offset.clone());
        }
        if let Some(width) = &attributes.width {
            let _ = builder.set_node_width(*node, width.clone());
        }
    }
}

fn mark_subtree_no_fill(builder: &mut DocumentBuilder, root: NodeId) {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if let Some(mut flags) = builder.node_flags(node) {
            flags.no_fill = true;
            let _ = builder.set_node_flags(node, flags);
        }
        // In mdoc literal displays, horizontal whitespace at the physical
        // line end is not part of the public text.  A whitespace-only line
        // therefore remains observable as an empty text node, while leading
        // indentation before a glyph is retained in the public AST.
        if builder.node_kind(node) == Some(NodeKind::Text)
            && let Some(normalized) = builder.node_text(node).and_then(|text| {
                let normalized = text.trim_end_matches([' ', '\t']);
                (normalized.len() != text.len()).then(|| normalized.to_owned())
            })
        {
            let _ = builder.text(node, normalized);
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().copied());
        }
    }
}

/// mdoc 填充文本不将物理行末空白发布到公开 AST；literal display 由其专用路径处理。
fn trim_mdoc_filled_text_trailing_whitespace(builder: &mut DocumentBuilder, flat: &[NodeId]) {
    for node in flat {
        if builder.node_kind(*node) != Some(NodeKind::Text)
            || builder.node_flags(*node).is_some_and(|flags| flags.no_fill)
        {
            continue;
        }
        let Some(normalized) = builder.node_text(*node).and_then(|text| {
            let normalized = text.trim_end_matches([' ', '\t']);
            (normalized.len() != text.len()).then(|| normalized.to_owned())
        }) else {
            continue;
        };
        let _ = builder.text(*node, normalized);
    }
}

/// Project the source-order `.nf`/`.fi` presentation state before structural
/// mdoc lowering moves scanner events under package blocks.  A display block
/// is its own fill-state boundary: `-unfilled` starts no-fill, `.fi` can turn
/// it off, and `.Ed` restores the state that preceded the display.  The
/// controlling request itself keeps its incoming state, while only `.nf`
/// arguments observe the new state.
fn apply_presentation_flags(builder: &mut DocumentBuilder, flat: &[NodeId]) {
    let mut no_fill = false;
    let mut display_fill_restore = Vec::new();
    for node in flat {
        match builder.node_macro_name(*node) {
            Some("Bd") => {
                if no_fill {
                    mark_subtree_no_fill(builder, *node);
                }
                display_fill_restore.push(no_fill);
                no_fill = display_is_unfilled(builder, *node);
            }
            Some("Ed") => {
                if no_fill {
                    mark_subtree_no_fill(builder, *node);
                }
                if let Some(previous) = display_fill_restore.pop() {
                    no_fill = previous;
                }
            }
            Some("nf" | "fi") => {
                if no_fill {
                    mark_node_no_fill(builder, *node);
                }
                no_fill = builder.node_macro_name(*node) == Some("nf");
                if no_fill {
                    mark_children_no_fill(builder, *node);
                }
            }
            _ if no_fill => mark_subtree_no_fill(builder, *node),
            _ => {}
        }
    }
}

/// The first recognized display type owns fill state; later type options are
/// validation errors and cannot change the already selected public display.
fn display_is_unfilled(builder: &DocumentBuilder, node: NodeId) -> bool {
    for argument in builder.children(node).into_iter().flatten() {
        match builder.node_text(*argument) {
            Some("-literal" | "-unfilled") => return true,
            Some("-filled" | "-ragged" | "-centered") => return false,
            _ => {}
        }
    }
    false
}

/// In filled mdoc input, a terminal `\c` followed by a blank physical line
/// recovers to ordinary text and omits the blank source event.  Literal
/// displays retain both nodes; their scanner flags are already established by
/// [`apply_presentation_flags`] when this runs.
fn suppress_filled_c_blank_lines(builder: &mut DocumentBuilder, flat: &[NodeId]) -> Vec<NodeId> {
    let mut suppressed = Vec::new();
    for pair in flat.windows(2) {
        let [text, blank] = pair else {
            continue;
        };
        if builder.node_kind(*text) != Some(NodeKind::Text)
            || builder.node_kind(*blank) != Some(NodeKind::Text)
            || builder.node_text(*blank) != Some("")
        {
            continue;
        }
        let Some(value) = builder.node_text(*text).map(str::to_owned) else {
            continue;
        };
        let Some(value) = value.strip_suffix("\\c") else {
            continue;
        };
        let Some(mut flags) = builder.node_flags(*text) else {
            continue;
        };
        if flags.no_fill || !flags.line_continuation || value.ends_with("\\z") {
            continue;
        }
        if builder.set_node_text(*text, value) {
            flags.line_continuation = false;
            let _ = builder.set_node_flags(*text, flags);
            suppressed.push(*blank);
        }
    }
    suppressed
}

/// Normalize physical blank source events to the semantic vertical-space
/// request used by mdoc in fill mode.  The scanner retains empty text so it
/// can preserve exact source positions; changing the existing arena record
/// rather than allocating a synthetic node retains that provenance and keeps
/// source-order validation deterministic.
fn normalize_filled_blank_lines(
    builder: &mut DocumentBuilder,
    flat: &[NodeId],
    suppressed: &[NodeId],
) -> BTreeMap<NodeId, Recovery> {
    let mut recoveries = BTreeMap::new();
    for node in flat {
        if suppressed.contains(node)
            || builder.node_kind(*node) != Some(NodeKind::Text)
            || builder.node_text(*node) != Some("")
            || builder.node_flags(*node).is_none_or(|flags| flags.no_fill)
        {
            continue;
        }
        let location = blank_line_location(builder, *node);
        if builder.set_node_kind(*node, NodeKind::Element)
            && builder.macro_name(*node, "sp")
            && builder.clear_node_text(*node)
        {
            recoveries.insert(*node, Recovery::FilledBlankLine { location });
        }
    }
    recoveries
}

/// The scanner usually stores a blank physical line at its first source byte.
/// An execution-stage recovery may refine that position to the escape which
/// produced the semantic blank; retain that logical provenance when present.
fn blank_line_location(builder: &DocumentBuilder, node: NodeId) -> Option<SourceSpan> {
    let mut location = builder.node_location(node)?;
    let position = builder.node_source_position(node)?;
    location.logical_start = Some(crate::SourcePosition {
        line: position.line,
        column: position.column,
    });
    Some(location)
}

fn mark_children_no_fill(builder: &mut DocumentBuilder, root: NodeId) {
    let Some(children) = builder.children(root).map(<[NodeId]>::to_vec) else {
        return;
    };
    for child in children {
        mark_subtree_no_fill(builder, child);
    }
}

fn mark_node_no_fill(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(mut flags) = builder.node_flags(node) else {
        return;
    };
    flags.no_fill = true;
    let _ = builder.set_node_flags(node, flags);
}

fn mark_section_targets(builder: &mut DocumentBuilder, heads: &[NodeId]) {
    let mut fallback_sections = std::collections::BTreeMap::<String, NodeId>::new();
    let mut duplicate_fallback_sections = std::collections::BTreeSet::<String>::new();
    for head in heads {
        let Some(heading) = visible_head_text(builder, *head) else {
            continue;
        };
        let tag = deroff_section_heading(&heading);
        let candidate = tag.trim_start_matches('-');
        if !candidate
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic())
        {
            continue;
        }
        if tag.is_empty() {
            continue;
        }
        // libmandoc's parser retains an internal discretionary-hyphen marker
        // in section text.  The public AST deliberately drops that marker,
        // while `tag_put()` still observes it and stores the visible spelling
        // as an explicit tag.  Preserve that observable result without
        // leaking the private marker into native tree text.
        if tag == heading {
            if heading.contains('-') {
                mark_target(builder, *head, Some(&tag));
            } else {
                mark_target(builder, *head, None);
            }
        } else if !duplicate_fallback_sections.contains(&tag) {
            if let Some(previous) = fallback_sections.remove(&tag) {
                clear_target(builder, previous);
                duplicate_fallback_sections.insert(tag);
            } else {
                mark_target(builder, *head, Some(&tag));
                fallback_sections.insert(tag, *head);
            }
        }
    }
}

/// Extract the section-heading spelling used by libmandoc's `deroff()` plus
/// the space-to-underscore conversion in `post_section()`.  This is purpose-
/// built for title tags: public AST text continues to retain authored escapes.
fn deroff_section_heading(heading: &str) -> String {
    let heading = heading
        .strip_prefix("\\&")
        .or_else(|| heading.strip_prefix("\\ "))
        .unwrap_or(heading);
    // The scanner retains `\\t` spelling in public mdoc text, while
    // libmandoc's deroffed heading observes its tabulation whitespace.
    let heading = heading.replace("\\t", " ");
    let heading = heading.trim_start_matches(char::is_whitespace);
    heading
        .trim_end_matches(char::is_whitespace)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_")
}

/// `post_em` gives an emphasis macro a fallback automatic tag after its
/// ordinary delimiter validation.  As in libmandoc's `tag_put`, a fallback
/// name is useful only when it occurs exactly once: a second occurrence
/// removes the first target and leaves neither one addressable.
fn mark_emphasis_targets(builder: &mut DocumentBuilder, elements: &[NodeId]) {
    let mut fallback = std::collections::BTreeMap::<String, Vec<(NodeId, bool)>>::new();
    // Strong/manual targets are constructed before this fallback pass.  They
    // occupy the global `tag_put()` namespace, so a later Em/Sy fallback of
    // the same spelling must be ignored rather than reintroducing an ID.
    let occupied = elements
        .iter()
        .filter(|element| {
            builder
                .node_flags(**element)
                .is_some_and(|flags| flags.permalink)
        })
        .filter_map(|element| inline_target_name(builder, *element).map(|(name, _)| name))
        .collect::<std::collections::BTreeSet<_>>();
    for element in elements {
        // A definition-list head may already have consumed this element as a
        // strong destination.  It must not re-enter the weaker, unique-only
        // fallback namespace.
        if builder
            .node_flags(*element)
            .is_some_and(|flags| flags.permalink)
        {
            continue;
        }
        let Some(text) = builder
            .children(*element)
            .and_then(|children| children.first())
            .and_then(|child| builder.node_text(*child))
            .map(str::to_owned)
        else {
            continue;
        };
        let end = text
            .bytes()
            .position(|byte| matches!(byte, b' ' | b'\t' | b'\\'))
            .unwrap_or(text.len());
        let Some(name) = text.get(..end).filter(|name| !name.is_empty()) else {
            continue;
        };
        if occupied.contains(name) {
            continue;
        }
        fallback
            .entry(name.to_owned())
            .or_default()
            .push((*element, end != text.len()));
    }
    // Apply only unique fallback names.  Deferring the mutation matters when
    // `tag_move_id()` would otherwise have transferred the first candidate to
    // a paragraph: a later duplicate must leave no stale paragraph target.
    for (name, candidates) in fallback {
        let [(element, explicit)] = candidates.as_slice() else {
            continue;
        };
        mark_target(builder, *element, explicit.then_some(name.as_str()));
        move_inline_target_to_preceding_paragraph(builder, *element, &name);
    }
}

/// `post_tag()` makes the leading command-like macro of a semantic list item
/// a strong destination. `tag_postprocess()` then transfers that ID to the
/// `It` head (the rendered `<dt>` or marker term) while retaining the inline
/// macro's permalink. The source parser has already split the head, so
/// restricting this to its first event and events immediately following a
/// literal `|` reproduces the upstream eligibility rule without guessing
/// across prose.
fn mark_definition_item_head_targets(
    builder: &mut DocumentBuilder,
    list_body: NodeId,
    head: NodeId,
    children: &[NodeId],
) {
    if !matches!(
        builder.node_list_kind(list_body),
        Some(
            NormalizedListKind::Definition
                | NormalizedListKind::Bullet
                | NormalizedListKind::Ordered
        )
    ) {
        return;
    }
    for (index, candidate) in children.iter().copied().enumerate() {
        let eligible = index == 0
            || children
                .get(index.saturating_sub(1))
                .and_then(|previous| builder.node_text(*previous))
                == Some("|");
        if !eligible {
            continue;
        }
        let Some(element) = leading_definition_item_target(builder, candidate) else {
            continue;
        };
        let Some((tag, explicit)) = inline_target_name(builder, element) else {
            continue;
        };
        mark_target(builder, element, explicit.then_some(tag.as_str()));
        move_inline_target_to_item_head(builder, element, head, &tag);
    }
}

/// Return the command-like macro that owns an eligible definition-list term.
/// Implicit partial blocks are presentation wrappers, but mdoc's tag pass
/// descends through their Body before selecting the leading tag macro: for
/// example `.It Bq Er ENOENT` assigns the `ENOENT` destination to the `It`
/// Head and leaves `Er` as its permalink.  Do not search past the first Body
/// event; later prose or macros are not term-leading candidates.
fn leading_definition_item_target(builder: &DocumentBuilder, node: NodeId) -> Option<NodeId> {
    if builder
        .node_macro_name(node)
        .is_some_and(is_definition_item_target_macro)
    {
        return Some(node);
    }
    if !builder
        .node_macro_name(node)
        .is_some_and(is_implicit_partial_block_macro)
    {
        return None;
    }
    let body = builder.children(node)?.iter().copied().find(|child| {
        builder.node_kind(*child) == Some(NodeKind::Body)
            && builder.node_macro_name(*child) == builder.node_macro_name(node)
    })?;
    let first = builder.children(body)?.first().copied()?;
    leading_nested_definition_item_target(builder, first)
}

/// The error-name macro does not itself make a bare definition-list term a
/// destination (`.It Er one`).  It does when it is the first semantic child
/// of an enclosure wrapper (`.It Bq Er ENOENT`), which is the narrow upstream
/// `tag_postprocess()` shape exercised by the mdoc regression suite.
fn leading_nested_definition_item_target(
    builder: &DocumentBuilder,
    node: NodeId,
) -> Option<NodeId> {
    if builder.node_macro_name(node) == Some("Er")
        || builder
            .node_macro_name(node)
            .is_some_and(is_definition_item_target_macro)
    {
        return Some(node);
    }
    if !builder
        .node_macro_name(node)
        .is_some_and(is_implicit_partial_block_macro)
    {
        return None;
    }
    let body = builder.children(node)?.iter().copied().find(|child| {
        builder.node_kind(*child) == Some(NodeKind::Body)
            && builder.node_macro_name(*child) == builder.node_macro_name(node)
    })?;
    let first = builder.children(body)?.first().copied()?;
    leading_nested_definition_item_target(builder, first)
}

fn is_definition_item_target_macro(name: &str) -> bool {
    matches!(
        name,
        "Cm" | "Dv" | "Em" | "Ev" | "Fl" | "Ic" | "Li" | "Ms" | "No" | "Sy" | "Va"
    )
}

/// Complete the same `post_tag()` rule for the cross-line `.It Xo` form.
/// The first command-like child of Xo's body is logically the first item-head
/// macro, even though the public AST correctly nests it below an Xo block.
fn mark_definition_item_xo_head_targets(builder: &mut DocumentBuilder) {
    let mut pending = vec![DocumentBuilder::root()];
    while let Some(list) = pending.pop() {
        if builder.node_kind(list) == Some(NodeKind::Block)
            && builder.node_macro_name(list) == Some("Bl")
            && builder
                .children(list)
                .and_then(|children| {
                    children.iter().copied().find(|child| {
                        builder.node_kind(*child) == Some(NodeKind::Body)
                            && builder.node_macro_name(*child) == Some("Bl")
                    })
                })
                .is_some_and(|body| {
                    builder.node_list_kind(body) == Some(NormalizedListKind::Definition)
                })
        {
            let Some(list_body) = builder.children(list).and_then(|children| {
                children.iter().copied().find(|child| {
                    builder.node_kind(*child) == Some(NodeKind::Body)
                        && builder.node_macro_name(*child) == Some("Bl")
                })
            }) else {
                continue;
            };
            let items = builder
                .children(list_body)
                .map(<[NodeId]>::to_vec)
                .unwrap_or_default();
            for item in items {
                if builder.node_kind(item) != Some(NodeKind::Block)
                    || builder.node_macro_name(item) != Some("It")
                {
                    continue;
                }
                let Some(item_head) = builder.children(item).and_then(|children| {
                    children.iter().copied().find(|child| {
                        builder.node_kind(*child) == Some(NodeKind::Head)
                            && builder.node_macro_name(*child) == Some("It")
                    })
                }) else {
                    continue;
                };
                let Some(xo) = builder.children(item_head).and_then(|children| {
                    (children.len() == 1)
                        .then(|| children.first().copied())
                        .flatten()
                        .filter(|child| {
                            builder.node_kind(*child) == Some(NodeKind::Block)
                                && builder.node_macro_name(*child) == Some("Xo")
                        })
                }) else {
                    continue;
                };
                let Some(xo_body) = builder.children(xo).and_then(|children| {
                    children.iter().copied().find(|child| {
                        builder.node_kind(*child) == Some(NodeKind::Body)
                            && builder.node_macro_name(*child) == Some("Xo")
                    })
                }) else {
                    continue;
                };
                let Some(element) = builder
                    .children(xo_body)
                    .and_then(|children| children.first())
                    .copied()
                else {
                    continue;
                };
                if !matches!(
                    builder.node_macro_name(element),
                    Some(
                        "Cm" | "Dv" | "Em" | "Ev" | "Fl" | "Ic" | "Li" | "Ms" | "No" | "Sy" | "Va"
                    )
                ) {
                    continue;
                }
                let Some((tag, explicit)) = inline_target_name(builder, element) else {
                    continue;
                };
                mark_target(builder, element, explicit.then_some(tag.as_str()));
                move_inline_target_to_item_head(builder, element, item_head, &tag);
            }
        }
        if let Some(children) = builder.children(list) {
            pending.extend(children.iter().rev().copied());
        }
    }
}

/// Return the public destination spelling of a taggable inline macro and
/// whether it differs from its literal first child.  This mirrors the narrow
/// prefix treatment in libmandoc's `tag_put()` (`-`, `\\&`, `\\-`, `\\e`).
fn inline_target_name(builder: &DocumentBuilder, element: NodeId) -> Option<(String, bool)> {
    let source = builder
        .children(element)?
        .first()
        .and_then(|child| builder.node_text(*child))?;
    let mut candidate = source;
    if let Some(rest) = candidate.strip_prefix('-') {
        candidate = rest;
    } else {
        for prefix in ["\\&", "\\-", "\\e"] {
            if let Some(rest) = candidate.strip_prefix(prefix) {
                candidate = rest;
                break;
            }
        }
    }
    let end = candidate
        .bytes()
        .position(|byte| matches!(byte, b' ' | b'\t' | b'\\'))
        .unwrap_or(candidate.len());
    let tag = candidate.get(..end).filter(|tag| !tag.is_empty())?;
    Some((
        tag.to_owned(),
        candidate.len() != source.len() || end != candidate.len(),
    ))
}

/// Move a strong inline destination into its definition-list term unless a
/// previous strong destination already owns that term.  In the latter case,
/// both inline macro targets remain observable, exactly as `tag_move_id()`
/// does after `tag_put()` has resolved priorities.
fn move_inline_target_to_item_head(
    builder: &mut DocumentBuilder,
    element: NodeId,
    head: NodeId,
    tag: &str,
) {
    if builder
        .node_flags(head)
        .is_some_and(|flags| flags.deep_link_target)
    {
        return;
    }
    mark_manual_target(builder, head, tag);
    if let Some(mut flags) = builder.node_flags(element) {
        flags.deep_link_target = false;
        let _ = builder.set_node_flags(element, flags);
    }
}

/// `tag_move_id()` walks backward across ordinary inline siblings after a
/// successful Em/Sy fallback tag.  A preceding paragraph owns the stable
/// destination, while the inline element keeps only its permalink.  Stop at
/// the same major block boundary used by the upstream postprocessor.
fn move_inline_target_to_preceding_paragraph(
    builder: &mut DocumentBuilder,
    element: NodeId,
    tag: &str,
) {
    let mut current = element;
    loop {
        let Some(parent) = builder.node_parent(current) else {
            return;
        };
        let Some(siblings) = builder.children(parent) else {
            return;
        };
        let Some(index) = siblings.iter().position(|sibling| *sibling == current) else {
            return;
        };
        current = if index == 0 {
            parent
        } else {
            siblings[index - 1]
        };
        match builder.node_macro_name(current) {
            Some("Pp") => {
                let occupied = builder
                    .node_flags(current)
                    .is_some_and(|flags| flags.deep_link_target);
                let punctuation_fallback = builder
                    .node_tag(current)
                    .filter(|previous| matches!(*previous, "." | "!" | "?"))
                    .map(str::to_owned);
                if occupied && punctuation_fallback.is_none() {
                    return;
                }
                // `tag_move_id()` lets a later Em/Sy fallback replace an
                // earlier punctuation-only fallback on the same paragraph.
                // The punctuation macro keeps its permalink, while the
                // meaningful spelling owns the destination.
                if let Some(previous) = punctuation_fallback.as_deref() {
                    restore_punctuation_fallback_target(builder, current, previous);
                }
                mark_manual_target(builder, current, tag);
                if let Some(mut flags) = builder.node_flags(element) {
                    flags.deep_link_target = false;
                    let _ = builder.set_node_flags(element, flags);
                }
                return;
            }
            Some("Sh" | "Ss" | "Bd" | "Bl" | "D1" | "Dl" | "Rs") => return,
            _ => {}
        }
    }
}

/// Restore a punctuation fallback that was provisionally moved onto a
/// paragraph and has just been superseded by a later, meaningful fallback.
fn restore_punctuation_fallback_target(
    builder: &mut DocumentBuilder,
    paragraph: NodeId,
    tag: &str,
) {
    let Some(parent) = builder.node_parent(paragraph) else {
        return;
    };
    let Some(siblings) = builder.children(parent) else {
        return;
    };
    let Some(index) = siblings.iter().position(|sibling| *sibling == paragraph) else {
        return;
    };
    for candidate in &siblings[index + 1..] {
        if matches!(
            builder.node_macro_name(*candidate),
            Some("Pp" | "Sh" | "Ss")
        ) {
            return;
        }
        if !matches!(builder.node_macro_name(*candidate), Some("Em" | "Sy"))
            || !builder
                .node_flags(*candidate)
                .is_some_and(|flags| flags.permalink)
        {
            continue;
        }
        let Some((candidate_tag, explicit)) = inline_target_name(builder, *candidate) else {
            continue;
        };
        if candidate_tag != tag {
            continue;
        }
        mark_target(builder, *candidate, explicit.then_some(tag));
        return;
    }
}

/// `post_em()` is shared by Em and Sy in libmandoc's validation table.  Run
/// after source-order restructuring so elements originating in a nested body
/// or after an explicit closer participate in the same fallback namespace.
fn emphasis_fallback_elements(builder: &DocumentBuilder) -> Vec<NodeId> {
    let mut elements = Vec::new();
    let mut pending = vec![DocumentBuilder::root()];
    while let Some(node) = pending.pop() {
        if builder.node_kind(node) == Some(NodeKind::Element)
            && matches!(builder.node_macro_name(node), Some("Em" | "Sy"))
        {
            elements.push(node);
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().rev().copied());
        }
    }
    elements
}

fn visible_head_text(builder: &DocumentBuilder, head: NodeId) -> Option<String> {
    let values = builder
        .children(head)?
        .iter()
        .filter_map(|child| {
            builder.node_text(*child).or_else(|| {
                builder
                    .children(*child)
                    .and_then(|children| children.first())
                    .and_then(|child| builder.node_text(*child))
            })
        })
        .collect::<Vec<_>>();
    (!values.is_empty()).then(|| values.join(" "))
}

/// Mirror `tag_put(NULL, …)` for the automatic function-name destination.
/// The public AST retains formatting escapes, but the legacy tag contract ends
/// at the first whitespace or escape after skipping only its three permitted
/// leading zero-width spellings.
fn automatic_mdoc_function_tag(value: &str) -> Option<&str> {
    let value = value.strip_prefix('-').unwrap_or(value);
    let value = value
        .strip_prefix("\\&")
        .or_else(|| value.strip_prefix("\\-"))
        .or_else(|| value.strip_prefix("\\e"))
        .unwrap_or(value);
    let length = value.find([' ', '\t', '\\']).unwrap_or(value.len());
    (length > 0).then_some(&value[..length])
}

/// Commit automatic function tags only when their spelling appears once in
/// the document.  The target bit was already set at source-order time; this
/// pass supplies the global duplicate suppression performed by `tag_put()`.
fn mark_unique_function_targets(
    builder: &mut DocumentBuilder,
    targets: &[(NodeId, String, bool)],
    occurrences: &[String],
) {
    let mut counts = BTreeMap::<&str, usize>::new();
    for tag in occurrences {
        *counts.entry(tag).or_default() += 1;
    }
    let mut retained_duplicates = BTreeSet::<&str>::new();
    for (node, tag, exposes_tag) in targets {
        if *exposes_tag && counts.get(tag.as_str()) == Some(&1) {
            // `tag_put(NULL, …)` records the target bit without allocating a
            // redundant tag when the public first word is already the exact
            // destination spelling.  A separate tag is only observable when
            // normalization shortened or otherwise transformed that word.
            let public_first_word = builder
                .children(*node)
                .and_then(|children| children.first())
                .and_then(|child| builder.node_text(*child));
            if public_first_word != Some(tag.as_str()) {
                let _ = builder.set_node_tag(*node, tag.as_str());
            }
        } else if !retained_duplicates.insert(tag) {
            // `tag_put()` keeps the first declaration's destination bit for
            // a repeated automatic function spelling, then suppresses every
            // later candidate.  The spelling remains tagless in both cases
            // because it is not globally unique.
            clear_target(builder, *node);
        }
    }
}

fn mark_target(builder: &mut DocumentBuilder, head: NodeId, tag: Option<&str>) {
    let Some(mut flags) = builder.node_flags(head) else {
        return;
    };
    flags.deep_link_target = true;
    flags.permalink = true;
    let _ = builder.set_node_flags(head, flags);
    if let Some(tag) = tag {
        let _ = builder.set_node_tag(head, tag);
    }
}

fn mark_destination(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(mut flags) = builder.node_flags(node) else {
        return;
    };
    flags.deep_link_target = true;
    let _ = builder.set_node_flags(node, flags);
}

fn mark_permalink(builder: &mut DocumentBuilder, node: NodeId, tag: Option<&str>) {
    let Some(mut flags) = builder.node_flags(node) else {
        return;
    };
    flags.permalink = true;
    let _ = builder.set_node_flags(node, flags);
    if let Some(tag) = tag {
        let _ = builder.set_node_tag(node, tag);
    }
}

/// Move a same-line display destination to its first visible leaf.
///
/// A one-line D1/Dl body already owns the authored text at the time `.Tg`
/// is validated, unlike a multi-line Bd whose first visible line arrives in
/// a later source event.
fn mark_first_visible_permalink(builder: &mut DocumentBuilder, root: NodeId, tag: &str) {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if builder.node_kind(node) == Some(NodeKind::Text)
            && builder
                .node_flags(node)
                .is_some_and(|flags| !flags.no_print)
        {
            mark_permalink(builder, node, Some(tag));
            return;
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().rev().copied());
        }
    }
}

/// Attach a manual `.Tg` destination without also making the syntax node its
/// own permalink.  `tag_postprocess()` moves the latter to following text for
/// `.Pp` targets.
fn mark_manual_target(builder: &mut DocumentBuilder, node: NodeId, tag: &str) {
    let Some(mut flags) = builder.node_flags(node) else {
        return;
    };
    flags.deep_link_target = true;
    let _ = builder.set_node_flags(node, flags);
    let _ = builder.set_node_tag(node, tag);
}

fn clear_target(builder: &mut DocumentBuilder, head: NodeId) {
    let Some(mut flags) = builder.node_flags(head) else {
        return;
    };
    flags.deep_link_target = false;
    flags.permalink = false;
    let _ = builder.set_node_flags(head, flags);
    let _ = builder.clear_node_tag(head);
}

fn default_volume(section: &str) -> Option<String> {
    let section = section.strip_suffix('p').unwrap_or(section);
    Some(
        match section {
            "1" => "General Commands Manual",
            "2" => "System Calls Manual",
            "3" => "Library Functions Manual",
            "4" => "Kernel Interfaces Manual",
            "5" => "File Formats Manual",
            "6" => "Games Manual",
            "7" => "Miscellaneous Information Manual",
            "8" => "System Manager's Manual",
            "9" => "Kernel Developer's Manual",
            _ => return None,
        }
        .to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        AuthorMode, DiagnosticCode, DisplayKind, MacroSet, NodeKind, NormalizedFont,
        NormalizedListKind, Severity, Source, SourceName,
    };

    /// Most mdoc unit fixtures intentionally start with the construct under
    /// test, commonly a `DESCRIPTION` section.  Keep their assertions focused
    /// on that construct; the production parser still emits the prologue
    /// warning and `first_section_validation_uses_the_visible_heading` below
    /// covers it directly.
    #[derive(Default)]
    struct Parser(crate::Parser);

    impl Parser {
        fn parse(&self, source: Source<'_>) -> Result<crate::ParseReport, crate::FatalError> {
            let mut report = self.0.parse(source)?;
            report.diagnostics.retain(|diagnostic| {
                diagnostic.code.as_str() != DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME
            });
            Ok(report)
        }
    }

    #[test]
    fn fd_rebases_later_argument_locations_after_string_expansion() {
        let name = SourceName::new("fd-expansion-location.2").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FD 2\n.Os\n.Sh DESCRIPTION\n.ds s \\(sh\n.Fd \\*sunquoted unescaped\n",
            ))
            .unwrap();
        let fd = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("Fd"))
            .unwrap();
        let second = fd.children().nth(1).unwrap();
        assert_eq!(second.text(), Some("unescaped"));
        let position = report
            .document
            .source_position(second.location().unwrap())
            .unwrap();
        assert_eq!((position.line, position.column), (6, 18));
    }

    #[test]
    fn empty_fd_is_diagnosed_then_removed_from_public_flow() {
        let name = SourceName::new("fd-empty.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FD-EMPTY 1\n.Os\n.Sh SYNOPSIS\n.Fd\n.In stdlib.h\n.Sh DESCRIPTION\nleading\n.Fd\ntrailing\n",
            ))
            .unwrap();
        assert!(
            report
                .document
                .preorder()
                .all(|node| node.macro_name() != Some("Fd"))
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                (DiagnosticCode::MDOC_EMPTY_MACRO, "skipping empty macro: Fd"),
                (DiagnosticCode::MDOC_EMPTY_MACRO, "skipping empty macro: Fd"),
            ]
        );
    }

    #[test]
    fn inline_macro_rebases_later_locations_after_string_expansion() {
        let name = SourceName::new("inline-expansion-location.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt INLINE 1\n.Os\n.Sh DESCRIPTION\n.Fl isolated \\*(Ba em\\*(Babedded\n",
            ))
            .unwrap();
        let expanded = report
            .document
            .preorder()
            .find(|node| node.text() == Some(r"em\fR|\fPbedded"))
            .unwrap_or_else(|| {
                panic!(
                    "{:?}",
                    report
                        .document
                        .preorder()
                        .map(|node| (node.macro_name(), node.text()))
                        .collect::<Vec<_>>()
                )
            });
        let position = report
            .document
            .source_position(expanded.location().unwrap())
            .unwrap();
        assert_eq!((position.line, position.column), (5, 22));
    }

    #[test]
    fn option_rebases_nested_children_after_string_expansion() {
        let name = SourceName::new("option-expansion-location.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt OPTION 1\n.Os\n.Sh SYNOPSIS\n.Op Fl c Ar string \\*(Ba Fl s \\*(Ba Ar file Op Ar argument ...\n",
            ))
            .unwrap();
        let ellipsis = report
            .document
            .preorder()
            .find(|node| node.text() == Some("..."))
            .unwrap();
        let position = report
            .document
            .source_position(ellipsis.location().unwrap())
            .unwrap();

        assert_eq!((position.line, position.column), (5, 64));
    }

    #[test]
    fn empty_ad_is_discarded_before_delimiter_style_validation() {
        let name = SourceName::new("mdoc-ad-empty.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt AD-EMPTY 1\n.Os\n.Sh DESCRIPTION\n.Ad 0x3bc.\n.Ad\nend\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "skipping empty macro: Ad",
                "no blank before trailing delimiter: Ad 0x3bc.",
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
        assert_eq!(positions, [(6, 2), (5, 10)]);
        assert_eq!(
            report
                .document
                .preorder()
                .filter(|node| node.macro_name() == Some("Ad"))
                .count(),
            1
        );
    }

    #[test]
    fn an_options_are_private_and_validate_empty_duplicate_and_excess_forms() {
        let name = SourceName::new("mdoc-an-options.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt AN-OPTIONS 1\n.Os\n.Sh AUTHORS\n.An -split -nosplit author\n.An\n.An Ingo,\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "skipping duplicate argument: An -nosplit",
                "skipping excess arguments: An ... author",
                "skipping empty macro: An",
                "no blank before trailing delimiter: An Ingo,",
            ]
        );
        let author = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("An") && node.author_mode().is_some())
            .unwrap();
        assert_eq!(author.author_mode(), Some(AuthorMode::Split));
        assert_eq!(
            author
                .children()
                .map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            [Some("author")]
        );
    }

    #[test]
    fn structures_metadata_sections_lists_displays_and_fonts() {
        let name = SourceName::new("mdoc-structure.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SAMPLE 1\n.Os ExampleOS\n.Sh NAME\n.Nm sample\n.Nd sample manual\n.Sh DESCRIPTION\n.Pp\nparagraph\n.Bl -bullet -compact -offset indent\n.It\nitem\n.El\n.Bd -literal -offset 2n\nliteral\n.Ed\n.Bf -emphasis\nstyled\n.Ef\n",
            ))
            .unwrap();
        let document = &report.document;
        assert_eq!(document.macro_set(), MacroSet::Mdoc);
        assert_eq!(document.metadata().title.as_deref(), Some("SAMPLE"));
        assert_eq!(document.metadata().section.as_deref(), Some("1"));
        assert_eq!(document.metadata().os.as_deref(), Some("ExampleOS"));
        assert_eq!(document.metadata().name.as_deref(), Some("sample"));
        assert_eq!(document.metadata().date.as_deref(), Some("August 25, 2026"));

        let nodes = document.preorder().collect::<Vec<_>>();
        for control in ["Dd", "Dt", "Os"] {
            assert!(
                nodes
                    .iter()
                    .any(|node| node.macro_name() == Some(control) && node.flags().no_print)
            );
        }
        let list = nodes
            .iter()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bl"))
            .unwrap();
        assert_eq!(list.list_kind(), Some(NormalizedListKind::Bullet));
        assert!(list.compact());
        assert_eq!(list.offset(), Some("indent"));
        assert_eq!(list.width(), Some("2n"));
        let item = nodes
            .iter()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("It"))
            .unwrap();
        assert_eq!(
            item.children()
                .nth(1)
                .unwrap()
                .children()
                .next()
                .unwrap()
                .text(),
            Some("item")
        );

        let display = nodes
            .iter()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bd"))
            .unwrap();
        assert_eq!(display.display_kind(), Some(DisplayKind::Literal));
        assert_eq!(display.offset(), Some("2n"));
        assert!(
            display
                .children()
                .nth(1)
                .unwrap()
                .children()
                .next()
                .unwrap()
                .flags()
                .no_fill
        );
        let font = nodes
            .iter()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bf"))
            .unwrap();
        assert_eq!(font.font(), Some(NormalizedFont::Emphasis));
    }

    #[test]
    fn mdoc_retains_only_preamble_comments_in_the_public_tree() {
        let name = SourceName::new("mdoc-comments.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".\\\" preamble\n.Dd August 25, 2026\n.Dt COMMENTS 1\n.Os\n.\\\" internal\n.Sh DESCRIPTION\nbody\n",
            ))
            .unwrap();
        let comments = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Comment)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(comments, [" preamble"]);
    }

    #[test]
    fn name_metadata_excludes_zero_width_formatter_spelling() {
        let name = SourceName::new("metadata-nm.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SAMPLE 1\n.Os\n.Sh NAME\n.Nm \\&sample-name\n",
            ))
            .unwrap();
        assert_eq!(
            report.document.metadata().name.as_deref(),
            Some("sample-name")
        );
        assert!(
            report
                .document
                .preorder()
                .any(|node| node.text() == Some("\\&sample-name"))
        );
    }

    #[test]
    fn normalizes_mdoc_macro_layout_widths_without_rewriting_source_arguments() {
        let name = SourceName::new("mdoc-layout-width.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt WIDTH 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It term\nbody\n.El\n.Bl -inset\n.It term\nbody\n.El\n.Bl -enum\n.It item\nbody\n.El\n.Bd -offset Fl\nbody\n.Ed\n",
            ))
            .unwrap();
        let list = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bl"))
            .unwrap();
        assert_eq!(list.width(), Some("6n"));
        let widths = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bl"))
            .map(crate::NodeRef::width)
            .collect::<Vec<_>>();
        assert_eq!(widths, [Some("6n"), None, Some("3n")]);
        let display = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bd"))
            .unwrap();
        assert_eq!(display.offset(), Some("10n"));
    }

    #[test]
    fn display_options_use_first_type_and_keep_validation_out_of_the_public_tree() {
        let name = SourceName::new("mdoc-display-options.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt DISPLAY-OPTIONS 1\n.Os\n.Sh DESCRIPTION\n.Bd -ragged -compact -unfilled\nvisible\n.Ed tail\n.Bd\nrelinked\n.Ed\n",
            ))
            .unwrap();
        let displays = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bd"))
            .collect::<Vec<_>>();
        assert_eq!(displays.len(), 1);
        assert_eq!(displays[0].display_kind(), Some(DisplayKind::Filled));
        assert!(displays[0].compact());
        assert!(
            report
                .document
                .preorder()
                .any(|node| node.text() == Some("relinked"))
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "skipping all arguments: Ed tail",
                "skipping duplicate display type: Bd -unfilled",
                "skipping display without arguments: Bd",
            ]
        );
    }

    #[test]
    fn list_item_heads_coalesce_plain_phrases_but_preserve_column_cells() {
        let name = SourceName::new("mdoc-list-item-heads.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt LISTHEADS 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag\n.It outer tag\nbody\n.El\n.Bl -column first second\n.It left right\n.El\n",
            ))
            .unwrap();
        let item_heads = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("It"))
            .collect::<Vec<_>>();
        assert_eq!(item_heads.len(), 2);
        assert_eq!(
            item_heads[0]
                .children()
                .map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            [Some("outer tag")]
        );
        assert_eq!(
            item_heads[1]
                .children()
                .map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            []
        );
        let column_item = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("It"))
            .nth(1)
            .unwrap();
        let column_cells = column_item
            .children()
            .filter(|node| node.kind() == NodeKind::Body)
            .map(|body| {
                body.children()
                    .map(crate::NodeRef::text)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(column_cells, vec![vec![Some("left"), Some("right")]]);
    }

    #[test]
    fn diagnostic_list_item_heads_remain_literal_and_skip_empty_no() {
        let name = SourceName::new("mdoc-diag-list-literals.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt DIAG 1\n.Os\n.Sh DESCRIPTION\n.Bl -diag\n.It Nx\n.No Nx\n.It Fl flag\nbody\n.El\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            ["skipping empty macro: No"]
        );
        let position = report
            .document
            .source_position(report.diagnostics[0].primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((position.line, position.column), (7, 2));

        let items = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("It"))
            .collect::<Vec<_>>();
        assert_eq!(items.len(), 2);
        let first_head = items[0]
            .children()
            .find(|node| node.kind() == NodeKind::Head)
            .unwrap();
        assert_eq!(
            first_head
                .children()
                .map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            [Some("Nx")]
        );
        let second_head = items[1]
            .children()
            .find(|node| node.kind() == NodeKind::Head)
            .unwrap();
        assert_eq!(
            second_head
                .children()
                .map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            [Some("Fl flag")]
        );
        let first_body = items[0]
            .children()
            .find(|node| node.kind() == NodeKind::Body)
            .unwrap();
        let nx = first_body.children().next().unwrap();
        assert_eq!(nx.macro_name(), Some("Nx"));
        assert!(nx.flags().line_start);
        assert_eq!(
            nx.children().next().and_then(crate::NodeRef::text),
            Some("NetBSD")
        );
    }

    #[test]
    fn empty_no_requests_are_removed_and_keep_source_ordered_findings() {
        let name = SourceName::new("mdoc-empty-no.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EMPTY-NO 1\n.Os\n.Sh DESCRIPTION\n.No ( No b\n.No a No (\n.No \".\"\n.No a.\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "skipping empty macro: No",
                "skipping empty macro: No",
                "no blank before trailing delimiter: No a.",
            ]
        );
        let positions = report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                report
                    .document
                    .source_position(diagnostic.primary.as_ref().unwrap())
                    .map(|position| (position.line, position.column))
            })
            .collect::<Vec<_>>();
        assert_eq!(positions, [Some((6, 7)), Some((7, 2)), Some((8, 6))]);

        let nodes = report.document.preorder().collect::<Vec<_>>();
        assert!(
            !nodes.iter().any(|node| {
                node.macro_name() == Some("No") && node.children().next().is_none()
            })
        );
        assert!(nodes.iter().any(|node| node.text() == Some("(")));
        assert!(nodes.iter().any(|node| node.text() == Some(".")));
    }

    #[test]
    fn no_space_macro_reports_only_invalid_source_positions() {
        let name = SourceName::new("mdoc-no-space-position.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt NO-SPACE 1\n.Os\n.Sh DESCRIPTION\n.Ns Op after\n.Oo before Oc Ns : Op after\n.Oo before Oc : Ns Op after\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                ("mdoc.no-space-macro", "skipping no-space macro"),
                ("mdoc.no-space-macro", "skipping no-space macro"),
            ]
        );
        let positions = report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                report
                    .document
                    .source_position(diagnostic.primary.as_ref().unwrap())
                    .map(|position| (position.line, position.column))
            })
            .collect::<Vec<_>>();
        assert_eq!(positions, [Some((5, 2)), Some((6, 15))]);
    }

    #[test]
    fn empty_lists_remain_visible_and_report_their_openers() {
        let name = SourceName::new("mdoc-empty-lists.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EMPTY-LISTS 1\n.Os\n.Sh DESCRIPTION\n.Bl -bullet\n.El\n.Bl -column one two\n.El\n.Bl -diag\n.El\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            ["empty block: Bl", "empty block: Bl", "empty block: Bl"]
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
        assert_eq!(positions, [(5, 2), (7, 2), (9, 2)]);
        let lists = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bl"))
            .collect::<Vec<_>>();
        assert_eq!(lists.len(), 3);
        assert!(lists.iter().all(|list| {
            list.children()
                .find(|node| node.kind() == NodeKind::Body)
                .is_some_and(|body| body.children().next().is_none())
        }));
    }

    #[test]
    fn term_and_tag_list_kinds_report_an_empty_item_head() {
        let name = SourceName::new("mdoc-empty-list-heads.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EMPTY-HEADS 1\n.Os\n.Sh DESCRIPTION\n.Bl -hang\n.It\nbody\n.El\n.Bl -ohang\n.It\nbody\n.El\n.Bl -inset\n.It\nbody\n.El\n.Bl -diag\n.It\nbody\n.El\n.Bl -tag -width Ds\n.It\nbody\n.El\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "empty head in list item: Bl -hang It",
                "empty head in list item: Bl -ohang It",
                "empty head in list item: Bl -inset It",
                "empty head in list item: Bl -diag It",
                "empty head in list item: Bl -tag It",
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
        assert_eq!(positions, [(6, 2), (10, 2), (14, 2), (18, 2), (22, 2)]);
    }

    #[test]
    fn marker_list_items_validate_at_the_next_structural_boundary() {
        let name = SourceName::new("mdoc-empty-marker-items.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EMPTY-ITEMS 1\n.Os\n.Sh DESCRIPTION\n.Bl -bullet\n.It head argument\none\n.It\n.It\nthree\n.El\n.Bl -dash\n.It\none\n.It head argument\n.It\nthree\n.El\n.Bl -enum\n.It\none\n.It\n.It head argument\nthree\n.El\n.Bl -hyphen\n.It Sy head argument\none\n.It\n.It\nthree\n.El\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "skipping all arguments: It head argument",
                "empty list item: Bl -bullet It",
                "empty list item: Bl -dash It",
                "skipping all arguments: It head argument",
                "empty list item: Bl -enum It",
                "skipping all arguments: It head argument",
                "skipping all arguments: It Sy",
                "empty list item: Bl -hyphen It",
            ]
        );
    }

    #[test]
    fn item_list_heads_are_syntax_only_without_empty_item_warnings() {
        let name = SourceName::new("mdoc-item-list-heads.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt ITEM-LISTS 1\n.Os\n.Sh DESCRIPTION\n.Bl -item\n.It ignored\nbody\n.El\n.Bl -item -compact\n.It ignored\nbody\n.El\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "skipping all arguments: It ignored",
                "skipping all arguments: It ignored",
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
        assert_eq!(positions, [(6, 2), (10, 2)]);
    }

    #[test]
    fn tag_list_missing_width_reports_the_private_default_without_publishing_it() {
        let name = SourceName::new("mdoc-tag-list-width.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt TAG-WIDTH 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag\n.It tag\nbody\n.El\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            ["missing -width in -tag list, using 6n: Bl -tag"]
        );
        let list = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bl"))
            .unwrap();
        assert_eq!(list.width(), None);
    }

    #[test]
    fn leading_list_content_moves_out_at_item_and_close_boundaries() {
        let name = SourceName::new("mdoc-list-content-before-item.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt LIST-CONTENT 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\nstray text\n.Em stray macro\n.It tag\nbody\n.El\n.Bl -dash\nstray text\n.El\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "moving content out of list: text",
                "moving content out of list: Em",
                "moving content out of list: text",
                "empty block: Bl",
            ]
        );
        let lists = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bl"))
            .collect::<Vec<_>>();
        assert_eq!(lists.len(), 2);
        assert_eq!(
            lists[0]
                .children()
                .find(|node| node.kind() == NodeKind::Body)
                .unwrap()
                .children()
                .filter(|node| node.macro_name() == Some("It"))
                .count(),
            1
        );
        assert!(
            lists[1]
                .children()
                .find(|node| node.kind() == NodeKind::Body)
                .unwrap()
                .children()
                .next()
                .is_none()
        );
    }

    #[test]
    fn trailing_spacing_state_stays_with_the_first_list_item() {
        let name = SourceName::new("mdoc-list-spacing-state.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt LIST-SPACING 1\n.Os\n.Sh DESCRIPTION\n.Bl -bullet\nstray text\n.Sm off\n.It\nbody\n.El\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            ["moving content out of list: text"]
        );
        let list_body = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bl"))
            .unwrap();
        assert_eq!(
            list_body
                .children()
                .filter_map(crate::NodeRef::macro_name)
                .collect::<Vec<_>>(),
            ["Sm", "It"]
        );
    }

    #[test]
    fn trailing_explicit_tag_stays_with_the_first_list_item() {
        let name = SourceName::new("mdoc-list-item-tag.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt LIST-TAG 1\n.Os\n.Sh DESCRIPTION\n.Bl -bullet\n.Tg item\n.It\nbody\n.El\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty());
        let list_body = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bl"))
            .unwrap();
        assert_eq!(
            list_body
                .children()
                .filter_map(crate::NodeRef::macro_name)
                .collect::<Vec<_>>(),
            ["Tg", "It"]
        );
        let item_body = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("It"))
            .unwrap();
        assert_eq!(item_body.tag(), Some("item"));
        assert!(item_body.flags().deep_link_target);
    }

    #[test]
    fn marker_list_item_targets_move_from_the_inline_term_to_the_head() {
        let name = SourceName::new("mdoc-marker-item-target.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt MARKER-TARGET 1\n.Os\n.Sh DESCRIPTION\n.Bl -hyphen\n.It Sy head argument\nbody\n.El\n",
            ))
            .unwrap();
        let head = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("It"))
            .unwrap();
        assert_eq!(head.tag(), Some("head"));
        assert!(head.flags().deep_link_target);
        assert!(!head.flags().permalink);
        let sy = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Sy"))
            .unwrap();
        assert_eq!(sy.tag(), Some("head"));
        assert!(!sy.flags().deep_link_target);
        assert!(sy.flags().permalink);
    }

    #[test]
    fn explicit_tg_before_display_moves_destination_to_body_and_permalink_to_text() {
        let name = SourceName::new("mdoc-tg-display.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt TGDISPLAY 1\n.Os\n.Sh DESCRIPTION\n.Tg display\n.Bd -literal\nvisible text\n.Ed\n",
            ))
            .unwrap();
        let display_body = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bd"))
            .unwrap();
        assert!(display_body.flags().deep_link_target);
        assert!(!display_body.flags().permalink);
        assert_eq!(display_body.tag(), Some("display"));
        let text = display_body.children().next().unwrap();
        assert!(text.flags().permalink);
        assert_eq!(text.tag(), Some("display"));
        let tg = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("Tg"))
            .unwrap();
        assert!(tg.flags().no_print);
    }

    #[test]
    fn one_line_displays_retain_partial_block_phrases_targets_and_empty_warnings() {
        for macro_name in ["D1", "Dl"] {
            let name = SourceName::new(format!("mdoc-{macro_name}-display.1")).unwrap();
            let input = format!(
                ".Dd August 25, 2026\n.Dt DISPLAY 1\n.Os\n.Sh DESCRIPTION\n.Tg display\n.{macro_name} spacing  in  and around one-line displays\n.{macro_name}\n"
            );
            let report = Parser::default()
                .parse(Source::new(&name, input.as_bytes()))
                .unwrap();
            let displays = report
                .document
                .preorder()
                .filter(|node| {
                    node.kind() == NodeKind::Block && node.macro_name() == Some(macro_name)
                })
                .collect::<Vec<_>>();
            assert_eq!(displays.len(), 2);
            let first_body = displays[0]
                .children()
                .find(|node| node.kind() == NodeKind::Body)
                .unwrap();
            assert!(first_body.flags().deep_link_target);
            assert!(!first_body.flags().permalink);
            assert_eq!(first_body.tag(), Some("display"));
            let phrases = first_body
                .children()
                .map(|node| (node.text(), node.flags(), node.tag()))
                .collect::<Vec<_>>();
            assert_eq!(phrases.len(), 2);
            assert_eq!(phrases[0].0, Some("spacing"));
            assert!(phrases[0].1.permalink);
            assert_eq!(phrases[0].2, Some("display"));
            assert_eq!(phrases[1].0, Some("in and around one-line displays"));
            assert!(
                displays[1]
                    .children()
                    .find(|node| node.kind() == NodeKind::Body)
                    .unwrap()
                    .children()
                    .next()
                    .is_none()
            );
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code.as_str() == DiagnosticCode::MDOC_EMPTY_BLOCK)
            );
        }
    }

    #[test]
    fn literal_display_marks_bare_parentheses_as_attached_delimiters() {
        let name = SourceName::new("mdoc-display-delimiters.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt DISPLAY 1\n.Os\n.Sh DESCRIPTION\n.Dl name ( ) command\n",
            ))
            .unwrap();
        let display = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("Dl"))
            .expect("Dl display");
        let body = display
            .children()
            .find(|node| node.kind() == NodeKind::Body)
            .expect("Dl body");
        let children = body.children().collect::<Vec<_>>();
        assert_eq!(
            children
                .iter()
                .filter_map(|node| node.text())
                .collect::<Vec<_>>(),
            ["name", "(", ")", "command"]
        );
        assert!(children[1].flags().delimiter_open);
        assert!(children[2].flags().delimiter_close);
    }

    #[test]
    fn reference_fields_coalesce_direct_text_without_erasing_inline_boundaries() {
        let name = SourceName::new("mdoc-reference-fields.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt REFERENCES 1\n.Os\n.Sh DESCRIPTION\n.Rs\n.%A author name\n.%B book title\n.Re\n",
            ))
            .unwrap();
        let fields = report
            .document
            .preorder()
            .filter(|node| matches!(node.macro_name(), Some("%A" | "%B")))
            .collect::<Vec<_>>();
        assert_eq!(fields.len(), 2);
        assert_eq!(
            fields
                .iter()
                .map(|field| {
                    field
                        .children()
                        .map(crate::NodeRef::text)
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>(),
            [vec![Some("author name")], vec![Some("book title")]]
        );
    }

    #[test]
    fn reference_fields_follow_the_legacy_bibliography_order() {
        let name = SourceName::new("mdoc-reference-order.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt REFERENCES 1\n.Os\n.Sh DESCRIPTION\n.Rs\n.%O note\n.%A author\n.%D date\n.%T title\n.Re\n",
            ))
            .unwrap();
        let fields = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Rs"))
            .unwrap()
            .children()
            .map(crate::NodeRef::macro_name)
            .collect::<Vec<_>>();
        assert_eq!(fields, [Some("%A"), Some("%T"), Some("%D"), Some("%O")]);
    }

    #[test]
    fn non_joining_reference_fields_keep_individual_words() {
        let name = SourceName::new("mdoc-reference-word-boundaries.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt REFERENCES 1\n.Os\n.Sh DESCRIPTION\n.Rs\n.%N number of journal\n.%A author name\n.Re\n",
            ))
            .unwrap();
        let fields = report
            .document
            .preorder()
            .filter(|node| matches!(node.macro_name(), Some("%A" | "%N")))
            .collect::<Vec<_>>();
        assert_eq!(
            fields[0]
                .children()
                .map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            [Some("author name")]
        );
        assert_eq!(
            fields[1]
                .children()
                .map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            [Some("number"), Some("of"), Some("journal")]
        );
    }

    #[test]
    fn reference_blocks_report_direct_text_and_inline_content() {
        let name = SourceName::new("mdoc-reference-content.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt REFERENCES 1\n.Os\n.Sh DESCRIPTION\n.Rs\n.%A author\nunexpected prose\n.Em unexpected emphasis\n.Re\n",
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
                    DiagnosticCode::MDOC_REFERENCE_CONTENT,
                    "invalid content in Rs block: text",
                ),
                (
                    DiagnosticCode::MDOC_REFERENCE_CONTENT,
                    "invalid content in Rs block: Em",
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
        assert_eq!(positions, [(7, 1), (8, 2)]);
    }

    #[test]
    fn reference_blocks_report_any_non_bibliographic_direct_macro() {
        let name = SourceName::new("mdoc-reference-macro-content.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt REFERENCES 1\n.Os\n.Sh DESCRIPTION\n.Rs\n.%A author\n.Tg target\n.Re\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [(
                DiagnosticCode::MDOC_REFERENCE_CONTENT,
                "invalid content in Rs block: Tg",
            )]
        );
    }

    #[test]
    fn reference_blocks_leave_their_first_direct_child_unvalidated() {
        let name = SourceName::new("mdoc-reference-first-child.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt REFERENCES 1\n.Os\n.Sh DESCRIPTION\n.Rs\n.Tg target\n.%A author\n.Re\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn transparent_tags_remain_destinations_around_reference_blocks() {
        let name = SourceName::new("mdoc-reference-transparent-tags.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt REFERENCES 1\n.Os\n.Sh DESCRIPTION\n.Tg before\n.Rs\n.%A author\n.Re\n.Rs\n.%A author\n.Tg inside\n.Re\n",
            ))
            .unwrap();
        let targets = report
            .document
            .preorder()
            .filter(|node| node.macro_name() == Some("Tg"))
            .map(|node| (node.flags().deep_link_target, node.tag()))
            .collect::<Vec<_>>();
        assert_eq!(targets, [(true, None), (true, None)]);
    }

    #[test]
    fn empty_reference_blocks_report_at_their_openers() {
        let name = SourceName::new("mdoc-empty-reference-blocks.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt REFERENCES 1\n.Os\n.Sh DESCRIPTION\n.Rs\n.Re\n.Rs\n",
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
                    DiagnosticCode::MDOC_EMPTY_REFERENCE_BLOCK,
                    "empty reference block: Rs",
                ),
                (
                    DiagnosticCode::MDOC_EMPTY_REFERENCE_BLOCK,
                    "empty reference block: Rs",
                ),
                (
                    DiagnosticCode::MDOC_UNCLOSED_BLOCK,
                    "appending missing end of block: Rs",
                ),
            ]
        );
        let positions = report
            .diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.primary.as_ref())
            .filter_map(|span| report.document.source_position(span))
            .map(|position| (position.line, position.column))
            .collect::<Vec<_>>();
        assert_eq!(positions, [(5, 2), (7, 2), (7, 2)]);
    }

    #[test]
    fn reference_heads_discard_arguments_after_the_leading_selector_diagnostic() {
        let name = SourceName::new("mdoc-reference-head-arguments.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt REFERENCES 1\n.Os\n.Sh SEE ALSO\n.Rs bogus\n.%A author\n.Re\n.Rs Sy bogus\n.%A author\n.Re\n",
            ))
            .unwrap();
        let heads = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("Rs"))
            .collect::<Vec<_>>();
        assert_eq!(heads.len(), 2);
        assert!(heads.iter().all(|head| head.children().next().is_none()));
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                (
                    DiagnosticCode::MDOC_ARGUMENTS,
                    "skipping all arguments: Rs bogus"
                ),
                (
                    DiagnosticCode::MDOC_ARGUMENTS,
                    "skipping all arguments: Rs Sy"
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
        assert_eq!(positions, [(5, 5), (8, 5)]);
    }

    #[test]
    fn section_and_subsection_titles_are_single_semantic_phrases() {
        let name = SourceName::new("mdoc-section-phrases.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SECTIONS 1\n.Os\n.Sh SEE ALSO\n.Ss Further Reading\n",
            ))
            .unwrap();
        let headings = report
            .document
            .preorder()
            .filter(|node| {
                node.kind() == NodeKind::Head && matches!(node.macro_name(), Some("Sh" | "Ss"))
            })
            .collect::<Vec<_>>();
        assert_eq!(headings.len(), 2);
        assert_eq!(
            headings
                .iter()
                .map(|head| head.children().next().and_then(crate::NodeRef::text))
                .collect::<Vec<_>>(),
            [Some("SEE ALSO"), Some("Further Reading")]
        );
        assert!(headings.iter().all(|head| head.children().nth(1).is_none()));
    }

    #[test]
    fn section_title_validation_uses_inline_visible_text() {
        let name = SourceName::new("mdoc-section-inline-visible.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SECTIONS 1\n.Os\n.Sh SEE ALSO\n.Sh SEE Em ALSO\n",
            ))
            .unwrap();
        let head = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("Sh"))
            .nth(1)
            .unwrap();
        assert_eq!(head.children().count(), 2);
        assert_eq!(
            head.children().next().and_then(crate::NodeRef::text),
            Some("SEE")
        );
        assert_eq!(
            head.children().nth(1).and_then(crate::NodeRef::macro_name),
            Some("Em")
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [(
                DiagnosticCode::MDOC_DUPLICATE_SECTION,
                "duplicate section title: Sh SEE ALSO",
            )]
        );
    }

    #[test]
    fn first_section_validation_uses_the_visible_heading() {
        let name = SourceName::new("mdoc-first-section.1").unwrap();
        let report = crate::Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SECTIONS 1\n.Os\n.Sh DESCRIPTION\ntext\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [(
                DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME,
                "first section is not \"NAME\": Sh DESCRIPTION",
            )]
        );
    }

    #[test]
    fn empty_section_headers_report_without_creating_blocks() {
        let name = SourceName::new("mdoc-empty-section-heads.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SECTIONS 1\n.Os\n.Sh NAME\n.Nm sections\n.Nd example\n.Sh\n.Ss\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                (DiagnosticCode::MDOC_EMPTY_MACRO, "skipping empty macro: Sh"),
                (DiagnosticCode::MDOC_EMPTY_MACRO, "skipping empty macro: Ss"),
            ]
        );
        assert_eq!(
            report
                .document
                .preorder()
                .filter(|node| {
                    node.kind() == NodeKind::Block && matches!(node.macro_name(), Some("Sh" | "Ss"))
                })
                .count(),
            1
        );
    }

    #[test]
    fn a_section_header_partial_block_is_closed_by_the_next_section() {
        let name = SourceName::new("mdoc-section-header-partial.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt HEADER-PARTIAL 1\n.Os\n.Sh SYNOPSIS\n.Sh DESCRIPTION Xo\n.Sh BUGS\nknown issue\n",
            ))
            .unwrap();
        let description = report
            .document
            .preorder()
            .find(|node| {
                node.kind() == NodeKind::Block
                    && node.macro_name() == Some("Sh")
                    && node
                        .children()
                        .find(|child| child.kind() == NodeKind::Head)
                        .and_then(|head| head.children().next())
                        .and_then(crate::NodeRef::text)
                        == Some("DESCRIPTION")
            })
            .unwrap();
        let description_head = description
            .children()
            .find(|node| node.kind() == NodeKind::Head)
            .unwrap();
        let xo = description_head
            .children()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Xo"))
            .unwrap();
        assert_eq!(xo.children().count(), 2);
        let description_body = description
            .children()
            .find(|node| node.kind() == NodeKind::Body)
            .unwrap();
        assert!(description_body.flags().line_start);
        let bugs = report
            .document
            .preorder()
            .find(|node| {
                node.kind() == NodeKind::Block
                    && node.macro_name() == Some("Sh")
                    && node
                        .children()
                        .find(|child| child.kind() == NodeKind::Head)
                        .and_then(|head| head.children().next())
                        .and_then(crate::NodeRef::text)
                        == Some("BUGS")
            })
            .unwrap();
        assert!(!bugs.flags().line_start);
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [(
                DiagnosticCode::MDOC_BROKEN_BLOCK,
                "inserting missing end of block: Sh breaks Xo",
            )]
        );
    }

    #[test]
    fn a_mismatched_partial_closer_reports_without_closing_the_active_scope() {
        let name = SourceName::new("mdoc-partial-not-open.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt PARTIAL-NOT-OPEN 1\n.Os\n.Sh DESCRIPTION\n.Ao ao\n.Bo bo pc\n.Pc bc\n.Bc ac\n.Ac tail\n",
            ))
            .unwrap();
        let bracket_body = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bo"))
            .unwrap();
        assert_eq!(
            bracket_body
                .children()
                .map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            [Some("bo pc bc")]
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [(
                DiagnosticCode::MDOC_UNMATCHED_CLOSE,
                "skipping end of block that is not open: Pc",
            )]
        );
    }

    #[test]
    fn configuration_directives_join_plain_arguments_before_trailing_punctuation() {
        let name = SourceName::new("mdoc-cd-phrase.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt CONFIG 1\n.Os\n.Sh DESCRIPTION\n.Cd options INSECURE .\n",
            ))
            .unwrap();
        let directive = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Cd"))
            .unwrap();
        assert_eq!(
            directive
                .children()
                .map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            [Some("options INSECURE")]
        );
        let period = report
            .document
            .preorder()
            .find(|node| node.text() == Some("."))
            .unwrap();
        assert!(period.flags().delimiter_close);
        assert!(period.flags().sentence_end);
    }

    #[test]
    fn empty_configuration_directive_is_discarded_with_a_typed_warning() {
        let name = SourceName::new("mdoc-empty-cd.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EMPTY-CD 1\n.Os\n.Sh DESCRIPTION\n.Cd\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            ["skipping empty macro: Cd"]
        );
        assert!(
            !report
                .document
                .preorder()
                .any(|node| node.macro_name() == Some("Cd"))
        );
    }

    #[test]
    fn empty_command_modifiers_report_without_leaking_private_elements() {
        let name = SourceName::new("mdoc-cm-noarg.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt CM 1\n.Os\n.Sh DESCRIPTION\n.Nm mt Fl f Ar device Cm\n.Nm ps Fl x Cm Fl o Cm command.\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "skipping empty macro: Cm",
                "skipping empty macro: Cm",
                "no blank before trailing delimiter: Cm command.",
            ]
        );
        assert!(
            report.document.preorder().all(|node| {
                node.macro_name() != Some("Cm") || node.children().next().is_some()
            })
        );
    }

    #[test]
    fn cd_leading_delimiters_stay_in_outer_flow_before_reopening() {
        let name = SourceName::new("mdoc-cd-leading-delimiters.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt CD 1\n.Os\n.Sh DESCRIPTION\n.Cd ) z\n.Cd ( a\n.Cd | m\n.Cd )\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        for punctuation in [")", "(", "|"] {
            let node = nodes
                .iter()
                .copied()
                .find(|node| node.text() == Some(punctuation) && node.flags().line_start)
                .unwrap();
            assert!(!node.flags().sentence_end);
            assert!(!node.flags().delimiter_close);
        }
        let opening = nodes
            .iter()
            .copied()
            .find(|node| node.text() == Some("(") && node.flags().line_start)
            .unwrap();
        assert!(opening.flags().delimiter_open);
        assert_eq!(
            nodes
                .iter()
                .copied()
                .filter(|node| node.macro_name() == Some("Cd"))
                .filter_map(|node| node.children().next().and_then(crate::NodeRef::text))
                .collect::<Vec<_>>(),
            ["z", "a", "m"]
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            ["skipping empty macro: Cd"]
        );
    }

    #[test]
    fn ic_delimiters_reopen_only_after_visible_words() {
        let name = SourceName::new("mdoc-ic-leading-delimiters.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt IC 1\n.Os\n.Sh DESCRIPTION\n.Ic ) z\n.Ic ( a\n.Ic | m\n.Ic )\n.Ic ) )\n",
            ))
            .unwrap();
        let body = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Sh"))
            .unwrap();
        let children = body.children().collect::<Vec<_>>();
        assert_eq!(children.len(), 9);
        for (index, punctuation) in [(0, ")"), (2, "("), (4, "|"), (6, ")"), (7, ")")] {
            assert_eq!(children[index].text(), Some(punctuation));
        }
        assert!(children[0].flags().line_start);
        assert!(!children[0].flags().delimiter_close);
        assert!(children[2].flags().delimiter_open);
        assert!(!children[4].flags().delimiter_close);
        assert!(children[6].flags().line_start);
        assert!(!children[6].flags().delimiter_close);
        assert!(children[7].flags().line_start);
        assert!(!children[7].flags().delimiter_close);
        assert!(children[8].flags().delimiter_close);
        assert_eq!(
            [children[1], children[3], children[5]]
                .into_iter()
                .map(|node| (
                    node.macro_name(),
                    node.children().next().and_then(crate::NodeRef::text),
                ))
                .collect::<Vec<_>>(),
            [
                (Some("Ic"), Some("z")),
                (Some("Ic"), Some("a")),
                (Some("Ic"), Some("m")),
            ]
        );
        assert!(
            children
                .iter()
                .all(|node| node.macro_name() != Some("Ic") || node.children().next().is_some())
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            ["skipping empty macro: Ic", "skipping empty macro: Ic"]
        );
    }

    #[test]
    fn nested_tag_trailing_punctuation_marks_only_a_terminal_sentence() {
        let name = SourceName::new("mdoc-nested-tag-terminal-punctuation.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt TAGS 1\n.Os\n.Sh DESCRIPTION\n.Li a Li .\n.Li a Li . Li b\n",
            ))
            .unwrap();
        let periods = report
            .document
            .preorder()
            .filter(|node| node.text() == Some("."))
            .collect::<Vec<_>>();
        assert_eq!(periods.len(), 2);
        assert!(periods[0].flags().sentence_end);
        assert!(!periods[0].flags().delimiter_close);
        assert!(!periods[1].flags().sentence_end);
        assert!(!periods[1].flags().delimiter_close);
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>(),
            [
                DiagnosticCode::MDOC_EMPTY_MACRO,
                DiagnosticCode::MDOC_EMPTY_MACRO
            ]
        );
    }

    #[test]
    fn link_macros_retain_internal_delimiters_and_validate_empty_forms() {
        let name = SourceName::new("mdoc-link-recovery.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt LINKS 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.test/ ,\n.Lk https://example.test/ label,\n.Lk\n",
            ))
            .unwrap();
        let links = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Lk"))
            .collect::<Vec<_>>();
        assert_eq!(links.len(), 2);
        let first_children = links[0].children().collect::<Vec<_>>();
        assert_eq!(first_children.len(), 2);
        assert_eq!(first_children[0].text(), Some("https://example.test/"));
        assert_eq!(first_children[1].text(), Some(","));
        assert!(first_children[1].flags().delimiter_close);
        assert_eq!(
            links[1]
                .children()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            ["https://example.test/", "label,"]
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "skipping empty macro: Lk",
                "no blank before trailing delimiter: Lk ... label,",
            ]
        );
    }

    #[test]
    fn explicit_tg_before_a_column_list_moves_destination_to_its_body() {
        let name = SourceName::new("mdoc-tg-column-list.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt TGLIST 1\n.Os\n.Sh DESCRIPTION\n.Tg list\n.Bl -column one two\n.It one Ta two\n.El\n",
            ))
            .unwrap();
        let list_body = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bl"))
            .unwrap();
        assert!(list_body.flags().deep_link_target);
        assert!(!list_body.flags().permalink);
        assert_eq!(list_body.tag(), Some("list"));
        let tg = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("Tg"))
            .unwrap();
        assert!(tg.flags().no_print);
    }

    #[test]
    fn column_lists_materialize_rows_without_explicit_item_controls() {
        let name = SourceName::new("mdoc-column-implicit-items.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt COLUMN 1\n.Os\n.Sh DESCRIPTION\n.Bl -column one two\n.Sy a Ta b\n.Em c Ta d\n.El\n.Bl -column one two\na\tb\nc\td\n.El\n",
            ))
            .unwrap();
        let items = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("It"))
            .collect::<Vec<_>>();
        assert_eq!(items.len(), 4);
        assert!(items[0].flags().deep_link_target);
        assert_eq!(items[0].tag(), Some("a"));
        assert!(
            items[0]
                .children()
                .find(|node| node.kind() == NodeKind::Body)
                .unwrap()
                .children()
                .any(|node| node.macro_name() == Some("Sy") && node.flags().permalink)
        );
        for item in items {
            assert_eq!(
                item.children()
                    .filter(|node| node.kind() == NodeKind::Body)
                    .count(),
                2
            );
        }
    }

    #[test]
    fn column_lists_group_consecutive_tbl_rows_in_one_implicit_item() {
        let name = SourceName::new("mdoc-column-tbl-item.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt COLUMN-TBL 1\n.Os\n.Sh DESCRIPTION\n.Bl -column one two\n.Sy a Ta b\n.TS\nll.\n1\t2\n3\t4\n.TE\n.Em c Ta d\n.El\n",
            ))
            .unwrap();
        let items = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("It"))
            .collect::<Vec<_>>();
        assert_eq!(items.len(), 3);
        let table_item = items[1];
        let head = table_item
            .children()
            .find(|node| node.kind() == NodeKind::Head)
            .unwrap();
        assert_eq!(head.children().count(), 0);
        let tables = table_item
            .children()
            .find(|node| node.kind() == NodeKind::Body)
            .unwrap()
            .children()
            .filter(|node| node.kind() == NodeKind::Table)
            .count();
        assert_eq!(tables, 2);
    }

    #[test]
    fn list_item_headers_can_extend_through_explicit_partial_blocks() {
        let name = SourceName::new("mdoc-item-header-extension.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EXTEND 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Ao\n.No extended tag\n.Ac\nextended text\n.It prefix Ao\n.No prefixed tag\n.Ac\nprefixed text\n.El\n",
            ))
            .unwrap();
        let items = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("It"))
            .collect::<Vec<_>>();
        assert_eq!(items.len(), 2);
        for item in items {
            let head = item
                .children()
                .find(|node| node.kind() == NodeKind::Head)
                .unwrap();
            let enclosure = head
                .children()
                .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Ao"))
                .unwrap();
            assert!(
                enclosure
                    .children()
                    .find(|node| node.kind() == NodeKind::Body)
                    .unwrap()
                    .children()
                    .any(|node| node.macro_name() == Some("No"))
            );
            assert!(
                item.children()
                    .find(|node| node.kind() == NodeKind::Body)
                    .unwrap()
                    .flags()
                    .line_start
            );
        }
    }

    #[test]
    fn explicit_tg_before_list_items_selects_the_legacy_item_part() {
        let name = SourceName::new("mdoc-tg-list-item.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt TGLISTITEM 1\n.Os\n.Sh DESCRIPTION\n.Bl -dash\n.Tg bullet\n.It\nbody\n.El\n.Bl -tag\n.Tg term\n.It name\nbody\n.El\n",
            ))
            .unwrap();
        let items = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("It"))
            .collect::<Vec<_>>();
        assert_eq!(items.len(), 2);
        let bullet_body = items[0]
            .children()
            .find(|node| node.kind() == NodeKind::Body)
            .unwrap();
        assert!(bullet_body.flags().deep_link_target);
        assert!(bullet_body.flags().permalink);
        assert_eq!(bullet_body.tag(), Some("bullet"));
        let definition_head = items[1]
            .children()
            .find(|node| node.kind() == NodeKind::Head)
            .unwrap();
        assert!(definition_head.flags().deep_link_target);
        assert!(definition_head.flags().permalink);
        assert_eq!(definition_head.tag(), Some("term"));
        assert_eq!(
            report
                .document
                .preorder()
                .filter(|node| node.macro_name() == Some("Tg"))
                .filter(|node| node.flags().no_print)
                .count(),
            2
        );
    }

    #[test]
    fn list_item_long_option_prefix_collapses_adjacent_fl_macros() {
        let name = SourceName::new("mdoc-list-long-option.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt LONGOPTION 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag\n.It Fl Fl long\nbody\n.El\n",
            ))
            .unwrap();
        let item_head = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("It"))
            .unwrap();
        let flag = item_head.children().next().unwrap();
        assert_eq!(flag.macro_name(), Some("Fl"));
        assert_eq!(
            flag.children().next().and_then(crate::NodeRef::text),
            Some("\\-long")
        );
        assert_eq!(item_head.children().count(), 1);
        assert_eq!(item_head.tag(), Some("long"));
        assert!(flag.flags().permalink);
    }

    #[test]
    fn font_blocks_accept_legacy_macro_name_aliases() {
        let name = SourceName::new("mdoc-bf-aliases.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt BF 1\n.Os\n.Sh DESCRIPTION\n.Bf Em\n.Bf Li\n.Bf Sy\n.Ef\n.Ef\n.Ef\n",
            ))
            .unwrap();
        let blocks = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bf"))
            .collect::<Vec<_>>();
        let fonts = blocks.iter().map(|block| block.font()).collect::<Vec<_>>();
        assert_eq!(
            fonts,
            [
                Some(NormalizedFont::Emphasis),
                Some(NormalizedFont::Literal),
                Some(NormalizedFont::Symbolic),
            ]
        );
        assert_eq!(
            blocks
                .iter()
                .map(|block| block
                    .children()
                    .next()
                    .and_then(|head| head.children().next()))
                .map(|word| word.and_then(crate::NodeRef::text))
                .collect::<Vec<_>>(),
            [Some("Em"), Some("Li"), Some("Sy")]
        );
    }

    #[test]
    fn emphasis_coalesces_a_plain_argument_phrase() {
        let name = SourceName::new("mdoc-em-phrase.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EM 1\n.Os\n.Sh DESCRIPTION\n.Em several plain words\n",
            ))
            .unwrap();
        let emphasis = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Em"))
            .unwrap();
        assert_eq!(emphasis.children().count(), 1);
        assert_eq!(
            emphasis.children().next().and_then(crate::NodeRef::text),
            Some("several plain words")
        );
    }

    #[test]
    fn paragraphs_are_elements_and_tg_moves_its_permalink_to_following_text() {
        let name = SourceName::new("mdoc-tg-paragraph.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt TAG 1\n.Os\n.Sh NAME\n.Nm tag\n.Nd tag test\n.Sh DESCRIPTION\n.Tg anchor\n.Pp\nalpha beta\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let paragraph = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Pp"))
            .unwrap();
        assert!(paragraph.flags().deep_link_target);
        assert!(!paragraph.flags().permalink);
        assert_eq!(paragraph.tag(), Some("anchor"));

        let tg = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Tg"))
            .unwrap();
        assert!(tg.flags().no_print);

        let alpha = nodes
            .iter()
            .copied()
            .find(|node| node.text() == Some("alpha"))
            .unwrap();
        let beta = nodes
            .iter()
            .copied()
            .find(|node| node.text() == Some("beta"))
            .unwrap();
        assert!(alpha.flags().permalink);
        assert_eq!(alpha.tag(), Some("anchor"));
        assert!(!beta.flags().permalink);
        assert_eq!(beta.tag(), None);
        assert!(!beta.flags().line_start);
    }

    #[test]
    fn tg_recovers_invalid_spelling_and_keeps_consecutive_destination_topology() {
        let name = SourceName::new("mdoc-tg-recovery.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt TG-RECOVERY 1\n.Os\n.Sh DESCRIPTION\nintro\n.Pp\n.Tg start\ntext\n.Tg sub\n.Tg double\n.Ss Details\n.Tg \"\" ignored\n.Tg \\&bad\n.Tg\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .filter(|code| {
                    matches!(
                        *code,
                        "mdoc.empty-macro" | "mdoc.arguments" | "mdoc.invalid-tag"
                    )
                })
                .collect::<Vec<_>>(),
            [
                "mdoc.empty-macro",
                "mdoc.arguments",
                "mdoc.invalid-tag",
                "mdoc.empty-macro",
            ]
        );

        let nodes = report.document.preorder().collect::<Vec<_>>();
        let paragraph = nodes
            .iter()
            .copied()
            .find(|node| node.macro_name() == Some("Pp"))
            .unwrap();
        assert_eq!(paragraph.tag(), Some("start"));
        let sub = nodes
            .iter()
            .copied()
            .find(|node| {
                node.macro_name() == Some("Tg")
                    && node.children().next().and_then(crate::NodeRef::text) == Some("sub")
            })
            .unwrap();
        assert!(sub.flags().deep_link_target);
        assert_eq!(sub.tag(), None);
        let subsection = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("Ss"))
            .unwrap();
        assert_eq!(subsection.tag(), Some("double"));
        assert!(nodes.iter().copied().all(|node| {
            !(node.macro_name() == Some("Tg")
                && node.children().next().and_then(crate::NodeRef::text) == Some("\\&bad"))
        }));
    }

    #[test]
    fn parsed_inline_macros_diagnose_known_noncallable_spellings() {
        let name = SourceName::new("mdoc-non-callable-inline.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt NONCALLABLE 1\n.Os\n.Sh DESCRIPTION\n.Ic Dd\n.Ic \\&Dd\n.In Dd\n",
            ))
            .unwrap();
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code.as_str() == DiagnosticCode::MDOC_NON_CALLABLE_MACRO)
            .unwrap();
        assert_eq!(
            diagnostic.message.as_ref(),
            "macro neither callable nor escaped: Dd"
        );
        let position = diagnostic
            .primary
            .as_ref()
            .and_then(|span| report.document.source_position(span))
            .unwrap();
        assert_eq!((position.line, position.column), (5, 5));
        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code.as_str()
                    == DiagnosticCode::MDOC_NON_CALLABLE_MACRO)
                .count(),
            1
        );
    }

    #[test]
    fn callable_inline_macros_split_scanner_tokens_without_losing_delimiters() {
        let name = SourceName::new("mdoc-inline-sequence.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt INLINE 1\n.Os\n.Sh NAME\n.Nm inline\n.Nd inline test\n.Sh DESCRIPTION\n.Nm tool Fl f Ar path Cm pid , Ns Cm command\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        for (macro_name, text) in [("Fl", "f"), ("Ar", "path"), ("Cm", "pid")] {
            assert!(nodes.iter().copied().any(|node| {
                node.kind() == NodeKind::Element
                    && node.macro_name() == Some(macro_name)
                    && node.children().next().and_then(crate::NodeRef::text) == Some(text)
            }));
        }
        let delimiter = nodes
            .iter()
            .copied()
            .find(|node| node.text() == Some(","))
            .unwrap();
        assert!(delimiter.flags().delimiter_close);
        let no_space = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Ns"))
            .unwrap();
        assert_eq!(no_space.children().count(), 0);
    }

    #[test]
    fn ar_reopens_around_mdoc_delimiters_without_synthesizing_empty_defaults() {
        let name = SourceName::new("mdoc-ar-punctuation.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt AR 1\n.Os\n.Sh DESCRIPTION\n.Ar | m\n.Ar ( a\n.Ar a \"(\" b\n.Ar . z\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let arguments = nodes
            .iter()
            .copied()
            .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Ar"))
            .filter_map(|node| node.children().next().and_then(crate::NodeRef::text))
            .collect::<Vec<_>>();
        assert_eq!(arguments, ["m", "a", "a", "b", "file", "z"]);
        assert_eq!(
            nodes
                .iter()
                .filter(|node| node.text() == Some("file") && node.flags().generated)
                .count(),
            1,
            "only the closing-delimiter form defaults; the initial `|` does not"
        );
        let opening = nodes
            .iter()
            .copied()
            .find(|node| node.text() == Some("(") && node.flags().delimiter_open)
            .unwrap();
        assert!(opening.flags().line_start);
        let dot = nodes
            .iter()
            .copied()
            .find(|node| node.text() == Some("."))
            .unwrap();
        assert!(dot.flags().delimiter_close);
        assert!(!dot.flags().sentence_end);
    }

    #[test]
    fn formatter_reset_wrapped_bar_reopens_mdoc_inline_macro() {
        let name = SourceName::new("mdoc-inline-reset-bar.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt INLINE 1\n.Os\n.Sh DESCRIPTION\n.Fl isolated | em|bedded \\fR|\\fP formatted\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let arguments = nodes
            .iter()
            .copied()
            .filter(|node| node.macro_name() == Some("Fl"))
            .filter_map(|node| node.children().next().and_then(crate::NodeRef::text))
            .collect::<Vec<_>>();
        assert_eq!(arguments, ["isolated", "em|bedded", "formatted"]);
        assert!(
            nodes
                .iter()
                .copied()
                .any(|node| node.text() == Some(r"\fR|\fP"))
        );
    }

    #[test]
    fn symbolic_inline_macro_coalesces_an_unsplit_source_phrase() {
        let name = SourceName::new("mdoc-symbolic-phrase.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt INLINE 1\n.Os\n.Sh DESCRIPTION\n.Sy isolated \\(ba em\\(babedded \\fR\\(ba\\fP formatted\n",
            ))
            .unwrap();
        let symbolic = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("Sy"))
            .unwrap();
        assert_eq!(symbolic.children().count(), 1);
        assert_eq!(
            symbolic.children().next().and_then(crate::NodeRef::text),
            Some(r"isolated \(ba em\(babedded \fR\(ba\fP formatted")
        );
    }

    #[test]
    fn filled_mdoc_text_trims_physical_line_end_whitespace() {
        let name = SourceName::new("mdoc-trailing-whitespace.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt WHITESPACE 1\n.Os\n.Sh DESCRIPTION\nvisible  \n.Bd -literal\nliteral  \n.Ed\n",
            ))
            .unwrap();
        let text = report
            .document
            .preorder()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert!(text.contains(&"visible"));
        assert!(text.contains(&"literal"));
        assert!(!text.iter().any(|value| value.ends_with([' ', '\t'])));
    }

    #[test]
    fn system_name_macros_insert_generated_words_and_leave_periods_in_flow() {
        let name = SourceName::new("mdoc-system-names.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SYSTEMS 1\n.Os\n.Sh DESCRIPTION\n.Ux .\n.Bx .\n.Bsx .\n.Nx .\n.Fx .\n.Ox .\n.Dx .\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        for (macro_name, generated) in [
            ("Ux", "UNIX"),
            ("Bx", "BSD"),
            ("Bsx", "BSD/OS"),
            ("Nx", "NetBSD"),
            ("Fx", "FreeBSD"),
            ("Ox", "OpenBSD"),
            ("Dx", "DragonFly"),
        ] {
            let system = nodes
                .iter()
                .copied()
                .find(|node| node.macro_name() == Some(macro_name))
                .unwrap();
            let word = system.children().next().unwrap();
            assert_eq!(word.text(), Some(generated));
            assert!(word.flags().generated);
        }
        let periods = nodes
            .iter()
            .copied()
            .filter(|node| node.text() == Some("."))
            .collect::<Vec<_>>();
        assert_eq!(periods.len(), 7);
        assert!(periods.iter().all(|node| node.flags().delimiter_close));
        assert!(periods.iter().all(|node| node.flags().sentence_end));
    }

    #[test]
    fn compact_system_names_validate_attached_version_delimiters() {
        let name = SourceName::new("mdoc-system-name-delimiters.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt SYSTEMS 1\n.Os\n.Sh NAME\n.Nm systems\n.Nd delimiter validation\n.Sh DESCRIPTION\n.Bsx 5.1,\n.Dx 4.8.0,\n.Fx 11.0,\n.Nx 7.1,\n.Ox 6.1.\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "no blank before trailing delimiter: Bsx 5.1,",
                "no blank before trailing delimiter: Dx 4.8.0,",
                "no blank before trailing delimiter: Fx 11.0,",
                "no blank before trailing delimiter: Nx 7.1,",
                "no blank before trailing delimiter: Ox 6.1.",
            ]
        );
    }

    #[test]
    fn implicit_partial_blocks_expand_nested_system_name_macros() {
        let name = SourceName::new("mdoc-partial-system-name.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt PARTIAL 1\n.Os\n.Sh DESCRIPTION\n.Op Fl Ux\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let ux = nodes
            .iter()
            .copied()
            .find(|node| node.macro_name() == Some("Ux"))
            .unwrap();
        let generated = ux.children().collect::<Vec<_>>();
        assert_eq!(generated.len(), 1);
        assert_eq!(generated[0].text(), Some("UNIX"));
        assert!(generated[0].flags().generated);

        let fl = nodes
            .iter()
            .copied()
            .find(|node| node.macro_name() == Some("Fl"))
            .unwrap();
        assert!(fl.children().next().is_none());
        assert_eq!(fl.parent().and_then(crate::NodeRef::macro_name), Some("Op"));
        assert_eq!(
            fl.parent()
                .unwrap()
                .children()
                .map(crate::NodeRef::macro_name)
                .collect::<Vec<_>>(),
            [Some("Fl"), Some("Ux")]
        );
        let ux_line = ux.source_position().unwrap().line;
        assert_eq!(fl.source_position().unwrap().line, ux_line);
    }

    #[test]
    fn flags_validate_an_attached_trailing_delimiter_after_argument_expansion() {
        let name = SourceName::new("mdoc-flag-delimiter.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt FLAGS 1\n.Os\n.Sh DESCRIPTION\n.Fl a.\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            ["no blank before trailing delimiter: Fl a."]
        );
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            DiagnosticCode::MDOC_TRAILING_DELIMITER_SPACING
        );
        let position = report
            .document
            .source_position(report.diagnostics[0].primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((position.line, position.column), (5, 6));
    }

    #[test]
    fn a_flag_followed_by_es_keeps_the_enclosure_in_outer_flow() {
        let name = SourceName::new("mdoc-flag-es.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt FLAGS 1\n.Os\n.Sh DESCRIPTION\n.Fl Es < >\n",
            ))
            .unwrap();
        let children = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Sh"))
            .unwrap()
            .children()
            .collect::<Vec<_>>();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].macro_name(), Some("Fl"));
        assert!(children[0].children().next().is_none());
        assert_eq!(children[1].macro_name(), Some("Es"));
        assert_eq!(
            children[1]
                .children()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            ["<", ">"]
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            ["obsolete macro: Es"]
        );
    }

    #[test]
    fn cross_line_xo_closers_resume_their_control_line_tail() {
        let name = SourceName::new("mdoc-xo-tail.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt XO 1\n.Os\n.Sh DESCRIPTION\n.Pp\n.Xo Fl\n.Tg transparent\n.Xc suffix\n",
            ))
            .unwrap();
        let suffix = report
            .document
            .preorder()
            .find(|node| node.text() == Some("suffix"))
            .unwrap();
        assert_eq!(
            suffix.parent().and_then(crate::NodeRef::macro_name),
            Some("Sh")
        );
        assert!(suffix.flags().line_start);
        assert_eq!(
            suffix
                .location()
                .and_then(|span| report.document.source_position(span))
                .map(|position| (position.line, position.column)),
            Some((8, 5))
        );
        let paragraph = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("Pp"))
            .unwrap();
        assert_eq!(paragraph.tag(), Some("transparent"));
        assert!(paragraph.flags().deep_link_target);
        let transparent = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("Tg"))
            .unwrap();
        assert!(transparent.flags().no_print);
    }

    #[test]
    fn transparent_tags_after_empty_flags_split_targets_from_permalinks() {
        let name = SourceName::new("mdoc-transparent-flag-tag.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt FLAGS 1\n.Os\n.Sh DESCRIPTION\n.Pp\n.Fl\n.Tg transparent\n.Em word\n",
            ))
            .unwrap();
        let paragraph = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("Pp"))
            .unwrap();
        assert_eq!(paragraph.tag(), Some("transparent"));
        assert!(paragraph.flags().deep_link_target);
        let emphasis = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("Em"))
            .unwrap();
        assert_eq!(emphasis.tag(), Some("transparent"));
        assert!(!emphasis.flags().deep_link_target);
        assert!(emphasis.flags().permalink);
    }

    #[test]
    fn empty_function_declaration_macros_are_removed_after_validation() {
        let name = SourceName::new("mdoc-empty-function-declarations.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt FUNCTIONS 1\n.Os\n.Sh DESCRIPTION\n.Fo function excess\n.Fa\n.Fc\n.Ft\n.Fn\n",
            ))
            .unwrap();
        let head = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("Fo"))
            .unwrap();
        assert_eq!(
            head.children()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            ["function"]
        );
        assert!(
            report
                .document
                .preorder()
                .all(|node| { !matches!(node.macro_name(), Some("Fa" | "Fn" | "Ft")) })
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "skipping empty macro: Fa",
                "skipping empty macro: Ft",
                "skipping empty macro: Fn",
                "skipping excess arguments: Fo ... excess",
            ]
        );
    }

    #[test]
    fn repeated_automatic_function_spellings_keep_only_the_first_destination() {
        let name = SourceName::new("mdoc-repeated-function-targets.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt FUNCTIONS 1\n.Os\n.Sh DESCRIPTION\n.Ft int\n.Fn abs \"int i\"\n.Ft int\n.Fn abs \"int i\"\n.Fo labs\n.Fc\n.Fo labs\n.Fc\n",
            ))
            .unwrap();
        let functions = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Fn"))
            .collect::<Vec<_>>();
        assert_eq!(functions.len(), 2);
        assert!(functions[0].flags().deep_link_target);
        assert!(!functions[1].flags().deep_link_target);
        assert!(functions.iter().all(|node| node.tag().is_none()));

        let function_heads = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("Fo"))
            .collect::<Vec<_>>();
        assert_eq!(function_heads.len(), 2);
        assert!(function_heads[0].flags().deep_link_target);
        assert!(!function_heads[1].flags().deep_link_target);
        assert!(function_heads.iter().all(|node| node.tag().is_none()));
    }

    #[test]
    fn empty_fo_head_retains_its_block_and_reports_the_missing_function_name() {
        let name = SourceName::new("mdoc-empty-fo-head.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt FUNCTIONS 1\n.Os\n.Sh DESCRIPTION\n.Fo\n.Fa int\n.Fc\n",
            ))
            .unwrap();
        let head = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("Fo"))
            .unwrap();
        assert_eq!(head.children().count(), 0);
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [(
                "mdoc.function-name-missing",
                "missing function name, using \"\": Fo"
            )]
        );
    }

    #[test]
    fn obsolete_function_macros_preserve_their_distinct_public_forms() {
        let name = SourceName::new("mdoc-obsolete-function-macros.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt FUNCTIONS 1\n.Os\n.Sh DESCRIPTION\n.Ot fortran\n.Fr value\n",
            ))
            .unwrap();
        let macros = report
            .document
            .preorder()
            .filter_map(crate::NodeRef::macro_name)
            .collect::<Vec<_>>();
        assert!(macros.contains(&"Ft"));
        assert!(macros.contains(&"Fr"));
        assert!(!macros.contains(&"Ot"));
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                ("mdoc.obsolete", "obsolete macro: Ot"),
                ("mdoc.obsolete", "obsolete macro: Fr"),
            ]
        );
    }

    #[test]
    fn function_declaration_macros_defer_attached_punctuation_validation() {
        let name = SourceName::new("mdoc-function-punctuation.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt FUNCTIONS 1\n.Os\n.Sh DESCRIPTION\n.Ft double\n.Fn sin. \",\" cos \"Em\" italic\n.Pp\n.Fa x \",\" y: \"Sy\" bold\n.Pp\n.Ft int \",\" float: \"Sy\" bold\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    (
                        diagnostic.code.as_str(),
                        diagnostic.message.as_ref(),
                        diagnostic
                            .primary
                            .as_ref()
                            .and_then(|span| report.document.source_position(span))
                            .map(|position| (position.line, position.column)),
                    )
                })
                .collect::<Vec<_>>(),
            [
                (
                    "mdoc.trailing-delimiter-spacing",
                    "no blank before trailing delimiter: Fn sin.",
                    Some((6, 8)),
                ),
                (
                    "mdoc.trailing-delimiter-spacing",
                    "no blank before trailing delimiter: Fa y:",
                    Some((8, 12)),
                ),
                (
                    "mdoc.trailing-delimiter-spacing",
                    "no blank before trailing delimiter: Ft float:",
                    Some((10, 18)),
                ),
            ]
        );
        let function = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("Fn"))
            .unwrap();
        assert!(function.flags().deep_link_target);
        assert_eq!(function.tag(), None);
    }

    #[test]
    fn mailto_macro_validates_attached_trailing_punctuation() {
        let name = SourceName::new("mdoc-mailto-punctuation.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt MAIL 1\n.Os\n.Sh DESCRIPTION\n.Mt punctuation@localhost.\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    (
                        diagnostic.code.as_str(),
                        diagnostic.message.as_ref(),
                        diagnostic
                            .primary
                            .as_ref()
                            .and_then(|span| report.document.source_position(span))
                            .map(|position| (position.line, position.column)),
                    )
                })
                .collect::<Vec<_>>(),
            [(
                "mdoc.trailing-delimiter-spacing",
                "no blank before trailing delimiter: Mt punctuation@localhost.",
                Some((5, 26)),
            )]
        );
    }

    #[test]
    fn empty_mailto_macro_generates_a_nonbreaking_space_word() {
        let name = SourceName::new("mdoc-empty-mailto.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt MAIL 1\n.Os\n.Sh DESCRIPTION\n.Mt .\n",
            ))
            .unwrap();
        let mailto = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("Mt"))
            .unwrap();
        let default = mailto.children().next().unwrap();
        assert_eq!(default.text(), Some("~"));
        assert!(default.flags().generated);
    }

    #[test]
    fn description_blocks_own_following_paragraphs_and_validate_after_closure() {
        let name = SourceName::new("mdoc-nd-paragraph.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".\\\" $OpenBSD: par.in,v 1.2 2017/07/04 14:53:25 schwarze Exp $\n.Dd $Mdocdate: July 4 2017 $\n.Dt ND-PAR 1\n.Os\n.Sh NAME\n.Nm Nd-par\n.Nd paragraph macro\nafter one-line description\n.Pp\nUsually, there shouldn't be additional text in the NAME section.\n.Sh DESCRIPTION\nThe text belongs here.\n.Nd stray\ndescription macro\n.Pp\nBack to normal state.\n",
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
                    "mdoc.trailing-delimiter",
                    "trailing delimiter: Nd ... Usually, there shouldn't be additional text in the NAME section.",
                ),
                (
                    "mdoc.description-outside-name",
                    "description line outside NAME section: Nd",
                ),
                (
                    "mdoc.trailing-delimiter",
                    "trailing delimiter: Nd ... Back to normal state.",
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
        assert_eq!(positions, [(10, 64), (13, 2), (16, 21)]);

        for text in [
            "Usually, there shouldn't be additional text in the NAME section.",
            "Back to normal state.",
        ] {
            let node = report
                .document
                .preorder()
                .find(|node| node.text() == Some(text))
                .unwrap();
            assert_eq!(
                node.parent().and_then(crate::NodeRef::macro_name),
                Some("Nd")
            );
        }
    }

    #[test]
    fn empty_description_reports_when_its_body_closes() {
        let name = SourceName::new("mdoc-nd-empty.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt ND 1\n.Os\n.Sh NAME\n.Nm nd\n.Nd\n.Sh DESCRIPTION\ntext\n",
            ))
            .unwrap();

        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [(
                "mdoc.description-missing",
                "missing description line, using \"\": Nd",
            )]
        );
        let position = report
            .document
            .source_position(report.diagnostics[0].primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((position.line, position.column), (6, 2));
    }

    #[test]
    fn bx_inserts_no_space_nodes_and_title_cases_its_second_argument() {
        let name = SourceName::new("mdoc-bx.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt BX 1\n.Os\n.Sh DESCRIPTION\n.Bx 4.3 tahoe\n.Bx nett.\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let bx = nodes
            .iter()
            .copied()
            .filter(|node| node.macro_name() == Some("Bx"))
            .collect::<Vec<_>>();
        assert_eq!(bx.len(), 2);
        assert_eq!(
            bx[0]
                .children()
                .map(|child| (child.macro_name(), child.text(), child.flags().generated))
                .collect::<Vec<_>>(),
            [
                (None, Some("4.3"), false),
                (Some("Ns"), None, true),
                (None, Some("BSD"), true),
                (Some("Ns"), None, true),
                (None, Some("-"), true),
                (Some("Ns"), None, true),
                (None, Some("Tahoe"), false),
            ]
        );
        assert_eq!(
            bx[1]
                .children()
                .map(|child| (child.macro_name(), child.text(), child.flags().generated))
                .collect::<Vec<_>>(),
            [
                (None, Some("nett."), false),
                (Some("Ns"), None, true),
                (None, Some("BSD"), true),
            ]
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            ["no blank before trailing delimiter: Bx nett."]
        );
    }

    #[test]
    fn bx_quoted_trailing_delimiter_does_not_end_a_sentence() {
        let name = SourceName::new("mdoc-bx-quoted-delimiter.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt BX 1\n.Os\n.Sh DESCRIPTION\n.Bx 4.4 \".\"\n",
            ))
            .unwrap();
        let delimiter = report
            .document
            .preorder()
            .find(|node| node.text() == Some("."))
            .unwrap();
        assert!(delimiter.flags().delimiter_close);
        assert!(!delimiter.flags().sentence_end);
    }

    #[test]
    fn word_keep_blocks_discard_options_and_scope_system_name_flow() {
        let name = SourceName::new("mdoc-word-keep.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt KEEP 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.Ox 4.9 must remain together.\n.Ek\n",
            ))
            .unwrap();
        let keep = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bk"))
            .unwrap();
        let head = keep.children().next().unwrap();
        let body = keep.children().nth(1).unwrap();
        assert_eq!(head.kind(), NodeKind::Head);
        assert_eq!(head.children().count(), 0);
        assert_eq!(body.kind(), NodeKind::Body);
        assert!(body.children().next().is_some());
        let openbsd = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("Ox"))
            .unwrap();
        assert_eq!(
            openbsd
                .children()
                .map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            [Some("OpenBSD"), Some("4.9")]
        );
        assert!(
            report
                .document
                .preorder()
                .all(|node| node.macro_name() != Some("Ek"))
        );
    }

    #[test]
    fn synopsis_no_keeps_separate_words_and_fn_does_not_target_preceding_paragraph() {
        let name = SourceName::new("mdoc-synopsis-no.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SYNOPSIS 1\n.Os\n.Sh SYNOPSIS\n.No two words\n.Pp\n.Fn example\n",
            ))
            .unwrap();
        let no = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("No"))
            .unwrap();
        assert_eq!(
            no.children()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            ["two", "words"]
        );
        let paragraph = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Pp"))
            .unwrap();
        assert!(!paragraph.flags().deep_link_target);
    }

    #[test]
    fn empty_bk_reports_then_disappears_from_the_public_tree() {
        let name = SourceName::new("mdoc-empty-bk.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EMPTY-BK 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.Ek\n",
            ))
            .unwrap();
        assert!(
            report
                .document
                .preorder()
                .all(|node| node.macro_name() != Some("Bk"))
        );
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            DiagnosticCode::MDOC_EMPTY_BLOCK
        );
        assert_eq!(report.diagnostics[0].severity, Severity::Warning);
        assert_eq!(report.diagnostics[0].message.as_ref(), "empty block: Bk");
    }

    #[test]
    fn standard_exit_status_expands_generated_prose_and_name_list() {
        let name = SourceName::new("mdoc-ex-standard.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EXIT 1\n.Os\n.Sh EXIT STATUS\n.Ex -std first second\n",
            ))
            .unwrap();
        let exit_status = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Ex"))
            .unwrap();
        let children = exit_status.children().collect::<Vec<_>>();
        assert_eq!(children.len(), 6);
        assert_eq!(children[0].text(), Some("The"));
        assert_eq!(children[2].text(), Some("and"));
        assert_eq!(children[4].text(), Some("utilities exit\\~0"));
        assert_eq!(
            children[5].text(),
            Some("on success, and\\~>0 if an error occurs.")
        );
        assert!(children[0].flags().generated);
        assert!(children[5].flags().sentence_end);
        for (element, name) in [(children[1], "first"), (children[3], "second")] {
            assert_eq!(element.macro_name(), Some("Nm"));
            assert!(element.flags().generated);
            assert_eq!(
                element.children().next().and_then(crate::NodeRef::text),
                Some(name)
            );
        }
    }

    #[test]
    fn standard_return_value_expands_function_list_and_errno_clause() {
        let name = SourceName::new("mdoc-rv-standard.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt RETURNS 3\n.Os\n.Sh RETURN VALUES\n.Rv -std first second\n",
            ))
            .unwrap();
        let return_value = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Rv"))
            .unwrap();
        let children = return_value.children().collect::<Vec<_>>();
        assert_eq!(children.len(), 9);
        assert_eq!(children[0].text(), Some("The"));
        assert_eq!(children[2].text(), Some("and"));
        assert_eq!(children[4].text(), Some("functions return"));
        assert_eq!(children[5].text(), Some("the value\\~0 if successful;"));
        for (function, name) in [(children[1], "first"), (children[3], "second")] {
            assert_eq!(function.macro_name(), Some("Fn"));
            assert!(function.flags().generated);
            assert_eq!(
                function.children().next().and_then(crate::NodeRef::text),
                Some(name)
            );
        }
        assert_eq!(children[7].macro_name(), Some("Va"));
        assert_eq!(
            children[7].children().next().and_then(crate::NodeRef::text),
            Some("errno")
        );
        assert!(children[8].flags().sentence_end);
    }

    #[test]
    fn missing_standard_selectors_recover_to_standard_exit_and_return_expansions() {
        let name = SourceName::new("mdoc-missing-standard-selector.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt STANDARD-RECOVERY 1\n.Os\n.Sh EXIT STATUS\n.Ex utility\n.Sh RETURN VALUES\n.Rv function\n",
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
                    DiagnosticCode::MDOC_STANDARD_SELECTOR_MISSING,
                    "missing -std argument, adding it: Ex",
                ),
                (
                    DiagnosticCode::MDOC_SECTION_ORDER,
                    "sections out of conventional order: Sh RETURN VALUES",
                ),
                (
                    DiagnosticCode::MDOC_UNEXPECTED_SECTION,
                    "unexpected section: Sh RETURN VALUES for 2, 3, 9 only",
                ),
                (
                    DiagnosticCode::MDOC_STANDARD_SELECTOR_MISSING,
                    "missing -std argument, adding it: Rv",
                ),
            ]
        );
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let exit_status = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Ex"))
            .unwrap();
        assert_eq!(
            exit_status.children().next().and_then(crate::NodeRef::text),
            Some("The")
        );
        assert!(
            exit_status
                .children()
                .any(|child| child.macro_name() == Some("Nm")
                    && child.children().next().and_then(crate::NodeRef::text) == Some("utility"))
        );
        let return_value = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Rv"))
            .unwrap();
        assert_eq!(
            return_value
                .children()
                .next()
                .and_then(crate::NodeRef::text),
            Some("The")
        );
        assert!(
            return_value
                .children()
                .any(|child| child.macro_name() == Some("Fn")
                    && child.children().next().and_then(crate::NodeRef::text) == Some("function"))
        );
    }

    #[test]
    fn pf_owns_exactly_one_literal_argument_before_inline_flow_resumes() {
        let name = SourceName::new("mdoc-pf-one-argument.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt PF 1\n.Os\n.Sh DESCRIPTION\n.Pf Ar Ns Ar path\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let prefix = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Pf"))
            .unwrap();
        assert_eq!(
            prefix.children().next().and_then(crate::NodeRef::text),
            Some("Ar")
        );
        let no_space = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Ns"))
            .unwrap();
        assert_eq!(no_space.children().count(), 0);
        let argument = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Ar"))
            .unwrap();
        assert_eq!(
            argument.children().next().and_then(crate::NodeRef::text),
            Some("path")
        );
    }

    #[test]
    fn pf_keeps_a_leading_closing_delimiter_as_its_literal_prefix() {
        let name = SourceName::new("mdoc-pf-leading-close.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt PF 1\n.Os\n.Sh DESCRIPTION\n.Pf . right .\n.Em eos Pf .\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let prefixes = nodes
            .iter()
            .copied()
            .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Pf"))
            .collect::<Vec<_>>();
        assert_eq!(prefixes.len(), 2);
        let literal = prefixes[0].children().next().unwrap();
        assert_eq!(literal.text(), Some("."));
        assert!(!literal.flags().delimiter_close);
        assert!(!literal.flags().sentence_end);
        let terminal_literal = prefixes[1].children().next().unwrap();
        assert_eq!(terminal_literal.text(), Some("."));
        assert!(terminal_literal.flags().sentence_end);
        let right = nodes
            .iter()
            .copied()
            .find(|node| node.text() == Some("right"))
            .unwrap();
        assert_eq!(
            right.parent().and_then(crate::NodeRef::macro_name),
            Some("Sh")
        );
    }

    #[test]
    fn pf_reports_only_prefixes_without_same_line_following_content() {
        let name = SourceName::new("mdoc-pf-validation.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt PF-VALIDATION 1\n.Os\n.Sh DESCRIPTION\n.Pf prefixed\n.Em eos Pf .\n.Po text Pf . Pc\n.Em end Pf\n",
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
                    "mdoc.prefix-without-following",
                    "nothing follows prefix: Pf prefixed",
                ),
                (
                    "mdoc.prefix-without-following",
                    "nothing follows prefix: Pf .",
                ),
                (
                    "mdoc.prefix-without-following",
                    "nothing follows prefix: Pf at eol",
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
            [Some((5, 2)), Some((6, 9)), Some((8, 9))]
        );
    }

    #[test]
    fn fixed_argument_inline_macros_return_later_words_to_source_flow() {
        let name = SourceName::new("mdoc-in-fixed-argument.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt IN 1\n.Os\n.Sh DESCRIPTION\n.In header after\n",
            ))
            .unwrap();
        let children = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Sh"))
            .unwrap()
            .children()
            .collect::<Vec<_>>();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].macro_name(), Some("In"));
        assert_eq!(
            children[0].children().next().and_then(crate::NodeRef::text),
            Some("header")
        );
        assert_eq!(children[1].text(), Some("after"));
    }

    #[test]
    fn closing_brace_is_not_a_mdoc_spacing_delimiter() {
        let name = SourceName::new("mdoc-brace-literal.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt BRACE 1\n.Os\n.Sh DESCRIPTION\n.No value Ns }\n",
            ))
            .unwrap();
        let brace = report
            .document
            .preorder()
            .find(|node| node.text() == Some("}"))
            .unwrap();
        assert!(!brace.flags().delimiter_close);
    }

    #[test]
    fn fl_expands_each_argument_and_preserves_a_pipe_between_flags() {
        let name = SourceName::new("mdoc-fl-multiarg.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FL 1\n.Os\n.Sh DESCRIPTION\n.Fl a b c\n.Op Fl x | y\n",
            ))
            .unwrap();
        let flags = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Fl"))
            .collect::<Vec<_>>();
        assert_eq!(flags.len(), 5);
        assert_eq!(
            flags
                .iter()
                .filter_map(|flag| flag.children().next().and_then(crate::NodeRef::text))
                .collect::<Vec<_>>(),
            ["a", "b", "c", "x", "y"]
        );
        let option = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Op"))
            .unwrap();
        let body = option.children().nth(1).unwrap();
        assert!(body.children().any(|node| node.text() == Some("|")));
    }

    #[test]
    fn fl_with_a_leading_pipe_keeps_an_empty_flag_element() {
        let name = SourceName::new("mdoc-fl-leading-pipe.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FL 1\n.Os\n.Sh DESCRIPTION\n.Fl | and\n",
            ))
            .unwrap();
        let children = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Sh"))
            .unwrap()
            .children()
            .collect::<Vec<_>>();
        assert_eq!(children.len(), 3);
        assert_eq!(children[0].macro_name(), Some("Fl"));
        assert!(children[0].children().next().is_none());
        assert_eq!(children[1].text(), Some("|"));
        assert_eq!(children[2].macro_name(), Some("Fl"));
        assert_eq!(
            children[2].children().next().and_then(crate::NodeRef::text),
            Some("and")
        );
    }

    #[test]
    fn middle_delimiter_reopens_the_same_inline_macro() {
        let name = SourceName::new("mdoc-cm-middle-delimiter.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt CM 1\n.Os\n.Sh DESCRIPTION\n.Cm one | two\n",
            ))
            .unwrap();
        let children = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Sh"))
            .unwrap()
            .children()
            .collect::<Vec<_>>();
        assert_eq!(children.len(), 3);
        assert_eq!(children[0].macro_name(), Some("Cm"));
        assert_eq!(
            children[0].children().next().and_then(crate::NodeRef::text),
            Some("one")
        );
        assert_eq!(children[1].text(), Some("|"));
        assert_eq!(children[2].macro_name(), Some("Cm"));
        assert_eq!(
            children[2].children().next().and_then(crate::NodeRef::text),
            Some("two")
        );
    }

    #[test]
    fn middle_delimiter_drops_a_temporary_reopen_before_a_callable_macro() {
        let name = SourceName::new("mdoc-op-middle-delimiter.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt OP 1\n.Os\n.Sh SYNOPSIS\n.Op Ar one \\*(Ba Fl two\n",
            ))
            .unwrap();
        let option = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Op"))
            .unwrap();
        let body = option
            .children()
            .find(|node| node.kind() == NodeKind::Body)
            .unwrap();
        let children = body.children().collect::<Vec<_>>();
        assert_eq!(children.len(), 3);
        assert_eq!(children[0].macro_name(), Some("Ar"));
        assert_eq!(children[1].text(), Some(r"\fR|\fP"));
        assert_eq!(children[2].macro_name(), Some("Fl"));
        assert!(
            !children.iter().any(|node| {
                node.macro_name() == Some("Ar") && node.children().next().is_none()
            })
        );
    }

    #[test]
    fn closing_delimiter_reopens_the_same_inline_macro() {
        let name = SourceName::new("mdoc-ad-closing-delimiter.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt AD 1\n.Os\n.Sh DESCRIPTION\n.Ad before : after\n",
            ))
            .unwrap();
        let children = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Sh"))
            .unwrap()
            .children()
            .collect::<Vec<_>>();
        assert_eq!(children.len(), 3);
        assert_eq!(children[0].macro_name(), Some("Ad"));
        assert_eq!(
            children[0].children().next().and_then(crate::NodeRef::text),
            Some("before")
        );
        assert_eq!(children[1].text(), Some(":"));
        assert!(children[1].flags().delimiter_close);
        assert_eq!(children[2].macro_name(), Some("Ad"));
        assert_eq!(
            children[2].children().next().and_then(crate::NodeRef::text),
            Some("after")
        );
    }

    #[test]
    fn ap_and_ns_have_no_owned_arguments() {
        let name = SourceName::new("mdoc-inline-no-arguments.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt INLINE 1\n.Os\n.Sh DESCRIPTION\n.No two words Ns tail\n.Xr mantdoc 1 Ap s\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let no = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("No"))
            .unwrap();
        assert_eq!(
            no.children().next().and_then(crate::NodeRef::text),
            Some("two words")
        );
        for macro_name in ["Ap", "Ns"] {
            assert!(nodes.iter().any(|node| {
                node.kind() == NodeKind::Element
                    && node.macro_name() == Some(macro_name)
                    && node.children().next().is_none()
            }));
        }
        assert!(nodes.iter().any(|node| node.text() == Some("tail")));
        assert!(nodes.iter().any(|node| node.text() == Some("s")));
    }

    #[test]
    fn vt_is_a_synopsis_partial_block_with_inline_children() {
        let name = SourceName::new("mdoc-vt-literal.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt VT 1\n.Os\n.Sh SYNOPSIS\n.Vt extern Sy int Li errno\n",
            ))
            .unwrap();
        let vt = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Vt"))
            .unwrap();
        assert!(vt.flags().synopsis_pretty);
        let mut children = vt.children();
        let head = children.next().unwrap();
        let body = children.next().unwrap();
        assert_eq!(head.kind(), NodeKind::Head);
        assert_eq!(body.kind(), NodeKind::Body);
        assert!(head.flags().synopsis_pretty);
        assert!(body.flags().synopsis_pretty);
        assert_eq!(body.children().count(), 3);
        assert_eq!(
            body.children().next().and_then(crate::NodeRef::text),
            Some("extern")
        );
        assert_eq!(
            body.children().nth(1).and_then(crate::NodeRef::macro_name),
            Some("Sy")
        );
        assert_eq!(
            body.children().nth(2).and_then(crate::NodeRef::macro_name),
            Some("Li")
        );
    }

    #[test]
    fn body_vt_discards_empty_forms_and_validates_attached_delimiters() {
        let name = SourceName::new("mdoc-vt-validation.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt VT-VALIDATION 1\n.Os\n.Sh NAME\n.Nm vt-validation\n.Nd test\n.Sh DESCRIPTION\n.Vt signed int.\n.Vt unsigned long;\n.Vt\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                ("mdoc.empty-macro", "skipping empty macro: Vt"),
                (
                    "mdoc.trailing-delimiter-spacing",
                    "no blank before trailing delimiter: Vt ... int.",
                ),
            ]
        );
        assert_eq!(
            report
                .document
                .preorder()
                .filter(|node| node.macro_name() == Some("Vt"))
                .count(),
            2
        );
        let location = report
            .document
            .source_position(report.diagnostics[1].primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((location.line, location.column), (8, 15));
    }

    #[test]
    fn body_vt_retains_released_nested_macro_delimiters() {
        let name = SourceName::new("mdoc-vt-nested-delimiter.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt VT-NESTED 1\n.Os\n.Sh NAME\n.Nm vt-nested\n.Nd test\n.Sh DESCRIPTION\n.Vt unsigned Sy int ,\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty());
        let body = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Sh"))
            .nth(1)
            .unwrap();
        let children = body.children().collect::<Vec<_>>();
        assert_eq!(children.len(), 3);
        assert_eq!(children[0].macro_name(), Some("Vt"));
        assert_eq!(children[1].macro_name(), Some("Sy"));
        assert_eq!(children[2].text(), Some(","));
        assert!(children[2].flags().delimiter_close);
    }

    #[test]
    fn xr_validates_fixed_arguments_and_releases_leading_delimiters() {
        let name = SourceName::new("mdoc-xr-validation.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt XR-VALIDATION 1\n.Os\n.Sh NAME\n.Nm xr-validation\n.Nd test\n.Sh DESCRIPTION\n.Xr ( echo 1\n.Xr echo 1)\n.Xr echo\n.Xr echo,\n.Xr ,\n.Xr\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                ("mdoc.empty-macro", "skipping empty macro: Xr"),
                ("mdoc.empty-macro", "skipping empty macro: Xr"),
                (
                    "mdoc.trailing-delimiter-spacing",
                    "no blank before trailing delimiter: Xr ... 1)",
                ),
                (
                    "mdoc.reference-section-missing",
                    "missing section argument: Xr echo",
                ),
                (
                    "mdoc.reference-section-missing",
                    "missing section argument: Xr echo,",
                ),
                (
                    "mdoc.trailing-delimiter-spacing",
                    "no blank before trailing delimiter: Xr echo,",
                ),
            ]
        );
        let xrs = report
            .document
            .preorder()
            .filter(|node| node.macro_name() == Some("Xr"))
            .collect::<Vec<_>>();
        assert_eq!(xrs.len(), 4);
        assert!(!xrs[0].flags().line_start);
        assert_eq!(xrs[0].children().count(), 2);
        let opening = report
            .document
            .preorder()
            .find(|node| node.text() == Some("(") && node.flags().line_start)
            .unwrap();
        assert!(opening.flags().delimiter_open);
    }

    #[test]
    fn empty_synopsis_nm_generates_the_document_name_and_owns_following_flow() {
        let name = SourceName::new("mdoc-synopsis-name.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SYNOPSIS-NM 1\n.Os\n.Sh NAME\n.Nm utility\n.Nd synopsis test\n.Sh SYNOPSIS\n.Nm\n.Fl f\n.Pp\n.Fl g\n",
            ))
            .unwrap();
        let nm = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Nm"))
            .unwrap();
        assert!(nm.flags().synopsis_pretty);
        let mut children = nm.children();
        let head = children.next().unwrap();
        let body = children.next().unwrap();
        let generated = head.children().next().unwrap();
        assert_eq!(generated.text(), Some("utility"));
        assert!(generated.flags().generated);
        assert!(generated.flags().synopsis_pretty);
        assert_eq!(body.kind(), NodeKind::Body);
        assert_eq!(body.children().count(), 3);
        assert!(
            body.children()
                .filter(|node| node.macro_name() == Some("Fl"))
                .all(|node| node.flags().synopsis_pretty)
        );
    }

    #[test]
    fn authored_synopsis_nm_falls_back_to_document_name_after_an_invalid_name_entry() {
        let name = SourceName::new("mdoc-synopsis-authored-name.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SYNOPSIS-NM 1\n.Os\n.Sh NAME\n.Nm Bx\n.Nd invalid NAME entry\n.Sh SYNOPSIS\n.Nm utility\n",
            ))
            .unwrap();

        assert_eq!(report.document.metadata().name.as_deref(), Some("utility"));
    }

    #[test]
    fn synopsis_nm_keeps_same_line_partial_blocks_in_its_head() {
        let name = SourceName::new("mdoc-synopsis-name-partial.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SYNOPSIS-NM 1\n.Os\n.Sh SYNOPSIS\n.Nm before Bo within\n.Sh DESCRIPTION\n",
            ))
            .unwrap();
        let name = report
            .document
            .preorder()
            .find(|node| {
                node.kind() == NodeKind::Block
                    && node.macro_name() == Some("Nm")
                    && node.children().next().is_some_and(|head| {
                        head.children()
                            .any(|child| child.macro_name() == Some("Bo"))
                    })
            })
            .unwrap();
        let mut children = name.children();
        let head = children.next().unwrap();
        let body = children.next().unwrap();

        assert!(
            head.children()
                .any(|child| child.macro_name() == Some("Bo"))
        );
        assert!(body.flags().line_start);
        assert_eq!(body.children().count(), 0);
    }

    #[test]
    fn private_ns_register_drives_synopsis_topology_without_an_ast_request() {
        let name = SourceName::new("mdoc-ns-register.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt NS-REGISTER 1\n.Os\n.Sh NAME\n.Nm ns-register\n.Nd private synopsis state\n.Sh DESCRIPTION\n.nr nS 1\n.Nm\n.Fl a\n.nr nS 0\n.Pp\n.Fl b\n.nr nS 1\n.Nm\n.Oo Fl a\n.nr nS 0\n.Pp\n.Fl b Oc\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        assert!(nodes.iter().all(|node| node.macro_name() != Some("nr")));

        let names = nodes
            .iter()
            .copied()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Nm"))
            .collect::<Vec<_>>();
        assert_eq!(names.len(), 2);
        assert!(names.iter().all(|node| node.flags().synopsis_pretty));
        for name in names {
            let generated = name.children().next().unwrap().children().next().unwrap();
            assert!(generated.flags().generated);
            assert!(!generated.flags().synopsis_pretty);
        }

        let paragraphs = nodes
            .iter()
            .copied()
            .filter(|node| node.macro_name() == Some("Pp"))
            .collect::<Vec<_>>();
        assert_eq!(paragraphs.len(), 2);
        assert!(
            paragraphs
                .iter()
                .all(|paragraph| !paragraph.flags().synopsis_pretty)
        );

        let optional = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Oo"))
            .unwrap();
        assert!(optional.flags().synopsis_pretty);
        assert!(
            optional
                .children()
                .all(|child| child.flags().synopsis_pretty)
        );
    }

    #[test]
    fn implicit_partial_blocks_follow_inline_macros_as_siblings() {
        let name = SourceName::new("mdoc-op-sibling.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt OP 1\n.Os\n.Sh DESCRIPTION\n.Fl Op flag\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let fl = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Fl"))
            .unwrap();
        assert_eq!(fl.children().count(), 0);
        let op = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Op"))
            .unwrap();
        let body = op.children().nth(1).unwrap();
        assert_eq!(body.kind(), NodeKind::Body);
        assert_eq!(
            body.children().next().and_then(crate::NodeRef::text),
            Some("flag")
        );
    }

    #[test]
    fn callable_partial_blocks_end_an_inline_scope_and_parse_nested_mailto() {
        let name = SourceName::new("mdoc-an-partial-blocks.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt AUTHORS 1\n.Os\n.Sh DESCRIPTION\n.An Name Ao Mt addr Ac An Name Aq Mt addr\n",
            ))
            .unwrap();
        let authors = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("An"))
            .collect::<Vec<_>>();
        assert_eq!(authors.len(), 2);
        assert!(authors.iter().all(|author| {
            author.children().count() == 1
                && author.children().next().and_then(crate::NodeRef::text) == Some("Name")
        }));
        for enclosure in ["Ao", "Aq"] {
            let block = report
                .document
                .preorder()
                .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some(enclosure))
                .unwrap();
            let body = block
                .children()
                .find(|node| node.kind() == NodeKind::Body)
                .unwrap();
            let mailto = body.children().next().unwrap();
            assert_eq!(mailto.macro_name(), Some("Mt"));
            assert_eq!(
                mailto.children().next().and_then(crate::NodeRef::text),
                Some("addr")
            );
        }
    }

    #[test]
    fn implicit_partial_blocks_recurse_inside_parsed_arguments() {
        let name = SourceName::new("mdoc-op-nested.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt OP 1\n.Os\n.Sh DESCRIPTION\n.Op outer Op inner\n",
            ))
            .unwrap();
        let mut options = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Op"));
        let outer = options.next().unwrap();
        let inner = options.next().unwrap();
        assert_eq!(outer.children().nth(1).unwrap().children().count(), 2);
        assert_eq!(
            inner
                .children()
                .nth(1)
                .unwrap()
                .children()
                .next()
                .and_then(crate::NodeRef::text),
            Some("inner")
        );
    }

    #[test]
    fn implicit_partial_blocks_keep_a_leading_open_delimiter_outside_the_body() {
        let name = SourceName::new("mdoc-dq-open.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt DQ 1\n.Os\n.Sh DESCRIPTION\n.Dq \"(\" user@host)\n",
            ))
            .unwrap();
        let dq = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Dq"))
            .unwrap();
        let mut children = dq.children();
        assert_eq!(children.next().unwrap().kind(), NodeKind::Head);
        let opening = children.next().unwrap();
        assert_eq!(opening.text(), Some("("));
        assert!(opening.flags().delimiter_open);
        let body = children.next().unwrap();
        assert_eq!(body.kind(), NodeKind::Body);
        assert_eq!(
            body.children().next().and_then(crate::NodeRef::text),
            Some("user@host)")
        );
    }

    #[test]
    fn implicit_partial_blocks_publish_unescaped_closing_punctuation_as_a_tail() {
        let name = SourceName::new("mdoc-pq-tail.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt PQ 1\n.Os\n.Sh DESCRIPTION\n.Pq quite lonely .\n.Pq \\&.\n",
            ))
            .unwrap();
        let parens = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Pq"))
            .collect::<Vec<_>>();
        let first = parens[0].children().collect::<Vec<_>>();
        assert_eq!(first[1].kind(), NodeKind::Body);
        assert_eq!(
            first[1].children().next().and_then(crate::NodeRef::text),
            Some("quite lonely")
        );
        assert_eq!(first[2].text(), Some("."));
        assert!(first[2].flags().delimiter_close);
        assert!(first[2].flags().sentence_end);

        let second_body = parens[1].children().nth(1).unwrap();
        assert_eq!(
            second_body.children().next().and_then(crate::NodeRef::text),
            Some("\\&.")
        );
        assert_eq!(parens[1].children().count(), 2);
    }

    #[test]
    fn implicit_partial_blocks_preserve_internal_and_repeated_delimiter_boundaries() {
        let name = SourceName::new("mdoc-op-punctuation.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt OP 1\n.Os\n.Sh DESCRIPTION\n.Op | z\n.Op a ( z\n.Op . z\n.Op ( (\n.Op . .\n.Op a (\n",
            ))
            .unwrap();
        let options = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Op"))
            .collect::<Vec<_>>();

        for (option, expected) in options.iter().take(3).zip([
            ["|", "z"].as_slice(),
            ["a", "(", "z"].as_slice(),
            [".", "z"].as_slice(),
        ]) {
            let body = option
                .children()
                .find(|child| child.kind() == NodeKind::Body)
                .unwrap();
            assert_eq!(
                body.children()
                    .filter_map(crate::NodeRef::text)
                    .collect::<Vec<_>>(),
                expected
            );
        }
        let middle_open = options[1]
            .children()
            .find(|child| child.kind() == NodeKind::Body)
            .unwrap()
            .children()
            .nth(1)
            .unwrap();
        assert!(middle_open.flags().delimiter_open);
        let leading_close = options[2]
            .children()
            .find(|child| child.kind() == NodeKind::Body)
            .unwrap()
            .children()
            .next()
            .unwrap();
        assert!(!leading_close.flags().delimiter_close);

        let repeated_open = options[3].children().collect::<Vec<_>>();
        assert_eq!(repeated_open[0].kind(), NodeKind::Head);
        assert_eq!(repeated_open[1].text(), Some("("));
        assert_eq!(repeated_open[2].text(), Some("("));
        assert!(repeated_open[1].flags().delimiter_open);
        assert!(repeated_open[2].flags().delimiter_open);
        assert_eq!(repeated_open[3].kind(), NodeKind::Body);

        let repeated_close = options[4].children().collect::<Vec<_>>();
        assert_eq!(repeated_close[1].kind(), NodeKind::Body);
        for tail in &repeated_close[2..] {
            assert_eq!(tail.text(), Some("."));
            assert!(tail.flags().delimiter_close);
            assert!(tail.flags().sentence_end);
        }

        let terminal_open = options[5]
            .children()
            .find(|child| child.kind() == NodeKind::Body)
            .unwrap()
            .children()
            .nth(1)
            .unwrap();
        assert_eq!(terminal_open.text(), Some("("));
        assert!(!terminal_open.flags().delimiter_open);
    }

    #[test]
    fn column_cells_keep_cross_line_explicit_partial_scopes() {
        let name = SourceName::new("mdoc-column-partial.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt COLUMN 1\n.Os\n.Sh DESCRIPTION\n.Bl -column one two\n.It it Aq aq Ta ta Bo bo bc\n.Bc Pq pq\n.El\n",
            ))
            .unwrap();
        let item = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("It"))
            .unwrap();
        let second_cell = item.children().nth(2).unwrap();
        let names = second_cell
            .children()
            .filter_map(crate::NodeRef::macro_name)
            .collect::<Vec<_>>();
        assert_eq!(names, ["Bo", "Pq"]);
        assert!(
            second_cell
                .children()
                .filter(|node| node.macro_name().is_some())
                .all(|node| node.kind() == NodeKind::Block)
        );
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    }

    #[test]
    fn column_lists_validate_cells_and_preserve_tab_phrase_semantics() {
        let name = SourceName::new("mdoc-column-validation.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt COLUMN 1\n.Os\n.Sh DESCRIPTION\n.Bl -column \"a\" \"b\"\n.It\n.It \"a\"\n.It \"a\" Ta \"b\"\n.It \"a\" Ta \"b\" Ta \"c\"\n.It \"a\" Ta \"b\" Ta \"c\" Ta \"d\"\n.It \"a\" Ta \"b\" Ta \"c\" Ta \"d\" Ta \"e\"\n.It\n.El\n.Bl -column \"a\" \"b\" \"cc\"\n.It \"a\tb\"\tcc\n.El\n.Bl -column \"a\" \"b\"\n.It a \tb\n.El\n.Bl -column \"aa\" -width 6n -compact \"bb\" \"cc\"\n.It aa Ta bb Ta cc Ta dd\n.El\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "skipping empty macro: It",
                "wrong number of cells: 2 columns, 1 cells",
                "wrong number of cells: 2 columns, 4 cells",
                "wrong number of cells: 2 columns, 5 cells",
                "skipping empty macro: It",
                "skipping -width argument: Bl -column",
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
        assert_eq!(
            positions,
            [(6, 2), (7, 2), (10, 2), (11, 2), (12, 2), (20, 18)]
        );
    }

    #[test]
    fn column_cells_accept_inline_and_physical_ta_recovery() {
        let name = SourceName::new("mdoc-column-ta-recovery.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt COLUMN 1\n.Os\n.Sh DESCRIPTION\n.Bl -column \"first column\" \"second column\"\n.It\ntext\n.No macro Ta after tab\n.El\n.Bl -column aa bb\n.It aa\n.Ta bb\n.El\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "missing argument, using next line: Bl -column It",
                "first macro on line: Ta",
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
        assert_eq!(positions, [(6, 2), (12, 2)]);

        let items = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("It"))
            .collect::<Vec<_>>();
        assert_eq!(items.len(), 2);
        let first_cells = items[0]
            .children()
            .filter(|node| node.kind() == NodeKind::Body)
            .collect::<Vec<_>>();
        assert_eq!(first_cells.len(), 2);
        let no = first_cells[0]
            .children()
            .find(|node| node.macro_name() == Some("No"))
            .unwrap();
        assert_eq!(
            no.children().map(crate::NodeRef::text).collect::<Vec<_>>(),
            [Some("macro")]
        );
        assert_eq!(
            first_cells[1]
                .children()
                .map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            [Some("after tab")]
        );
        let second_cells = items[1]
            .children()
            .filter(|node| node.kind() == NodeKind::Body)
            .collect::<Vec<_>>();
        assert_eq!(second_cells.len(), 2);
        assert_eq!(
            second_cells
                .iter()
                .map(|cell| cell.children().next().and_then(crate::NodeRef::text))
                .collect::<Vec<_>>(),
            [Some("aa"), Some("bb")]
        );
        assert!(second_cells[1].flags().line_start);
    }

    #[test]
    fn column_cells_expand_each_system_name_macro() {
        let name = SourceName::new("mdoc-column-system-name.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt COLUMN 1\n.Os\n.Sh DESCRIPTION\n.Bl -column \"aa\" \"OpenBSD OpenBSD OpenBSD\" \"tail\"\n.It aa Ta Ox Ox Ox Ta tab-tab\n.It aa\t Ox Ox Ox\tta/bl-ta\n.It aa\tbb\t\ntab at eol\n.El\n",
            ))
            .unwrap();
        let systems = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Ox"))
            .collect::<Vec<_>>();
        let first_row = systems
            .iter()
            .copied()
            .filter(|node| {
                node.location()
                    .and_then(|span| report.document.source_position(span))
                    .is_some_and(|position| position.line == 6)
            })
            .collect::<Vec<_>>();
        assert_eq!(first_row.len(), 3);
        assert!(first_row.iter().all(|node| {
            node.children()
                .next()
                .is_some_and(|child| child.text() == Some("OpenBSD") && child.flags().generated)
        }));
        assert_eq!(
            systems
                .iter()
                .filter(|node| {
                    node.location()
                        .and_then(|span| report.document.source_position(span))
                        .is_some_and(|position| position.line == 7)
                })
                .count(),
            2
        );
        let retained = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(|node| {
                let position = node
                    .location()
                    .and_then(|span| report.document.source_position(span))?;
                Some((node.text(), position.line, position.column))
            })
            .collect::<Vec<_>>();
        assert!(retained.contains(&(Some(""), 7, 8)));
        assert!(retained.contains(&(Some(r"\&"), 8, 2)));
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    }

    #[test]
    fn sm_spacing_controls_partial_block_text_coalescing() {
        let name = SourceName::new("mdoc-sm-spacing.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SM 1\n.Os\n.Sh DESCRIPTION\n.Sm off\n.Pq now off\n.Sm\n.Pq now on\n.Sm off\n.No macro2 macro3\n.Sm\n.No macro4 macro5\n",
            ))
            .unwrap();
        let parens = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Pq"))
            .collect::<Vec<_>>();
        assert_eq!(parens.len(), 2);

        let disabled = parens[0].children().nth(1).unwrap();
        assert_eq!(
            disabled
                .children()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            ["now", "off"]
        );

        let enabled = parens[1].children().nth(1).unwrap();
        assert_eq!(
            enabled
                .children()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            ["now on"]
        );

        let no_space = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("No"))
            .collect::<Vec<_>>();
        assert_eq!(no_space.len(), 2);
        assert_eq!(
            no_space[0]
                .children()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            ["macro2", "macro3"]
        );
        assert_eq!(
            no_space[1]
                .children()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            ["macro4 macro5"]
        );
    }

    #[test]
    fn invalid_sm_boolean_argument_warns_without_changing_spacing_state() {
        let name = SourceName::new("mdoc-sm-invalid.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SM 1\n.Os\n.Sh NAME\n.Nm sm\n.Nd spacing control\n.Sh DESCRIPTION\n.Sm off\n.Sm bad\n.Pq still off\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    (
                        diagnostic.code.as_str(),
                        diagnostic.message.as_ref(),
                        diagnostic
                            .primary
                            .as_ref()
                            .and_then(|span| report.document.source_position(span)),
                    )
                })
                .collect::<Vec<_>>(),
            [(
                "mdoc.boolean-argument",
                "invalid Boolean argument: Sm bad",
                Some(crate::SourcePosition { line: 9, column: 5 }),
            )]
        );
        let parens = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Pq"))
            .unwrap();
        let body = parens.children().nth(1).unwrap();
        assert_eq!(
            body.children()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            ["still", "off"]
        );
    }

    #[test]
    fn sm_off_disables_em_and_sy_word_joining() {
        let name = SourceName::new("mdoc-sm-join.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SM 1\n.Os\n.Sh NAME\n.Nm sm\n.Nd spacing control\n.Sh DESCRIPTION\n.Em enabled words\n.Sy symbolic words\n.Sm off\n.Em disabled words\n.Sy literal words\n",
            ))
            .unwrap();
        let contents = report
            .document
            .preorder()
            .filter(|node| {
                node.kind() == NodeKind::Element && matches!(node.macro_name(), Some("Em" | "Sy"))
            })
            .map(|node| {
                (
                    node.macro_name().unwrap(),
                    node.children()
                        .filter_map(crate::NodeRef::text)
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            contents,
            [
                ("Em", vec!["enabled words"]),
                ("Sy", vec!["symbolic words"]),
                ("Em", vec!["disabled", "words"]),
                ("Sy", vec!["literal", "words"]),
            ]
        );
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    }

    #[test]
    fn st_expands_known_selectors_and_defers_unknown_selector_diagnostics() {
        let name = SourceName::new("mdoc-st.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt ST 1\n.Os\n.Sh NAME\n.Nm st\n.Nd standard selector\n.Sh STANDARDS\n.St -p1003.1-2004\n.St -murks\n.St\n",
            ))
            .unwrap();
        let standards = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("St"))
            .collect::<Vec<_>>();
        assert_eq!(standards.len(), 1);
        let children = standards[0].children().collect::<Vec<_>>();
        assert_eq!(children.len(), 2);
        assert_eq!(
            children[0].text(),
            Some("IEEE Std 1003.1-2004 (\\(lqPOSIX.1\\(rq)")
        );
        assert!(children[0].flags().generated);
        assert_eq!(children[1].text(), Some("-p1003.1-2004"));
        assert!(children[1].flags().no_print);
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    (
                        diagnostic.code.as_str(),
                        diagnostic.message.as_ref(),
                        diagnostic
                            .primary
                            .as_ref()
                            .and_then(|span| report.document.source_position(span)),
                    )
                })
                .collect::<Vec<_>>(),
            [
                (
                    "mdoc.empty-macro",
                    "skipping empty macro: St",
                    Some(crate::SourcePosition {
                        line: 10,
                        column: 2,
                    }),
                ),
                (
                    "mdoc.unknown-standard",
                    "unknown standard specifier: St -murks",
                    Some(crate::SourcePosition { line: 9, column: 5 }),
                ),
            ]
        );
    }

    #[test]
    fn empty_ar_synthesizes_generated_file_ellipsis_words() {
        let name = SourceName::new("mdoc-ar-default.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt AR 1\n.Os\n.Sh SYNOPSIS\n.Ar\n",
            ))
            .unwrap();
        let argument = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Ar"))
            .unwrap();
        let words = argument.children().collect::<Vec<_>>();
        assert_eq!(
            words
                .iter()
                .filter_map(|word| word.text())
                .collect::<Vec<_>>(),
            ["file", "..."]
        );
        assert!(
            words
                .iter()
                .all(|word| word.flags().generated && word.flags().synopsis_pretty)
        );
    }

    #[test]
    fn explicit_partial_blocks_consume_same_line_closers_and_restore_tail_flow() {
        let name = SourceName::new("mdoc-do-close.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt DO 1\n.Os\n.Sh DESCRIPTION\n.Do \"(\" full) Dc one Sy bold .\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let block = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Do"))
            .unwrap();
        let mut children = block.children();
        let opening = children.next().unwrap();
        assert_eq!(opening.text(), Some("("));
        assert!(opening.flags().delimiter_open);
        assert_eq!(children.next().unwrap().kind(), NodeKind::Head);
        let body = children.next().unwrap();
        assert_eq!(body.kind(), NodeKind::Body);
        assert_eq!(
            body.children().next().and_then(crate::NodeRef::text),
            Some("full)")
        );
        assert!(!nodes.iter().any(|node| node.macro_name() == Some("Dc")));
        assert!(nodes.iter().any(|node| node.text() == Some("one")));
        assert!(nodes.iter().any(|node| node.macro_name() == Some("Sy")));
    }

    #[test]
    fn explicit_partial_scopes_pair_nested_inline_and_cross_line_closers() {
        let name = SourceName::new("mdoc-oo-nested-lines.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt OO 1\n.Os\n.Sh SYNOPSIS\n.Bk -words\n.Oo\n.Oo No a Oc Oo No b Oc Oc Pq tail\n.Ek\n",
            ))
            .unwrap();
        let keep = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bk"))
            .unwrap();
        let keep_body = keep
            .children()
            .find(|node| node.kind() == NodeKind::Body)
            .unwrap();
        let keep_children = keep_body.children().collect::<Vec<_>>();
        assert_eq!(keep_children.len(), 2);
        assert_eq!(keep_children[0].macro_name(), Some("Oo"));
        assert_eq!(keep_children[1].macro_name(), Some("Pq"));
        let outer_body = keep_children[0]
            .children()
            .find(|node| node.kind() == NodeKind::Body)
            .unwrap();
        assert_eq!(
            outer_body
                .children()
                .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Oo"))
                .count(),
            2
        );
        assert!(
            !report
                .document
                .preorder()
                .any(|node| node.text() == Some("Oc"))
        );
    }

    #[test]
    fn bro_uses_the_brc_partial_close_pair() {
        let name = SourceName::new("mdoc-bro-close.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt BRO 1\n.Os\n.Sh DESCRIPTION\n.Bro \"(\" full) Brc one\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let block = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bro"))
            .unwrap();
        let mut children = block.children();
        let opening = children.next().unwrap();
        assert_eq!(opening.text(), Some("("));
        assert!(opening.flags().delimiter_open);
        assert_eq!(children.next().unwrap().kind(), NodeKind::Head);
        assert_eq!(
            children
                .next()
                .unwrap()
                .children()
                .next()
                .and_then(crate::NodeRef::text),
            Some("full)")
        );
        assert!(!nodes.iter().any(|node| node.macro_name() == Some("Brc")));
        assert!(nodes.iter().any(|node| node.text() == Some("one")));
    }

    #[test]
    fn eo_scope_uses_a_head_body_and_tail_across_physical_lines() {
        let name = SourceName::new("mdoc-eo-tail.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EO 1\n.Os\n.Sh DESCRIPTION\n.Eo open\nbody\n.Ec close\nnext\n",
            ))
            .unwrap();
        let block = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Eo"))
            .unwrap();
        let mut children = block.children();
        let head = children.next().unwrap();
        let body = children.next().unwrap();
        let tail = children.next().unwrap();
        assert_eq!(head.kind(), NodeKind::Head);
        assert_eq!(
            head.children().next().and_then(crate::NodeRef::text),
            Some("open")
        );
        assert_eq!(body.kind(), NodeKind::Body);
        assert_eq!(
            body.children().next().and_then(crate::NodeRef::text),
            Some("body")
        );
        assert_eq!(tail.kind(), NodeKind::Tail);
        assert_eq!(tail.macro_name(), Some("Eo"));
        assert_eq!(
            tail.children().next().and_then(crate::NodeRef::text),
            Some("close")
        );
        assert!(
            report
                .document
                .preorder()
                .any(|node| node.text() == Some("next"))
        );
    }

    #[test]
    fn inline_eo_after_no_and_ns_opens_a_scope() {
        let name = SourceName::new("mdoc-inline-eo.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EO 1\n.Os\n.Sh DESCRIPTION\n.No prefix Ns Eo\n.Ec close\n",
            ))
            .unwrap();
        let block = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Eo"))
            .unwrap();
        assert_eq!(block.children().count(), 3);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn ec_tail_stops_before_a_following_callable_macro() {
        let name = SourceName::new("mdoc-ec-tail-inline.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EC 1\n.Os\n.Sh DESCRIPTION\n.Eo <<\nbody\n.Ec >> \"Sy\" bold\n",
            ))
            .unwrap();
        let block = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Eo"))
            .unwrap();
        let tail = block.children().nth(2).unwrap();
        assert_eq!(tail.kind(), NodeKind::Tail);
        assert_eq!(tail.children().count(), 1);
        assert_eq!(
            tail.children().next().and_then(crate::NodeRef::text),
            Some(">>")
        );
        assert!(
            report
                .document
                .preorder()
                .any(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Sy"))
        );
    }

    #[test]
    fn inline_ec_closes_eo_and_a_stray_ec_becomes_br() {
        let name = SourceName::new("mdoc-inline-ec.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EC 1\n.Os\n.Sh DESCRIPTION\n.Eo <<\n.No prefix Ns Ec\n.Ec >>\n",
            ))
            .unwrap();
        let block = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Eo"))
            .unwrap();
        assert_eq!(block.children().count(), 3);
        assert!(
            report
                .document
                .preorder()
                .any(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("br"))
        );
        assert!(
            report
                .document
                .preorder()
                .any(|node| node.text() == Some(">>"))
        );
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].code.as_str(), "mdoc.unmatched-close");
    }

    #[test]
    fn inline_fc_closes_a_function_scope() {
        let name = SourceName::new("mdoc-inline-fc.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FC 1\n.Os\n.Sh SYNOPSIS\n.Fo call\n.Nm name Fc tail\n",
            ))
            .unwrap();
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code.as_str() == "mdoc.unclosed-block")
        );
    }

    #[test]
    fn unclosed_eo_retains_only_its_head_and_body_prefix() {
        let name = SourceName::new("mdoc-eo-unclosed.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EO 1\n.Os\n.Sh DESCRIPTION\n.Eo open\n",
            ))
            .unwrap();
        let block = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Eo"))
            .unwrap();
        assert_eq!(block.children().count(), 2);
        assert_eq!(report.diagnostics[0].code.as_str(), "mdoc.unclosed-block");
    }

    #[test]
    fn fo_parts_inherit_synopsis_presentation() {
        let name = SourceName::new("mdoc-fo-synopsis.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FO 1\n.Os\n.Sh SYNOPSIS\n.Fo call\n.Fa void\n.Fc\n",
            ))
            .unwrap();
        let block = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Fo"))
            .unwrap();
        assert!(block.flags().synopsis_pretty);
        assert!(block.children().all(|child| child.flags().synopsis_pretty));
    }

    #[test]
    fn fo_head_is_a_non_synopsis_target_and_consumes_a_pending_tg() {
        let name = SourceName::new("mdoc-fo-tag.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FO 1\n.Os\n.Sh DESCRIPTION\n.Tg manual\n.Fo call\n.Fa void\n.Fc\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let head = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("Fo"))
            .unwrap();
        assert!(head.flags().deep_link_target);
        assert!(head.flags().permalink);
        assert_eq!(head.tag(), Some("manual"));
        let tg = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Tg"))
            .unwrap();
        assert!(tg.flags().no_print);
    }

    #[test]
    fn paragraph_precedes_fo_function_target_and_fc_inline_macro_keeps_line_context() {
        let name = SourceName::new("mdoc-fo-paragraph-target.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FO 1\n.Os\n.Sh DESCRIPTION\n.Pp\n.Fo prefix\\\\fIname\\\\fPsuffix\n.Fa void\n.Fc \"Sy\" bold\n",
            ))
            .unwrap();
        let paragraph = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Pp"))
            .unwrap();
        assert!(paragraph.flags().deep_link_target);
        assert_eq!(paragraph.tag(), Some("prefix"));
        let head = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("Fo"))
            .unwrap();
        assert!(!head.flags().deep_link_target);
        assert!(head.flags().permalink);
        assert_eq!(head.tag(), Some("prefix"));
        let symbolic = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Sy"))
            .unwrap();
        assert!(!symbolic.flags().line_start);
    }

    #[test]
    fn roff_break_keeps_a_preceding_paragraph_eligible_for_fo_targets() {
        let name = SourceName::new("mdoc-fo-break-target.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FO 1\n.Os\n.Sh DESCRIPTION\n.Pp\nfunction declaration:\n.br\n.Fo call\n.Fa void\n.Fc\n",
            ))
            .unwrap();
        let paragraph = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Pp"))
            .unwrap();
        assert!(paragraph.flags().deep_link_target);
        assert_eq!(paragraph.tag(), Some("call"));
        let head = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("Fo"))
            .unwrap();
        assert!(head.flags().permalink);
        assert_eq!(head.tag(), None);
    }

    #[test]
    fn fn_uses_an_eligible_paragraph_as_its_target() {
        let name = SourceName::new("mdoc-fn-tag.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FN 1\n.Os\n.Sh DESCRIPTION\n.Pp\n.Tg manual\n.Fn call void\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let paragraph = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Pp"))
            .unwrap();
        assert!(paragraph.flags().deep_link_target);
        assert_eq!(paragraph.tag(), Some("manual"));
        let function = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Fn"))
            .unwrap();
        assert!(!function.flags().deep_link_target);
        assert!(function.flags().permalink);
        assert_eq!(function.tag(), Some("manual"));
    }

    #[test]
    fn later_functions_in_one_paragraph_do_not_gain_a_second_automatic_target() {
        let name = SourceName::new("mdoc-fn-one-target.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FN 1\n.Os\n.Sh DESCRIPTION\n.Pp\n.Fn first\nand\n.Fn second\n",
            ))
            .unwrap();
        let functions = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Fn"))
            .collect::<Vec<_>>();
        assert_eq!(functions.len(), 2);
        assert!(!functions[0].flags().deep_link_target);
        assert!(functions[0].flags().permalink);
        assert!(!functions[1].flags().deep_link_target);
        assert!(!functions[1].flags().permalink);
    }

    #[test]
    fn standalone_fn_is_a_target_only_outside_synopsis() {
        let name = SourceName::new("mdoc-fn-synopsis.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FN 1\n.Os\n.Sh SYNOPSIS\n.Fn synopsis void\n.Sh DESCRIPTION\n.Fn detail void\n",
            ))
            .unwrap();
        let functions = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Fn"))
            .collect::<Vec<_>>();
        assert_eq!(functions.len(), 2);
        assert!(!functions[0].flags().deep_link_target);
        assert!(functions[1].flags().deep_link_target);
        assert!(functions[1].flags().permalink);
    }

    #[test]
    fn standalone_function_targets_use_the_first_phrase_word_and_fc_releases_punctuation() {
        let name = SourceName::new("mdoc-fn-fc-eos.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FN 1\n.Os\n.Sh DESCRIPTION\n.Fn \"double sin\" \"double x\" .\n.Fo cos\n.Fa double x\n.Fc .\n",
            ))
            .unwrap();
        let function = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Fn"))
            .unwrap();
        assert!(function.flags().deep_link_target);
        assert_eq!(function.tag(), Some("double"));
        let periods = report
            .document
            .preorder()
            .filter(|node| node.text() == Some("."))
            .collect::<Vec<_>>();
        assert_eq!(periods.len(), 2);
        assert!(periods.iter().all(|period| period.flags().sentence_end));
        assert!(periods[1].flags().line_start);
        assert!(periods[1].flags().delimiter_close);
    }

    #[test]
    fn function_type_declarations_restart_standalone_function_targeting() {
        let name = SourceName::new("mdoc-fn-type-targets.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FN 1\n.Os\n.Sh DESCRIPTION\n.Ft int\n.Fn first void\n.Ft int\n.Fn second void\n",
            ))
            .unwrap();
        let functions = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Fn"))
            .collect::<Vec<_>>();
        assert_eq!(functions.len(), 2);
        assert!(
            functions
                .iter()
                .all(|function| function.flags().deep_link_target && function.flags().permalink)
        );
    }

    #[test]
    fn tg_inside_fo_is_a_visible_destination() {
        let name = SourceName::new("mdoc-fo-tg.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FO 1\n.Os\n.Sh DESCRIPTION\n.Fo call\n.Tg argument\n.Fc\n",
            ))
            .unwrap();
        let tg = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Tg"))
            .unwrap();
        assert!(tg.flags().deep_link_target);
        assert!(!tg.flags().no_print);
        assert_eq!(tg.tag(), None);
    }

    #[test]
    fn fo_transparent_targets_are_limited_only_in_synopsis() {
        let name = SourceName::new("mdoc-fo-tg-context.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FO 1\n.Os\n.Sh SYNOPSIS\n.Fo synopsis\n.Tg first\n.Tg second\n.Fc\n.Sh DESCRIPTION\n.Fo detail\n.Tg third\n.Tg fourth\n.Fc\n",
            ))
            .unwrap();
        let targets = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Tg"))
            .map(|node| (node.flags().deep_link_target, node.flags().no_print))
            .collect::<Vec<_>>();
        assert_eq!(
            targets,
            [(true, false), (false, true), (true, false), (true, false)]
        );
    }

    #[test]
    fn pending_tg_can_name_the_following_section_head() {
        let name = SourceName::new("mdoc-tg-section.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt TG 1\n.Os\n.Sh NAME\n.Tg section-tag\n.Sh DESCRIPTION\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let head = nodes
            .iter()
            .copied()
            .find(|node| {
                node.kind() == NodeKind::Head
                    && node.macro_name() == Some("Sh")
                    && node.children().next().and_then(crate::NodeRef::text) == Some("DESCRIPTION")
            })
            .unwrap();
        assert_eq!(head.tag(), Some("section-tag"));
        let tg = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Tg"))
            .unwrap();
        assert!(tg.flags().no_print);
    }

    #[test]
    fn pending_tg_can_name_the_following_subsection_head() {
        let name = SourceName::new("mdoc-tg-subsection.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt TG 1\n.Os\n.Sh DESCRIPTION\n.Tg subsection-tag\n.Ss DETAILS\n",
            ))
            .unwrap();
        let subsection = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("Ss"))
            .unwrap();
        assert!(subsection.flags().deep_link_target);
        assert!(subsection.flags().permalink);
        assert_eq!(subsection.tag(), Some("subsection-tag"));
        let tg = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Tg"))
            .unwrap();
        assert!(tg.flags().no_print);
    }

    #[test]
    fn normalizes_deterministic_mdocdate_without_consulting_host_time() {
        let name = SourceName::new("mdoc-date.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd $Mdocdate: Jul 6 2017 $\n.Dt DATE 1\n.Os\n.Sh NAME\ndate\n",
            ))
            .unwrap();
        assert_eq!(
            report.document.metadata().date.as_deref(),
            Some("July 6, 2017")
        );
        let date = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("Dd"))
            .unwrap();
        assert_eq!(date.children().count(), 1);
        assert_eq!(
            date.children().next().and_then(crate::NodeRef::text),
            Some("$Mdocdate: Jul 6 2017 $")
        );

        let literal = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd $Mdocdate$\n.Dt DATE 1\n.Os\n.Sh NAME\ndate\n",
            ))
            .unwrap();
        assert_eq!(
            literal.document.metadata().date.as_deref(),
            Some("$Mdocdate$")
        );
    }

    #[test]
    fn assigns_and_suppresses_mdoc_section_destination_tags() {
        let name = SourceName::new("mdoc-tags.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt TAGS 1\n.Sh NAME\nname\n.Sh \"SEE ALSO\"\nfirst\n.Ss \"SEE ALSO\"\nsecond\n",
            ))
            .unwrap();
        let heads = report
            .document
            .preorder()
            .filter(|node| matches!(node.macro_name(), Some("Sh" | "Ss")))
            .filter(|node| node.kind() == NodeKind::Head)
            .collect::<Vec<_>>();
        assert_eq!(heads.len(), 3);
        assert!(heads[0].flags().deep_link_target);
        assert_eq!(heads[0].tag(), None);
        assert!(
            heads[1..]
                .iter()
                .all(|head| !head.flags().deep_link_target && head.tag().is_none())
        );
    }

    #[test]
    fn section_targets_preserve_discretionary_hyphen_and_deroff_heading_spellings() {
        let name = SourceName::new("mdoc-section-tag-spelling.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SECTION-TAGS 1\n.Os\n.Sh DESCRIPTION\n.Ss Sub-section\n.Sh \\&\\t WEIRD SECTION\\t \n",
            ))
            .unwrap();
        let heads = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Head)
            .filter(|node| matches!(node.macro_name(), Some("Sh" | "Ss")))
            .collect::<Vec<_>>();

        assert_eq!(heads.len(), 3);
        assert_eq!(heads[1].tag(), Some("Sub-section"));
        assert_eq!(heads[2].tag(), Some("WEIRD_SECTION"));
    }

    #[test]
    fn assigns_unique_emphasis_fallback_targets_like_libmandoc() {
        let name = SourceName::new("mdoc-emphasis-tags.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt EMPHASIS 1\n.Os\n.Sh DESCRIPTION\n.Em unique\\fBbold\\fP\n.Em duplicate\n.Em duplicate\n",
            ))
            .unwrap();
        let elements = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Em"))
            .collect::<Vec<_>>();
        assert_eq!(elements.len(), 3);
        assert!(elements[0].flags().deep_link_target);
        assert_eq!(elements[0].tag(), Some("unique"));
        assert!(
            elements[1..]
                .iter()
                .all(|element| !element.flags().deep_link_target && element.tag().is_none())
        );
    }

    #[test]
    fn emphasis_fallback_moves_its_destination_to_a_preceding_paragraph() {
        let name = SourceName::new("mdoc-emphasis-paragraph-tag.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EMPHASIS 1\n.Os\n.Sh DESCRIPTION\n.Pp\ncontext\n.Sy target\n",
            ))
            .unwrap();
        let paragraph = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Pp"))
            .unwrap();
        assert!(paragraph.flags().deep_link_target);
        assert!(!paragraph.flags().permalink);
        assert_eq!(paragraph.tag(), Some("target"));
        let emphasis = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Sy"))
            .unwrap();
        assert!(!emphasis.flags().deep_link_target);
        assert!(emphasis.flags().permalink);
    }

    #[test]
    fn meaningful_emphasis_fallback_replaces_a_moved_punctuation_target() {
        let name = SourceName::new("mdoc-emphasis-punctuation-target.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EMPHASIS 1\n.Os\n.Sh DESCRIPTION\n.Pp\n.Em \". b Nm\"\n.Sy bold\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let paragraph = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Pp"))
            .unwrap();
        assert!(paragraph.flags().deep_link_target);
        assert_eq!(paragraph.tag(), Some("bold"));
        let emphasis = nodes
            .iter()
            .copied()
            .find(|node| {
                node.kind() == NodeKind::Element
                    && node.macro_name() == Some("Em")
                    && node.tag() == Some(".")
            })
            .unwrap();
        assert!(emphasis.flags().deep_link_target);
        assert!(emphasis.flags().permalink);
        let symbolic = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Sy"))
            .unwrap();
        assert!(!symbolic.flags().deep_link_target);
        assert!(symbolic.flags().permalink);
    }

    #[test]
    fn duplicate_emphasis_fallback_does_not_leave_a_paragraph_target() {
        let name = SourceName::new("mdoc-emphasis-duplicate-paragraph.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EMPHASIS 1\n.Os\n.Sh DESCRIPTION\n.Pp\ncontext\n.Sy duplicate\n.Sy duplicate\n",
            ))
            .unwrap();
        assert!(
            report
                .document
                .preorder()
                .all(|node| node.macro_name() != Some("Pp"))
        );
        assert!(
            report
                .document
                .preorder()
                .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Sy"))
                .all(|node| !node.flags().deep_link_target && !node.flags().permalink)
        );
    }

    #[test]
    fn resolves_mdoc_author_and_stateful_enclosure_semantics() {
        let name = SourceName::new("mdoc-enclosure.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt ENCLOSURE 1\n.Os\n.Sh AUTHORS\n.An -nosplit Alice Example\n.Es << >>\n.En enclosed\n.An -split Bob Example\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let authors = nodes
            .iter()
            .filter(|node| node.macro_name() == Some("An"))
            .collect::<Vec<_>>();
        assert_eq!(authors.len(), 2);
        assert_eq!(authors[0].author_mode(), Some(AuthorMode::NoSplit));
        assert_eq!(authors[1].author_mode(), Some(AuthorMode::Split));
        let enclosure = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("En"))
            .and_then(crate::NodeRef::enclosure)
            .unwrap();
        assert_eq!(enclosure.opening.as_ref(), "<<");
        assert_eq!(enclosure.closing.as_deref(), Some(">>"));
        let enclosure_block = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("En"))
            .unwrap();
        assert_eq!(enclosure_block.children().count(), 2);
        assert_eq!(
            enclosure_block
                .children()
                .nth(1)
                .and_then(|body| body.children().next())
                .and_then(crate::NodeRef::text),
            Some("enclosed")
        );
        assert!(
            nodes
                .iter()
                .any(|node| node.macro_name() == Some("Es") && !node.flags().no_print)
        );
    }

    #[test]
    fn obsolete_enclosure_macros_emit_typed_warnings() {
        let name = SourceName::new("mdoc-obsolete-enclosure.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt OBSOLETE 1\n.Os\n.Sh DESCRIPTION\n.Es << >>\n.En words\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.severity))
                .collect::<Vec<_>>(),
            [
                ("mdoc.obsolete", crate::Severity::Warning),
                ("mdoc.obsolete", crate::Severity::Warning),
            ]
        );
    }

    #[test]
    fn obsolete_debug_macros_keep_their_end_of_line_arguments() {
        let name = SourceName::new("mdoc-obsolete-debug.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt OBSOLETE 1\n.Os\n.Sh DESCRIPTION\n.Db\n.Db on\n.Db foo bar\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                ("mdoc.obsolete", "obsolete macro: Db"),
                ("mdoc.obsolete", "obsolete macro: Db"),
                ("mdoc.obsolete", "obsolete macro: Db"),
            ]
        );
        assert_eq!(
            report
                .document
                .preorder()
                .filter(|node| node.macro_name() == Some("Db"))
                .flat_map(crate::NodeRef::children)
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            ["on", "foo", "bar"]
        );
    }

    #[test]
    fn duplicate_date_prologues_keep_the_last_metadata_value() {
        let name = SourceName::new("mdoc-duplicate-date.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 1, 2014\n.Dt DUPLICATE 1\n.Os\n.Dd August 3, 2014\n.Sh NAME\n.Nm duplicate-date\n.Nd date test\n.Sh DESCRIPTION\ninitial text\n.Dd August 5, 2014\nfinal text\n",
            ))
            .unwrap();
        assert_eq!(
            report.document.metadata().date.as_deref(),
            Some("August 5, 2014")
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                ("mdoc.duplicate-prologue", "duplicate prologue macro: Dd"),
                ("mdoc.duplicate-prologue", "duplicate prologue macro: Dd"),
            ]
        );
    }

    #[test]
    fn operating_system_prologues_keep_the_first_legacy_validation_flavour() {
        let name = SourceName::new("mdoc-operating-system-prologues.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".\\\" $OpenBSD: os.in,v 1.0 2026/08/26 00:00:00 maintainer Exp $\n.Dd $Mdocdate: August 26 2026 $\n.Os NetBSD\n.Dt OS 1\n.Os FreeBSD\n.Sh DESCRIPTION\n.Os OpenBSD\n",
            ))
            .unwrap();

        assert_eq!(report.document.metadata().os.as_deref(), Some("OpenBSD"));
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.severity))
                .collect::<Vec<_>>(),
            [
                ("mdoc.operating-system-explicit", Severity::Style),
                ("mdoc.mdocdate-found", Severity::Style),
                ("mdoc.prologue-order", Severity::Warning),
                ("mdoc.duplicate-prologue", Severity::Error),
                ("mdoc.operating-system-explicit", Severity::Style),
                ("mdoc.mdocdate-found", Severity::Style),
                ("mdoc.duplicate-prologue", Severity::Error),
                ("mdoc.operating-system-explicit", Severity::Style),
                ("mdoc.rcs-id-missing", Severity::Style),
            ]
        );
    }

    #[test]
    fn operating_system_validation_distinguishes_late_arbitrary_and_missing_prologues() {
        let late_name = SourceName::new("mdoc-late-os.1").unwrap();
        let late = Parser::default()
            .parse(Source::new(
                &late_name,
                b".Dd August 26, 2026\n.Dt LATE-OS 1\n.Sh DESCRIPTION\ntext\n.Os\n",
            ))
            .unwrap();
        assert_eq!(
            late.diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [("mdoc.late-operating-system", "late prologue macro: Os")]
        );

        let arbitrary_name = SourceName::new("mdoc-arbitrary-os.1").unwrap();
        let arbitrary = Parser::default()
            .parse(Source::new(
                &arbitrary_name,
                b".Dd $Mdocdate: August 26 2026 $\n.Dt ARBITRARY-OS 1\n.Os ExampleBSD\n.Sh DESCRIPTION\ntext\n",
            ))
            .unwrap();
        assert_eq!(
            arbitrary
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [(
                "mdoc.operating-system-explicit",
                "operating system explicitly specified: Os ExampleBSD (NetBSD)",
            )]
        );

        let missing_name = SourceName::new("mdoc-missing-os.1").unwrap();
        let missing = Parser::default()
            .parse(Source::new(
                &missing_name,
                b".Dd August 26, 2026\n.Dt MISSING-OS 1\n.Sh DESCRIPTION\ntext\n",
            ))
            .unwrap();
        assert_eq!(missing.document.metadata().os.as_deref(), Some(""));
        assert_eq!(
            missing
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [(
                "mdoc.operating-system-missing",
                "missing Os macro, using \"\"",
            )]
        );
    }

    #[test]
    fn duplicate_and_late_title_prologues_keep_the_last_pre_body_title() {
        let name = SourceName::new("mdoc-duplicate-title.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt FIRST 2 first_arch\n.Os\n.Dt DUPLICATE 1\n.Sh NAME\n.Nm duplicate-title\n.Nd title test\n.Sh DESCRIPTION\ninitial text\n.Dt LATE 3 late_arch\nfinal text\n",
            ))
            .unwrap();
        assert_eq!(
            report.document.metadata().title.as_deref(),
            Some("DUPLICATE")
        );
        assert_eq!(report.document.metadata().section.as_deref(), Some("1"));
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                ("mdoc.duplicate-prologue", "duplicate prologue macro: Dt"),
                ("mdoc.late-title", "skipping late title macro: Dt"),
            ]
        );
    }

    #[test]
    fn late_only_title_reports_the_missing_eof_title_after_its_source_error() {
        let name = SourceName::new("mdoc-late-only-title.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd July 4, 2017\n.Os\n.Sh NAME\n.Nm late-title\n.Nd title test\n.Sh DESCRIPTION\ninitial text\n.Dt LATE 1\nfinal text\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                ("mdoc.late-title", "skipping late title macro: Dt"),
                (
                    "mdoc.title-missing",
                    "missing manual title, using UNTITLED: EOF"
                ),
            ]
        );
        assert_eq!(
            report.document.metadata().title.as_deref(),
            Some("UNTITLED")
        );
        assert_eq!(report.document.metadata().volume.as_deref(), Some("LOCAL"));
    }

    #[test]
    fn title_discards_and_reports_the_first_fourth_argument() {
        let name = SourceName::new("mdoc-title-four-arguments.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt FOUR-ARGUMENTS 1 amd64 bogus ignored\n.Os\n.Sh DESCRIPTION\ntext\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [("mdoc.arguments", "skipping excess arguments: Dt ... bogus")]
        );
        assert_eq!(report.document.metadata().arch.as_deref(), Some("amd64"));
    }

    #[test]
    fn obsolete_es_keeps_only_its_delimiter_pair() {
        let name = SourceName::new("mdoc-obsolete-es-arguments.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt OBSOLETE 1\n.Os\n.Sh DESCRIPTION\n.Es << >> surplus\n",
            ))
            .unwrap();
        let es = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Es"))
            .unwrap();
        assert_eq!(es.children().count(), 2);
        assert!(
            report
                .document
                .preorder()
                .any(|node| node.text() == Some("surplus"))
        );
    }

    #[test]
    fn definition_item_command_tags_cover_pipes_xo_and_an_empty_tg() {
        let name = SourceName::new("mdoc-definition-item-tags.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt TAGS 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Cm one | \\&two\ntext\n.It Xo\n.Cm three\n.Xc\ntext\n.El\n.Tg\n.Cm four\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let item_tags = nodes
            .iter()
            .copied()
            .filter(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("It"))
            .map(|node| (node.tag(), node.flags().deep_link_target))
            .collect::<Vec<_>>();
        assert_eq!(item_tags, [(Some("one"), true), (Some("three"), true)]);

        let xo = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Xo"))
            .unwrap();
        assert_eq!(xo.children().count(), 2);

        let commands = nodes
            .iter()
            .copied()
            .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Cm"))
            .map(|node| {
                (
                    node.children().next().and_then(crate::NodeRef::text),
                    node.tag(),
                    node.flags().deep_link_target,
                    node.flags().permalink,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            commands,
            [
                (Some("one"), None, false, true),
                (Some("\\&two"), Some("two"), true, true),
                (Some("three"), None, false, true),
                (Some("four"), None, true, true),
            ]
        );
        assert!(nodes.iter().copied().any(|node| {
            node.kind() == NodeKind::Element
                && node.macro_name() == Some("Tg")
                && node.flags().no_print
        }));
    }

    #[test]
    fn enclosed_error_terms_move_their_destination_to_the_definition_head() {
        let name = SourceName::new("mdoc-enclosed-error-term.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt ERROR-TERMS 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Er\n.It Er one\nplain error term\n.It Bq Er ENOENT\nenclosed error term\n.El\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let heads = nodes
            .iter()
            .copied()
            .filter(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("It"))
            .map(|node| (node.tag(), node.flags().deep_link_target))
            .collect::<Vec<_>>();
        assert_eq!(heads, [(None, false), (Some("ENOENT"), true)]);

        let errors = nodes
            .iter()
            .copied()
            .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Er"))
            .map(|node| {
                (
                    node.children().next().and_then(crate::NodeRef::text),
                    node.flags().deep_link_target,
                    node.flags().permalink,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            errors,
            [(Some("one"), false, false), (Some("ENOENT"), false, true)]
        );
        assert!(nodes.iter().copied().any(|node| {
            node.kind() == NodeKind::Block
                && node.macro_name() == Some("Bq")
                && node.children().count() == 2
        }));
    }

    #[test]
    fn empty_definition_item_is_safe_for_xo_tag_postprocessing() {
        let name = SourceName::new("mdoc-empty-definition-item.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EMPTY 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It\n.El\n",
            ))
            .unwrap();
        assert!(
            report
                .document
                .preorder()
                .any(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("It"))
        );
    }

    #[test]
    fn no_fill_toggles_are_scoped_by_mdoc_display_blocks() {
        let name = SourceName::new("mdoc-display-fill-state.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FILL 1\n.Os\n.Sh DESCRIPTION\n.nf\nouter literal\n.fi\n.Bd -unfilled\ndisplay literal\n.fi\ndisplay filled\n.Ed\n.Bd -filled\n.nf\ninner literal\n.Ed\nouter filled\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        for (text, expected_no_fill) in [
            ("outer literal", true),
            ("display literal", true),
            ("display filled", false),
            ("inner literal", true),
            ("outer filled", false),
        ] {
            let node = nodes
                .iter()
                .copied()
                .find(|node| node.text() == Some(text))
                .unwrap();
            assert_eq!(node.flags().no_fill, expected_no_fill, "{text}");
        }
    }

    #[test]
    fn filled_c_blank_recovery_omits_only_the_filled_pair() {
        let name = SourceName::new("mdoc-c-blank.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt C-BLANK 1\n.Os\n.Sh DESCRIPTION\nfilled\\c\n\nnext\n.Bd -literal\nliteral\\c\n\nnext literal\n.Ed\n",
            ))
            .unwrap();
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let filled = nodes
            .iter()
            .copied()
            .find(|node| node.text() == Some("filled"))
            .unwrap();
        assert!(!filled.flags().line_continuation);
        assert!(
            !nodes
                .iter()
                .any(|node| node.text() == Some("") && !node.flags().no_fill)
        );

        let literal = nodes
            .iter()
            .copied()
            .find(|node| node.text() == Some("literal\\c"))
            .unwrap();
        assert!(literal.flags().no_fill);
        assert!(literal.flags().line_continuation);
        assert!(
            nodes
                .iter()
                .any(|node| node.text() == Some("") && node.flags().no_fill)
        );
    }

    #[test]
    fn filled_blank_lines_and_transparent_tags_share_paragraph_control_recovery() {
        let name = SourceName::new("mdoc-blank-layout-tags.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt BLANK-TAGS 1\n.Os\n.Sh NAME\n.Nm blank-tags\n.Nd paragraph layout\n.Sh DESCRIPTION\n.br\n.Tg direct\n.sp\n.Pp\n.Tg paragraph\n\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>(),
            [
                "input.blank-line-in-filled-text",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
            ]
        );

        let nodes = report.document.preorder().collect::<Vec<_>>();
        let tag = |name| {
            nodes
                .iter()
                .copied()
                .find(|node| {
                    node.macro_name() == Some("Tg")
                        && node.children().any(|child| child.text() == Some(name))
                })
                .unwrap()
        };
        let direct = tag("direct");
        assert!(direct.flags().deep_link_target);
        assert!(!direct.flags().no_print);
        let paragraph = tag("paragraph");
        assert!(paragraph.flags().no_print);
        let paragraph_owner = nodes
            .iter()
            .copied()
            .find(|node| node.macro_name() == Some("Pp") && node.tag() == Some("paragraph"))
            .unwrap();
        assert!(paragraph_owner.flags().deep_link_target);
    }

    #[test]
    fn list_tail_paragraphs_move_before_outer_paragraph_validation() {
        let name = SourceName::new("mdoc-list-tail-paragraphs.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt LIST-TAILS 1\n.Os\n.Sh NAME\n.Nm list-tails\n.Nd paragraph layout\n.Sh DESCRIPTION\n.Bl -item\n.It\nfirst\n.Pp\n.It\nsecond\n.Pp\n.El\n.Pp\nend\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| { (diagnostic.code.as_str(), diagnostic.message.as_ref(),) })
                .collect::<Vec<_>>(),
            [
                (
                    "mdoc.paragraph-before-block",
                    "skipping paragraph macro: Pp before It",
                ),
                (
                    "mdoc.paragraph-moved-out-of-list",
                    "moving paragraph macro out of list: Pp",
                ),
                (
                    "mdoc.paragraph-before-block",
                    "skipping paragraph macro: Pp before Pp",
                ),
            ]
        );
        let item_bodies = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("It"))
            .collect::<Vec<_>>();
        assert_eq!(item_bodies.len(), 2);
        assert!(item_bodies.iter().all(|body| {
            body.children()
                .all(|child| child.macro_name() != Some("Pp"))
        }));
    }

    #[test]
    fn literal_display_normalizes_whitespace_only_lines_without_losing_indent() {
        let name = SourceName::new("mdoc-literal-whitespace.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt LITERAL 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n \n \t \n x  \n.Ed\n",
            ))
            .unwrap();
        let literal_lines = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text && node.flags().no_fill)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(literal_lines, ["", "", " x"]);
    }

    #[test]
    fn reports_mismatched_and_unclosed_mdoc_scope_blocks() {
        let name = SourceName::new("mdoc-recovery.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt RECOVERY 1\n.Os\n.Sh DESCRIPTION\n.El\n.Bl -bullet\n.Bd -literal\n",
            ))
            .unwrap();
        let codes = report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            codes,
            [
                crate::DiagnosticCode::MDOC_UNMATCHED_CLOSE,
                crate::DiagnosticCode::MDOC_UNCLOSED_BLOCK,
                crate::DiagnosticCode::MDOC_UNCLOSED_BLOCK,
            ]
        );
        assert!(!report.statistics.truncated);
    }

    #[test]
    fn outer_mdoc_closers_report_a_badly_nested_full_block() {
        let name = SourceName::new("mdoc-break.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt BREAK 1\n.Os\n.Sh DESCRIPTION\n.Bl -bullet\n.Bd -literal\ntext\n.El\n.Ed\n",
            ))
            .unwrap();
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            crate::DiagnosticCode::MDOC_BADLY_NESTED_BLOCK
        );
    }

    #[test]
    fn explicit_partial_closers_report_crossed_partial_blocks() {
        let name = SourceName::new("mdoc-partial-break.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt BREAK 1\n.Os\n.Sh DESCRIPTION\n.Eo <<\n.Bo\n.Ec >>\n.Bc\n.Bo\n.Eo <<\n.Bc\n.Ec >>\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "blocks badly nested: Eo breaks Bo",
                "blocks badly nested: Bo breaks Eo",
            ]
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    let position = report
                        .document
                        .source_position(diagnostic.primary.as_ref().unwrap())
                        .unwrap();
                    (position.line, position.column)
                })
                .collect::<Vec<_>>(),
            [(7, 2), (11, 2)]
        );
        let enclosures = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Eo"))
            .collect::<Vec<_>>();
        assert_eq!(enclosures.len(), 2);
        assert_eq!(enclosures[0].children().count(), 2);
        assert_eq!(enclosures[1].children().count(), 3);
        let first_outer_body = enclosures[0]
            .children()
            .find(|node| node.kind() == NodeKind::Body)
            .unwrap();
        let first_inner_body = first_outer_body
            .children()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bo"))
            .unwrap()
            .children()
            .find(|node| node.kind() == NodeKind::Body)
            .unwrap();
        assert!(first_inner_body.children().any(|node| {
            node.kind() == NodeKind::Body
                && node.macro_name() == Some("Eo")
                && node.children().any(|child| child.text() == Some(">>"))
        }));
        let second_outer_body = enclosures[1]
            .children()
            .find(|node| node.kind() == NodeKind::Body)
            .unwrap();
        assert!(
            second_outer_body
                .children()
                .any(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bo"))
        );
    }

    #[test]
    fn implicit_partial_body_preserves_a_crossed_explicit_closer_boundary() {
        let name = SourceName::new("mdoc-implicit-crossed-closer.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt CROSSED 1\n.Os\n.Sh DESCRIPTION\n.Ao ao\n.Bo bo\n.Pq pq bc Bc ac\n.Ac\n",
            ))
            .unwrap();
        let parenthetical = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Pq"))
            .unwrap();
        let body = parenthetical
            .children()
            .find(|node| node.kind() == NodeKind::Body)
            .unwrap();
        assert_eq!(
            body.children()
                .map(|node| (node.kind(), node.macro_name(), node.text()))
                .collect::<Vec<_>>(),
            [
                (NodeKind::Text, None, Some("pq bc")),
                (NodeKind::Body, Some("Bo"), None),
                (NodeKind::Text, None, Some("ac")),
            ]
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            ["blocks badly nested: Bo breaks Pq"]
        );
        let position = report
            .document
            .source_position(report.diagnostics[0].primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((position.line, position.column), (7, 11));
    }

    #[test]
    fn validates_the_first_mdoc_root_content_before_a_section() {
        let display_name = SourceName::new("mdoc-before-section.1").unwrap();
        let display_report = Parser::default()
            .parse(Source::new(
                &display_name,
                b".Dd August 25, 2026\n.Dt BEFORE 1\n.Os\n.Bd -filled\nintro\n.Ed\n.Sh DESCRIPTION\nbody\n",
            ))
            .unwrap();
        assert_eq!(
            display_report.diagnostics[0].code.as_str(),
            crate::DiagnosticCode::MDOC_CONTENT_BEFORE_SECTION
        );
        assert_eq!(
            display_report.diagnostics[0].message.as_ref(),
            "content before first section header: Bd"
        );

        let paragraph_name = SourceName::new("mdoc-paragraph-before-section.1").unwrap();
        let paragraph_report = Parser::default()
            .parse(Source::new(
                &paragraph_name,
                b".Dd August 25, 2026\n.Dt PARAGRAPH 1\n.Os\n.Pp\n.Sh DESCRIPTION\nbody\n",
            ))
            .unwrap();
        assert_eq!(
            paragraph_report.diagnostics[0].code.as_str(),
            crate::DiagnosticCode::MDOC_PARAGRAPH_BEFORE_BLOCK
        );
        assert!(
            !paragraph_report
                .document
                .preorder()
                .any(|node| node.macro_name() == Some("Pp"))
        );
    }

    #[test]
    fn retains_an_explicit_partial_scope_across_a_broken_display_close() {
        let name = SourceName::new("mdoc-broken-display-close.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt BREAK 1\n.Os\n.Sh DESCRIPTION\n.Bd -filled\n.Bo\ninside\n.Ed\nafter display\n.Bc\nafter both\n",
            ))
            .unwrap();
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            crate::DiagnosticCode::MDOC_BADLY_NESTED_BLOCK
        );
        let bracket_body = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bo"))
            .unwrap();
        let children = bracket_body.children().collect::<Vec<_>>();
        assert!(
            children
                .iter()
                .any(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bd"))
        );
        assert!(
            children
                .iter()
                .any(|node| node.text() == Some("after display"))
        );
        assert!(
            !children
                .iter()
                .any(|node| node.text() == Some("after both"))
        );
        assert!(
            report
                .document
                .preorder()
                .any(|node| node.text() == Some("after both"))
        );
    }

    #[test]
    fn retains_a_full_display_scope_across_a_broken_partial_close() {
        let name = SourceName::new("mdoc-broken-partial-close.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt BREAK 1\n.Os\n.Sh DESCRIPTION\n.Bo\n.Bd -filled\ninside\n.Bc\nafter bracket\n.Ed\nafter both\n",
            ))
            .unwrap();
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            crate::DiagnosticCode::MDOC_BADLY_NESTED_BLOCK
        );
        let display_body = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bd"))
            .unwrap();
        let children = display_body.children().collect::<Vec<_>>();
        assert!(
            children
                .iter()
                .any(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bo"))
        );
        assert!(
            children
                .iter()
                .any(|node| node.text() == Some("after bracket"))
        );
    }

    #[test]
    fn removes_a_noncompact_preceding_layout_control_before_a_display() {
        let name = SourceName::new("mdoc-display-previous-paragraph.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt DISPLAY 1\n.Os\n.Sh DESCRIPTION\ntext\n.br\n.Bd -filled\nbody\n.Ed\n",
            ))
            .unwrap();
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            crate::DiagnosticCode::MDOC_PARAGRAPH_BEFORE_BLOCK
        );
        assert!(
            !report
                .document
                .preorder()
                .any(|node| node.macro_name() == Some("br"))
        );

        let compact_name = SourceName::new("mdoc-compact-display-previous-paragraph.1").unwrap();
        let compact_report = Parser::default()
            .parse(Source::new(
                &compact_name,
                b".Dd August 25, 2026\n.Dt COMPACT 1\n.Os\n.Sh DESCRIPTION\ntext\n.br\n.Bd -filled -compact\nbody\n.Ed\n",
            ))
            .unwrap();
        assert!(compact_report.diagnostics.is_empty());
        assert!(
            compact_report
                .document
                .preorder()
                .any(|node| node.macro_name() == Some("br"))
        );
    }

    #[test]
    fn reports_each_normally_closed_empty_display_without_removing_it() {
        let name = SourceName::new("mdoc-empty-displays.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EMPTY 1\n.Os\n.Sh DESCRIPTION\n.Bd -filled\n.Ed\n.Bd -literal\n.Ed\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>(),
            [
                crate::DiagnosticCode::MDOC_EMPTY_BLOCK,
                crate::DiagnosticCode::MDOC_EMPTY_BLOCK,
            ]
        );
        assert_eq!(
            report
                .document
                .preorder()
                .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bd"))
                .count(),
            2
        );
    }

    #[test]
    fn library_catalogue_expands_known_names_and_rehomes_outer_punctuation() {
        let name = SourceName::new("mdoc-library.3").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt LIBRARY 3\n.Os\n.Sh LIBRARY\n.Lb libbsd\n.Lb mylib .\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>(),
            [crate::DiagnosticCode::MDOC_UNKNOWN_LIBRARY]
        );
        let libraries = report
            .document
            .preorder()
            .filter(|node| node.macro_name() == Some("Lb"))
            .collect::<Vec<_>>();
        assert_eq!(libraries.len(), 2);

        let known = libraries[0].children().collect::<Vec<_>>();
        assert_eq!(
            known.iter().map(|node| node.text()).collect::<Vec<_>>(),
            [
                Some("Utility functions from BSD systems (libbsd, \\-lbsd)"),
                Some("libbsd"),
            ]
        );
        assert!(known[0].flags().generated);
        assert!(known[1].flags().no_print);

        let unknown = libraries[1].children().collect::<Vec<_>>();
        assert_eq!(
            unknown.iter().map(|node| node.text()).collect::<Vec<_>>(),
            [Some("library"), Some(r"\(lq"), Some("mylib"), Some(r"\(rq")]
        );
        let siblings = libraries[1]
            .parent()
            .expect("library has a semantic parent")
            .children()
            .collect::<Vec<_>>();
        let position = siblings
            .iter()
            .position(|node| node.id() == libraries[1].id())
            .expect("library stays in its parent");
        let outer_period = siblings
            .get(position + 1)
            .copied()
            .expect("period was moved to outer flow");
        assert_eq!(outer_period.text(), Some("."));
        assert!(outer_period.flags().delimiter_close);
        assert!(outer_period.flags().sentence_end);
    }

    #[test]
    fn item_breaks_nested_list_scope_and_relocates_pre_item_content() {
        let name = SourceName::new("mdoc-item-break.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt BREAK 1\n.Os\n.Sh DESCRIPTION\n.Bl -item\nstray text\n.Ao\nnested text\n.It\nitem text\n.El\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>(),
            [
                crate::DiagnosticCode::MDOC_BROKEN_BLOCK,
                crate::DiagnosticCode::MDOC_CONTENT_OUTSIDE_LIST,
                crate::DiagnosticCode::MDOC_CONTENT_OUTSIDE_LIST,
            ]
        );
        let list_body = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bl"))
            .unwrap();
        assert!(
            list_body
                .children()
                .all(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("It"))
        );
        assert!(
            report
                .document
                .preorder()
                .any(|node| node.text() == Some("stray text"))
        );
    }

    #[test]
    fn nm_validates_attached_trailing_delimiters_after_name_recovery() {
        let name = SourceName::new("mdoc-nm-delimiter.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt NM-DELIMITER 1\n.Os\n.Sh NAME\n.Nm nm-delimiter\n.Nd test\n.Sh DESCRIPTION\n.Nm nm-delimiter.\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| { (diagnostic.code.as_str(), diagnostic.message.as_ref(),) })
                .collect::<Vec<_>>(),
            [(
                "mdoc.trailing-delimiter-spacing",
                "no blank before trailing delimiter: Nm nm-delimiter.",
            )]
        );
        let location = report
            .document
            .source_position(report.diagnostics[0].primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((location.line, location.column), (8, 17));
    }

    #[test]
    fn nm_leading_delimiters_select_empty_recovery_or_reopened_name_flow() {
        let name = SourceName::new("mdoc-nm-leading-delimiters.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt NM 1\n.Os\n.Sh NAME\n.Nm base\n.Nd test\n.Sh DESCRIPTION\n.Nm ) z\n.Nm ( a\n.Nm | m\n",
            ))
            .unwrap();
        let names = report
            .document
            .preorder()
            .filter(|node| node.macro_name() == Some("Nm"))
            .filter_map(|node| node.children().next().and_then(crate::NodeRef::text))
            .collect::<Vec<_>>();
        assert_eq!(names, ["base", "base", "a", "m"]);
        let outer_z = report
            .document
            .preorder()
            .find(|node| node.text() == Some("z"))
            .unwrap();
        assert_eq!(
            outer_z.parent().and_then(crate::NodeRef::macro_name),
            Some("Sh")
        );
        let opening = report
            .document
            .preorder()
            .find(|node| node.text() == Some("(") && node.flags().line_start)
            .unwrap();
        assert!(opening.flags().delimiter_open);
        let reopened = report
            .document
            .preorder()
            .find(|node| {
                node.macro_name() == Some("Nm")
                    && node.children().next().and_then(crate::NodeRef::text) == Some("a")
            })
            .unwrap();
        assert!(!reopened.flags().line_start);
    }

    #[test]
    fn pa_validates_attached_trailing_delimiters() {
        let name = SourceName::new("mdoc-pa-delimiter.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt PA-DELIMITER 1\n.Os\n.Sh DESCRIPTION\n.Pa path.\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [(
                "mdoc.trailing-delimiter-spacing",
                "no blank before trailing delimiter: Pa path.",
            )]
        );
        let location = report
            .document
            .source_position(report.diagnostics[0].primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((location.line, location.column), (5, 9));
    }

    #[test]
    fn tn_discards_empty_forms_and_defers_its_useless_macro_style_finding() {
        let name = SourceName::new("mdoc-tn-validation.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt TN-VALIDATION 1\n.Os\n.Sh DESCRIPTION\n.Tn IBM\n.Tn\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                ("mdoc.empty-macro", "skipping empty macro: Tn"),
                ("mdoc.useless-macro", "useless macro: Tn"),
            ]
        );
        assert_eq!(
            report
                .document
                .preorder()
                .filter(|node| node.macro_name() == Some("Tn"))
                .count(),
            1
        );
    }

    #[test]
    fn ud_and_bt_keep_compatibility_nodes_but_validate_their_arguments() {
        let name = SourceName::new("mdoc-useless-compatibility.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt USELESS 1\n.Os\n.Sh DESCRIPTION\n.Ud\n.Bt value\n.Ud first second\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                ("mdoc.useless-macro", "useless macro: Ud"),
                ("mdoc.useless-macro", "useless macro: Bt"),
                ("mdoc.arguments", "skipping all arguments: Bt value"),
                ("mdoc.useless-macro", "useless macro: Ud"),
                ("mdoc.arguments", "skipping all arguments: Ud first"),
            ]
        );
        assert_eq!(
            report
                .document
                .preorder()
                .filter(|node| matches!(node.macro_name(), Some("Ud" | "Bt")))
                .count(),
            3
        );
        let generated_sentences = report
            .document
            .preorder()
            .filter(|node| node.flags().generated)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert_eq!(
            generated_sentences,
            [
                "currently under development.",
                "is currently in beta test.",
                "currently under development.",
            ]
        );
        assert!(
            report
                .document
                .preorder()
                .filter(|node| matches!(node.macro_name(), Some("Ud" | "Bt")))
                .all(|node| node.children().next().is_none())
        );
    }
}
