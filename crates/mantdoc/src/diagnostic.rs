//! Typed, source-addressable diagnostics independent of display wording.

use std::fmt;

use crate::SourceId;

/// Severity of a recoverable parser finding.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Severity {
    /// A construct is valid but outside the implemented semantic subset.
    Unsupported,
    /// A recoverable source error may leave output incomplete.
    Error,
    /// A suspicious or non-portable construct was recovered.
    Warning,
    /// A style recommendation was violated without changing semantics.
    Style,
}

/// Stable parser-defined identifier for a diagnostic.
///
/// It is stored independently from the message.  The M0 legacy adapter maps
/// its two wrapper-only depth findings to their historical names; new native
/// code uses lower-case dotted identifiers such as `limits.tree-depth`.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DiagnosticCode(Box<str>);

impl DiagnosticCode {
    /// Code for the historical finite syntax-tree prefix diagnostic.
    pub const LEGACY_SYNTAX_TREE_DEPTH_LIMIT: &'static str = "legacy.syntax-tree-depth-limit";
    /// Code for the historical finite equation-tree prefix diagnostic.
    pub const LEGACY_EQUATION_TREE_DEPTH_LIMIT: &'static str = "legacy.equation-tree-depth-limit";
    /// A physical source line exceeded the scanner byte limit.
    pub const LIMIT_LINE_BYTES: &'static str = "limits.line-bytes";
    /// An AST node budget stopped scanner-stage tree construction.
    pub const LIMIT_NODES: &'static str = "limits.nodes";
    /// A retained visible-text budget stopped scanner-stage tree construction.
    pub const LIMIT_TEXT_BYTES: &'static str = "limits.text-bytes";
    /// An include would exceed the configured source nesting boundary.
    pub const LIMIT_INCLUDE_DEPTH: &'static str = "limits.include-depth";
    /// One `.while` request exceeded its local iteration boundary.
    pub const LIMIT_LOOP_ITERATIONS: &'static str = "limits.loop-iterations";
    /// All `.while` requests combined exceeded the session iteration boundary.
    pub const LIMIT_TOTAL_LOOP_ITERATIONS: &'static str = "limits.total-loop-iterations";
    /// An include would exceed the configured source-count boundary.
    pub const LIMIT_SOURCES: &'static str = "limits.sources";
    /// An include would exceed the configured aggregate source-byte boundary.
    pub const LIMIT_SOURCE_BYTES: &'static str = "limits.source-bytes";
    /// A resolved source would exceed the source-map line boundary.
    pub const LIMIT_SOURCE_LINES: &'static str = "limits.source-lines";
    /// A control-line argument list exceeded a scanner-stage parser limit.
    pub const ARGUMENT_LIMIT: &'static str = "arguments.limit";
    /// A quoted control-line argument reached end of line without a close quote.
    pub const ARGUMENT_UNTERMINATED_QUOTE: &'static str = "arguments.unterminated-quote";
    /// An escape opener lacked the required following bytes.
    pub const ESCAPE_UNTERMINATED: &'static str = "escape.unterminated";
    /// An unknown named special character was retained for later recovery.
    pub const ESCAPE_UNKNOWN_SPECIAL_CHARACTER: &'static str = "escape.unknown-special-character";
    /// An escape spelling is unknown at the scanner stage.
    pub const ESCAPE_UNKNOWN: &'static str = "escape.unknown";
    /// A recognized escape used an invalid syntax shape.
    pub const ESCAPE_INVALID: &'static str = "escape.invalid";
    /// String/register expansion is intentionally deferred to the roff executor.
    pub const ESCAPE_DEFERRED_EXPANSION: &'static str = "escape.deferred-expansion";
    /// Escape work exceeded a deterministic per-line step limit.
    pub const ESCAPE_EXPANSION_LIMIT: &'static str = "limits.line-expansion-steps";
    /// Aggregate roff expansion work exceeded the session step limit.
    pub const LIMIT_EXPANSION_STEPS: &'static str = "limits.expansion-steps";
    /// Escaped visible output exceeded a deterministic per-line byte limit.
    pub const ESCAPE_OUTPUT_LIMIT: &'static str = "limits.expanded-line-bytes";
    /// A roff environment definition exceeded the shared count limit.
    pub const ROFF_DEFINITION_LIMIT: &'static str = "limits.definitions";
    /// A roff environment definition exceeded the shared byte limit.
    pub const ROFF_DEFINITION_BYTES_LIMIT: &'static str = "limits.definition-bytes";
    /// A number-register expression could not be evaluated deterministically.
    pub const ROFF_REGISTER_EXPRESSION: &'static str = "roff.register-expression";
    /// A number-register expression divided or took a remainder by zero.
    pub const ROFF_DIVISION_BY_ZERO: &'static str = "roff.division-by-zero";
    /// A string or number-register reference named no defined value.
    pub const ROFF_UNDEFINED_REFERENCE: &'static str = "roff.undefined-reference";
    /// A `.de` or `.am` request reached source end before its terminator.
    pub const ROFF_UNTERMINATED_DEFINITION: &'static str = "roff.unterminated-definition";
    /// A standalone copy-mode end marker did not close an active block.
    pub const ROFF_UNMATCHED_END: &'static str = "roff.unmatched-end";
    /// A roff `.ig` request reached source end before its marker.
    pub const ROFF_UNCLOSED_IGNORE: &'static str = "roff.unclosed-ignore-block";
    /// A roff conditional or loop scope reached source end before its `\\}` closer.
    pub const ROFF_UNTERMINATED_SCOPE: &'static str = "roff.unterminated-scope";
    /// A macro body closed a `.while` scope that was opened by its caller.
    pub const ROFF_WHILE_INNER_SCOPE: &'static str = "roff.while-inner-scope";
    /// A `.while` scope remained active after its macro invocation returned.
    pub const ROFF_WHILE_OUT_OF_SCOPE: &'static str = "roff.while-out-of-scope";
    /// A loop-control request had no active `.while` scope to continue.
    pub const ROFF_WHILE_CANNOT_CONTINUE: &'static str = "roff.while-cannot-continue";
    /// A `.while` request was opened while another loop was still active.
    pub const ROFF_WHILE_NESTED: &'static str = "roff.while-nested";
    /// Nested roff scope collection exceeded the configured structural boundary.
    pub const LIMIT_SCOPE_DEPTH: &'static str = "limits.scope-depth";
    /// Public AST assembly exceeded a caller-configured structural boundary.
    pub const LIMIT_TREE_DEPTH: &'static str = "limits.tree-depth";
    /// A `.so` target was unavailable from the caller-provided resolver.
    pub const ROFF_INCLUDE_UNAVAILABLE: &'static str = "roff.include-unavailable";
    /// A caller-provided resolver rejected or failed a `.so` target.
    pub const ROFF_INCLUDE_RESOLVER: &'static str = "roff.include-resolver";
    /// A `.so` target would re-enter the active include stack.
    pub const ROFF_INCLUDE_CYCLE: &'static str = "roff.include-cycle";
    /// A scanner-stage roff conditional did not contain a supported predicate.
    pub const ROFF_CONDITION: &'static str = "roff.condition";
    /// A `.shift` request did not contain a supported non-negative count.
    pub const ROFF_SHIFT: &'static str = "roff.shift";
    /// A roff request requiring a numeric value was given no numeric prefix.
    pub const ROFF_NON_NUMERIC_ARGUMENT: &'static str = "roff.non-numeric-argument";
    /// A roff character-control request retained only its first character.
    pub const ROFF_EXCESS_ARGUMENTS: &'static str = "roff.excess-arguments";
    /// A roff `.ft` request selected an unknown font name.
    pub const ROFF_UNKNOWN_FONT: &'static str = "roff.unknown-font";
    /// A roff identifier used an escape other than a literal escaped delimiter.
    pub const ROFF_ESCAPED_NAME: &'static str = "roff.escaped-name";
    /// A roff `.char` request did not name one bracketed character escape.
    pub const ROFF_INVALID_CHARACTER_ARGUMENT: &'static str = "roff.invalid-character-argument";
    /// A roff request kept no semantic macro definition after conditional recovery.
    pub const ROFF_UNKNOWN_MACRO: &'static str = "roff.unknown-macro";
    /// A `.return` request occurred without an active user-macro invocation.
    pub const ROFF_RETURN_OUTSIDE_MACRO: &'static str = "roff.return-outside-macro";
    /// A `\$` argument escape occurred without an active user-macro invocation.
    pub const ROFF_MACRO_ARGUMENT_OUTSIDE: &'static str = "roff.macro-argument-outside";
    /// A roff request requiring visible content was invoked without it.
    pub const ROFF_EMPTY_REQUEST: &'static str = "roff.empty-request";
    /// A `.tr` request ended with an unpaired source glyph.
    pub const ROFF_ODD_TRANSLATION: &'static str = "roff.odd-translation";
    /// A roff macro-definition terminator retained an invalid argument tail.
    pub const ROFF_ALL_ARGUMENTS: &'static str = "roff.all-arguments";
    /// Nested user macro expansion reached the configured depth boundary.
    pub const ROFF_MACRO_DEPTH_LIMIT: &'static str = "limits.macro-depth";
    /// A man(7) closing macro had no matching open semantic block.
    pub const MAN_UNMATCHED_CLOSE: &'static str = "man.unmatched-close";
    /// A man(7) semantic block reached end of input without its closing macro.
    pub const MAN_UNCLOSED_BLOCK: &'static str = "man.unclosed-block";
    /// A one-line man font scope reached end of input without content.
    pub const MAN_LINE_SCOPE_BROKEN: &'static str = "man.line-scope-broken";
    /// A blank physical line was skipped inside a one-line man font scope.
    pub const MAN_BLANK_LINE_SCOPE: &'static str = "man.blank-line-scope";
    /// A man(7) `.TH` date was retained verbatim because it is not canonical.
    pub const MAN_TITLE_DATE_UNPARSEABLE: &'static str = "man.title-date-unparseable";
    /// A man(7) `.TH` supplied an explicitly empty date argument.
    pub const MAN_TITLE_DATE_MISSING: &'static str = "man.title-date-missing";
    /// A man(7) document had no usable manual title.
    pub const MAN_TITLE_MISSING: &'static str = "man.title-missing";
    /// A man(7) `.TH` omitted its manual section argument.
    pub const MAN_TITLE_SECTION_MISSING: &'static str = "man.title-section-missing";
    /// A man(7) document contained metadata but no visible body.
    pub const MAN_NO_DOCUMENT_BODY: &'static str = "man.no-document-body";
    /// A man(7) paragraph macro was empty and removed during recovery.
    pub const MAN_EMPTY_PARAGRAPH: &'static str = "man.empty-paragraph";
    /// A man(7) extension macro received arguments it discards.
    pub const MAN_EXCESS_ARGUMENTS: &'static str = "man.excess-arguments";
    /// A man(7) extension macro requires a resource identifier.
    pub const MAN_MISSING_RESOURCE: &'static str = "man.missing-resource";
    /// A man(7) option macro was invoked without its option string.
    pub const MAN_MISSING_OPTION: &'static str = "man.missing-option";
    /// A zero-argument man(7) macro was invoked with ignored arguments.
    pub const MAN_ALL_ARGUMENTS: &'static str = "man.all-arguments";
    /// A man(7) `.RE` requested a nesting level with too few open `.RS` blocks.
    pub const MAN_FEWER_INDENTS: &'static str = "man.fewer-indents";
    /// A man(7) extension block was empty and removed during recovery.
    pub const MAN_EMPTY_BLOCK: &'static str = "man.empty-block";
    /// A man(7) `.TH` title contains lower-case ASCII letters.
    pub const MAN_TITLE_NOT_UPPERCASE: &'static str = "man.title-not-uppercase";
    /// A man(7) example macro requested the fill state already in effect.
    pub const MAN_REDUNDANT_FILL_MODE: &'static str = "man.redundant-fill-mode";
    /// A package AST retains a legacy `\\U` Unicode escape that mandoc warns about.
    pub const ESCAPE_UNSUPPORTED_UNICODE: &'static str = "escape.unsupported-unicode";
    /// One source byte is not representable as portable manual input text.
    pub const INPUT_INVALID_BYTE: &'static str = "input.invalid-byte";
    /// A physical input line ends in horizontal whitespace.
    pub const INPUT_TRAILING_WHITESPACE: &'static str = "input.trailing-whitespace";
    /// A zero-width escaped control character introduced a comment request.
    pub const INPUT_BAD_COMMENT_STYLE: &'static str = "input.bad-comment-style";
    /// A package input text line exceeds the portable style width.
    pub const INPUT_LINE_TOO_LONG: &'static str = "input.line-too-long";
    /// A literal tab occurred while the package was in fill mode.
    pub const INPUT_TAB_IN_FILLED_TEXT: &'static str = "input.tab-in-filled-text";
    /// A physical blank line occurred while the package was in fill mode.
    pub const INPUT_BLANK_LINE_IN_FILLED_TEXT: &'static str = "input.blank-line-in-filled-text";
    /// tbl preprocessing exceeded its configured logical row boundary.
    pub const LIMIT_TABLE_ROWS: &'static str = "limits.table-rows";
    /// tbl preprocessing exceeded its configured logical column boundary.
    pub const LIMIT_TABLE_COLUMNS: &'static str = "limits.table-columns";
    /// tbl preprocessing exceeded its configured logical cell boundary.
    pub const LIMIT_TABLE_CELLS: &'static str = "limits.table-cells";
    /// tbl preprocessing exceeded its configured span boundary.
    pub const LIMIT_TABLE_SPAN: &'static str = "limits.table-span";
    /// tbl preprocessing exceeded its configured text-block byte boundary.
    pub const LIMIT_TABLE_TEXT_BYTES: &'static str = "limits.table-text-bytes";
    /// eqn preprocessing exceeded its configured token boundary.
    pub const LIMIT_EQUATION_TOKENS: &'static str = "limits.equation-tokens";
    /// eqn preprocessing exceeded its configured nesting boundary.
    pub const LIMIT_EQUATION_DEPTH: &'static str = "limits.equation-depth";
    /// eqn preprocessing exceeded its configured definition boundary.
    pub const LIMIT_EQUATION_DEFINITIONS: &'static str = "limits.equation-definitions";
    /// eqn preprocessing exceeded its configured expansion-step boundary.
    pub const LIMIT_EQUATION_EXPANSION_STEPS: &'static str = "limits.equation-expansion-steps";
    /// A tbl `.TS` range reached source end without a matching `.TE`.
    pub const TBL_UNCLOSED_TABLE: &'static str = "tbl.unclosed-table";
    /// A tbl `T{` cell block reached its table boundary without a `T}` closer.
    pub const TBL_UNCLOSED_TEXT_BLOCK: &'static str = "tbl.unclosed-text-block";
    /// A tbl layout terminator appeared before any layout columns.
    pub const TBL_EMPTY_LAYOUT: &'static str = "tbl.empty-layout";
    /// A tbl layout font modifier did not name a recognized roff font.
    pub const TBL_UNKNOWN_FONT: &'static str = "tbl.unknown-font";
    /// A tbl layout row begins with a horizontal-span cell.
    pub const TBL_LEADING_SPAN: &'static str = "tbl.leading-span";
    /// A roff macro was encountered inside a tbl text block.
    pub const TBL_MACRO: &'static str = "tbl.macro";
    /// A tbl data row provided more cells than its active layout accepts.
    pub const TBL_EXTRA_DATA_CELLS: &'static str = "tbl.extra-data-cells";
    /// A complete tbl range did not contain any data cells.
    pub const TBL_NO_DATA: &'static str = "tbl.no-data";
    /// A tbl layout contains more than two adjacent vertical bars.
    pub const TBL_VERTICAL_BAR: &'static str = "tbl.vertical-bar";
    /// The first tbl layout row starts a column with a vertical span.
    pub const TBL_LEADING_DOWN: &'static str = "tbl.leading-down";
    /// A tbl horizontal or vertical span received visible data.
    pub const TBL_SPANNED_DATA: &'static str = "tbl.spanned-data";
    /// A tbl option requiring parentheses omitted its argument.
    pub const TBL_OPTION_ARGUMENT: &'static str = "tbl.option-argument";
    /// A tbl option argument did not have its required character count.
    pub const TBL_OPTION_ARGUMENT_SIZE: &'static str = "tbl.option-argument-size";
    /// An option list contained a byte that cannot begin an option name.
    pub const TBL_OPTION_CHARACTER: &'static str = "tbl.option-character";
    /// tbl encountered an option name outside its supported grammar.
    pub const TBL_UNKNOWN_OPTION: &'static str = "tbl.unknown-option";
    /// A tbl layout uses an excessive inter-column spacing modifier.
    pub const TBL_EXCESSIVE_SPACING: &'static str = "tbl.excessive-spacing";
    /// tbl's eqn delimiter option is parsed but not applied to roff prose.
    pub const TBL_EQN_DELIMITER_OPTION: &'static str = "tbl.eqn-delimiter-option";
    /// An eqn `.EQ` range reached source end without a matching `.EN`.
    pub const EQN_UNCLOSED_DISPLAY: &'static str = "eqn.unclosed-display";
    /// An eqn definition expanded through itself and was recoverably stopped.
    pub const EQN_RECURSIVE_DEFINITION: &'static str = "eqn.recursive-definition";
    /// An eqn definition request omitted its required name or replacement.
    pub const EQN_EMPTY_REQUEST: &'static str = "eqn.empty-request";
    /// An eqn binary operator was missing a required operand.
    pub const EQN_MISSING_BOX: &'static str = "eqn.missing-box";
    /// An mdoc closing macro had no matching open semantic block.
    pub const MDOC_UNMATCHED_CLOSE: &'static str = "mdoc.unmatched-close";
    /// An mdoc semantic block reached end of input without its closing macro.
    pub const MDOC_UNCLOSED_BLOCK: &'static str = "mdoc.unclosed-block";
    /// An mdoc display macro did not receive its required body text.
    pub const MDOC_EMPTY_BLOCK: &'static str = "mdoc.empty-block";
    /// An mdoc list item retained no body after structural recovery.
    pub const MDOC_EMPTY_LIST_ITEM: &'static str = "mdoc.empty-list-item";
    /// An mdoc list relocated direct content before its first item.
    pub const MDOC_CONTENT_OUTSIDE_LIST: &'static str = "mdoc.content-outside-list";
    /// An mdoc inline macro requiring content was invoked without it.
    pub const MDOC_EMPTY_MACRO: &'static str = "mdoc.empty-macro";
    /// An mdoc manual target contains whitespace or an unsupported escape.
    pub const MDOC_INVALID_TAG: &'static str = "mdoc.invalid-tag";
    /// A parsed mdoc macro received a known but non-callable macro spelling.
    pub const MDOC_NON_CALLABLE_MACRO: &'static str = "mdoc.non-callable-macro";
    /// An mdoc cross reference omitted its manual section argument.
    pub const MDOC_REFERENCE_SECTION_MISSING: &'static str = "mdoc.reference-section-missing";
    /// Cross references in an mdoc `SEE ALSO` section are out of order.
    pub const MDOC_REFERENCE_ORDER: &'static str = "mdoc.reference-order";
    /// An mdoc `.Ns` request cannot suppress spacing at its source position.
    pub const MDOC_NO_SPACE_MACRO: &'static str = "mdoc.no-space-macro";
    /// An mdoc boolean control did not receive `on` or `off`.
    pub const MDOC_BOOLEAN_ARGUMENT: &'static str = "mdoc.boolean-argument";
    /// An mdoc reference block contains a non-bibliographic direct child.
    pub const MDOC_REFERENCE_CONTENT: &'static str = "mdoc.reference-content";
    /// An mdoc reference block closed without a bibliographic child.
    pub const MDOC_EMPTY_REFERENCE_BLOCK: &'static str = "mdoc.empty-reference-block";
    /// The first mdoc section was not the conventional NAME section.
    pub const MDOC_FIRST_SECTION_NOT_NAME: &'static str = "mdoc.first-section-not-name";
    /// An mdoc macro repeated an option whose first occurrence already won.
    pub const MDOC_DUPLICATE_ARGUMENT: &'static str = "mdoc.duplicate-argument";
    /// An `.At` selector did not name a standardized AT&T UNIX version.
    pub const MDOC_UNKNOWN_AT_VERSION: &'static str = "mdoc.unknown-at-version";
    /// A display `-offset` option did not provide a width.
    pub const MDOC_EMPTY_ARGUMENT: &'static str = "mdoc.empty-argument";
    /// A display had no selected type and recovered as ragged.
    pub const MDOC_MISSING_DISPLAY_TYPE: &'static str = "mdoc.missing-display-type";
    /// A display supplied a second type after its first type had won.
    pub const MDOC_DUPLICATE_DISPLAY_TYPE: &'static str = "mdoc.duplicate-display-type";
    /// A display requested the unsupported file-backed source form.
    pub const MDOC_UNSUPPORTED_DISPLAY_FILE: &'static str = "mdoc.unsupported-display-file";
    /// A display had no options and was removed while retaining its body.
    pub const MDOC_DISPLAY_WITHOUT_ARGUMENTS: &'static str = "mdoc.display-without-arguments";
    /// A display block occurred inside another display block.
    pub const MDOC_NESTED_DISPLAY: &'static str = "mdoc.nested-display";
    /// A font block did not select a recognized type.
    pub const MDOC_MISSING_FONT_TYPE: &'static str = "mdoc.missing-font-type";
    /// A font block selected an unknown legacy font name.
    pub const MDOC_UNKNOWN_FONT_TYPE: &'static str = "mdoc.unknown-font-type";
    /// An mdoc structural block forcibly closed an open block.
    pub const MDOC_BROKEN_BLOCK: &'static str = "mdoc.broken-block";
    /// An mdoc or roff layout request supplied arguments that mandoc discards.
    pub const MDOC_ARGUMENTS: &'static str = "mdoc.arguments";
    /// An mdoc compatibility macro is obsolete but remains recoverably parsed.
    pub const MDOC_OBSOLETE: &'static str = "mdoc.obsolete";
    /// An mdoc prologue request repeated an earlier request of the same kind.
    pub const MDOC_DUPLICATE_PROLOGUE: &'static str = "mdoc.duplicate-prologue";
    /// An mdoc `.Os` request explicitly named a legacy-checked operating system.
    pub const MDOC_OPERATING_SYSTEM_EXPLICIT: &'static str = "mdoc.operating-system-explicit";
    /// A NetBSD-style mdoc document uses an `$Mdocdate` date prologue.
    pub const MDOC_MDOCDATE_FOUND: &'static str = "mdoc.mdocdate-found";
    /// An OpenBSD-style RCS id lacks an `$Mdocdate` date prologue.
    pub const MDOC_MDOCDATE_MISSING: &'static str = "mdoc.mdocdate-missing";
    /// A legacy-checked operating system lacks a matching RCS id comment.
    pub const MDOC_RCS_ID_MISSING: &'static str = "mdoc.rcs-id-missing";
    /// An mdoc `.Os` request appeared after visible document content.
    pub const MDOC_LATE_OPERATING_SYSTEM: &'static str = "mdoc.late-operating-system";
    /// An mdoc document did not provide an `.Os` prologue request.
    pub const MDOC_OPERATING_SYSTEM_MISSING: &'static str = "mdoc.operating-system-missing";
    /// An mdoc `.Pf` prefix had no following presentation token.
    pub const MDOC_PREFIX_WITHOUT_FOLLOWING: &'static str = "mdoc.prefix-without-following";
    /// An mdoc compatibility macro had no semantic presentation effect.
    pub const MDOC_USELESS_MACRO: &'static str = "mdoc.useless-macro";
    /// An mdoc title request occurred after body parsing had begun.
    pub const MDOC_LATE_TITLE: &'static str = "mdoc.late-title";
    /// An mdoc `.Dt` title contains lower-case ASCII letters.
    pub const MDOC_TITLE_NOT_UPPERCASE: &'static str = "mdoc.title-not-uppercase";
    /// An mdoc `.Dt` section is not a recognized manual section identifier.
    pub const MDOC_TITLE_SECTION_UNKNOWN: &'static str = "mdoc.title-section-unknown";
    /// An mdoc `.Dt` omitted its manual section argument.
    pub const MDOC_TITLE_SECTION_MISSING: &'static str = "mdoc.title-section-missing";
    /// An mdoc `.Dd` request omitted its date argument.
    pub const MDOC_DATE_MISSING: &'static str = "mdoc.date-missing";
    /// An mdoc `.Dd` date could not be parsed and was retained verbatim.
    pub const MDOC_DATE_UNPARSEABLE: &'static str = "mdoc.date-unparseable";
    /// An mdoc `.Dd` date uses the obsolete man(7) ISO spelling.
    pub const MDOC_DATE_LEGACY: &'static str = "mdoc.date-legacy";
    /// An mdoc prologue request appeared after visible document body content.
    pub const MDOC_LATE_PROLOGUE: &'static str = "mdoc.late-prologue";
    /// An mdoc prologue request appeared after an incompatible prologue peer.
    pub const MDOC_PROLOGUE_ORDER: &'static str = "mdoc.prologue-order";
    /// An mdoc document reached end of input without a title prologue.
    pub const MDOC_TITLE_MISSING: &'static str = "mdoc.title-missing";
    /// An mdoc document contained no visible document body.
    pub const MDOC_NO_DOCUMENT_BODY: &'static str = "mdoc.no-document-body";
    /// An mdoc name macro had no explicit or previously declared name.
    pub const MDOC_NAME_MISSING: &'static str = "mdoc.name-missing";
    /// An mdoc `.Fo` declaration omitted its required function name.
    pub const MDOC_FUNCTION_NAME_MISSING: &'static str = "mdoc.function-name-missing";
    /// A standard exit-status expansion had no available utility name.
    pub const MDOC_EXIT_NAME_MISSING: &'static str = "mdoc.exit-name-missing";
    /// A standard Ex/Rv expansion omitted its required `-std` selector.
    pub const MDOC_STANDARD_SELECTOR_MISSING: &'static str = "mdoc.standard-selector-missing";
    /// An mdoc document begins with visible content rather than a section header.
    pub const MDOC_CONTENT_BEFORE_SECTION: &'static str = "mdoc.content-before-section";
    /// A conventional mdoc section is not appropriate for the manual section.
    pub const MDOC_UNEXPECTED_SECTION: &'static str = "mdoc.unexpected-section";
    /// A conventional mdoc section repeated the preceding named section.
    pub const MDOC_DUPLICATE_SECTION: &'static str = "mdoc.duplicate-section";
    /// A conventional mdoc section occurred before the preceding named section.
    pub const MDOC_SECTION_ORDER: &'static str = "mdoc.section-order";
    /// An mdoc paragraph macro was discarded before a following block.
    pub const MDOC_PARAGRAPH_BEFORE_BLOCK: &'static str = "mdoc.paragraph-before-block";
    /// A trailing mdoc paragraph macro was moved outside its enclosing list.
    pub const MDOC_PARAGRAPH_MOVED_OUT_OF_LIST: &'static str = "mdoc.paragraph-moved-out-of-list";
    /// A full mdoc block was closed through an explicit partial block.
    pub const MDOC_BADLY_NESTED_BLOCK: &'static str = "mdoc.badly-nested-block";
    /// An mdoc item request occurred without an active list.
    pub const MDOC_ITEM_OUTSIDE_LIST: &'static str = "mdoc.item-outside-list";
    /// An mdoc tabular-column request occurred outside a column list.
    pub const MDOC_COLUMN_OUTSIDE_LIST: &'static str = "mdoc.column-outside-list";
    /// An implicit mdoc enclosure argument directly precedes its closing delimiter.
    pub const MDOC_TRAILING_DELIMITER_SPACING: &'static str = "mdoc.trailing-delimiter-spacing";
    /// An mdoc macro retained a delimiter that must be outer syntax.
    pub const MDOC_TRAILING_DELIMITER: &'static str = "mdoc.trailing-delimiter";
    /// An mdoc description line occurred outside the `NAME` section.
    pub const MDOC_DESCRIPTION_OUTSIDE_NAME: &'static str = "mdoc.description-outside-name";
    /// An mdoc description line did not contain a visible description.
    pub const MDOC_DESCRIPTION_MISSING: &'static str = "mdoc.description-missing";
    /// A direct child of an mdoc `NAME` section was not `.Nm` or `.Nd`.
    pub const MDOC_NAME_SECTION_CONTENT: &'static str = "mdoc.name-section-content";
    /// Consecutive mdoc names in `NAME` were not separated by a comma.
    pub const MDOC_NAME_SECTION_COMMA_MISSING: &'static str = "mdoc.name-section-comma-missing";
    /// An mdoc `NAME` section did not contain a direct `.Nm` child.
    pub const MDOC_NAME_SECTION_NAME_MISSING: &'static str = "mdoc.name-section-name-missing";
    /// An mdoc `NAME` section did not contain a direct `.Nd` child.
    pub const MDOC_NAME_SECTION_DESCRIPTION_MISSING: &'static str =
        "mdoc.name-section-description-missing";
    /// A direct mdoc `.Nd` was not the last child of the `NAME` section.
    pub const MDOC_NAME_SECTION_DESCRIPTION_NOT_LAST: &'static str =
        "mdoc.name-section-description-not-last";
    /// An mdoc `AUTHORS` section did not contain a populated `.An` macro.
    pub const MDOC_AUTHORS_MISSING: &'static str = "mdoc.authors-missing";
    /// An mdoc function name retained a parenthesis requiring semantic syntax.
    pub const MDOC_FUNCTION_NAME_PARENTHESIS: &'static str = "mdoc.function-name-parenthesis";
    /// An mdoc function argument retained a comma before a callback/array suffix.
    pub const MDOC_FUNCTION_ARGUMENT_COMMA: &'static str = "mdoc.function-argument-comma";
    /// An mdoc library selector was not present in the built-in catalogue.
    pub const MDOC_UNKNOWN_LIBRARY: &'static str = "mdoc.unknown-library";
    /// An mdoc `.St` selector was not present in the built-in standard catalogue.
    pub const MDOC_UNKNOWN_STANDARD: &'static str = "mdoc.unknown-standard";

