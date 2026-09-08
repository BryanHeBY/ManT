# mant-roff

## Name

mant-roff — native man, mdoc, tbl, eqn, and roff compatibility in ManT

## Description

ManT reads native manual pages through a vendored `libmandoc` 1.14.6 parser and lowers its validated owned syntax tree into [mant-ir(7)](mant-ir.md). The supported authoring languages are `man(7)` and `mdoc(7)` with the subset of roff requests, escapes, `tbl(7)`, and `eqn(7)` that occur inside those manuals.

ManT is a semantic manual reader, not a general troff formatter. Device geometry, page headers and footers, traps, diversions, arbitrary postprocessor commands, and print-specific typography are outside its output model.

## Support Levels

This reference uses four distinct support levels:

| Level | Meaning |
| --- | --- |
| Semantic | ManT emits a dedicated IR node or typed property |
| Visible | libmandoc parses the construct and ManT retains its visible children, but source-specific semantics may be flattened |
| Presentation-only | Arguments are consumed and deliberately omitted because the IR has no device state |
| Rejected | Processing stops or the unsafe operation is denied |

A macro not listed as semantic may still be visible because libmandoc expands or validates it before lowering. That behavior is compatibility fallback, not a promise that every groff layout detail is reproduced.

## Input Boundary

Native discovery accepts ordinary, gzip, and zstd-compressed manual sources. Decompression and reads are bounded before bytes enter libmandoc. The original source path remains in the IR.

Input bytes use libmandoc's deterministic UTF-8/Latin-1 detection. A leading UTF-8 byte-order mark and recognized `coding: UTF-8`, `coding: latin-1`, `coding: iso-latin-1`, or `coding: ISO-8859-1` declarations select the corresponding supported path. A declaration naming another charset cannot make the parse more destructive: because the bundled converter cannot implement that charset, ManT retains automatic UTF-8/Latin-1 detection rather than replacing every high byte with `?`. This is a preservation fallback, not a promise to interpret arbitrary declared encodings such as ISO-8859-9.

Indexed redirect-only pages containing `.so target` may resolve only to another discovered page inside the same approved manual hierarchy. Standalone `--input` files reject `.so` redirects because no trusted hierarchy accompanies them. An embedded `.so` request is not followed; libmandoc reports the denied include while ManT preserves the surrounding page content.

libmandoc file inclusion is disabled. Requests that read, write, execute, pipe, or include arbitrary files remain denied or ignored by the upstream safe parser. ManT never invokes the host `man`, `groff`, `nroff`, or shell executable.

## Native-to-IR Semantic Mapping

Native macros are parser evidence, not frontend instructions. Lowering consumes
that evidence once and emits the same source-neutral facts used by Markdown:

| Native evidence | Shared IR result |
| --- | --- |
| `SH`/`SS`, `Sh`/`Ss` | Recursive `Section` nodes |
| `TP`/`TQ`/qualified `IP`, definition-form `It` | `DefinitionItem` content |
| Reliable option, command, variable, or value context | Optional `EntryFacts` on that content |
| `Tg` and resolvable `Sx` | Zero-width anchor or local section destination |
| `Xr`, `MR`, conservative structured manual reference | Typed `Manual` graph edge |
| `UR`/`UE`, `MT`/`ME`, `Lk`, `Mt` | External or email host-action target |

The rebuildable `SemanticIndex` later groups identified definitions into
logical entries while retaining authored forms and nested ownership. It does
not scan rendered prose to invent entries. Likewise, only typed manual targets
can expand a multi-document scope; link-looking text is not a graph edge. This
keeps native and Markdown queries aligned even though their producer syntax is
different. The complete shared model is defined by [mant-ir(7)](mant-ir.md).

## man Language

The following `man(7)` macros have dedicated lowering behavior:

| Macros | ManT result |
| --- | --- |
| `TH` | Title, native manual section, date, source, and volume metadata |
| `SH`, `SS` | Top-level sections and child sections |
| `P`, `PP`, `LP`, `HP` | Paragraph boundaries and retained vertical spacing |
| `RS`, `RE` | Nested indentation boundary |
| `IP`, `TP`, `TQ` | Bullet, ordered-list, or definition-list items, explicit multi-tag heads, hanging layout, and widths |
| `PD` | Paragraph, definition-item, and heading spacing |
| `B`, `SB` | Strong inline content |
| `I` | Emphasized inline content |
| `BI`, `BR`, `IB`, `IR`, `RB`, `RI` | Alternating inline font runs without inserted spaces |
| `EX`, `EE` | Preformatted no-fill region through libmandoc's fill state |
| `SY`, `YS` | Synopsis head plus body; inside `EX` the source lines remain one preformatted block |
| `UR`, `UE` | Inline external link; a label and its target both remain visible without splitting the surrounding sentence |
| `MT`, `ME` | Inline email link; a label and its address both remain visible without splitting the surrounding sentence |
| `MR` | Typed manual-page reference |

`br` inside a flow becomes an inline line break. `sp` becomes explicit vertical space. Filled source lines normally join with spaces; an indented input line and no-fill input preserve line boundaries. In a no-fill display, a run of raw blank input lines is one visual separator, while an explicit `sp` retains its requested separation. A final unescaped `\c` suppresses that implicit space or line break and joins the next input line directly.

