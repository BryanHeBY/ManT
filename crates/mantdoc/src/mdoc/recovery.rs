use crate::SourceSpan;

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
    /// An OpenBSD-style RCS id requires an `$Mdocdate` date prologue.
    MdocDateMissing {
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
