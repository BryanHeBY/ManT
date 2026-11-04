"""Deterministic checks for the fidelity audit helpers, without a host reference renderer."""
from __future__ import annotations

import json
import tempfile
from collections import Counter
from pathlib import Path

from .arguments import ROOT, compile_source_patterns
from .model import (
    AuditArtifact,
    AuditRecord,
    Finding,
    SyntaxProfile,
)
from .layout import (
    layout_comparison,
    no_fill_source_layout,
)
from .text import (
    broken_phrase_candidates,
    differential_signatures,
    fidelity_signatures,
    labeled_mdoc_links,
    missing_token_candidates,
    normalized_visible_text,
    omit_mandoc_labeled_link_destinations,
    portable_table_row_pairs,
    strip_reference_chrome,
    table_phrase_is_only_terminal_geometry,
    token_key,
    token_lines,
    tokens,
)
from .report import (
    write_review_bundle,
)
from scripts.roff.lib.roff_reference import (
    self_check as reference_runner_self_check,
)
from .sources import (
    manual_section,
    relative_label,
    source_digest,
    manual_topic,
    manual_hierarchy_root,
    source_audit_identity,
    source_bytes_are_context_independent,
    redirect_only_target,
    confined_redirect_target,
)
from .sampling import (
    feature_counts,
    rare_feature_sample,
)
from .ledger import (
    AUDIT_DATABASE_FIELDS,
    read_source_identities,
    reusable_cross_corpus_sources,
    read_audit_database,
    write_audit_database,
    merged_review_status,
)


