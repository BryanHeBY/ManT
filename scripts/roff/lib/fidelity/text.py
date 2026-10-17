"""Conservative source-conditioned text comparisons; no file or renderer access."""
from __future__ import annotations

import re
import unicodedata
from collections import Counter
from typing import Sequence

ANSI = re.compile(r"\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07]*(?:\x07|\x1b\\))")
# Audit the document's visible language, not only its ASCII substrings.
# Python Unicode word matching retains accented and non-Latin words.  NFC
# makes a terminal's decomposed accents equivalent to normalised text from
# another renderer.  The narrow ASCII punctuation tail keeps paths, qualified
# names, URLs, and options useful as single candidates.
TOKEN = re.compile(r"\w[\w.+:/-]{2,}", re.UNICODE)
# Only join a wrapped URL/path component after a non-slash component. A bare
# slash at the end of an unrelated token (for example Perl's `tr//` followed
# by a new sentence) must not consume the semantic line boundary.
URL_WRAP = re.compile(
    r"(https?://[^\s\n]*/)[ \t]*\n[ \t]*(?=[A-Za-z0-9%_~?&=][^\s\n]*[./_~?&=%-])"
)
# A terminal soft-wrap hyphen joins two word characters.  Do not join a line
# merely because it ends in dashes: no-fill tables routinely use operation
# masks such as -W----, and treating those as soft hyphenation merges the next
# table row into the mask before fidelity comparison.
DEHYPHENATE = re.compile(r"(?<=\w)-[ \t]*\n[ \t]*(?=\w)", re.UNICODE)
SOURCE_LITERAL_HYPHEN_PAIR = re.compile(
    r"(?<![\w.+:/-])(?P<left>\w[\w.+:/-]*-)[ \t]+(?P<right>\w[\w.+:/-]*)",
    re.UNICODE,
)
BORDERS = re.compile(r"[\u2500-\u257f\u2022\u00b7]")
ANGLE_LINK = re.compile(r"<((?:https?|mailto):[^<>]{1,4096})>", re.DOTALL)
# ManT's terminal presentation uses compact Unicode angle brackets for typed
# external targets, while mandoc's `man_term.c:post_UR` writes ASCII brackets.
# Treat either delimiter as presentation around the same URI before lexing;
# punctuation outside the delimiter remains observable.
COMPACT_ANGLE_LINK = re.compile(r"⟨((?:https?|mailto):[^⟨⟩]{1,4096})⟩", re.DOTALL)
RUNNING_HEADER = re.compile(
    r"^\s*(?P<label>\S+\([^)\s]+\))\s+.+\s+(?P=label)\s*$",
    re.IGNORECASE,
)
TERMINAL_FOOTER = re.compile(
    r"^\s+.+\s{2,}\S+\([^\)\s]+\)\s*$",
    re.IGNORECASE,
)
UNICODE_ESCAPE = re.compile(
    r"\\\[u[0-9A-Fa-f]{4,6}(?:_[0-9A-Fa-f]{4,6})*\]"
)
GLUED_MARKER = re.compile(r"^[ \t]*\u2022[A-Za-z(\"']", re.MULTILINE)
# Include both the historical and fixed-CVS private layout sentinels. These
# are never visible product text (tabs/newlines remain legitimate separators).
INTERNAL_MARKER = re.compile("[\u001a\u001c-\u001f]")
MDOC_NAME_DESCRIPTION = re.compile(r"^[.']Nd(?:\s|$)", re.MULTILINE)
MDOC_MULTI_OPERAND_FA = re.compile(
    r'''^[.']Fa(?:[ \t]+"(?:[^"\\]|\\.)*"){2,}[ \t]*$''',
    re.MULTILINE,
)
MDOC_FUNCTION_NAME = re.compile(r"^[.'](?:Fn|Fo)[ \t]+(?P<name>[A-Za-z_][A-Za-z0-9_.-]*)", re.MULTILINE)
AUTHORED_LITERAL_UNICODE_ESCAPE = re.compile(
    # A manual can document an escape by writing its leading backslash with
    # `\\`, groff's `\e`, or the portable `\[rs]` glyph.  Only these direct,
    # adjacent source spellings prove that a visible `\[u…]` is intentional;
    # a generic source occurrence of `uXXXX` is not enough to suppress a
    # possible parser leak.
    r"(?:\\\\|\\e|\\\[rs\])(\[u[0-9A-Fa-f]{4,6}(?:_[0-9A-Fa-f]{4,6})*\])"
)
# CVS preconv.c rewrites an ordinary non-ASCII scalar as ``\\[uXXXX]`` before
# roff parsing. A source author can deliberately write a literal backslash
# immediately before such a scalar, for example while documenting a malformed
# or locale-specific roff spelling. The parser then correctly preserves the
# resulting ``\\[uXXXX]`` as visible text. Keep this source form separate
# from general Unicode so it cannot hide an actual escape leak.
AUTHORED_LITERAL_UTF8_AFTER_BACKSLASH = re.compile(r"(?<!\\)\\([^\x00-\x7f])")
EM_DASH_ATTACHED_TO_WORD = re.compile(r"—(?=\w)")
EXTERNAL_ROFF_CONTEXT = re.compile(
    rb"(?:^|[ \t])[.'](?:so|mso)(?:[ \t]|$)", re.MULTILINE
)
ROFF_REQUEST = re.compile(r"^[.'](?P<name>[A-Za-z][A-Za-z0-9]*)(?:[ \t]+(?P<args>.*))?$")
ROFF_FONT_ESCAPE = re.compile(r"\\f(?:\[[^]]*]|.)")
TableRow = tuple[tuple[str, ...], ...]
TableRowPair = tuple[TableRow, TableRow]

