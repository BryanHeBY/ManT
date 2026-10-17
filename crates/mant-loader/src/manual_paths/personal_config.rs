//! Portable personal `man.conf` parsing; native system dialects remain separate.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

use super::path_rules::{config_path_key, deduplicate_windows_paths, windows_paths_equivalent};
use super::{
    ExpansionOutcome, MAX_EXPANDED_CONFIG_CANDIDATES, MAX_EXPANDED_CONFIG_PATHS,
    MAX_MANUAL_PATH_CONFIG_BYTES, ManualPathDiagnostic, ManualRootDiscovery, ScanBudget,
    expansion::{GlobCase, expand_path_pattern_with_case},
};
use mant_sources::settings::is_absolute_configuration_path;

const MAX_CONFIG_TREE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_CONFIG_LINES: usize = 4096;

struct ConfigBudget {
    bytes: u64,
    lines: usize,
    windows: bool,
}

impl Default for ConfigBudget {
    fn default() -> Self {
        Self {
            bytes: MAX_CONFIG_TREE_BYTES,
            lines: MAX_CONFIG_LINES,
            windows: true,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct PersonalConfigPlan {
    roots: Vec<PathBuf>,
    mappings: Vec<(PathBuf, PathBuf)>,
    mandatory: Vec<PathBuf>,
    include_patterns: Vec<PathBuf>,
    diagnostics: Vec<ManualPathDiagnostic>,
    windows: bool,
}

pub(super) fn load_personal(
    path: &Path,
    environment: &HashMap<OsString, OsString>,
    executable_paths: &[PathBuf],
    windows: bool,
) -> ManualRootDiscovery {
    let mut budget = ConfigBudget {
        windows,
        ..ConfigBudget::default()
    };
    let mut diagnostics = Vec::new();
    let Some(text) = read_config(path, &mut diagnostics, &mut budget) else {
        return ManualRootDiscovery {
            roots: Vec::new(),
            diagnostics,
        };
    };
    let mut plan = parse_bounded(&text, path, environment, true, &mut budget);
    let included = collect_include_paths_with_case(
        &plan.include_patterns,
        |pattern, budget| {
            expand_path_pattern_with_case(
                pattern,
                budget,
                if windows {
                    GlobCase::AsciiInsensitive
                } else {
                    GlobCase::Sensitive
                },
            )
        },
        windows,
    );
    if included.candidate_truncated {
        plan.diagnostics.push(file_diagnostic(
            path,
            &format!("MANCONFIG expansion exceeds its {MAX_EXPANDED_CONFIG_CANDIDATES}-step work budget or 4096-unit path limit; incomplete patterns were discarded and later patterns were not traversed"),
        ));
    }
    if included.fragment_truncated {
        plan.diagnostics.push(file_diagnostic(
            path,
            &format!("MANCONFIG expansion exceeds the {MAX_EXPANDED_CONFIG_PATHS}-unique-fragment limit; remaining matches and patterns were not read"),
        ));
    }

    for included_path in included.paths {
        if budget.lines == 0 || budget.bytes == 0 {
            plan.diagnostics.push(file_diagnostic(
                path,
                "configuration tree budget exhausted; remaining fragments were not read",
            ));
            break;
        }
        let mut diagnostics = Vec::new();
        if let Some(text) = read_config(&included_path, &mut diagnostics, &mut budget) {
            let fragment = parse_bounded(&text, &included_path, environment, false, &mut budget);
            plan.roots.extend(fragment.roots);
            plan.mappings.extend(fragment.mappings);
            plan.mandatory.extend(fragment.mandatory);
            plan.diagnostics.extend(fragment.diagnostics);
        }
        plan.diagnostics.extend(diagnostics);
    }

    materialize(plan, executable_paths)
}

fn read_config(
    path: &Path,
    diagnostics: &mut Vec<ManualPathDiagnostic>,
    budget: &mut ConfigBudget,
) -> Option<String> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
        Err(_) => {
            diagnostics.push(file_diagnostic(
                path,
                "manual-path configuration is unreadable",
            ));
            return None;
        }
    };
    if !metadata.is_file() {
        diagnostics.push(file_diagnostic(
            path,
            "manual-path configuration is not a regular file",
        ));
        return None;
    }
    if metadata.len() > MAX_MANUAL_PATH_CONFIG_BYTES {
        diagnostics.push(file_diagnostic(
            path,
            "manual-path configuration exceeds the 1 MiB limit",
        ));
        return None;
    }
    if let Ok(text) = super::read_config_text(path, MAX_MANUAL_PATH_CONFIG_BYTES.min(budget.bytes))
    {
        budget.bytes -= text.len() as u64;
        Some(text)
    } else {
        // Conservatively charge a failed read too: an invalid UTF-8 file or a
        // growing file must not grant subsequent fragments a fresh budget.
        budget.bytes = budget.bytes.saturating_sub(MAX_MANUAL_PATH_CONFIG_BYTES);
        diagnostics.push(file_diagnostic(
            path,
            "manual-path configuration is not readable UTF-8 text within the remaining read budget",
        ));
        None
    }
}

fn file_diagnostic(path: &Path, message: &str) -> ManualPathDiagnostic {
    ManualPathDiagnostic {
        config_path: path.to_path_buf(),
        line: None,
        message: message.to_owned(),
    }
}

#[cfg(test)]
fn parse(
    text: &str,
    source: &Path,
    environment: &HashMap<OsString, OsString>,
    allow_includes: bool,
) -> PersonalConfigPlan {
    parse_bounded(
        text,
        source,
        environment,
        allow_includes,
        &mut ConfigBudget::default(),
    )
}

fn parse_bounded(
    text: &str,
    source: &Path,
    environment: &HashMap<OsString, OsString>,
    allow_includes: bool,
    budget: &mut ConfigBudget,
) -> PersonalConfigPlan {
    let environment = environment
        .iter()
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut plan = PersonalConfigPlan {
        windows: budget.windows,
        ..PersonalConfigPlan::default()
    };
    for (index, raw_line) in text.lines().enumerate() {
        if budget.lines == 0 {
            plan.diagnostics.push(line_diagnostic(
                source,
                index + 1,
                "configuration tree exceeds the 4096-line limit; remaining directives were omitted",
            ));
            break;
        }
        budget.lines -= 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (directive, value) = directive_and_value(line);
        if !is_known_directive(directive) {
            continue;
        }
        let line = index + 1;
        if value.is_empty() {
            plan.diagnostics.push(line_diagnostic(
                source,
                line,
                "directive has the wrong number of path arguments",
            ));
            continue;
        }
        if directive.eq_ignore_ascii_case("manpath") {
            push_single_path(
                &mut plan.roots,
                &mut plan.diagnostics,
                source,
                line,
                value,
                &environment,
                budget.windows,
            );
        } else if directive.eq_ignore_ascii_case("mandatory_manpath") {
            push_single_path(
                &mut plan.mandatory,
                &mut plan.diagnostics,
                source,
                line,
                value,
                &environment,
                budget.windows,
            );
        } else if directive.eq_ignore_ascii_case("manconfig") {
            if allow_includes {
                push_single_path(
                    &mut plan.include_patterns,
                    &mut plan.diagnostics,
                    source,
                    line,
                    value,
                    &environment,
                    budget.windows,
                );
            }
        } else if directive.eq_ignore_ascii_case("manpath_map") {
            match split_arguments(value, 2) {
                Ok(arguments) => {
                    let binary = parse_path(&arguments[0], &environment, budget.windows);
                    let manual = parse_path(&arguments[1], &environment, budget.windows);
                    match binary.zip(manual) {
                        Some(mapping) => plan.mappings.push(mapping),
                        None => plan.diagnostics.push(line_diagnostic(
                            source,
                            line,
                            "MANPATH_MAP contains an undefined variable or non-absolute path",
                        )),
                    }
                }
                Err(message) => plan
                    .diagnostics
                    .push(line_diagnostic(source, line, message)),
            }
        }
    }
    plan
}

fn directive_and_value(line: &str) -> (&str, &str) {
    line.split_once(char::is_whitespace)
        .map_or((line, ""), |(directive, value)| (directive, value.trim()))
}

fn is_known_directive(directive: &str) -> bool {
    ["manpath", "mandatory_manpath", "manconfig", "manpath_map"]
        .iter()
        .any(|known| directive.eq_ignore_ascii_case(known))
}

#[derive(Debug, Default, Eq, PartialEq)]
struct IncludedPaths {
    paths: Vec<PathBuf>,
    candidate_truncated: bool,
    fragment_truncated: bool,
}

#[cfg(test)]
fn collect_include_paths(patterns: &[PathBuf]) -> IncludedPaths {
    collect_include_paths_with(patterns, |pattern, budget| {
        expand_path_pattern_with_case(pattern, budget, GlobCase::AsciiInsensitive)
    })
}

#[cfg(test)]
fn collect_include_paths_with(
    patterns: &[PathBuf],
    expand: impl FnMut(&Path, &mut ScanBudget) -> ExpansionOutcome,
) -> IncludedPaths {
    collect_include_paths_with_case(patterns, expand, true)
}

fn collect_include_paths_with_case(
    patterns: &[PathBuf],
    mut expand: impl FnMut(&Path, &mut ScanBudget) -> ExpansionOutcome,
    windows: bool,
) -> IncludedPaths {
    let mut included = Vec::new();
    let mut seen = HashSet::new();
    let mut budget = ScanBudget::new(MAX_EXPANDED_CONFIG_CANDIDATES);
    let mut candidate_truncated = false;
    let mut fragment_truncated = false;
    for (index, pattern) in patterns.iter().enumerate() {
        if included.len() == MAX_EXPANDED_CONFIG_PATHS {
            fragment_truncated = true;
            break;
        }
        if budget.remaining == 0 {
            candidate_truncated = true;
            break;
        }
        let ExpansionOutcome {
            paths: expanded,
            exhausted: pattern_truncated,
        } = expand(pattern, &mut budget);
        for included_path in expanded {
            if !seen.insert(config_path_key(&included_path, windows)) {
                continue;
            }
            if included.len() == MAX_EXPANDED_CONFIG_PATHS {
                fragment_truncated = true;
                break;
            }
            included.push(included_path);
        }
        if fragment_truncated {
            break;
        }
        if pattern_truncated {
            candidate_truncated = true;
            break;
        }
        if budget.remaining == 0 && index + 1 < patterns.len() {
            candidate_truncated = true;
            break;
        }
        if included.len() == MAX_EXPANDED_CONFIG_PATHS && index + 1 < patterns.len() {
            fragment_truncated = true;
            break;
        }
    }
    if windows {
        included.sort_unstable_by_key(|path| config_path_key(path, true));
    } else {
        included.sort_unstable();
    }
    IncludedPaths {
        paths: included,
        candidate_truncated,
        fragment_truncated,
    }
}

fn push_single_path(
    target: &mut Vec<PathBuf>,
    diagnostics: &mut Vec<ManualPathDiagnostic>,
    source: &Path,
    line: usize,
    value: &str,
    environment: &BTreeMap<OsString, OsString>,
    windows: bool,
) {
    match split_arguments(value, 1)
        .ok()
        .and_then(|arguments| parse_path(&arguments[0], environment, windows))
    {
        Some(path) => target.push(path),
        None => diagnostics.push(line_diagnostic(
            source,
            line,
            "path contains invalid quoting/expansion, unavailable home/variable, or is not absolute",
        )),
    }
}

fn line_diagnostic(path: &Path, line: usize, message: &str) -> ManualPathDiagnostic {
    ManualPathDiagnostic {
        config_path: path.to_path_buf(),
        line: Some(line),
        message: message.to_owned(),
    }
}

fn split_arguments(value: &str, expected: usize) -> Result<Vec<String>, &'static str> {
    if expected == 1 && !value.starts_with('"') {
        if value.contains('"') {
            return Err("an unquoted path contains a double quote");
        }
        return Ok(vec![value.to_owned()]);
    }