def self_check() -> None:
    reference_runner_self_check()
    for marker in ("\x1a", "\x1c", "\x1d", "\x1e", "\x1f"):
        assert fidelity_signatures("before" + marker + "after")[0] == [
            "internal libmandoc marker leaked"
        ]
    digest = "0" * 64
    assert source_audit_identity("share/man/man1/git.1.gz", digest) == (
        digest,
        "git",
        "1",
    )
    assert not source_bytes_are_context_independent(b".so man1/git.1\n")
    assert not source_bytes_are_context_independent(b".if n .mso fallback.tmac\n")
    assert source_bytes_are_context_independent(b".SH DESCRIPTION\nordinary text\n")
    assert strip_reference_chrome(
        "delim $$\nMM2GV(1) General Commands Manual MM2GV(1)\ncontent\nfooter\n"
    ).strip() == "content"
    assert strip_reference_chrome(
        "PROBE(1) Commands PROBE(1)\ncontent\nLinux 6.18\n"
        "   July 3, 2025                  PROBE(1)\n"
    ).strip() == "content"
    assert normalized_visible_text(
        "read <https://example.test/api/\nversion.3.html> now"
    ) == "read https://example.test/api/version.3.html now"
    assert normalized_visible_text(
        "read ⟨https://example.test/api/version.3.html⟩. now"
    ) == "read https://example.test/api/version.3.html. now"
    assert token_key("line-break") == token_key("linebreak")
    assert manual_section(Path("git.1.gz")) == "1"
    assert manual_section(Path("SSL_read.3ssl")) == "3ssl"
    assert manual_topic(Path("SSL_read.3ssl.gz")) == "SSL_read"
    assert manual_hierarchy_root(
        Path("/usr/share/man/fr/man3/printf.3.gz"),
        [Path("/usr/share/man")],
    ) == Path("/usr/share/man/fr")
    assert manual_hierarchy_root(
        Path("/usr/share/man/man3/printf.3bsd.gz"),
        [Path("/usr/share/man")],
    ) == Path("/usr/share/man")
    assert relative_label(
        Path("/opt/tools/share/man/man1/demo.1"),
        [Path("/opt/tools/man"), Path("/opt/tools/share/man")],
    ) == "share/man/man1/demo.1"
    assert source_digest(Path(__file__)) is not None
    profile_a = SyntaxProfile(frozenset({"common", "rare-a"}), 0)
    profile_b = SyntaxProfile(frozenset({"common"}), 0)
    profile_c = SyntaxProfile(frozenset({"common", "rare-c"}), 0)
    sample_pages = [Path("a.1"), Path("b.1"), Path("c.1")]
    sample_profiles = {
        sample_pages[0]: profile_a,
        sample_pages[1]: profile_b,
        sample_pages[2]: profile_c,
    }
    sample_frequencies = feature_counts(sample_pages, sample_profiles)
    sample_coverage = Counter({"common": 1, "rare-a": 1})
    assert rare_feature_sample(
        sample_pages,
        1,
        "self-check",
        sample_profiles,
        sample_frequencies,
        sample_coverage,
    ) == [Path("c.1")]
    interaction_profiles = {
        sample_pages[0]: SyntaxProfile(frozenset({"common", "rare-atomic"}), 0),
        sample_pages[1]: SyntaxProfile(
            frozenset({"common", "interaction:macro:SY+flag:no-fill"}), 0
        ),
    }
    interaction_frequencies = feature_counts(
        sample_pages[:2], interaction_profiles
    )
    assert rare_feature_sample(
        sample_pages[:2],
        1,
        "interaction-self-check",
        interaction_profiles,
        interaction_frequencies,
        Counter({"common": 1}),
    ) == [Path("b.1")]
    assert token_key("alloca.") == token_key("alloca")
    assert token_key("docs.example/path") != token_key("docs.example")
    assert tokens("one line-\nbreak here") == ["one", "linebreak", "here"]
    # CVS term_fill() may wrap after an authored trailing hyphen that is an
    # operator rather than a discretionary word break.  The direct source
    # spelling retains that operator; an unproved physical wrap still joins.
    literal_hyphen_source = r"\fIx-\fR enumerates from index"
    assert tokens("x-\n enumerates from index", literal_hyphen_source) == [
        "enumerates",
        "from",
        "index",
    ]
    assert tokens("x-\n enumerates from index") == ["xenumerates", "from", "index"]
    assert tokens("compound-\n word", literal_hyphen_source) == ["compoundword"]
    assert tokens("-W----\nZIO_STAGE_ENCRYPT") == ["W----", "ZIO_STAGE_ENCRYPT"]
    assert tokens("Prikaže café 日本語") == ["Prikaže", "café", "日本語"]
    assert tokens("cafe\u0301") == ["café"]
    assert missing_token_candidates(["alpha", "missing"], ["alpha"]) == ["missing"]
    assert missing_token_candidates(["fBpackage.json"], ["package.json"]) == []
    assert missing_token_candidates(["defsReport"], ["defs", "Report"]) == []
    assert missing_token_candidates(["PATHList"], ["PATH", "List"]) == []
    assert missing_token_candidates(["CPANCPAN"], ["CPAN"]) == []
    assert tokens("tr//\nA new feature") == ["tr//", "new", "feature"]
    assert tokens("https://example.test/path/\nW3C title") == [
        "https://example.test/path/",
        "W3C",
        "title",
    ]
    assert tokens("https://example.test/path/\nnext-part") == [
        "https://example.test/path/next-part"
    ]
    assert tokens("https://example.test/path/\n- next item") == [
        "https://example.test/path/",
        "next",
        "item",
    ]
    labeled_links = labeled_mdoc_links(
        '.Lk https://example.test/path "Example label" .\n'
        ".Lk https://example.test/unlabelled\n"
    )
    assert labeled_links == [
        ((token_key("https://example.test/path"),), ("example", "label"))
    ]
    assert labeled_mdoc_links(
        '.Lk https://example.test/continued\\\n "Continued label" .\n'
        '.Pq Lk decode.html#peer "peer status word" .\n'
    ) == [
        (("https://example.test/continued",), ("continued", "label")),
        (("decode.html", "peer"), ("peer", "status", "word")),
    ]
    assert omit_mandoc_labeled_link_destinations(
        [
            ["Example", "label:", "https://example.test/path"],
            ["https://example.test/path"],
        ],
        '.Lk https://example.test/path "Example label" .',
    ) == [["Example", "label:"], ["https://example.test/path"]]
    assert broken_phrase_candidates(
        [["one", "two", "three", "four"]],
        ["one", "two", "inserted", "three", "four"],
        4,
    ) == []
    assert broken_phrase_candidates(
        [["one", "two", "three", "four"]],
        ["one", "two", "four", "elsewhere", "three"],
        4,
    ) == ["one two three four"]
    literal_table_source = """.TS
tab(@);
l l.
T{
alpha beta
T}@T{
gamma delta
T}
.TE
"""
    literal_table_rows = portable_table_row_pairs(
        literal_table_source,
        "alpha beta | gamma delta\n",
    )
    assert len(literal_table_rows) == 1
    assert broken_phrase_candidates(
        [["alpha", "beta", "gamma", "delta"]],
        ["alpha", "beta", "delta", "elsewhere", "gamma"],
        4,
        literal_table_rows,
    ) == []
    geometry_table_source = """.TS
tab(@);
l l.
T{
The color for a folder or a shortcut to a folder as an RGB hex string.
T}@T{
string
T}
.TE
"""
    geometry_table_rows = portable_table_row_pairs(
        geometry_table_source,
        "The color for a folder or a shortcut to a folder as an RGB hex string. | string\n",
    )
    # The reference table formatter can line up `color for folder` beside the
    # next cell's `string`. The exact source/portable row proves that this
    # non-contiguous sequence is geometry, not a missing ManT phrase.
    assert broken_phrase_candidates(
        [["color", "for", "folder", "string"]],
        [
            "the", "color", "for", "folder", "shortcut", "folder",
            "rgb", "hex", "string", "string",
        ],
        4,
        geometry_table_rows,
    ) == []
    # Contiguous prose inside a verified table cell remains a real ordering
    # candidate when ManT does not contain it.
    assert not table_phrase_is_only_terminal_geometry(
        ("color", "for", "folder"), geometry_table_rows[0][0]
    )
    # A page that merely contains a tbl request cannot exempt an unrelated
    # literal pipe in source code: no exact source-table row binds it.
    assert portable_table_row_pairs(
        literal_table_source,
        "alpha beta | ordinary code\n",
    ) == []
    mixed_table_source = """.TS
l l l l.
T{
alpha
T}\t1\tbeta\tT{
gamma
T}
.TE
"""
    assert portable_table_row_pairs(
        mixed_table_source,
        "alpha | (1) | beta | gamma\n",
    ) == [
        (
            (("alpha",), (), ("beta",), ("gamma",)),
            (("alpha",), (), ("beta",), ("gamma",)),
        )
    ]
    raw_prefix_table_source = """.TS
l l l.
Keymap\tT{
complete keyboard description
T}\tT{
aliases for keys
T}
.TE
"""
    assert portable_table_row_pairs(
        raw_prefix_table_source,
        "Keymap | complete keyboard description | aliases for keys\n",
    ) == [
        (
            (
                ("keymap",),
                ("complete", "keyboard", "description"),
                ("aliases", "for", "keys"),
            ),
            (
                ("keymap",),
                ("complete", "keyboard", "description"),
                ("aliases", "for", "keys"),
            ),
        )
    ]
    custom_separator_table_source = """.TS
tab(@);
l l.
left@T{
right
T}
.TE
"""
    assert portable_table_row_pairs(custom_separator_table_source, "left | right\n") == [
        ((("left",), ("right",)), (("left",), ("right",)))
    ]
    # CVS tbl_term.c emits a U+2502 frame between physical cells. The global
    # token comparison still observes both cells; phrase ordering must not
    # borrow that renderer-owned geometry as a missing-content claim.
    assert token_lines("│ one two │ three four │") == []
    assert token_lines("one two three four") == [["one", "two", "three", "four"]]
    hard, review = fidelity_signatures(r"text \[u2192]")
    assert not hard
    assert review == [
        "bracketed Unicode escape is visible; verify documented syntax"
    ]
    hard, review = fidelity_signatures(
        r"literal \[u2192] example", r"author writes \\[u2192] literally"
    )
    assert not hard
    assert not review
    # CVS preconv.c rewrites the raw Ukrainian `е` to `\[u0435]`; the
    # preceding authored backslash remains literal, so this is documentation
    # text rather than an unhandled parser escape.
    hard, review = fidelity_signatures(
        r"literal \[u0435] example", "author writes \\е literally"
    )
    assert not hard
    assert not review
    for source in [
        r"author writes \\[u2192] literally",
        r"author writes \e[u2192] literally",
        r"author writes \[rs][u2192] literally",
    ]:
        hard, review = fidelity_signatures(r"literal \[u2192] example", source)
        assert not hard
        assert not review
    hard, review = fidelity_signatures(r"literal \[u2192] example", r"u2192 appears in prose")
    assert not hard
    assert review == [
        "bracketed Unicode escape is visible; verify documented syntax"
    ]
    assert differential_signatures(
        ".Nd description\n", "name — description", "name —description"
    ) == [
        "mdoc Nd separator is attached to its description (reference=0, mant=1)"
    ]
    assert differential_signatures(
        ".Fo function\n.Fc\n", "function();", "function()"
    ) == ["mdoc function declaration terminator is missing for function"]
    assert not differential_signatures(
        ".Fn function\n", "function();", "function();"
    )
    assert differential_signatures(
        '.Fo function\n.Fa "int first" "int second"\n.Fc\n',
        "function(int first, int second);",
        "function(int first int second);",
    )[-1] == "mdoc multi-operand Fa separator is missing in int first, int second"
    assert not differential_signatures(
        '.Fo function\n.Fa "int first" "int second"\n.Fc\n',
        "function(int first, int second); prose, has unrelated commas, too.",
        "function(int first, int second);",
    )
    assert redirect_only_target(b'.\\" alias\n.so man1/target.1\n') == "man1/target.1"
    assert redirect_only_target(b".so man1/target.1\ntext\n") is None
    layout_source = ".EX\nplain first\n  plain second\n.EE\n"
    synopsis_source = (
        ".EX\n.SY #!\\f[I]interpreter\\f[]\n.RI [ optional-arg ]\n.YS\n.EE\n"
    )
    synopsis_layout = no_fill_source_layout(synopsis_source)
    assert synopsis_layout.anchors == {"#!interpreter", "[optional-arg]"}
    assert synopsis_layout.authored_indents["#!interpreter"] == 0
    assert synopsis_layout.consecutive_pairs == {("#!interpreter", "[optional-arg]")}
    collapsed_layout = layout_comparison(
        "  plain first\n    plain second\n",
        "plain first\nplain second\n",
        layout_source,
    )
    assert collapsed_layout.shared_anchors == 2
    assert any("authored relative indentation may collapse" in item for item in collapsed_layout.candidates)
    merged_layout = layout_comparison(
        "  plain first\n  plain second\n",
        "plain first plain second\n",
        layout_source,
    )
    assert any("line boundaries may merge" in item for item in merged_layout.candidates)
    same_text_elsewhere = layout_comparison(
        "  plain first\n  plain second\n",
        "plain first plain second\n",
        "plain first\nplain second\n.EX\nplain first\nplain second\n.EE\n",
    )
    assert not any("line boundaries may merge" in item for item in same_text_elsewhere.candidates)
    assert "plain first plain second" in no_fill_source_layout(
        '.EX\n.B "plain first plain second"\n.EE\n'
    ).source_lines
    spacing_layout = layout_comparison(
        "  plain first\n  plain second\n",
        "plain first\n\nplain second\n",
        ".EX\nplain first\n\nplain second\n.EE\n",
    )
    assert any("spacing divergence" in item for item in spacing_layout.candidates)
    assert len(compile_source_patterns([r"^\.Dd", r"^\.Fn"])) == 2
    clean = AuditRecord(
        "host", "man/demo.1", "1", "0" * 64, "clean", "not-required", ""
    )
    pending = AuditRecord(
        "host", "man/demo.1", "1", "0" * 64, "review", "pending", ""
    )
    assert merged_review_status(None, "review") == "pending"
    assert merged_review_status(clean, "hard-failure") == "pending"
    assert merged_review_status(pending, "clean") == "not-required"
    artifact = AuditArtifact(
        finding=Finding(path="manual/1/demo", status="clean"),
        source=b".TH DEMO 1\n",
        reference_output="DEMO(1)\n",
        mant_output="DEMO\n",
    )
    with tempfile.TemporaryDirectory() as temporary:
        bundle = Path(temporary) / "review"
        write_review_bundle(bundle, [(artifact.finding.path, artifact)])
        manifest = json.loads((bundle / "manifest.json").read_text(encoding="utf-8"))
        assert manifest["tool"] == "mant-roff-fidelity-review/v1"
        assert len(manifest["pages"]) == 1
        page = manifest["pages"][0]
        assert page["path"] == "manual/1/demo"
        assert page["status"] == "clean"
        assert set(page["files"]) == {"source", "reference", "mant", "finding"}
        assert (bundle / page["files"]["source"]).read_bytes() == artifact.source
        assert (bundle / page["files"]["reference"]).read_text(encoding="utf-8") == artifact.reference_output
        assert (bundle / page["files"]["mant"]).read_text(encoding="utf-8") == artifact.mant_output
        mandoc_database = Path(temporary) / "mandoc.csv"
        mandoc_record = AuditRecord(
            "fixture",
            "man3/example.3",
            "3",
            "1" * 64,
            "clean",
            "not-required",
            "",
            "mandoc",
            "mandoc-test",
        )
        write_audit_database(
            mandoc_database,
            [mandoc_record],
            "mandoc",
            "mandoc-test",
        )
        assert read_audit_database(
            mandoc_database,
            "mandoc",
            "mandoc-test",
        ) == {("fixture", "man3/example.3", "1" * 64): mandoc_record}
        hierarchy = Path(temporary) / "share/man"
        alias = hierarchy / "man1/alias.1"
        target = hierarchy / "man1/target.1.gz"
        alias.parent.mkdir(parents=True)
        alias.write_bytes(b".so man1/target.1\n")
        target.write_bytes(b"not actually compressed")
        assert confined_redirect_target(alias, hierarchy, "man1/target.1") == target
        assert confined_redirect_target(alias, hierarchy, "../outside.1") is None
        source_ledger = Path(temporary) / "source-ledger.csv"
        source_ledger.write_text(
            ",".join(AUDIT_DATABASE_FIELDS)
            + "\n"
            + f"known,man/man1/example.1,1,{'2' * 64},clean,not-required,\n",
            encoding="utf-8",
        )
        assert read_source_identities(source_ledger, "known") == {
            ("man/man1/example.1", "2" * 64)
        }
    reusable_page = ROOT / "tests/fixtures/roff/nested-fl-mdoc.1"
    reusable_digest = source_digest(reusable_page)
    assert reusable_digest is not None
    reusable_record = AuditRecord(
        "prior",
        "man1/nested-fl-mdoc.1",
        "1",
        reusable_digest,
        "clean",
        "not-required",
        "",
    )
    assert reusable_cross_corpus_sources(
        [reusable_page],
        {reusable_page: ("share/man1/nested-fl-mdoc.1", reusable_digest)},
        {
            (
                reusable_record.corpus,
                reusable_record.path,
                reusable_record.digest,
            ): reusable_record
        },
        "current",
    ) == {reusable_page: reusable_record}
