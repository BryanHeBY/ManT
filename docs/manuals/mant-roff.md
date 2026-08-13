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

Indexed redirect-only pages containing `.so target` may resolve only to another discovered page inside the same approved manual hierarchy. Standalone `--input` files reject `.so` redirects because no trusted hierarchy accompanies them.

libmandoc file inclusion is disabled. Requests that read, write, execute, pipe, or include arbitrary files remain denied or ignored by the upstream safe parser. ManT never invokes the host `man`, `groff`, `nroff`, or shell executable.

## man Language

The following `man(7)` macros have dedicated lowering behavior:

| Macros | ManT result |
| --- | --- |
| `TH` | Title, native manual section, date, source, and volume metadata |
| `SH`, `SS` | Top-level sections and child sections |
| `P`, `PP`, `LP`, `HP` | Paragraph boundaries and retained vertical spacing |
| `RS`, `RE` | Nested indentation boundary |
| `IP`, `TP`, `TQ` | Bullet or definition-list items, aliases, hanging layout, and widths |
| `PD` | Paragraph, definition-item, and heading spacing |
| `B`, `SB` | Strong inline content |
| `I` | Emphasized inline content |
| `BI`, `BR`, `IB`, `IR`, `RB`, `RI` | Alternating inline font runs without inserted spaces |
| `EX`, `EE` | Preformatted no-fill region through libmandoc's fill state |
| `UR`, `UE` | External link block |
| `MT`, `ME` | Email link block |
| `MR` | Typed manual-page reference |

`br` inside a flow becomes an inline line break. `sp` becomes explicit vertical space. Filled source lines normally join with spaces; an indented input line and no-fill input preserve line boundaries.

`OP`, `SY`, `YS`, `AT`, `DT`, `SM`, `UC`, and other libmandoc-recognized man macros retain printable children where available but do not currently have a dedicated ManT semantic variant. For example, `SM` does not preserve point size, and `OP` does not become a distinct optional-argument node.

Traditional styled references such as `printf(3)` are recognized after lowering when the font and punctuation structure is unambiguous. `MR printf 3` is preferred because it carries explicit manual-reference semantics.

## mdoc Structure

The required mdoc prologue and structural macros are normalized as follows:

| Macros | ManT result |
| --- | --- |
| `Dd`, `Dt`, `Os` | Date, title, native manual section, architecture, and OS metadata |
| `Sh`, `Ss` | Top-level sections and child sections |
| `Nm`, `Nd` | Strong document name and NAME description dash |
| `Pp` | Explicit vertical paragraph separation |
| `Tg` | Zero-width navigation anchor when validated by libmandoc |
| `Sx` | Resolved same-document section link or visible text when unresolved |
| `Xr` | Typed link to a manual name and section |
| `Lk`, `Mt` | External URI or email link |

Validated libmandoc tags on mdoc definitions are retained for page-local navigation. Tags and section IDs share one document-local namespace and are disambiguated during IR validation.

## mdoc Lists and Displays

`Bl`/`It`/`El` lists are normalized by list type:

| mdoc list type | IR result |
| --- | --- |
| `-bullet`, `-dash`, `-hyphen` | Bullet list |
| `-enum` | Ordered list |
| `-diag`, `-hang`, `-inset`, `-ohang`, `-tag` | Definition list |
| `-column` | Definition-list representation with column semantics retained by terms |
| `-item` | Plain list |

`-compact`, `-offset`, and `-width` are normalized where they affect terminal structure. Definition descriptions and list items retain nested blocks.

Displays lower as follows:

| Macros | ManT result |
| --- | --- |
| `Bd -literal`, `Bd -unfilled` | Preformatted block |
| `Bd -filled`, `Bd -ragged`, `Bd -centered` | Filled blocks; device alignment is not retained |
| `D1`, `Dl` | Single preformatted display |
| `Bf -emphasis` | Emphasis applied to contained blocks |
| `Bf -literal` | Code styling applied to contained blocks |
| `Bf -symbolic` | Strong styling applied to contained blocks |
| `An -split`, `An -nosplit` | Author layout mode used while forming visible author content |

Closing macros such as `Ed`, `Ef`, and `El` terminate libmandoc scopes and do not produce independent visible nodes.

## mdoc Inline Semantics

The following macros receive dedicated inline treatment:

| Semantics | Macros |
| --- | --- |
| Strong | `Nm`, `Fl`, `Cm`, `Ic`, `Sy` |
| Emphasis | `Ar`, `Pa`, `Em`, `Va`, `Vt`, `Ft`, `Fa` |
| Code | `Li` |
| Manual link | `Xr` |
| External or email link | `Lk`, `Mt` |
| Section link | `Sx` |
| No-space control | `Ns`, `Pf` |
| Apostrophe attachment | `Ap` |

