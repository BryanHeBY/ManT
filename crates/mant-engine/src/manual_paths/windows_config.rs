//! ManT-owned Windows `man.conf` parsing and root materialization.

use std::{
    collections::{HashMap, HashSet},
    ffi::{OsStr, OsString},
    fs, io,
    path::{Path, PathBuf},
};

use super::{
    MAX_EXPANDED_CONFIG_PATHS, MAX_MANUAL_PATH_CONFIG_BYTES, ManualPathDiagnostic,
    ManualRootDiscovery, config_directive, expand_path_pattern,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct WindowsConfigPlan {
    roots: Vec<PathBuf>,
    mappings: Vec<(PathBuf, PathBuf)>,
    mandatory: Vec<PathBuf>,
    include_patterns: Vec<PathBuf>,
    diagnostics: Vec<ManualPathDiagnostic>,
}

pub(super) fn load(
    path: &Path,
    environment: &HashMap<OsString, OsString>,
    executable_paths: &[PathBuf],
) -> ManualRootDiscovery {
    let mut diagnostics = Vec::new();
    let Some(text) = read_config(path, &mut diagnostics) else {
        return ManualRootDiscovery {
            roots: Vec::new(),
            diagnostics,
        };
    };
    let mut plan = parse(&text, path, environment, true);
    let mut included = Vec::new();
    let mut seen = HashSet::new();
    for pattern in &plan.include_patterns {
        for included_path in expand_path_pattern(pattern) {
            if included.len() >= MAX_EXPANDED_CONFIG_PATHS {
                break;
            }
            if seen.insert(normalized_windows_path(&included_path)) {
                included.push(included_path);
            }
        }
    }
    included.sort_unstable_by_key(|path| normalized_windows_path(path));

    for included_path in included {
        let mut diagnostics = Vec::new();
        if let Some(text) = read_config(&included_path, &mut diagnostics) {
            let fragment = parse(&text, &included_path, environment, false);
            plan.roots.extend(fragment.roots);
            plan.mappings.extend(fragment.mappings);
            plan.mandatory.extend(fragment.mandatory);
            plan.diagnostics.extend(fragment.diagnostics);
        }
        plan.diagnostics.extend(diagnostics);
    }

    materialize(plan, executable_paths)
}

fn read_config(path: &Path, diagnostics: &mut Vec<ManualPathDiagnostic>) -> Option<String> {
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
    if let Ok(text) = fs::read_to_string(path) {
        Some(text)
    } else {
        diagnostics.push(file_diagnostic(
            path,
            "manual-path configuration is not readable UTF-8 text",
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

fn parse(
    text: &str,
    source: &Path,
    environment: &HashMap<OsString, OsString>,
    allow_includes: bool,
) -> WindowsConfigPlan {
    let mut plan = WindowsConfigPlan::default();
    for (index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((directive, value)) = config_directive(line) else {
            continue;
        };
        let line = index + 1;
        if directive.eq_ignore_ascii_case("manpath") {
            push_single_path(
                &mut plan.roots,
                &mut plan.diagnostics,
                source,
                line,
                value,
                environment,
            );
        } else if directive.eq_ignore_ascii_case("mandatory_manpath") {
            push_single_path(
                &mut plan.mandatory,
                &mut plan.diagnostics,
                source,
                line,
                value,
                environment,
            );
        } else if directive.eq_ignore_ascii_case("manconfig") {
            if allow_includes {
                push_single_path(
                    &mut plan.include_patterns,
                    &mut plan.diagnostics,
                    source,
                    line,
                    value,
                    environment,
                );
            }
        } else if directive.eq_ignore_ascii_case("manpath_map") {
            match split_arguments(value, 2) {
                Ok(arguments) => {
                    let binary = parse_path(&arguments[0], environment);
                    let manual = parse_path(&arguments[1], environment);
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

fn push_single_path(
    target: &mut Vec<PathBuf>,
    diagnostics: &mut Vec<ManualPathDiagnostic>,
    source: &Path,
    line: usize,
    value: &str,
    environment: &HashMap<OsString, OsString>,
) {
    match split_arguments(value, 1)
        .ok()
        .and_then(|arguments| parse_path(&arguments[0], environment))
    {
        Some(path) => target.push(path),
        None => diagnostics.push(line_diagnostic(
            source,
            line,
            "path contains invalid quoting, an undefined variable, or is not absolute",
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

fn parse_path(value: &str, environment: &HashMap<OsString, OsString>) -> Option<PathBuf> {
    let expanded = expand_environment(value, environment)?;
    is_absolute_windows_path(&expanded).then(|| PathBuf::from(expanded))
}

fn expand_environment(value: &str, environment: &HashMap<OsString, OsString>) -> Option<OsString> {
    let mut output = OsString::new();
    let mut remaining = value;
    while let Some(start) = remaining.find('%') {
        output.push(&remaining[..start]);
        remaining = &remaining[start + 1..];
        if let Some(literal) = remaining.strip_prefix('%') {
            output.push("%");
            remaining = literal;
            continue;
        }
        let end = remaining.find('%')?;
        let name = &remaining[..end];
        if name.is_empty() {
            return None;
        }
        let value = environment.iter().find_map(|(candidate, value)| {
            candidate
                .to_string_lossy()
                .eq_ignore_ascii_case(name)
                .then_some(value)
        })?;
        output.push(value);
        remaining = &remaining[end + 1..];
    }
    output.push(remaining);
    Some(output)
}

fn is_absolute_windows_path(path: &OsStr) -> bool {
    let value = path.to_string_lossy();
    let bytes = value.as_bytes();
    bytes.get(..3).is_some_and(|prefix| {
        prefix[0].is_ascii_alphabetic() && prefix[1] == b':' && is_separator(prefix[2])
    }) || bytes
        .get(..2)
        .is_some_and(|prefix| is_separator(prefix[0]) && is_separator(prefix[1]))
}

const fn is_separator(byte: u8) -> bool {
    byte == b'\\' || byte == b'/'
}

fn materialize(plan: WindowsConfigPlan, executable_paths: &[PathBuf]) -> ManualRootDiscovery {
    let mut roots = plan
        .roots
        .iter()
        .flat_map(|path| expand_path_pattern(path))
        .collect::<Vec<_>>();
    for executable in executable_paths {
        roots.extend(
            plan.mappings
                .iter()
                .filter(|(configured, _)| windows_paths_equivalent(configured, executable))
                .map(|(_, manual)| manual.clone()),
        );
    }
    roots.extend(plan.mandatory);
    ManualRootDiscovery {
        roots: deduplicate_windows_paths(roots),
        diagnostics: plan.diagnostics,
    }
}

fn windows_paths_equivalent(left: &Path, right: &Path) -> bool {
    normalized_windows_path(left) == normalized_windows_path(right)
        || fs::canonicalize(left)
            .ok()
            .zip(fs::canonicalize(right).ok())
            .is_some_and(|(left, right)| {
                normalized_windows_path(&left) == normalized_windows_path(&right)
            })
}

fn normalized_windows_path(path: &Path) -> String {
    path.as_os_str()
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

fn deduplicate_windows_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(normalized_windows_path(path)))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, ffi::OsString, path::PathBuf};

    use super::{materialize, parse, split_arguments};

    #[test]
    fn single_paths_preserve_spaces_and_accept_one_optional_double_quote_pair() {
        let environment = HashMap::from([(
            OsString::from("ProgramFiles"),
            OsString::from(r"C:\Program Files"),
        )]);
        let plan = parse(
            "manpath C:\\Manual Trees\\plain\nMANPATH \"%PROGRAMFILES%\\Tool\\man\"\n",
            PathBuf::from(r"C:\config\man.conf").as_path(),
            &environment,
            true,
        );
        assert_eq!(
            plan.roots,
            vec![
                PathBuf::from(r"C:\Manual Trees\plain"),
                PathBuf::from(r"C:\Program Files\Tool\man")
            ]
        );
        assert!(plan.diagnostics.is_empty());
    }

    #[test]
    fn environment_expansion_is_case_insensitive_single_pass_and_supports_literal_percent() {
        let environment = HashMap::from([
            (OsString::from("ROOT"), OsString::from(r"D:\Manuals")),
            (OsString::from("NESTED"), OsString::from("%ROOT%")),
        ]);
        let plan = parse(
            "manpath %root%\\one\nmanpath C:\\100%%\\man\nmanpath %NESTED%\\two\n",
            PathBuf::from(r"C:\config\man.conf").as_path(),
            &environment,
            true,
        );
        assert_eq!(
            plan.roots,
            vec![
                PathBuf::from(r"D:\Manuals\one"),
                PathBuf::from(r"C:\100%\man")
            ]
        );
        assert_eq!(plan.diagnostics.len(), 1);
    }

    #[test]
    fn path_maps_require_two_arguments_and_match_windows_paths() {
        let environment = HashMap::new();
        let source = PathBuf::from(r"C:\config\man.conf");
        let plan = parse(
            "manpath C:\\direct\nMANPATH_MAP \"C:\\Program Files\\Tool\\bin\" \"D:\\Tool Manuals\"\nMANDATORY_MANPATH C:\\required\n",
            &source,
            &environment,
            true,
        );
        let discovery = materialize(plan, &[PathBuf::from("c:/program files/tool/bin/")]);
        assert_eq!(
            discovery.roots,
            vec![
                PathBuf::from(r"C:\direct"),
                PathBuf::from(r"D:\Tool Manuals"),
                PathBuf::from(r"C:\required")
            ]
        );
        assert!(discovery.diagnostics.is_empty());

        assert!(split_arguments(r"C:\one C:\two C:\three", 2).is_err());
    }

    #[test]
    fn invalid_windows_paths_are_omitted_with_source_lines() {
        let plan = parse(
            "manpath relative\\man\nmanpath %MISSING%\\man\nmanpath \"C:\\unterminated\nMANPATH_MAP C:\\only-one\n",
            PathBuf::from(r"C:\config\man.conf").as_path(),
            &HashMap::new(),
            true,
        );
        assert!(plan.roots.is_empty());
        assert_eq!(
            plan.diagnostics
                .iter()
                .filter_map(|diagnostic| diagnostic.line)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
    }

    #[test]
    fn fragment_includes_are_not_recursive() {
        let environment = HashMap::new();
        let source = PathBuf::from(r"C:\config\fragment.conf");
        let plan = parse(
            "MANCONFIG C:\\nested\\*.conf\nMANPATH C:\\from-fragment\n",
            &source,
            &environment,
            false,
        );
        assert!(plan.include_patterns.is_empty());
        assert_eq!(plan.roots, vec![PathBuf::from(r"C:\from-fragment")]);
    }

    #[cfg(windows)]
    #[test]
    fn windows_config_file_orders_direct_fragment_mapped_and_mandatory_roots() {
        use std::fs;

        let fixture =
            std::env::temp_dir().join(format!("mant-windows-man-conf-{}", std::process::id()));
        let _ = fs::remove_dir_all(&fixture);
        let fragments = fixture.join("man.d");
        fs::create_dir_all(&fragments).expect("create configuration fragments");

        let environment = HashMap::from([(
            OsString::from("MANT_TEST_ROOT"),
            fixture.as_os_str().to_owned(),
        )]);
        fs::write(
            fixture.join("man.conf"),
            "manpath \"%MANT_TEST_ROOT%\\direct root\"\n\
             MANCONFIG \"%MANT_TEST_ROOT%\\man.d\\*.conf\"\n\
             MANPATH_MAP \"%MANT_TEST_ROOT%\\bin path\" \"%MANT_TEST_ROOT%\\mapped root\"\n\
             MANDATORY_MANPATH \"%MANT_TEST_ROOT%\\required root\"\n",
        )
        .expect("write main configuration");
        fs::write(
            fragments.join("10-tool.conf"),
            "MANPATH \"%MANT_TEST_ROOT%\\fragment root\"\n\
             MANCONFIG \"%MANT_TEST_ROOT%\\nested\\*.conf\"\n",
        )
        .expect("write configuration fragment");

        let discovery = super::load(
            &fixture.join("man.conf"),
            &environment,
            &[fixture.join("BIN PATH")],
        );
        assert_eq!(
            discovery.roots,
            vec![
                fixture.join("direct root"),
                fixture.join("fragment root"),
                fixture.join("mapped root"),
                fixture.join("required root"),
            ]
        );
        assert!(discovery.diagnostics.is_empty());

        fs::remove_dir_all(fixture).expect("remove configuration fixture");
    }
}
