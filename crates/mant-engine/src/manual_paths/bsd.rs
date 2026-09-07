//! Existing bsd manual-root dialect policy; no host subprocesses.
use super::{
    MAX_EXPANDED_CONFIG_CANDIDATES, MAX_EXPANDED_CONFIG_PATHS, MAX_MANUAL_PATH_CONFIG_BYTES, Path,
    PathBuf, ScanBudget, config_directive, config_lines, deduplicate_paths,
    expand_path_pattern_bounded, read_config, read_config_text,
};
pub(super) fn mandoc_configured_manual_roots(path: &Path) -> Vec<PathBuf> {
    read_config(path)
        .map(|text| parse_mandoc_manpaths(&text))
        .map(deduplicate_paths)
        .unwrap_or_default()
}

pub(super) fn macos_configuration_roots(path: &Path) -> Vec<PathBuf> {
    let Some(text) = read_config(path) else {
        return Vec::new();
    };
    let mut budget = ScanBudget::new(MAX_EXPANDED_CONFIG_CANDIDATES);
    let mut bytes = 8 * MAX_MANUAL_PATH_CONFIG_BYTES - text.len() as u64;
    let configuration = parse_bsd_man_config_bounded(&text, &mut budget);
    let mut roots = configuration.paths;
    let pattern = configuration
        .include_pattern
        .unwrap_or_else(|| PathBuf::from("/usr/local/etc/man.d/*.conf"));
    for included in expand_path_pattern_bounded(&pattern, &mut budget)
        .paths
        .into_iter()
        .take(MAX_EXPANDED_CONFIG_PATHS)
    {
        if bytes == 0 || budget.remaining == 0 {
            break;
        }
        let limit = bytes.min(MAX_MANUAL_PATH_CONFIG_BYTES);
        let Ok(text) = read_config_text(&included, limit) else {
            bytes -= limit;
            continue;
        };
        bytes -= text.len() as u64;
        roots.extend(parse_bsd_man_config_bounded(&text, &mut budget).paths);
    }
    deduplicate_paths(roots)
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct BsdManConfig {
    pub(super) paths: Vec<PathBuf>,
    pub(super) include_pattern: Option<PathBuf>,
}

#[cfg(test)]
pub(super) fn parse_bsd_man_config(text: &str) -> BsdManConfig {
    parse_bsd_man_config_bounded(text, &mut ScanBudget::new(MAX_EXPANDED_CONFIG_CANDIDATES))
}

pub(super) fn parse_bsd_man_config_bounded(text: &str, budget: &mut ScanBudget) -> BsdManConfig {
    let mut configuration = BsdManConfig::default();
    for line in config_lines(text) {
        if !budget.charge() {
            break;
        }
        let Some((directive, value)) = config_directive(line) else {
            continue;
        };
        match directive {
            "MANPATH" | "manpath" => configuration
                .paths
                .extend(expand_path_pattern_bounded(Path::new(value), budget).paths),
            "MANCONFIG" => configuration.include_pattern = Some(PathBuf::from(value)),
            _ => {}
        }
    }
    configuration
}

pub(super) fn parse_mandoc_manpaths(text: &str) -> Vec<PathBuf> {
    let mut budget = ScanBudget::new(MAX_EXPANDED_CONFIG_CANDIDATES);
    config_lines(text)
        .take(MAX_EXPANDED_CONFIG_CANDIDATES)
        .filter_map(config_directive)
        .filter_map(|(directive, value)| (directive == "manpath").then_some(value))
        .flat_map(|path| expand_path_pattern_bounded(Path::new(path), &mut budget).paths)
        .collect()
}