Delimiter macros preserve their visible punctuation and libmandoc spacing roles:

| Delimiters | Opening form | Closing form |
| --- | --- | --- |
| Optional brackets | `Op`, `Oo` | `Oc` |
| Brackets | `Bq`, `Bo` | `Bc` |
| Double quotes | `Dq`, `Do`, `Qq`, `Qo` | `Dc`, `Qc` |
| Single quotes | `Sq`, `So`, `Ql` | `Sc` |
| Parentheses | `Pq`, `Po` | `Pc` |
| Braces | `Brq`, `Bro` | `Brc` |
| Angles | `Aq`, `Ao` | `Ac` |

The opener owns the complete scoped body in libmandoc's tree, so ManT surrounds that body once. Closing macros terminate the scope and do not emit a second delimiter.

Other standard mdoc semantic macros, including `Fn`, `Fo`, `Fc`, `In`, `Fd`, `Cd`, `Dv`, `Er`, `Ev`, `Rv`, `Ex`, `Lb`, `St`, `Rs`, and bibliography fields, currently use visible-child fallback. Text remains readable, but specialized typography, punctuation synthesis, or domain identity is not guaranteed unless listed above.

## Roff Requests

libmandoc preprocesses macro definitions, strings, registers, conditionals, loops, translations, and supported compatibility requests before ManT receives the owned tree. ManT does not expose that formatter state as IR.

Requests with direct lowering behavior are:

| Request | ManT result |
| --- | --- |
| `br` | Inline line break |
| `sp` | Vertical-space block, with normalized height |
| `nf`, `fi` | Enter and leave preformatted flow |
| `ft` | Consumed formatter state; explicit text font escapes remain semantic |
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
| `\p` | Inline line break |
| `\(XX`, `\[NAME]`, `\C'desc'` | Known named special character |
| `\E` | Copy-mode-safe nested escape |
| `\X'tty: link URI'` | External terminal link start; `\X'tty: link'` ends it |

Named characters with dedicated Unicode or ASCII lowering are `en`, `em`, `aq`, `cq`, `dq`, `lq`, `rq`, `co`, `rg`, `tm`, `bu`, `ha`, `ti`, and `rs`. libmandoc may normalize additional common characters before this stage; ManT does not claim complete glyph coverage for the full groff character catalog.

Font names map as follows:

| Roff font | IR style |
| --- | --- |
| `B`, `3` | Strong |
| `I`, `2` | Emphasis |
| `BI`, `4` | Strong emphasis |
| `C`, `CR`, `CW`, `V` | Code |
| `CB`, `VB` | Strong code |
| `CI`, `VI` | Emphasized code |
| Other names, `R`, `P`, `1` | Regular |

Color, point size, motion, drawing, overstrike, register, string, device, and postprocessor escape operands are consumed so control syntax cannot leak into prose. Their presentation effect is omitted. Known zero-width spacing and formatter controls remain zero width. An otherwise undefined one-character escape follows roff's visible-trigger fallback after terminal-control filtering.

## Tables

`tbl(7)` rows become IR tables. ManT retains cell text, left/center/right alignment, column spans, and row spans supplied by libmandoc. It does not reproduce line drawing, exact column widths, vertical positioning, fonts, or device-specific rules.

Cell text passes through the same roff inline decoder as ordinary prose. Complex nested block markup inside a cell may flatten to the visible cell payload exposed by libmandoc 1.14.6.

## Equations

`eqn(7)` input becomes an `equation` block containing libmandoc's normalized expression text. ManT preserves the expression for text, Markdown, JSON, and TUI consumers; it does not typeset mathematical layout or execute an external `eqn` preprocessor.

Deeply nested equations and document trees are bounded before recursive Rust lowering. Excessive nesting fails safely rather than overflowing the process stack.

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
5. Inspect the normalized result and diagnostics with `mant --input PAGE --format json`.

## Upstream References

The upstream references define the source languages; this manual defines ManT's lowering contract:

- [mandoc mdoc(7)](https://mandoc.bsd.lv/man/mdoc.7.html)
- [mandoc man(7)](https://mandoc.bsd.lv/man/man.7.html)
- [mandoc roff(7)](https://mandoc.bsd.lv/man/roff.7.html)
- [mandoc_char(7)](https://mandoc.bsd.lv/man/mandoc_char.7.html)
- [GNU troff manual](https://www.gnu.org/software/groff/manual/groff.html)

## See Also

[mant(1)](mant.md), [mant-ir(7)](mant-ir.md), [mant-markdown(7)](mant-markdown.md), and [mant-protocol(5)](mant-protocol.md)