Display offsets use terminal-column unit conversion. Unsupported or excessive
offsets use the default indentation; cumulative indentation is capped at 4096
columns with a `manual.indentation-limit` warning rather than integer overflow.

The `IP` and `TP` macros are source-ambiguous: a leading mark can introduce a
bullet, an enumerated paragraph, or a glossary-style definition. ManT
recognizes single-glyph bullet marks at that source boundary. A punctuated
integer such as `1.`, `1)`, `(1)`, or `[1]` is sufficient evidence for an
ordered item, including a one-item footnote list. Adjacent, same-style,
consecutively increasing marks join one list; a gap or style change begins a
new list at the explicit number. A bare integer is treated as ordered only
when the original `IP` call uses roff's pre-increment number-register form.
This preserves real option value domains such as `0`/`1` as definitions while
keeping numbered instructions and references out of the semantic-entry index.
An immediately following `RS` region remains content of the current item, as
required for generated references and hierarchically indented lists.

`RS` uses its authored finite literal distance, or the prevailing tag width when omitted; a new relative-indent scope starts with a seven-cell tag width and `RE` restores the outer scope. Distances accumulate in source basic units before conversion to terminal cells, preserving fractional offsets. `TP`/`IP` widths determine both label fitting and the actual description origin. Ordinary man paragraphs (`PP`, `P`, `LP`) restore the default seven-cell tag width; `HP` is not that reset boundary. Unsupported measurements and excessive cumulative offsets produce `manual.indentation-limit` rather than wrapping arithmetic. This layout policy does not change semantic parentage: visual indentation alone never turns a top-level command into its parameter.

mdoc definition styles retain distinct layout: `-tag` fits against its width, `-hang` permits run-in heads, `-inset` and `-diag` use one and two separating cells respectively without a fixed body indent, and `-ohang` places the body below the head. List `-offset` moves the complete list, not each descendant. Explicit widths use mdoc's two-cell buffer; native normalization supplies default bullet and enum widths. A width or offset without a recognized numeric unit is measured as printable text. `Bd` without an offset and `-offset left` do not add indentation; `indent` and `indent-two` resolve to six and twelve cells, and `D1`/`Dl` use six. Recovered numbered lists preserve their original body column rather than replacing it with a generic list indent.

When indented continuation blocks are reattached to a preceding definition, explicit vertical spacing does not by itself end ownership. A consecutive run of spacing belongs to that continuation only if the next content block remains more deeply indented. Same-level or outer content, other layout-less boundaries, and the end of a container stop collection; trailing spacing is left outside. This retains successive `RS` regions, nested definitions, and trailing examples in node excerpts without deleting blank lines or absorbing the next option or section. Hanging-definition recovery uses the same continuation boundary. Restoring content ownership does not declare those children to be an exhaustive value domain.

For an inline definition, only the initial paragraph is attached to the label's line. Subsequent paragraphs, code and nested blocks retain the same structural origin as a non-inline description, independently of label width. Explicit leading vertical space is kept between the term and description rather than trimmed away to force an inline presentation.

`OP` retains its optional-argument brackets, bold option name, and emphasized metavariable both inside and outside a `SY` synopsis; it does not create a separate IR variant. `AT`, `DT`, `SM`, `UC`, and other libmandoc-recognized man macros retain printable children where available but do not currently have a dedicated ManT semantic variant. For example, `SM` does not preserve point size.

## mdoc Structure

Option aliases come from invocation names separated by explicit alias punctuation, not arbitrary later dash-prefixed argument tokens. Emphasized argument spans retain their spelling in forms but cannot introduce another alias. This applies to historical slash notation as well: `Fl n Ns / Ns Ar -NUM` retains the form `-n/-NUM`, but only `-n` is addressable; `Fl n Ns / Ns Fl -number` exposes both `-n` and `--number`. Slashes inside argument spans or paths are not alias separators.

Styled command aliases are likewise grouped before separating each command name from its arguments. A single environment assignment retains its complete value, including commas and pipes, but only its variable name becomes a selector. Mixed assignments with unprovable name/value boundaries remain unclassified rather than introducing guessed aliases. Variable subscripts must be completely closed, with no trailing text or repeated brackets.

Declaration grouping preserves bracket nesting and parameter styling across inline wrappers. Commas and pipes inside `[a|b]`, `{+|-}`, or a styled parameter do not create names. A subsequent explicit declaration can resume at a literal separator, such as `-L` followed by an emphasized `dir,--FAKE` and literal `, --library`; only `-L` and `--library` are names. Unclosed or excessively nested syntax is kept as a complete form without guessing additional declarations. Literal shell commands such as `[` remain valid names.

Layout-inferred paragraph heads must pass a complete declaration check before an indented following block becomes their description. Finding one valid comma-separated word inside prose is insufficient: an environment declaration group is accepted or rejected as a whole. Option heads may contain bounded argument syntax and styled placeholders, but ordinary explanatory suffixes do not establish a new owner. Explicit `TP`/`IP`/definition `It` labels keep their authored boundary even when the label remains an unclassified Term.

An mdoc definition headed by `Fl` or `Ev` retains that option or environment-variable evidence independently of its section heading. This includes punctuation flags such as `-@`, `-%`, and `-,`, and an environment name followed by an `Ar` placeholder. `Ic` and `Cm` preserve literal naming evidence but do not alone prove that a definition is a command; a named Term is valid when context is inconclusive. Unsupported name syntax does not erase a proved native role. The same macros in ordinary body text remain inline mentions, not new definitions.