TRANSLATION = str.maketrans(
    {
        "\u2010": "-",
        "\u2011": "-",
        "\u2212": "-",
        "\u00ad": "",
        "\u00a0": " ",
        "\u2018": "'",
        "\u2019": "'",
        "\u201c": '"',
        "\u201d": '"',
        "\u2013": "-",
        "\u2014": "-",
        "\u2022": " ",
        "\u00b7": " ",
    }
)


def strip_terminal_formatting(value: str) -> str:
    value = ANSI.sub("", value)
    while "\b" in value:
        updated = re.sub(r"[^\n]\x08", "", value)
        if updated == value:
            break
        value = updated
    return value.replace("\r\n", "\n").replace("\r", "\n")


def strip_reference_chrome(value: str) -> str:
    lines = strip_terminal_formatting(value).splitlines()
    visible = [index for index, line in enumerate(lines) if line.strip()]
    if len(visible) >= 3:
        # CVS termp emits a two-row terminal footer: host or operating-system
        # text followed by a date/manual label row. The second row identifies
        # the pair; retaining the preceding host row fabricates a missing
        # content candidate because ManT has no terminal chrome.
        if TERMINAL_FOOTER.fullmatch(lines[visible[-1]]):
            lines[visible[-2]] = ""
        lines[visible[0]] = ""
        lines[visible[-1]] = ""
        for index in visible[:4]:
            if RUNNING_HEADER.fullmatch(lines[index]):
                lines[index] = ""
    return "\n".join(lines)


def unwrap_angle_links(value: str) -> str:
    value = ANGLE_LINK.sub(
        lambda match: re.sub(r"[ \t]*\n[ \t]*", "", match.group(1)), value
    )
    return COMPACT_ANGLE_LINK.sub(
        lambda match: re.sub(r"[ \t]*\n[ \t]*", "", match.group(1)), value
    )


