//! Compile-time CLI capabilities. Protocol schemas intentionally stay complete.

// In an all-feature build every arm happens to be true; retaining the individual
// cfg expressions is essential to the single-feature and no-default contracts.
#[allow(clippy::match_like_matches_macro)]
pub(super) const fn enabled(feature: &str) -> bool {
    // A normal match (rather than environment inspection) keeps help deterministic.
    match feature.as_bytes() {
        b"roff" => cfg!(feature = "roff"),
        b"tui" => cfg!(feature = "tui"),
        b"pager" => cfg!(feature = "pager"),
        b"mcp" => cfg!(feature = "mcp"),
        b"update" => cfg!(feature = "update"),
        _ => false,
    }
}

/// Filter clap's relationships together with their corresponding skipped fields.
/// A disabled argument must neither parse nor remain as a dangling group ID.
pub(super) fn argument_ids<const N: usize>(ids: [&'static str; N]) -> Vec<&'static str> {
    ids.into_iter()
        .filter(|id| match *id {
            "manual" | "man_section" => enabled("roff"),
            "mcp" => enabled("mcp"),
            "update_docs" | "update_tldr" | "prune_docs" | "dry_run" => enabled("update"),
            _ => true,
        })
        .collect()
}

/// Explicit action metadata preserves the full build's established synopsis.
pub(super) fn usage() -> String {
    let mut lines = vec!["mant <SELECTOR> [OPTIONS]"];
    if enabled("roff") {
        lines.push("mant <MAN_SECTION> <NAME> [OPTIONS]");
    }
    lines.extend([
        "mant --document <SELECTOR>... [--follow-links] [OPTIONS]",
        "mant --input <PATH|-> [--input-format <FORMAT>] [OPTIONS]",
        "mant --list [FILTERS]",
        "mant --find <PATTERN> [FILTERS]",
        "mant --request-json [--format <FORMAT>] [--compact]",
        "mant --doctor [--format <text|json>] [--compact]",
        "mant --schema <CONTRACT> [--compact]",
    ]);
    if enabled("update") {
        lines.extend([
            "mant --update-docs [--compact]",
            "mant --prune-docs [--dry-run] [--compact]",
            "mant --update-tldr [--compact]",
        ]);
    }
    lines.push("mant --protocol-version [--compact]");
    if enabled("mcp") {
        lines.push("mant --mcp");
    }
    lines.join("\n       ")
}

#[cfg(test)]
mod tests;