    /// Construct a validated code from an ASCII dotted identifier.
    ///
    /// # Errors
    ///
    /// Returns [`crate::InvalidDiagnosticCode`] when `value` is empty or not composed
    /// of lowercase ASCII letters, digits, hyphens, and non-leading dots.
    pub fn new(value: impl AsRef<str>) -> Result<Self, InvalidDiagnosticCode> {
        let value = value.as_ref();
        let valid = !value.is_empty()
            && !value.starts_with('.')
            && !value.ends_with('.')
            && !value.contains("..")
            && value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
            });
        valid
            .then(|| Self(value.into()))
            .ok_or(InvalidDiagnosticCode)
    }

    /// Borrow the stable code string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A value that is not a valid [`DiagnosticCode`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidDiagnosticCode;

impl fmt::Display for InvalidDiagnosticCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("diagnostic codes must be lower-case dotted ASCII identifiers")
    }
}

impl std::error::Error for InvalidDiagnosticCode {}

/// One byte-oriented source range.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSpan {
    /// Document-local source identity, resolved through [`crate::Document`].
    pub source: SourceId,
    /// Zero-based byte offset of the first byte.
    pub start: u32,
    /// Zero-based exclusive byte offset after the last byte.
    pub end: u32,
    /// Parser-defined logical start position when byte offsets alone cannot
    /// represent the legacy location, for example after physical roff-line
    /// continuation joins.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub logical_start: Option<crate::SourcePosition>,
}