Complete named heads can retain bounded trailing parenthesized annotations or angle-bracket placeholders without adding those suffixes to names: `AUTO_CD (-J) <D>`, `NAME <TLS backend>`, and `update (-u)` preserve their full forms. Nested parenthesized defaults and literal key bindings such as `C-[` remain intact. A single assignment binds its left-hand name, not its value; assignment spelling alone does not prove an Option or ConfigurationKey role (`if=FILE` can remain a named Term). Configuration contexts support exact dotted keys. Templates such as `[url-protocol]_PROXY`, malformed groups and prose suffixes are not expanded into guessed names, and neither case variants nor annotations create alias relationships.

Complete command, configuration-key and variable heads can use the same paragraph-plus-indented-description shape as options. Command/variable recovery requires a literal-styled head in a matching context; configuration/variable contexts also accept complete unstyled dotted keys. A mixed-case assignment label such as `WorkingDirectory=` can establish a configuration head under a topical section, including when styled in italics. Spacing-only runs between a validated head and its deeper body are preserved, not treated as new owners. Recovery stops at outer content, another same-level head, a table boundary or the end of the container. A complete local dash declaration takes precedence over inherited value/configuration hints without changing its place in the document tree.

Adjacent literal native runs may form one multiword command name, such as `Nm zfs Cm get`; the first argument ends that name. Native literal heads with explicit option-group syntax can establish a command under a topical heading; other ambiguous `Ic`/`Cm` heads remain named Terms. Parameter alternations and later argument fragments never create extra names, and multiple names do not imply aliases.

A complete short/long pair such as `-a --ascii` or `-a or --ascii` exposes both names without implying an alias relationship. This is a bounded declaration convention, not general argv parsing: an arbitrary third literal token or later dash-prefixed argument does not restart name recognition, and parameter paths/alternatives remain part of their original form.

Inherited section/parent categories are defaults, not accepted-value guarantees. Local option spellings and proved `Fl`/`Ev` roles take precedence. A setting assignment inside an option description can be a ConfigurationKey (`color=[yes|no]`); a mixed-case setting such as `Environment=` is not automatically an environment variable because its heading contains “Environment”. Uppercase underscore names nested under an option remain named Terms without separate environment evidence. Negative numbers and regex forms are not promoted by the local flag rule. These are bounded, best-effort category rules, not runtime validation or exhaustive value-domain declarations.

The required mdoc prologue and structural macros are normalized as follows:

| Macros | ManT result |
| --- | --- |
| `Dd`, `Dt`, `Os` | Date, title, native manual section, architecture, and OS metadata |
| `Sh`, `Ss` | Top-level sections and child sections |
| `Nm`, `Nd` | Strong document name and NAME description dash |
| `Pp` | Explicit vertical paragraph separation |
| `Tg` | Zero-width navigation anchor when validated by libmandoc; no visible placeholder text |
| `Sx` | Resolved same-document section link, including one unique parenthetical heading qualifier, or visible text when unresolved |
| `Xr` | Typed link to a manual name and section |
| `Lk`, `Mt` | External URI or email link; an unlabeled target remains visible and any trailing sentence punctuation stays outside the link |
| `Bx` | BSD lifecycle forms such as `-alpha`, `-beta`, and `-devel` expand to their portable descriptive text; version forms render as canonical `versionBSD` names with an optional release |

Validated libmandoc tags on man and mdoc definitions are retained for page-local navigation. Every target receives a normalized internal `NodeId`, allocated uniquely against section IDs and earlier targets before IR validation. Explicit mdoc `Tg` destinations additionally retain their exact source spelling as a fragment alias at the same location, so values such as `Mixed.Target` and `--option` remain valid external deep links without violating the internal ID grammar. An argument-less `Tg` derives its destination from the following source macro; it never inherits an earlier parser tag. Automatic wrapper fallbacks likewise use source tokens rather than rendered text, so spacing and decoration cannot change an identity. When libmandoc moves a target onto a paragraph, display, list, item, function block, or section wrapper, ManT attaches it to the corresponding addressable lowered descendant (or to the section itself) without adding visible text. Target-only and empty mdoc list items retain zero-width anchors for bullet, ordered, plain, column, and definition layouts rather than assigning them to a neighbouring item. Root content and section content pass through the same local-link and traditional-manual-reference resolution.

After source lowering, ManT assigns semantic identities only inside a reliable structural context. Definition lists under environment sections recognize a complete term in bare `NAME`, shell `$NAME`, PowerShell `$Env:NAME` and `${Env:NAME}`, Windows `%NAME%`, or one assignment `NAME=value` form through the same grammar used by explicit Markdown declarations. The assignment value is not part of the selector. Named variables and similar entries may also carry one explicitly delimited trailing parenthetical annotation, such as Readline's `(On)` default notation; that annotation remains in the authored form but not the selector. ManT never takes only the first word of a term, and a composite heading such as `ENVIRONMENT OPTIONS` selects the more specific option grammar. Hanging paragraph plus relative-indent layouts are reconstructed as definitions only in that environment context. Ordinary prose and unrelated uppercase terms are never scanned or promoted. A definition-shaped term that fails the selected grammar remains visible as an unclassified term and emits `manual.semantic-entry.unclassified-definition`; the outline then reports `semanticsComplete: false` rather than claiming a complete semantic inventory.