def source_literal_hyphen_pairs(source: str | None) -> Counter[tuple[str, str]]:
    """Return direct same-line ``left- right`` source pairs.

    CVS ``term_fill()`` may wrap after every hyphen, including a literal
    trailing operator such as ``x-``.  The usual terminal dehyphenation would
    then manufacture ``xenumerates``.  Only a direct source line with the
    same literal spelling can protect that boundary; macro interpolation,
    continuation, and arbitrary source-wide word presence are not evidence.
    """
    pairs: Counter[tuple[str, str]] = Counter()
    if source is None:
        return pairs
    for raw_line in source.splitlines():
        if raw_line.endswith("\\"):
            continue
        line = ROFF_FONT_ESCAPE.sub("", raw_line)
        line = (
            line.replace(r"\&", "")
            .replace(r"\%", "")
            .replace(r"\~", " ")
            .replace(r"\^", "")
        )
        if "\\" in line:
            continue
        for match in SOURCE_LITERAL_HYPHEN_PAIR.finditer(line):
            pairs[(match["left"], match["right"])] += 1
    return pairs


def dehyphenate_terminal_wraps(value: str, source: str | None = None) -> str:
    """Rejoin formatter wrapping without deleting source-proven operators."""
    protected = source_literal_hyphen_pairs(source)
    if not protected:
        return DEHYPHENATE.sub("", value)

    def replace(match: re.Match[str]) -> str:
        # The pattern deliberately consumes only the hyphen and physical line
        # break.  Recover the immediately surrounding words to test the exact
        # source-proven operator spelling before changing presentation text.
        start = match.start()
        end = match.end()
        left = re.search(r"\w[\w.+:/-]*$", value[:start])
        right = re.match(r"\w[\w.+:/-]*", value[end:])
        if left is None or right is None:
            return ""
        key = (left[0] + "-", right[0])
        if protected[key] <= 0:
            return ""
        protected[key] -= 1
        return "- "

    return DEHYPHENATE.sub(replace, value)


def normalized_visible_text(value: str, source: str | None = None) -> str:
    value = strip_terminal_formatting(value).translate(TRANSLATION)
    value = unwrap_angle_links(value)
    value = URL_WRAP.sub(r"\1", value)
    value = dehyphenate_terminal_wraps(value, source)
    value = BORDERS.sub(" ", value)
    return unicodedata.normalize("NFC", " ".join(value.split()))


def tokens(value: str, source: str | None = None) -> list[str]:
    return TOKEN.findall(normalized_visible_text(value, source))


def token_lines(value: str, source: str | None = None) -> list[list[str]]:
    """Return ordinary reference rows eligible for ordered-phrase comparison.

    Token occurrence comparison still sees every table cell. Ordered phrase
    comparison does not: CVS ``tbl_term.c`` renders a physical table row by
    interleaving cells with vertical frame glyphs, while ManT intentionally
    projects the same typed cells into a one-dimensional portable form. An
    n-gram crossing a frame is therefore renderer geometry, not evidence that
    a content phrase disappeared or changed order. Keep plain rows intact so
    prose, literal blocks, and unboxed tables remain subject to the existing
    strict ordering check.
    """
    value = strip_terminal_formatting(value).translate(TRANSLATION)
    value = unwrap_angle_links(value)
    value = URL_WRAP.sub(r"\1", value)
    value = dehyphenate_terminal_wraps(value, source)
    lines = []
    for line in value.splitlines():
        # The UTF-8 terminal renderer's U+2502 is an emitted table-cell frame,
        # not authored cell text. Do not discard the line from the global
        # token stream; only exclude cross-cell n-gram evidence here.
        if "\u2502" in line:
            continue
        lines.append(TOKEN.findall(unicodedata.normalize("NFC", BORDERS.sub(" ", line))))
    return lines