impl SourceSpan {
    /// Construct a span after checking monotonic byte offsets.
    ///
    /// # Errors
    ///
    /// Returns [`crate::InvalidSpan`] when `end` precedes `start`.
    pub fn new(source: SourceId, start: u32, end: u32) -> Result<Self, InvalidSpan> {
        (start <= end)
            .then_some(Self {
                source,
                start,
                end,
                logical_start: None,
            })
            .ok_or(InvalidSpan)
    }

    /// Associate this byte range with a parser-defined logical start.
    ///
    /// The byte range remains available for source slicing.  Consumers that
    /// ask the owning document for the position receive this logical value.
    #[must_use]
    pub const fn with_logical_start(mut self, position: crate::SourcePosition) -> Self {
        self.logical_start = Some(position);
        self
    }
}

/// A span with a human explanation of its relationship to a diagnostic.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelatedSpan {
    /// Related source span.
    pub span: SourceSpan,
    /// Concise relationship label.
    pub message: Box<str>,
}

/// One deterministic non-fatal parser finding.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    /// Stable machine-readable identifier.
    pub code: DiagnosticCode,
    /// Severity independent from the display message.
    pub severity: Severity,
    /// Primary source range, when parser state can identify one.
    pub primary: Option<SourceSpan>,
    /// Other source ranges that explain the finding.
    pub related: Vec<RelatedSpan>,
    /// Human-readable explanation not used as a programmatic identifier.
    pub message: Box<str>,
}

