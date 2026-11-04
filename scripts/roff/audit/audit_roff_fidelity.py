#!/usr/bin/env python3
"""Find likely roff fidelity gaps without treating layout as a contract.

The audit compares ManT's plain manual rendering with a local man(1)/groff
reference. It is deliberately a developer and release-time discovery tool:
ordinary CI keeps the focused, deterministic Rust regressions derived from
confirmed findings instead of installing or trusting a host reference renderer.
Pages containing .so requests are rendered through ManT's indexed-manual path
so aliases exercise the same bounded hierarchy resolution as product queries.
"""

from __future__ import annotations

import os
import shutil
import sys
from pathlib import Path
from typing import Sequence

from scripts.roff.lib.fidelity.arguments import (
    FIXTURE_ROOT,
    ROOT,
    compile_source_patterns,
    parse_arguments,
)
from scripts.roff.lib.fidelity.model import (
    AuditArtifact,
    AuditRecord,
    AuditSummary,
    Finding,
    SyntaxProfile,
)
from scripts.roff.lib.fidelity.layout import (
    layout_comparison,
    no_fill_source_layout,
)
from scripts.roff.lib.fidelity.text import (
    EXTERNAL_ROFF_CONTEXT,
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
from scripts.roff.lib.fidelity.report import (
    print_finding,
    update_summary,
    write_json_report,
    write_review_bundle,
    write_syntax_report,
)

from scripts.roff.oracle import mandoc_oracle
from scripts.roff.lib.roff_reference import reference_environment, run_renderer
from scripts.roff.lib.roff_reference import self_check as reference_runner_self_check

# Retain helper names on the importable driver facade used by combined audits.
from scripts.roff.lib.fidelity.sources import (
    MANUAL_SUFFIX,
    discover_pages,
    manual_section,
    relative_label,
    source_bytes,
    source_digest,
    explicit_pages,
    manual_topic,
    manual_hierarchy_root,
    filter_pages_by_source,
    source_audit_identity,
    source_is_context_independent,
    source_bytes_are_context_independent,
    contains_so_request,
    contains_external_request,
    redirect_only_target,
    confined_redirect_target,
    mant_render_command,
    reference_render_command,
)
from scripts.roff.lib.fidelity.syntax import (
    SYNTAX_PROFILE_SCHEMA,
    SYNTAX_CACHE_VERSION,
    syntax_cache_key,
    read_syntax_cache,
    write_syntax_cache,
    run_syntax_profile_batch,
    syntax_profiles,
)
from scripts.roff.lib.fidelity.sampling import (
    stable_sample,
    stable_sample_by_section,
    feature_counts,
    rare_feature_sample,
    syntax_feature_weight,
    rare_feature_sample_by_section,
)
from scripts.roff.lib.fidelity.ledger import (
    AUDIT_DATABASE_FIELDS,
    MANDOC_AUDIT_DATABASE_FIELDS,
    read_source_identities,
    reusable_cross_corpus_sources,
    read_audit_database,
    write_audit_database,
    merged_review_status,
)


def self_check() -> None:
    from scripts.roff.lib.fidelity.self_check import self_check as run_self_check

    run_self_check()


def audit_page(
    path: Path,
    label: str,
    roots: Sequence[Path],
    mant: Path,
    reference: str,
    reference_kind: str,
    timeout: int,
    ngram: int,
    layout_signals: bool,
) -> AuditArtifact:
    """Use the facade's source reader for hierarchy selection and reference input."""
    environment = reference_environment()
    raw_source = source_bytes(path)
    # An unavailable initial read provides no .so evidence. Avoid a helper
    # re-read for hierarchy selection while retaining None in the audit input.
    mant_command, hierarchy_root = mant_render_command(
        path, roots, mant, source=raw_source if raw_source is not None else b"",
    )
    if hierarchy_root is not None:
        environment["MANT_MANPATH"] = str(hierarchy_root)
        # GNU man resolves redirect-only `.so` pages through MANPATH even
        # with `-l`. Keep the reference on the same localized hierarchy as
        # ManT instead of accidentally comparing a translated alias with the
        # default-language target.
        environment["MANPATH"] = str(hierarchy_root)
    reference_command, reference_input, skip_detail = reference_render_command(
        path,
        reference,
        reference_kind,
        hierarchy_root,
        raw_source,
        source_reader=source_bytes,
    )
    if reference_command is None:
        return AuditArtifact(
            finding=Finding(
                path=label,
                status="skipped",
                signatures=["source is not comparable with the selected reference"],
                detail=skip_detail,
            ),
            source=raw_source,
            reference_output=None,
            mant_output=None,
        )
    mant_status, mant_output, mant_error = run_renderer(
        mant_command,
        timeout,
        environment,
    )
    if mant_status != 0:
        return AuditArtifact(
            finding=Finding(
                path=label,
                status="hard-failure",
                signatures=["ManT failed to render the page"],
                detail=mant_error.strip() or f"exit status {mant_status}",
            ),
            source=raw_source,
            reference_output=None,
            mant_output=mant_output,
        )

    reference_status, reference_output, reference_error = run_renderer(
        reference_command, timeout, environment, reference_input
    )
    if reference_status != 0:
        return AuditArtifact(
            finding=Finding(
                path=label,
                status="hard-failure",
                signatures=["reference renderer failed; page was not compared"],
                detail=reference_error.strip() or f"reference exit status {reference_status}",
            ),
            source=raw_source,
            reference_output=reference_output,
            mant_output=mant_output,
        )

    return compare_rendered(label, raw_source, reference_output, mant_output,
                            reference_kind, ngram, layout_signals)


def compare_rendered(
    label: str, raw_source: bytes | None, reference_output: str, mant_output: str,
    reference_kind: str, ngram: int = 4, layout_signals: bool = True,
) -> AuditArtifact:
    """The legacy oracle, independent of discovery and subprocess transport.

    Callers must establish successful bounded renders and source identity first.
    No ledger policy or interpretation differs from ``audit_page``.
    """
    reference_output = strip_reference_chrome(reference_output)
    source_text = raw_source.decode("utf-8", errors="replace") if raw_source is not None else None
    reference_lines = token_lines(reference_output, source_text)
    if reference_kind == "mandoc" and raw_source is not None:
        reference_lines = omit_mandoc_labeled_link_destinations(
            reference_lines,
            raw_source.decode("utf-8", errors="replace"),
        )
    reference_tokens = [value for line in reference_lines for value in line]
    mant_tokens = tokens(mant_output, source_text)
    if not reference_tokens:
        return AuditArtifact(
            finding=Finding(
                path=label,
                status="hard-failure",
                reference_tokens=len(reference_tokens),
                mant_tokens=len(mant_tokens),
                signatures=["reference renderer produced no comparable visible tokens"],
                detail="the page cannot be classified as clean without a reference corpus",
            ),
            source=raw_source,
            reference_output=reference_output,
            mant_output=mant_output,
        )
    if not mant_tokens:
        return AuditArtifact(
            finding=Finding(
                path=label,
                status="hard-failure",
                reference_tokens=len(reference_tokens),
                mant_tokens=0,
                signatures=["ManT produced no comparable visible tokens"],
                detail="the reference renderer produced visible content",
            ),
            source=raw_source,
            reference_output=reference_output,
            mant_output=mant_output,
        )

    missing = missing_token_candidates(reference_tokens, mant_tokens)
    phrases = broken_phrase_candidates(
        reference_lines,
        mant_tokens,
        ngram,
        portable_table_row_pairs(source_text, mant_output),
    )
    hard_signatures, review_signatures = fidelity_signatures(mant_output, source_text)
    if raw_source is not None:
        review_signatures.extend(
            differential_signatures(
                source_text,
                reference_output,
                mant_output,
            )
        )
    if not mant_output.strip():
        hard_signatures.append("ManT produced empty output")
    signatures = hard_signatures + review_signatures
    status = "hard-failure" if hard_signatures else "review" if missing or phrases or review_signatures else "clean"
    layout = (
        layout_comparison(
            reference_output,
            mant_output,
            raw_source.decode("utf-8", errors="replace") if raw_source is not None else None,
        )
        if layout_signals
        else None
    )
    return AuditArtifact(
        finding=Finding(
            path=label,
            status=status,
            reference_tokens=len(reference_tokens),
            mant_tokens=len(mant_tokens),
            missing_tokens=missing,
            broken_phrases=phrases,
            signatures=signatures,
            layout=layout,
        ),
        source=raw_source,
        reference_output=reference_output,
        mant_output=mant_output,
    )


def validate_tools(mant: Path, reference: str) -> None:
    if not mant.is_file():
        raise ValueError(f"ManT executable not found: {mant}; run `cargo build -p mant`")
    if os.sep not in reference and shutil.which(reference) is None:
        raise ValueError(f"reference command not found: {reference}")


def validate_syntax_profiler(path: Path) -> None:
    if not path.is_file():
        raise ValueError(
            f"syntax profiler not found: {path}; run "
            "`cargo build -p libmandoc-rs --example roff_ast_profile`"
        )


def main(argv: Sequence[str]) -> int:
    arguments = parse_arguments(argv)
    if arguments.self_check:
        self_check()
        print("roff fidelity audit self-check succeeded")
        return 0

    try:
        explicit_page_set = arguments.pages_file is not None
        if arguments.recorded_only and arguments.audit_db is None:
            raise ValueError("--recorded-only requires --audit-db")
        if arguments.retry_skipped and arguments.audit_db is None:
            raise ValueError("--retry-skipped requires --audit-db")
        if arguments.pending_only and arguments.audit_db is None:
            raise ValueError("--pending-only requires --audit-db")
        if arguments.dedupe_across_corpora and arguments.audit_db is None:
            raise ValueError("--dedupe-across-corpora requires --audit-db")
        if arguments.replay_source_records and not arguments.corpus:
            raise ValueError("--replay-source-records requires an explicit --corpus")
        if (
            arguments.reference_kind == "mandoc"
            and (not arguments.reference_id
                 or arguments.oracle_attestation is None
                 or arguments.oracle_archive is None)
        ):
            raise ValueError("mandoc audits require --reference-id, --oracle-attestation and --oracle-archive")
        if arguments.reference_kind == "mandoc":
            reference_path = Path(arguments.reference)
            if not reference_path.is_absolute():
                raise ValueError("mandoc reference must be an explicit absolute path")
            arguments.oracle_preflight = mandoc_oracle.preflight(
                ROOT, reference_path, arguments.oracle_archive.resolve(),
                arguments.oracle_attestation.resolve(), arguments.reference_id, "utf8",
            )
        if arguments.recorded_only and arguments.recheck_recorded:
            raise ValueError("--recorded-only and --recheck-recorded are mutually exclusive")
        exclusive_database_selections = sum(
            int(selected)
            for selected in [
                arguments.retry_skipped,
                arguments.pending_only,
                arguments.recorded_only,
                arguments.recheck_recorded,
            ]
        )
        if exclusive_database_selections > 1:
            raise ValueError(
                "--retry-skipped, --pending-only, --recorded-only, and "
                "--recheck-recorded are mutually exclusive"
            )
        if arguments.dedupe_across_corpora and exclusive_database_selections:
            raise ValueError(
                "--dedupe-across-corpora cannot be combined with an explicit "
                "audit database recheck mode"
            )
        if explicit_page_set and (
            arguments.max_pages
            or arguments.max_pages_per_section
            or arguments.man_section
            or arguments.source_pattern
            or arguments.syntax_priority
            or arguments.syntax_cache is not None
            or arguments.syntax_report is not None
            or arguments.dedupe_across_corpora
            or exclusive_database_selections
        ):
            raise ValueError(
                "--pages-file cannot be combined with sampling, source filters, "
                "syntax selection, or ledger selection options"
            )
        if arguments.replay_source_records and (
            arguments.max_pages
            or arguments.max_pages_per_section
            or arguments.man_section
            or arguments.source_pattern
            or arguments.syntax_priority
            or arguments.dedupe_across_corpora
        ):
            raise ValueError(
                "--replay-source-records cannot be combined with sampling, "
                "source filters, syntax-priority selection, or cross-corpus deduplication"
            )
        validate_tools(arguments.mant, arguments.reference)
        syntax_enabled = (
            arguments.syntax_priority
            or arguments.syntax_report is not None
            or arguments.syntax_cache is not None
        )
        if syntax_enabled:
            validate_syntax_profiler(arguments.syntax_profiler)
        roots = [path.resolve() for path in arguments.manpath] if arguments.manpath else [FIXTURE_ROOT]
        if explicit_page_set:
            all_pages = explicit_pages(arguments.pages_file, roots)
            source_filter_errors: list[Path] = []
        else:
            all_pages = discover_pages(roots)
            if arguments.man_section:
                selected_sections = set(arguments.man_section)
                all_pages = [
                    path for path in all_pages if manual_section(path) in selected_sections
                ]
            source_patterns = compile_source_patterns(arguments.source_pattern)
            all_pages, source_filter_errors = filter_pages_by_source(
                all_pages, source_patterns
            )
        pages = list(all_pages)
        corpus = arguments.corpus or (
            "fixtures" if not arguments.manpath else "local-manpath"
        )
        reference_id = arguments.reference_id or arguments.reference
        database = (
            read_audit_database(
                arguments.audit_db,
                arguments.reference_kind,
                reference_id,
            )
            if arguments.audit_db
            else {}
        )
        page_records = {
            path: (relative_label(path, roots), source_digest(path)) for path in pages
        }
        replay_targets: set[tuple[str, str]] = set()
        if arguments.replay_source_records:
            replay_targets = read_source_identities(arguments.source_ledger, corpus)
            available = {
                (label, digest)
                for label, digest in page_records.values()
                if digest is not None
            }
            missing = sorted(replay_targets - available)
            if missing:
                examples = ", ".join(
                    f"{label}@{digest[:12]}" for label, digest in missing[:5]
                )
                raise ValueError(
                    f"selected roots do not reproduce {len(missing)} of "
                    f"{len(replay_targets)} source identities for {corpus}: {examples}"
                )
            pages = [
                path
                for path in pages
                if (page_records[path][0], page_records[path][1]) in replay_targets
            ]
        cross_corpus_duplicates = (
            reusable_cross_corpus_sources(
                pages, page_records, database, corpus
            )
            if arguments.dedupe_across_corpora and not explicit_page_set
            else {}
        )
        recorded = {
            path
            for path, (label, digest) in page_records.items()
            if digest is not None and (corpus, label, digest) in database
        }
        completed = set()
        for path in recorded:
            label, digest = page_records[path]
            if digest is not None and database[
                (corpus, label, digest)
            ].scan_status != "skipped":
                completed.add(path)
        incomplete = recorded - completed
        pending = set()
        for path in recorded:
            label, digest = page_records[path]
            if digest is not None and database[
                (corpus, label, digest)
            ].review_status == "pending":
                pending.add(path)
        if explicit_page_set:
            # Exact page sets are callers' explicit intent.  They may still
            # update an audit database, but never become empty simply because
            # the same source was audited in an earlier broad scan.
            pass
        elif arguments.pending_only:
            pages = [path for path in pages if path in pending]
        elif arguments.retry_skipped:
            pages = [path for path in pages if path in incomplete]
        elif arguments.recorded_only:
            pages = [path for path in pages if path in recorded]
        elif not arguments.recheck_recorded:
            pages = [
                path
                for path in pages
                if path not in completed and path not in cross_corpus_duplicates
            ]
        profiles: dict[Path, SyntaxProfile] = {}
        if syntax_enabled:
            syntax_cache = read_syntax_cache(arguments.syntax_cache)
            profiles = syntax_profiles(
                all_pages,
                roots,
                page_records,
                corpus,
                arguments.syntax_profiler,
                arguments.syntax_timeout,
                syntax_cache,
            )
            if arguments.syntax_cache:
                write_syntax_cache(arguments.syntax_cache, syntax_cache)
            frequencies = feature_counts(all_pages, profiles)
            coverage = feature_counts(
                completed | set(cross_corpus_duplicates), profiles
            )
            pages = (
                rare_feature_sample_by_section(
                    pages,
                    arguments.max_pages_per_section,
                    arguments.seed,
                    profiles,
                    frequencies,
                    coverage,
                )
                if arguments.max_pages_per_section
                else rare_feature_sample(
                    pages,
                    arguments.max_pages,
                    arguments.seed,
                    profiles,
                    frequencies,
                    coverage,
                )
            )
            if arguments.syntax_report:
                write_syntax_report(
                    arguments.syntax_report,
                    roots,
                    corpus,
                    all_pages,
                    completed,
                    cross_corpus_duplicates,
                    pages,
                    page_records,
                    profiles,
                )
        else:
            pages = (
                stable_sample_by_section(
                    pages, arguments.max_pages_per_section, arguments.seed
                )
                if arguments.max_pages_per_section
                else stable_sample(pages, arguments.max_pages, arguments.seed)
            )
    except ValueError as error:
        print(f"audit-roff-fidelity: {error}", file=sys.stderr)
        return 2
    if arguments.syntax_report:
        print(f"syntax report: {arguments.syntax_report}")
    if arguments.syntax_cache:
        print(f"syntax cache:  {arguments.syntax_cache}")
    for path in source_filter_errors:
        print(
            "audit-roff-fidelity: source pattern could not inspect unreadable path: "
            f"{path}",
            file=sys.stderr,
        )
    if not pages and arguments.audit_db and (completed or cross_corpus_duplicates):
        print(
            "audit-roff-fidelity: no new or changed manual pages; "
            f"{len(completed)} unchanged pages are already complete and "
            f"{len(cross_corpus_duplicates)} sources were reused across corpora"
        )
        return 0
    if not pages:
        print("audit-roff-fidelity: no manual pages discovered", file=sys.stderr)
        return 2

    print("ManT roff fidelity audit")
    print(f"  mant:      {arguments.mant}")
    print(f"  reference: {arguments.reference}")
    print(f"  profile:   {arguments.reference_kind}/{reference_id}")
    print(f"  roots:     {', '.join(str(root) for root in roots)}")
    print(f"  pages:     {len(pages)}")
    if arguments.pages_file:
        print(f"  page set:  {arguments.pages_file}")
    if arguments.replay_source_records:
        print(
            f"  replay:    {len(replay_targets)} exact identities from "
            f"{arguments.source_ledger}"
        )
    if arguments.audit_db:
        print(
            f"  audit db:  {arguments.audit_db} "
            f"({len(completed)} completed pages skipped from {corpus}; "
            f"{len(incomplete)} historical skips selected again; "
            f"{len(cross_corpus_duplicates)} cross-corpus sources reused)"
        )
    if arguments.man_section:
        print(f"  sections:  {', '.join(arguments.man_section)}")
    if arguments.source_pattern:
        print(f"  source:    {' AND '.join(arguments.source_pattern)}")
    print(
        "  contract:  visible tokens, continuity, and source-conditioned punctuation; "
        "layout is intentionally ignored unless --layout-signals is selected"
    )
    print()

    findings: list[Finding] = []
    review_artifacts: list[tuple[str, AuditArtifact]] = []
    summary = AuditSummary()
    for path in pages:
        label, digest = page_records[path]
        artifact = audit_page(
            path,
            label,
            roots,
            arguments.mant,
            arguments.reference,
            arguments.reference_kind,
            arguments.timeout,
            arguments.ngram,
            arguments.layout_signals,
        )
        finding = artifact.finding
        findings.append(finding)
        if arguments.review_dir is not None:
            review_artifacts.append((label, artifact))
        update_summary(summary, finding)
        if (
            not arguments.findings_only
            or finding.status in {"review", "hard-failure"}
            or (finding.layout is not None and finding.layout.candidates)
        ):
            print_finding(finding, arguments.show)
        if arguments.audit_db and digest is not None:
            key = (corpus, label, digest)
            previous = database.get(key)
            database[key] = AuditRecord(
                corpus=corpus,
                path=label,
                section=manual_section(path) or "",
                digest=digest,
                scan_status=finding.status,
                review_status=merged_review_status(previous, finding.status),
                note=previous.note if previous is not None else "",
                reference_kind=arguments.reference_kind,
                reference_id=reference_id,
            )
            if summary.examined % arguments.checkpoint_every == 0:
                write_audit_database(
                    arguments.audit_db,
                    database.values(),
                    arguments.reference_kind,
                    reference_id,
                )

    print()
    print(
        "summary: "
        f"examined={summary.examined}, clean={summary.clean}, review={summary.review}, "
        f"hard={summary.hard_failures}, skipped={summary.skipped}"
    )
    print("REVIEW findings are candidates, not failures; confirm them before adding a Rust regression.")
    if arguments.json:
        write_json_report(
            arguments.json,
            roots,
            arguments.mant,
            arguments.reference,
            arguments.reference_kind,
            reference_id,
            arguments.layout_signals,
            summary,
            findings,
        )
        print(f"report: {arguments.json}")
    if arguments.review_dir:
        write_review_bundle(arguments.review_dir, review_artifacts)
        print(f"review bundle: {arguments.review_dir}")
    if arguments.audit_db:
        if arguments.reference_kind == "mandoc":
            after = mandoc_oracle.preflight(
                ROOT, Path(arguments.reference), arguments.oracle_archive.resolve(),
                arguments.oracle_attestation.resolve(), arguments.reference_id, "utf8",
            )
            if after != arguments.oracle_preflight:
                print("audit-roff-fidelity: oracle identity drifted during the audit", file=sys.stderr)
                return 2
        write_audit_database(
            arguments.audit_db,
            database.values(),
            arguments.reference_kind,
            reference_id,
        )
        print(f"audit database: {arguments.audit_db}")
    return 1 if summary.hard_failures else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