def source_text_table_rows(source: str | None) -> list[TableRow]:
    """Return only conservative tbl rows whose physical cell boundaries survive.

    The fidelity oracle compares words globally, but an ordered n-gram may not
    cross table cells: terminal tbl output aligns cells horizontally whereas
    ManT deliberately serializes them with `` | ``.  This is deliberately a
    source lens rather than a tbl executor.  It accepts balanced ``T{``/``T}``
    cells and ordinary cells separated by the table's declared delimiter. A
    candidate must later equal one emitted ManT pipe row exactly before it can
    exempt a cross-cell phrase, so prose and layout guesses cannot suppress
    continuity evidence.
    """
    if source is None:
        return []
    rows: list[TableRow] = []
    inside_table = False
    reading_format = False
    separator = "\t"
    current_row: list[tuple[str, ...]] = []
    current_cell: list[str] | None = None

    def finish_cell() -> bool:
        nonlocal current_cell
        if current_cell is None:
            return False
        current_row.append(source_table_cell_tokens(current_cell))
        current_cell = None
        return True

    def finish_row() -> None:
        nonlocal current_row
        if current_row:
            rows.append(tuple(current_row))
        current_row = []

    def append_raw_cell(value: str) -> None:
        current_row.append(source_table_cell_tokens([value]))

    def begin_row_from_raw(raw_line: str) -> None:
        """Consume an ordinary tbl row or a raw prefix before ``T{``."""
        nonlocal current_cell
        if separator not in raw_line and raw_line.strip() != "T{":
            return
        cells = raw_line.split(separator)
        for index, cell in enumerate(cells):
            if cell.strip() != "T{":
                append_raw_cell(cell)
                continue
            # A multi-line cell owns the remainder of its physical row.
            if index != len(cells) - 1:
                current_row.clear()
                return
            current_cell = []
            return
        finish_row()

    for raw_line in source.splitlines():
        stripped = raw_line.strip()
        if not inside_table:
            if stripped in {".TS", "'TS"}:
                inside_table = True
                reading_format = True
                separator = "\t"
            continue
        if stripped in {".TE", "'TE"}:
            # A malformed or unsupported table is simply not evidence. Other
            # independent tbl blocks in the same manual may still be exact.
            current_cell = None
            current_row = []
            inside_table = False
            reading_format = False
            continue
        if reading_format:
            # tbl options precede the format and may select `tab(@);`.
            option = re.search(r"(?:^|[ \t])tab\((.)\)", raw_line)
            if option is not None:
                separator = option.group(1)
            if stripped.endswith("."):
                reading_format = False
            continue
        if current_cell is None:
            if stripped in {"_", "=", ".sp", "'sp"}:
                continue
            begin_row_from_raw(raw_line)
            continue
        closing = raw_line.lstrip()
        if not closing.startswith("T}"):
            current_cell.append(raw_line)
            continue
        if not finish_cell():
            current_row = []
            continue

        suffix = closing[2:]
        # A delimiter immediately after `T}` starts the first following cell;
        # only additional delimiters denote explicit empty cells.
        if suffix.startswith(separator):
            suffix = suffix[len(separator) :]
        if not suffix:
            finish_row()
            continue
        cells = suffix.split(separator)
        for index, cell in enumerate(cells):
            if cell.strip() != "T{":
                append_raw_cell(cell)
                continue
            if index != len(cells) - 1:
                current_row.clear()
                break
            current_cell = []
            break
        else:
            finish_row()
    # An unfinished final table is not useful, but previously complete rows
    # remain exact independent evidence.
    return rows


def source_table_cell_tokens(lines: Sequence[str]) -> tuple[str, ...]:
    """Normalise the literal payload of one conservative ``T{`` cell."""
    visible = []
    for line in lines:
        match = ROFF_REQUEST.fullmatch(line)
        if match is not None:
            request = match.group("name")
            arguments = match.group("args") or ""
            # These controls own no text. Other inline macro operands remain
            # useful as anchors, even though this source lens does not execute
            # their font or spacing state.
            if request in {"br", "sp"}:
                continue
            visible.append(arguments)
            continue
        visible.append(line)
    value = "\n".join(visible)
    value = ROFF_FONT_ESCAPE.sub("", value)
    value = (
        value.replace(r"\&", "")
        .replace(r"\%", "")
        .replace(r"\~", " ")
        .replace(r"\-", "-")
    )
    return tuple(token_key(token) for token in tokens(value))


