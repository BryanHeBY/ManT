//! Existing bsd manual-root dialect policy; no host subprocesses.
use super::{
    MAX_EXPANDED_CONFIG_CANDIDATES, MAX_EXPANDED_CONFIG_PATHS, MAX_MANUAL_PATH_CONFIG_BYTES,
    ManualPathDiagnostic, ManualRootDiscovery, Path, PathBuf, ScanBudget, config_directive,
    config_lines, deduplicate_paths, expand_path_pattern_bounded, read_config, read_config_text,
};
pub(super) fn mandoc_configured_manual_roots(path: &Path) -> ManualRootDiscovery {
    load(path, false, MAX_EXPANDED_CONFIG_CANDIDATES)
}

pub(super) fn macos_configuration_roots(path: &Path) -> ManualRootDiscovery {
    load(path, true, MAX_EXPANDED_CONFIG_CANDIDATES)
}

fn finding(result: &mut ManualRootDiscovery, path: &Path, message: &str) {
    result.diagnostics.push(ManualPathDiagnostic {
        config_path: path.into(),
        line: None,
        message: message.into(),
    });
}

fn append(configuration: BsdManConfig, path: &Path, result: &mut ManualRootDiscovery) -> bool {
    result.roots.extend(configuration.paths);
    if configuration.truncated {
        finding(
            result,
            path,
            "manual-path expansion exhausted its shared work budget or path limit; incomplete patterns and later roots were omitted",
        );
    }
    configuration.truncated
}

fn load(path: &Path, bsd: bool, work: usize) -> ManualRootDiscovery {
    let mut result = ManualRootDiscovery::default();
    let Some(text) = read_config(path) else {
        return result;
    };
    let mut budget = ScanBudget::new(work);
    let mut bytes = 8 * MAX_MANUAL_PATH_CONFIG_BYTES - text.len() as u64;
    let configuration = parse(&text, &mut budget, bsd);
    let pattern = configuration
        .include_pattern
        .clone()
        .unwrap_or_else(|| PathBuf::from("/usr/local/etc/man.d/*.conf"));
    if append(configuration, path, &mut result) || !bsd {
        result.roots = deduplicate_paths(result.roots);
        return result;
    }
    let expansion = expand_path_pattern_bounded(&pattern, &mut budget);
    if expansion.exhausted {
        finding(
            &mut result,
            path,
            "MANCONFIG expansion exhausted its shared work budget or path limit; incomplete patterns and later fragments were omitted",
        );
    }
    if expansion.paths.len() > MAX_EXPANDED_CONFIG_PATHS {
        finding(
            &mut result,
            path,
            "MANCONFIG exceeds the 256-fragment limit; later fragments were omitted",
        );
    }
    for included in expansion.paths.into_iter().take(MAX_EXPANDED_CONFIG_PATHS) {
        if bytes == 0 || budget.remaining == 0 {
            finding(
                &mut result,
                path,
                "configuration tree byte or work budget exhausted; later fragments were omitted",
            );
            break;
        }
        let limit = bytes.min(MAX_MANUAL_PATH_CONFIG_BYTES);
        let Ok(text) = read_config_text(&included, limit) else {
            bytes -= limit;
            finding(
                &mut result,
                &included,
                "manual-path configuration fragment is not readable regular UTF-8 within its byte budget",
            );
            continue;
        };
        bytes -= text.len() as u64;
        if append(parse(&text, &mut budget, true), &included, &mut result) {
            break;
        }
    }
    result.roots = deduplicate_paths(result.roots);
    result
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct BsdManConfig {
    pub(super) paths: Vec<PathBuf>,
    pub(super) include_pattern: Option<PathBuf>,
    pub(super) truncated: bool,
}

#[cfg(test)]
pub(super) fn parse_bsd_man_config(text: &str) -> BsdManConfig {
    parse(
        text,
        &mut ScanBudget::new(MAX_EXPANDED_CONFIG_CANDIDATES),
        true,
    )
}

fn parse(text: &str, budget: &mut ScanBudget, bsd: bool) -> BsdManConfig {
    let mut configuration = BsdManConfig::default();
    for line in config_lines(text) {
        if !budget.charge() {
            configuration.truncated = true;
            break;
        }
        let Some((directive, value)) = config_directive(line) else {
            continue;
        };
        if directive == "manpath" || (bsd && directive == "MANPATH") {
            let expansion = expand_path_pattern_bounded(Path::new(value), budget);
            configuration.paths.extend(expansion.paths);
            if expansion.exhausted {
                configuration.truncated = true;
                break;
            }
        } else if bsd && directive == "MANCONFIG" {
            configuration.include_pattern = Some(PathBuf::from(value));
        }
    }
    configuration
}

#[cfg(test)]
pub(super) fn parse_mandoc_manpaths(text: &str) -> Vec<PathBuf> {
    parse(
        text,
        &mut ScanBudget::new(MAX_EXPANDED_CONFIG_CANDIDATES),
        false,
    )
    .paths
}

#[cfg(test)]
mod tests;