Command contexts accept `COMMAND`/`COMMANDS`, `SUBCOMMAND`/`SUBCOMMANDS`, and
the explicit `BUILTIN COMMAND`/`BUILTIN COMMANDS` phrase as complete normalized
words, not arbitrary substrings such as `SUBCOMMANDER`. More specific option,
environment and variable contexts keep their precedence: `SUBCOMMAND OPTIONS`
does not promote every definition to a command. Correcting a previously
unclassified command can intentionally change its kind, names and derived ID;
the authored form and body remain unchanged.

Independent `IP`, `TP`, and definition-form `It` items retain separate semantic
owners, including items without a description. Neither `PD 0`, a later `PD`
reset, font changes nor suppressed line breaks prove shared ownership. An
explicit `TQ` adds a tag only to the immediately preceding empty definition;
several such tags retain their source order under one owner. Multiple names or
forms do not imply an explicit alias relationship.

Declaration adjacency uses executed native flow generations, retained before
validation removes empty paragraphs. Active conditionals and invoked macros
can therefore end a reading group; skipped branches and uncalled macro bodies
cannot. Both source-backed loading and lowering an owned parser report consume
the same facts without scanning source lines or replaying roff conditionals.

This conservative boundary means that a short declaration in a compact manual
may have no independent body. Its names, complete form and source remain
addressable; explain can additionally supply a bounded declaration group's
original context, with its actual provider identified. ManT does not borrow the
next item's physical body or treat the lack of an independent description as
budget truncation. Truly isolated heads retain their containing-section reading
coordinates instead of silently taking unrelated later prose.

A `TP` whose complete tag is the named roff bullet `\(bu` or `\[bu]`
(including leading escaped spacing) becomes a regular bullet item, not a
semantic term named `•`. This narrow recovery retains body, layout and targets;
it does not apply `IP`'s broader marker convention to literal `TP` operators
such as `*`, `-`, or `+`.

## Manual References

ManT retains explicit manual-reference semantics and recognizes two conservative compatibility forms:

| Source form | ManT result |
| --- | --- |
| mdoc `.Xr name section` | Typed link to the exact manual name and section |
| GNU man `.MR name section` | Typed link to the exact manual name and section |
| Traditional styled `name` immediately followed by `(section)`, such as `.BR printf (3)` | Typed link when the font and punctuation structure is unambiguous |
| Legacy Sphinx `name(section) \%<>` | Typed link with the empty destination marker removed after validation |

The legacy Sphinx rule matches formatter evidence rather than rendered prose. It applies to discovered manuals and direct roff `--input` alike; it does not depend on a filename extension, MANPATH location, or a surrounding `SEE ALSO` section. Recognition is limited to filled, non-code text that is not already inside an external link. Invalid candidates keep their visible `<>` marker instead of silently losing source text.

For this compatibility form, a manual name must contain 1–256 bytes of ASCII letters, digits, `.`, `_`, `+`, `:`, or `-`. A section must be a single `l` or `n`, or begin with `1` through `9` and continue with ASCII letters or digits, with a maximum length of 16 bytes. Path-like and email-like prefixes are rejected. These rules retain names such as `g++(1)` and `systemd.slice(5)` while rejecting ambiguous text such as `group(qgroup)`, `function(0)`, `/tmp/tool(1)`, and `user@tool(1)`.

Bare `name(section)` prose is never inferred as a link. Authors should prefer `Xr` or `MR`, which carry explicit semantics and avoid compatibility recognition entirely.

Parsing does not consult the installed manual index, so the same roff bytes produce the same IR and JSON on every host. An interactive consumer resolves the exact name and section only when the link is followed; an unavailable target leaves the current document and navigation history unchanged.

## mdoc Lists and Displays

`Bl`/`It`/`El` lists are normalized by list type:

| mdoc list type | IR result |
| --- | --- |
| `-bullet`, `-dash`, `-hyphen` | Bullet list |
| `-enum` | Ordered list |
| `-diag`, `-hang`, `-inset`, `-ohang` | Definition list |
| `-tag` | Definition list, except a complete proven ordinal sequence |
| `-column` | Definition-list representation with column semantics retained by terms |
| `-item` | Plain list |

`-compact`, `-offset`, and `-width` are normalized where they affect terminal structure. Definition descriptions and list items retain nested blocks.

Consecutive mdoc `It` heads remain independent, even when they name the same option or only the final item has a body. Multiple forms inside a single `It`/`Xo` retain that authored owner. Empty items, their targets and source locations are not moved to the next described item.

Independence does not discard useful reading context. A bounded run of complete native declaration heads with no readable body, followed by a described declaration, can carry a `declarationGroups` annotation. Explain returns the group's original heads and final description as explicitly recovered context, not as aliases or inherited children/value domains. Source paragraph/container boundaries stop grouping; arbitrary later prose is never a fallback explanation. Ordinary Markdown items are not automatically grouped, and the annotation does not change full-document rendering.