def portable_table_row_pairs(
    source: str | None, mant: str
) -> list[TableRowPair]:
    """Bind ManT `` | `` rows to exact, literal source-table rows.

    A literal pipe in prose or code is not a table. Require every displayed
    cell to have the exact same normalised token sequence as one source ``T{``
    row before using it as phrase-boundary evidence.
    """
    source_rows = source_text_table_rows(source)
    if not source_rows:
        return []
    output = strip_terminal_formatting(mant).translate(TRANSLATION)
    rows = []
    for line in output.splitlines():
        if " | " not in line:
            continue
        portable = tuple(
            tuple(token_key(token) for token in tokens(cell))
            for cell in line.split(" | ")
        )
        if not portable:
            continue
        for source_row in source_rows:
            if portable == source_row:
                rows.append((source_row, portable))
                break
    return rows


def phrase_crosses_table_cells(
    phrase: Sequence[str], cells: Sequence[Sequence[str]]
) -> bool:
    """Whether one ordered phrase is a subsequence spanning table cells."""
    cell_index = 0
    token_index = 0
    visited: set[int] = set()
    for wanted in (token_key(token) for token in phrase):
        while cell_index < len(cells):
            cell = cells[cell_index]
            while token_index < len(cell):
                if cell[token_index] == wanted:
                    visited.add(cell_index)
                    token_index += 1
                    break
                token_index += 1
            else:
                cell_index += 1
                token_index = 0
                continue
            break
        else:
            return False
    return len(visited) > 1


def labeled_mdoc_links(source: str) -> list[tuple[tuple[str, ...], tuple[str, ...]]]:
    """Return ``.Lk`` destinations whose authored label remains visible.

    Mandoc's terminal renderer appends a labelled link's destination after
    the label, while ManT's text projection keeps the destination in the link
    object and prints only its authored label.  This is not a fidelity loss.
    Keep the label tokens as a line-local guard so an unlabelled occurrence of
    the same URL is never ignored.
    """
    links = []
    logical_lines = []
    continued = ""
    for physical_line in source.splitlines():
        continued += physical_line
        if continued.endswith("\\"):
            continued = continued[:-1]
            continue
        logical_lines.append(continued)
        continued = ""
    if continued:
        logical_lines.append(continued)

    for line in logical_lines:
        if not line.startswith((".", "'")):
            continue
        match = re.search(
            r"(?:^[.']Lk|[ \t]Lk)[ \t]+(?:\"([^\"]+)\"|(\S+))(?:[ \t]+(.*))?$",
            line,
        )
        if match is None:
            continue
        destination = match.group(1) or match.group(2)
        label = (match.group(3) or "").strip()
        # A final punctuation operand is formatter syntax, not a link label.
        label = re.sub(r"(?:^|[ \t]+)[.,:;!?]$", "", label).strip()
        # These transparent source escapes change neither the authored label
        # nor mandoc's visible token boundaries.
        label = label.replace(r"\-", "-").replace(r"\&", "")
        label_tokens = tuple(token_key(value) for value in tokens(label))
        destination_tokens = tuple(token_key(value) for value in tokens(destination))
        if destination_tokens and label_tokens:
            links.append((destination_tokens, label_tokens))
    return links


def omit_mandoc_labeled_link_destinations(
    lines: Sequence[Sequence[str]], source: str
) -> list[list[str]]:
    """Drop mandoc-only URL display tokens beside their authored labels."""
    labeled_links = labeled_mdoc_links(source)
    output = []
    for line in lines:
        keys = [token_key(value) for value in line]
        omitted = {
            destination
            for destinations, label in labeled_links
            for destination in destinations
            if all(value in keys for value in destinations)
            and all(value in keys for value in label)
        }
        output.append(
            [value for value in line if token_key(value) not in omitted]
        )
    return output


def token_key(value: str) -> str:
    # Sentence punctuation is not a semantic part of a token, and reference
    # renderers disagree about whether punctuation abuts inline markup. Keep
    # dots, slashes, and colons inside a token so qualified names and URLs stay
    # useful discovery probes.
    return value.casefold().rstrip(".:/").replace("-", "")


def reference_font_escape_key(value: str) -> str | None:
    """Return the token behind a leaked groff font escape such as ``fBgit``."""
    if len(value) > 3 and value[0] == "f" and value[1] in "BIRP":
        return token_key(value[2:])
    return None