    let mut arguments = Vec::new();
    let mut remaining = value.trim();
    while !remaining.is_empty() {
        if let Some(quoted) = remaining.strip_prefix('"') {
            let Some(end) = quoted.find('"') else {
                return Err("a quoted path is missing its closing double quote");
            };
            arguments.push(quoted[..end].to_owned());
            remaining = &quoted[end + 1..];
            if !remaining.is_empty() && !remaining.chars().next().is_some_and(char::is_whitespace) {
                return Err("unexpected text follows a quoted path");
            }
        } else {
            let end = remaining
                .find(char::is_whitespace)
                .unwrap_or(remaining.len());
            let argument = &remaining[..end];
            if argument.contains('"') {
                return Err("an unquoted path contains a double quote");
            }
            arguments.push(argument.to_owned());
            remaining = &remaining[end..];
        }
        remaining = remaining.trim_start();
    }
    if arguments.len() != expected || arguments.iter().any(String::is_empty) {
        return Err("directive has the wrong number of path arguments");
    }
    Ok(arguments)
}

fn parse_path(
    value: &str,
    environment: &BTreeMap<OsString, OsString>,
    windows: bool,
) -> Option<PathBuf> {
    let expanded =
        mant_sources::settings::expand_configuration_path(value, environment, windows).ok()?;
    is_absolute_configuration_path(&expanded, windows).then_some(expanded)
}

fn materialize(plan: PersonalConfigPlan, executable_paths: &[PathBuf]) -> ManualRootDiscovery {
    // All root directives are literal; only MANCONFIG accepts patterns.
    let mut roots = plan.roots;
    for executable in executable_paths {
        roots.extend(
            plan.mappings
                .iter()
                .filter(|(configured, _)| {
                    if plan.windows {
                        windows_paths_equivalent(configured, executable)
                    } else {
                        configured == executable
                    }
                })
                .map(|(_, manual)| manual.clone()),
        );
    }
    roots.extend(plan.mandatory);
    ManualRootDiscovery {
        roots: if plan.windows {
            deduplicate_windows_paths(roots)
        } else {
            crate::source::deduplicate_paths(roots)
        },
        diagnostics: plan.diagnostics,
    }
}

#[cfg(test)]
mod tests;