The same rule covers man `IP`/`TP` (including suppressed-newline heads) and
mdoc definition-style `It`; an explicit `TQ` multi-label owner remains one
member. Complete parameterized heads can supply Form evidence without invented
template names. Headless `IP` continuations after an item's `RS` region return
to the original definition, not to the last nested term: callback notes, examples
and return-status paragraphs remain available in that owner's explanation.

Some deployed mdoc pages use `Bl -tag` for numbered procedures instead of the
standard `Bl -enum`. ManT recovers ordered-list semantics only when the entire
tag list contains at least two described, consecutively increasing integers
with one explicit punctuation style such as `1.`/`2.`. A singleton, a gap, a
style change, a bare integer, or a decimal remains a definition term. This
keeps procedures out of the semantic-entry index without reclassifying numeric
configuration values.

Displays lower as follows:

| Macros | ManT result |
| --- | --- |
| `Bd -literal`, `Bd -unfilled` | Preformatted flow; nested `tbl` rows remain structured tables |
| `Bd -filled`, `Bd -ragged`, `Bd -centered` | Filled blocks; device alignment is not retained |
| `D1`, `Dl` | Single preformatted display |
| `Bf -emphasis` | Scoped emphasis default; inner font selections can override it |
| `Bf -literal` | Scoped code default; inner font selections can override it |
| `Bf -symbolic` | Scoped strong default; inner font selections can override it |
| `An -split`, `An -nosplit` | Author layout mode used while forming visible author content |

Closing macros such as `Ed`, `Ef`, and `El` terminate libmandoc scopes and do not produce independent visible nodes.

Literal and unfilled flows preserve physical line boundaries without resetting inline font or spacing state. `br` ends a line once; `sp N` adds vertical blank rows rather than printing its argument; `Sm` changes spacing without adding a row. A trailing `\c` joins the following source line unless an explicit break intervenes.

The line cursor follows executed descendant words, not just the outer macro. Thus a multiline `Fo`/`Fa` function keeps each argument's source line, and `Eo` delimiters retain their source positions around nested font scopes. Font and enclosure wrappers do not hide structural payloads: nested lists and tables remain blocks, with opening and closing content surrounding them.

Styling wrappers such as `Bf` are not line boundaries. Continuation and pending spacing pass through their opening and closing nodes, including a body containing only state requests. An explicit break inside a wrapper still terminates the current logical line.

No-fill changes line layout, not content reachability: nested tables and lists retain their cells, terms, bodies and targets even inside font scopes. Such structural payloads interrupt the current preformatted run rather than being flattened into partial inline text.

## mdoc Inline Semantics

The following macros receive dedicated inline treatment:

| Semantics | Macros |
| --- | --- |
| Strong | `Nm`, `Fl`, `Cm`, `Ic`, `Sy`, `Ms` |
| Emphasis | `Ar`, `Pa`, `Em`, `Va`, `Vt`, `Ft`, `Fa`, `Ad`, `Fr` |
| Regular | `No`, `Dv` |
| Code | `Li` |
| Header reference | `In` (`<header>` in prose; `#include <header>` at the start of a synopsis source line) |
| Manual link | `Xr` |
| External or email link | `Lk`, `Mt` |
| Emphasized section link | `Sx` |
| No-space boundary | `Ns` |
| Visible no-space prefix | `Pf` (prefix retained) |
| Automatic spacing mode | `Sm on`, `Sm off` |
| Apostrophe attachment | `Ap` |
| Function declaration | `Fn` |
| Multi-line function declaration | `Fo`, `Fa`, `Fc` |

Delimiter macros preserve their visible punctuation and libmandoc spacing roles:

| Delimiters | Opening form | Closing form |
| --- | --- | --- |
| Optional brackets | `Op`, `Oo` | `Oc` |
| Brackets | `Bq`, `Bo` | `Bc` |
| Typographic double quotes (`“…”`) | `Dq`, `Do` | `Dc` |
| Typewriter double quotes (`"…"`) | `Qq`, `Qo` | `Qc` |
| Single quotes | `Sq`, `So`, `Ql` | `Sc` |
| Parentheses | `Pq`, `Po` | `Pc` |
| Braces | `Brq`, `Bro` | `Brc` |
| Angles | `Aq`, `Ao` | `Ac` |
| Arbitrary | `Eo opening`, body | `Ec closing` |
| Stateful (obsolete) | `Es opening closing`, then `En` | Resolved per `En` use |

The opener owns the complete scoped body in libmandoc's tree, so ManT surrounds that body once. Closing macros terminate the scope and do not emit a second delimiter. `Eo` and `Ec` retain their literal, author-supplied delimiters. The obsolete `Es` macro changes parser state but emits no text; libmandoc resolves that state onto each `En` invocation before ManT lowers it.

An empty `Eo`/`Ec` scope is a zero-width word event. If an authored enclosure has no closing delimiter, its closing boundary still releases an internal no-space request. Empty strings and zero-width text can consume word boundaries; invisible targets and hidden nodes cannot.

Inline spacing follows output order across prose, definition heads, literal displays, and supported tbl source recovery. `Ap` attaches on both sides, including across styled siblings (`.No x Ap y` becomes `x'y`). A generated closing bracket or quote consumes an internal `Ns`/`Pf` boundary; it does not carry that boundary outside the enclosure. An explicitly external `Ns` can still join the following text. Styles and zero-width targets do not consume pending boundaries; real line and paragraph breaks terminate them.