def reference_glued_words(value: str, mine_keys: set[str]) -> bool:
    """Recognize two ManT tokens glued together by reference layout."""
    return any(
        token_key(value[:offset]) in mine_keys
        and token_key(value[offset:]) in mine_keys
        for offset in range(1, len(value))
    )


def missing_token_candidates(reference: Sequence[str], mine: Sequence[str]) -> list[str]:
    mine_keys = {token_key(value) for value in mine}
    output: list[str] = []
    seen: set[str] = set()
    for value in reference:
        key = token_key(value)
        escaped_key = reference_font_escape_key(value)
        if escaped_key is not None and escaped_key in mine_keys:
            continue
        if reference_glued_words(value, mine_keys):
            continue
        if key not in mine_keys and key not in seen:
            output.append(value)
            seen.add(key)
    return output


def appears_with_small_insertions(
    phrase: tuple[str, ...], mine: Sequence[str], maximum_insertions: int = 2
) -> bool:
    """Accept a reference phrase separated only by a few ManT-owned tokens.

    Markdown link targets and structural labels may be visible in ManT while a
    terminal man renderer omits them. Those additions are worth inspecting in
    the rendered output, but they should not flood the fidelity audit with
    overlapping broken-phrase candidates.
    """
    maximum_width = len(phrase) + maximum_insertions
    for start, value in enumerate(mine):
        if value != phrase[0]:
            continue
        cursor = 1
        for candidate in mine[start + 1 : start + maximum_width]:
            if candidate == phrase[cursor]:
                cursor += 1
                if cursor == len(phrase):
                    return True
    return False


def table_phrase_is_only_terminal_geometry(
    phrase: Sequence[str], cells: Sequence[Sequence[str]]
) -> bool:
    """Return whether a reference n-gram is a non-source tbl projection.

    ``tbl_term.c`` may visually interleave a subset of one cell with another
    column. Once a complete source row has exactly matched ManT's portable
    row, a reference n-gram that is merely an ordered subsequence of that
    source row is not evidence that ManT lost or reordered prose. Require it
    not to be contiguous source text: a genuine sentence inside one cell must
    remain eligible for the ordinary broken-phrase check.
    """
    flattened = tuple(token for cell in cells for token in cell)
    if len(phrase) > len(flattened):
        return False
    contiguous = any(
        flattened[index : index + len(phrase)] == tuple(phrase)
        for index in range(len(flattened) - len(phrase) + 1)
    )
    if contiguous:
        return False
    cursor = 0
    for token in flattened:
        if cursor < len(phrase) and token == phrase[cursor]:
            cursor += 1
    return cursor == len(phrase)


def broken_phrase_candidates(
    reference_lines: Sequence[Sequence[str]], mine: Sequence[str], width: int,
    table_rows: Sequence[TableRowPair] = (),
) -> list[str]:
    if len(mine) < width:
        return []
    mine_keys = [token_key(value) for value in mine]
    mine_tokens = set(mine_keys)
    mine_phrases = {
        tuple(mine_keys[index : index + width])
        for index in range(len(mine_keys) - width + 1)
    }
    output: list[str] = []
    seen: set[tuple[str, ...]] = set()
    for line in reference_lines:
        for index in range(len(line) - width + 1):
            display = line[index : index + width]
            key = tuple(token_key(value) for value in display)
            if (
                key in mine_phrases
                or key in seen
                or not all(value in mine_tokens for value in key)
                or appears_with_small_insertions(key, mine_keys)
                or any(
                    phrase_crosses_table_cells(key, source_row)
                    and phrase_crosses_table_cells(key, portable_row)
                    for source_row, portable_row in table_rows
                )
                or any(
                    table_phrase_is_only_terminal_geometry(key, source_row)
                    and source_row == portable_row
                    for source_row, portable_row in table_rows
                )
            ):
                continue
            output.append(" ".join(display))
            seen.add(key)
    return output