impl Diagnostic {
    /// Construct a finding without source locations.
    #[must_use]
    pub fn new(code: DiagnosticCode, severity: Severity, message: impl Into<Box<str>>) -> Self {
        Self {
            code,
            severity,
            primary: None,
            related: Vec::new(),
            message: message.into(),
        }
    }

    /// Attach a primary source range.
    #[must_use]
    pub fn with_primary(mut self, primary: SourceSpan) -> Self {
        self.primary = Some(primary);
        self
    }

    /// Attach one explanatory source range.
    #[must_use]
    pub fn with_related(mut self, span: SourceSpan, message: impl Into<Box<str>>) -> Self {
        self.related.push(RelatedSpan {
            span,
            message: message.into(),
        });
        self
    }
}

/// A span whose end offset precedes its start offset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidSpan;

impl fmt::Display for InvalidSpan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("source span end precedes its start")
    }
}

impl std::error::Error for InvalidSpan {}

#[cfg(test)]
mod tests {
    use super::{Diagnostic, DiagnosticCode, Severity};

    #[test]
    fn codes_are_data_not_message_inference() {
        let code = DiagnosticCode::new(DiagnosticCode::ROFF_UNDEFINED_REFERENCE)
            .expect("native roff code must be valid");
        let first = Diagnostic::new(code.clone(), Severity::Unsupported, "first wording");
        let second = Diagnostic::new(code, Severity::Unsupported, "improved wording");
        assert_eq!(first.code, second.code);
        assert_ne!(first.message, second.message);
    }

    #[test]
    fn invalid_code_shapes_cannot_leak_into_reports() {
        assert!(DiagnosticCode::new("Parser.NotImplemented").is_err());
        assert!(DiagnosticCode::new("parser..not-implemented").is_err());
        assert!(DiagnosticCode::new("parser.not_implemented").is_err());
    }
}