The generated dash of `Fl` joins its own operands. An empty string or zero-width `\&` operand consumes that internal join and leaves the next external argument separate (`Fl "" Ar file` becomes `- file`). An operand-less `Fl` is different: it joins the next non-text sibling on the same source line (`Fl Ar file` becomes `-file`, and `Fl Fl Ar file` becomes `--file`). Zero-width targets remain non-consuming, but do not extend the internal join beyond its operand scope. Explicit `Ns`/`Pf` effects remain distinct and can still request an external join.

Font state has a different lifetime from spacing state. A font-selecting mdoc macro pushes its effective font, rather than adding a wrapper to the inherited style: `No` selects regular even inside `Bf -emphasis`, and `Em \fBword` produces bold, not bold-plus-emphasis. Explicit escapes override the macro's initial selection. On scope exit the outer current font resumes; the previous-selection register is not rolled back. ManT follows mandoc's font stack here: `No \fBword\fIinner` followed by `\fPtail` makes `tail` bold, while groff makes it italic.

Plain text and transparent macros such as `Pf` do not create font scopes. Their escapes can change subsequent text until another font selection intervenes. `Bf` establishes a scoped default through nested lists and displays; local macros can override it, and `Ef` resumes the outer font without inventing a paragraph or line boundary. ManT retains code presentation for `Li` and `Bf -literal`, although a terminal formatter may use its ordinary monospaced font. These rules do not change the separate font-reset policy for man macros. Spacing controls and zero-width targets remain independent of font push/pop.

Bibliographies and table cells inherit the enclosing font state; a rejected cell recovery does not commit its partial state. Generated function and manual-reference punctuation participates in the same output flow as its operands. `Fo` is an inline scope in ordinary prose, while SYNOPSIS retains declaration boundaries. Font changes inside a man `SY` body persist across physical no-fill lines and reset when that macro scope ends.

`Lk` evaluates its optional label in an emphasis scope before evaluating the URI in the inherited font, even when compact link presentation hides the URI. `Mt` uses one emphasis scope for its whole address sequence. `In` uses the native prose/synopsis font scope while retaining ManT's code presentation; `Xr` does not create a font scope. Pure link-target extraction does not execute font escapes. These choices follow mandoc CVS `mdoc_term.c` revision 1.388, including its unstyled URI policy, rather than the older vendored renderer's URI styling.

Inside `Fo`, a generated comma separates adjacent logical `Fa` parameters. Nonprinting controls and targets do not break that adjacency, but intervening prose or a visible container does; an authored closing delimiter is not duplicated. Generated punctuation is emitted before following controls, so `.Fa x`, `.Sm off`, `.Fa y` retains `x, y`.

`Ns` suppresses a boundary only when libmandoc does not mark it as starting a source line. `Pf` retains its prefix but joins the next sibling only when that sibling exists on the same source line. Line-start `Ns` and `Pf` without that successor are recovery cases, not recommended authoring forms; native diagnostics remain observable. Formatter differences remain relevant: groff rejects bare line-start `Ap`, and its handling of ordinary text under `Sm off` differs from mandoc. ManT deliberately preserves ordinary filled source-line word boundaries even under `Sm off`, following the documented distinction between source text and macro auto-spacing. Macro operands still obey `Sm off`, and explicit `\c`, `Ns`, `Pf`, or `Ap` joins take precedence. This is a deliberate difference from mandoc's concatenation of ordinary text lines, not a promise of byte-identical formatter output.

`Fn` and `Fo` retain the function name, join their arguments inside parentheses, and preserve the formatter-owned terminating semicolon when libmandoc marks the declaration for synopsis presentation. Each operand of `Fa` inside `Fo` is a separate parameter; quote multi-word parameters (`.Fa "const char *path"`). Spacing controls and zero-width targets retain their effect and position without becoming arguments. Outside `Fo`, `Fa` keeps ordinary spaced operands. The same `Fn` in prose remains an inline function reference without a semicolon. For example, `Fo audit_open` with two `Fa` lines lowers to `audit_open(arg1, arg2);` in `SYNOPSIS` rather than discarding the function name or punctuation.

Other standard mdoc semantic macros, including `Fd`, `Cd`, `Er`, `Ev`, `Rv`, `Ex`, `Lb`, `St`, `Rs`, and bibliography fields, currently use visible-child fallback. Text remains readable, but specialized typography, punctuation synthesis, or domain identity is not guaranteed unless listed above.

The pinned parser's `St` name catalogue includes the upstream OpenBSD entries
for C23 (`-isoC-2023`) and POSIX.1-2024 (`-p1003.1-2024`). The resulting
standard title is formatter-owned text; older BSD formatters can use slightly
different wording for the same source key.

## Roff Requests

libmandoc preprocesses macro definitions, strings, registers, conditionals, loops, translations, and supported compatibility requests before ManT receives the owned tree. ManT does not expose that formatter state as IR.

Each `.while` loop is limited to 10,000 body executions. One parse also permits
at most 10,000 aggregate body replays across all loops and user-macro calls;
the first source occurrence of each body is not a replay. Reaching either limit
keeps the finite prefix, emits libmandoc's infinite-loop diagnostic, and
continues with the source after the loop. These bounds apply equally to
discovered manuals and direct roff `--input`, so hostile formatter control flow
cannot hold a CLI or MCP parser session indefinitely or multiply many bounded
loops into unbounded memory growth.