def fidelity_signatures(value: str, source: str | None = None) -> tuple[list[str], list[str]]:
    hard: list[str] = []
    review: list[str] = []
    visible_unicode_escapes = set(UNICODE_ESCAPE.findall(value))
    authored_unicode_escapes: set[str] = set()
    if source is not None:
        authored_unicode_escapes.update(
            "\\" + suffix
            for suffix in AUTHORED_LITERAL_UNICODE_ESCAPE.findall(source)
        )
        authored_unicode_escapes.update(
            f"\\[u{ord(character):04X}]"
            for character in AUTHORED_LITERAL_UTF8_AFTER_BACKSLASH.findall(source)
        )
    if visible_unicode_escapes - authored_unicode_escapes:
        # Manuals about roff deliberately print Unicode escape examples. A
        # visible escape is evidence to inspect, not proof that the parser
        # leaked control syntax.
        review.append("bracketed Unicode escape is visible; verify documented syntax")
    if INTERNAL_MARKER.search(value):
        hard.append("internal libmandoc marker leaked")
    if GLUED_MARKER.search(value):
        review.append("list or enumeration marker may be glued to following text")
    return hard, review


def mdoc_multi_operand_fa_phrases(source: str) -> list[str]:
    """Return directly comparable multi-operand ``Fa`` phrases in ``Fo`` runs.

    Counting every comma in an entire manual is not evidence that an ``Fa``
    separator was lost: documentation prose, code, and unrelated lists all
    contribute commas. Recover only simple authored operands whose exact
    source-level phrase can be found in a rendered synopsis. Complex roff
    escapes stay deliberately unprofiled rather than guessed.
    """
    phrases = []
    function_depth = 0
    for line in source.splitlines():
        if re.match(r"^[.']Fo(?:[ \t]|$)", line):
            function_depth += 1
            continue
        if re.match(r"^[.']Fc(?:[ \t]|$)", line):
            function_depth = max(0, function_depth - 1)
            continue
        if not function_depth or not MDOC_MULTI_OPERAND_FA.fullmatch(line):
            continue
        operands = re.findall(r'"((?:[^"\\]|\\.)*)"', line)
        if any(re.search(r"\\(?![\\\"])", operand) for operand in operands):
            continue
        phrase = ", ".join(re.sub(r'\\([\\\"])', r'\1', operand) for operand in operands)
        if phrase:
            phrases.append(phrase)
    return phrases


def mdoc_function_names(source: str) -> set[str]:
    """Return plain function names with a declaration form safe to compare."""
    return {match.group("name") for match in MDOC_FUNCTION_NAME.finditer(source)}


def differential_signatures(
    source: str, reference: str, mant: str
) -> list[str]:
    """Return source-conditioned rendering differences worth human review.

    Token presence deliberately ignores punctuation and whitespace. These
    probes cover formatter-owned mdoc syntax where those characters carry
    semantics, while keeping the general comparison tolerant of wrapping and
    layout differences.
    """
    review = []
    reference = strip_terminal_formatting(reference)
    mant = strip_terminal_formatting(mant)
    if MDOC_NAME_DESCRIPTION.search(source):
        reference_attached = len(EM_DASH_ATTACHED_TO_WORD.findall(reference))
        mant_attached = len(EM_DASH_ATTACHED_TO_WORD.findall(mant))
        if mant_attached > reference_attached:
            review.append(
                "mdoc Nd separator is attached to its description "
                f"(reference={reference_attached}, mant={mant_attached})"
            )
    reference_visible = normalized_visible_text(reference)
    mant_visible = normalized_visible_text(mant)
    for name in sorted(mdoc_function_names(source)):
        declaration = re.compile(rf"(?<![A-Za-z0-9_.-]){re.escape(name)}\s*\([^)]*\)\s*;")
        if declaration.search(reference_visible) and not declaration.search(mant_visible):
            review.append(f"mdoc function declaration terminator is missing for {name}")
    for phrase in mdoc_multi_operand_fa_phrases(source):
        if phrase in reference_visible and phrase not in mant_visible:
            review.append(f"mdoc multi-operand Fa separator is missing in {phrase}")
    return review
