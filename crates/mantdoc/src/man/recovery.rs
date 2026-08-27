use crate::SourceSpan;

/// One macro-package recovery that the parser boundary classifies as a typed
/// diagnostic after applying the shared report budget.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Recovery {
    /// The `.TH` manual title contains lower-case ASCII letters.
    TitleNotUppercase {
        /// 用于兼容可见诊断的原始标题拼写。
        title: Box<str>,
        /// Source location of the title argument.
        location: Option<SourceSpan>,
    },
    /// The `.TH` date remains visible but does not use a supported date form.
    TitleDateUnparseable {
        /// Authored date spelling retained in metadata.
        date: Box<str>,
        /// Source location of the date argument.
        location: Option<SourceSpan>,
    },
    /// The `.TH` date argument was explicitly empty.
    TitleDateMissing {
        /// Source location of the empty date argument.
        location: Option<SourceSpan>,
    },
    /// The `.TH` request omitted or emptied its title argument.
    TitleArgumentMissing {
        /// Source location of the title request or explicit empty argument.
        location: Option<SourceSpan>,
    },
    /// The `.TH` request omitted or emptied its section argument.
    TitleSectionMissing {
        /// Authored title used by the validator to identify the request.
        title: Option<Box<str>>,
        /// Source location of the title request or explicit empty argument.
        location: Option<SourceSpan>,
    },
    /// The document omitted a usable `.TH` title.
    MissingManualTitle,
    /// The document omitted a usable `.TH` date.
    MissingManualDate,
    /// The document had no visible body after its title metadata.
    NoDocumentBody,
    /// A closing macro did not correspond to any active semantic block.
    UnmatchedClose {
        /// Closing macro spelling.
        macro_name: &'static str,
        /// Source location of the closer, when retained by the scanner.
        location: Option<SourceSpan>,
    },
    /// An open semantic block reached end of input without a closer.
    UnclosedBlock {
        /// Opening macro spelling.
        macro_name: &'static str,
        /// Source location of the opener, when retained by the scanner.
        location: Option<SourceSpan>,
    },
    /// A next-line font element reached end of input before it received text.
    LineScopeBroken {
        /// Opening font macro spelling.
        macro_name: &'static str,
        /// Source location of the opener, when retained by the scanner.
        location: Option<SourceSpan>,
    },
    /// A pending section title was interrupted by a following macro.
    LineScopeInterrupted {
        scope: Box<str>,
        breaker: Box<str>,
        location: Option<SourceSpan>,
    },
    /// A blank physical input line was skipped while a font scope remained open.
    BlankLineInScope {
        /// Source location of the blank line, when retained by the scanner.
        location: Option<SourceSpan>,
    },
    /// An empty implicit paragraph was discarded before a later scope boundary.
    EmptyParagraph {
        /// Paragraph macro spelling.
        macro_name: &'static str,
        /// Source location of the empty opener.
        location: Option<SourceSpan>,
    },
    /// A paragraph control immediately followed a section opener.
    ParagraphAfterSection {
        /// Authored paragraph-like request spelling.
        macro_name: &'static str,
        /// Section-level request that owns the empty body.
        section_name: &'static str,
        /// Source location of the paragraph opener.
        location: Option<SourceSpan>,
    },
    /// A redundant paragraph-like roff request was removed by its immediate
    /// sibling or containing man block validation.
    ParagraphSkip {
        /// Discarded request spelling.
        macro_name: &'static str,
        /// Fixed contextual relation, such as `before` or `after`.
        relation: &'static str,
        /// Request or macro spelling completing the diagnostic.
        context: &'static str,
        /// Source location of the discarded request, or its predecessor when
        /// an incoming `sp` discards a preceding `br`.
        location: Option<SourceSpan>,
    },
    /// A line-break request completed a section body.  mandoc retains the
    /// authored roff node but diagnoses its otherwise redundant placement.
    ParagraphAtSectionEnd {
        /// Authored paragraph-control spelling.
        macro_name: &'static str,
        /// Owning section-level macro spelling.
        section_name: &'static str,
        /// Source location of the retained request.
        location: Option<SourceSpan>,
    },
    ExcessArguments {
        macro_name: &'static str,
        argument: Box<str>,
        location: Option<SourceSpan>,
    },
    MissingResource {
        macro_name: &'static str,
        location: Option<SourceSpan>,
    },
    MissingOption {
        macro_name: &'static str,
        location: Option<SourceSpan>,
    },
    AllArguments {
        macro_name: &'static str,
        first_argument: Box<str>,
        has_more: bool,
        location: Option<SourceSpan>,
    },
    /// A roff request ignores its complete argument tail, whose full spelling
    /// is retained in the legacy diagnostic rather than abbreviated.
    IgnoredArguments {
        macro_name: &'static str,
        arguments: Box<str>,
        location: Option<SourceSpan>,
    },
    FewerIndents {
        target: usize,
        location: Option<SourceSpan>,
    },
    RedundantFillMode {
        message: &'static str,
        location: Option<SourceSpan>,
    },
    EmptyBlock {
        macro_name: &'static str,
        location: Option<SourceSpan>,
    },
}