Requests with direct lowering behavior are:

| Request | ManT result |
| --- | --- |
| `br` | Inline line break |
| `sp` | Vertical-space block, with normalized height |
| `nf`, `fi` | Enter and leave preformatted flow |
| `ft` | Changes the current/previous inline font without emitting its argument |
| `in` | Consumed indentation state around structures normalized by libmandoc |
| `ad`, `na` | Adjustment state omitted |
| `hy`, `nh` | Hyphenation state omitted |
| `ne` | Page-layout reservation omitted |
| `nr` | Register request omitted after upstream evaluation |
| `ta` | Tab-stop state omitted |

`ce`, `rj`, `ll`, `mc`, `po`, and `ti` can be represented by libmandoc nodes but ManT does not promise their device-specific alignment or page geometry. Printable descendants remain visible where the upstream AST provides them.

`TS`/`TE` and `EQ`/`EN` are handled as structured preprocessors, described below. For the complete distinction between requests implemented, ignored, unsupported, and insecure in the pinned parser, consult upstream `roff(7)` for mandoc 1.14.6. ManT adds the stricter source and include boundary described in this manual.

## Escapes

ManT decodes visible roff text after libmandoc parsing. These escape families have explicit behavior:

| Escape | Result |
| --- | --- |
| `\fX`, `\f(XX`, `\f[NAME]` | Strong, emphasis, combined, code, or regular font state |
| `\-` | Copyable ASCII hyphen-minus |
| `\e`, `\\` | Visible reverse solidus |
| `\ `, `\~`, `\0` | Visible space |
| `\c` at the end of an input line | Suppress the implicit space or line break before the next input line |
| `\h'N'` with a positive literal relative distance | Preserve at least one visible word boundary; exact horizontal geometry is not reproduced |
| `\p` | Inline line break |
| `\(XX`, `\[NAME]`, `\C'desc'` | Named special character from the pinned libmandoc catalog; bracketed `uXXXX` Unicode names and `_`-joined scalar sequences are decoded, while an unknown name remains visible in escaped source form |
| `\E` | Copy-mode-safe nested escape |
| `\N'number'` | Numbered glyph in the pinned mandoc terminal range 0–255, with control filtering; unsupported or malformed indices remain visibly escaped, not interpreted as arbitrary Unicode |
| `\X'tty: link URI'` | External terminal link start; `\X'tty: link'` ends it |

Named characters resolve through the complete character catalog compiled from the pinned libmandoc source. ManT deliberately applies copy-friendly compatibility folds to common quotes and symbols; other catalog entries use their declared Unicode scalar. Groff-style bracketed Unicode names such as `\[u2192]` and composite names such as `\[u0061_0301]` are decoded independently of that catalog. A name absent from both forms is retained as `\(XX`, `\[NAME]`, or `\C'desc'` instead of being silently deleted, while known zero-width controls remain invisible.

Font names map as follows:

| Roff font | IR style |
| --- | --- |
| `B`, `3` | Strong |
| `I`, `2` | Emphasis |
| `BI`, `4` | Strong emphasis |
| `C`, `CR`, `CW`, `V` | Code |
| `CB`, `VB` | Strong code |
| `CI`, `VI` | Emphasized code |
| `P` or empty font operand | Restore the previous font selection |
| Other names, `R`, `1` | Regular |

Continuous man text retains font state across source line breaks. A font macro establishes its initial font without overriding later escapes inside its operands; ordinary man paragraph/font scopes reset to regular, while `SM` retains the current font. These rules also apply to inline-only table recovery.

Adjacent runs with the same effective style are one semantic span in Markdown
output. For example, `\fB\-\fP\fB\-emulate\fP` becomes
`**--emulate**`, rather than two neighboring emphasis delimiters. Markdown
escaping is minimal but lossless: intraword underscores such as the one in
`PATH_SCRIPT` remain literal, while delimiter-active underscores are escaped.

Color, point size, vertical or non-literal motion, drawing, overstrike, register, string, device, and postprocessor escape operands are consumed so control syntax cannot leak into prose. Their presentation effect is omitted. A positive literal relative horizontal motion retains one space as a text-mode approximation, including before a `\c` line join; negative, absolute, register-based, and compound motions remain presentation-only. Known zero-width spacing and formatter controls remain zero width. An otherwise undefined one-character escape follows roff's visible-trigger fallback after terminal-control filtering.

The zero-advance `\z` escape retains its complete following glyph, including named glyph escapes, at ordinary advance as a text-mode approximation. It never exposes a partial glyph operand or removes the glyph's visible content.

Decoded text is never interpreted a second time as roff syntax. In particular, literal font-escape spellings authored with `\e` or `\[rs]` remain visible even when their letters resemble the currently selected font.

## Tables

Text, Markdown and TUI preserve the logical column after a horizontal span; covered slots stay empty instead of shifting later cells left. Tables wider than 256 logical columns use explicit column labels in text/Markdown and stacked cells in the TUI, avoiding span-driven allocation amplification.

Separate native tables remain separate IR blocks even when adjacent. A `T&` layout restart changes rows within the same table; leading rule-only rows do not hide its boundary.

Rule cells retain their column positions but no printable body. Both layout rules and data rules suppress their payload; source recovery never resurrects that intentionally hidden text. Escaped literal underscores remain ordinary text.

`tbl(7)` rows become IR tables, including tables nested inside an mdoc literal or unfilled display. ManT retains cell text, left/center/right alignment, column spans, and row spans supplied by libmandoc. It does not reproduce line drawing, exact column widths, vertical positioning, tbl-specific font directives, or device-specific rules.

Cell text passes through the same roff inline decoder as ordinary prose. For source-backed `T{`/`T}` text blocks, supported inline requests are parsed together using the document's man/mdoc dialect and lowered through the ordinary inline path. This preserves callable macro nesting, enclosure closure, links, and spacing state across lines; an argument-less `.Nm` resolves to the validated document name. The private parse accepts no includes or nested block/table requests and does not invent navigation targets. Recovery is bounded to 64 requests and 64 callable tokens per cell; exceeding either limit retains complete native cell text or source spelling with `manual.unhandled-table-text-block`. Other complex nested block markup may flatten to the visible cell payload exposed by libmandoc. If an empty semantic text block cannot be recovered from the bounded input source, ManT emits the same diagnostic instead of claiming silent fidelity.

Recovery is all-or-nothing at the cell boundary. If a mixed request sequence such as `.B` followed by `.PP` or `.TP` cannot be reconstructed completely, a partial inline result never replaces the native cell. ManT retains the complete native cell text, or the whole source spelling when native text is absent, and reports `manual.unhandled-table-text-block`. This preserves content without claiming support for the mixed block semantics.

Some formatter-specific strings disappear before libmandoc exposes a cell. For ordinary tab-separated rows, ManT compares the validated cells with the bounded source row and retains an otherwise missing cell in its original escaped spelling. It emits one `manual.unexpanded-table-cell` diagnostic for the document rather than presenting an empty table or pretending that the formatter-specific value was evaluated.

## Equations

Display `eqn(7)` input becomes an `equation` block containing libmandoc's normalized expression text. Delimiter-selected equations inside filled prose remain inline symbolic tokens rather than splitting the paragraph. The same active delimiters are applied to ordinary `tbl(7)` cells, whose opaque cell strings are normalized through the pinned eqn parser. Configuration-only `EQ`/`EN` blocks emit no empty equation. The common GNU `ldots` macro is normalized to `...`.

ManT preserves these expressions for text, Markdown, JSON, and TUI consumers; it does not typeset mathematical layout or execute an external `eqn` preprocessor. At most 256 distinct opaque table expressions are reparsed per document. Later expressions remain visible in their source spelling and produce `manual.inline-equation-budget`, preventing adversarial tables from turning semantic recovery into unbounded parser work.

Deeply nested equations and document trees are bounded before recursive Rust lowering. The owned native tree stops descending after 256 levels and returns the finite prefix. A separate native construction guard stops input dispatch after a syntax node exceeds 512 parent levels, before finalization and validation; that larger violation returns a whole-document parse error. Native reference renderers reject syntax or equation nesting beyond 256 levels independently of output size. Native tree cleanup is iterative.
The retained document carries `manual.syntax-depth-truncated` or
`manual.equation-depth-truncated`, respectively, so structured consumers can
detect either omission without matching diagnostic prose.

## Diagnostics and Fallback

libmandoc style, warning, error, and unsupported findings become structured document diagnostics with source locations when available. A nonfatal finding does not discard an otherwise useful manual.

Unknown source macros can be expanded by an earlier `.de` definition. If no visible semantic subtree results, ManT does not invent content. Formatter arguments such as widths, font names, register values, and macro-control tokens are never emitted merely to avoid dropping syntax.

Terminal-unsafe control bytes are masked before native parsing. Roff comments and nodes marked non-printing by libmandoc remain invisible.

## Compatibility Guidance

For manuals intended to work across mandoc, groff, and ManT:

1. Prefer standard `mdoc(7)` semantic macros or the portable core of `man(7)`.
2. Use `Xr` or `MR` for cross-manual links, `Sx` for mdoc section links, and `Lk`/`UR` for external links.
3. Use `Bl`/`It`, `TP`/`IP`, `Bd`, `EX`/`EE`, `tbl`, and `eqn` only where their retained structure matters.
4. Avoid relying on device geometry, page traps, custom diversions, color, point size, or arbitrary file inclusion.
5. Inspect a concrete normalized outline and its diagnostics with
   `mant --input ./widget.1 --input-format roff --outline --outline-entries all --format json --compact`.

## Upstream References

The upstream references define the source languages; this manual defines ManT's lowering contract:

- [mandoc mdoc(7)](https://mandoc.bsd.lv/man/mdoc.7.html)
- [mandoc man(7)](https://mandoc.bsd.lv/man/man.7.html)
- [mandoc roff(7)](https://mandoc.bsd.lv/man/roff.7.html)
- [mandoc_char(7)](https://mandoc.bsd.lv/man/mandoc_char.7.html)
- [GNU troff manual](https://www.gnu.org/software/groff/manual/groff.html)

## See Also

[mant(1)](mant.md), [mant-ir(7)](mant-ir.md), [mant-markdown(7)](mant-markdown.md), and [mant-protocol(5)](mant-protocol.md)
