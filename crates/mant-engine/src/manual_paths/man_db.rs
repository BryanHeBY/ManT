//! Existing man-db manual-root dialect policy; no host subprocesses.
use super::{
    HashMap, OsString, Path, PathBuf, deduplicate_paths, env, environment_value, fs, read_config,
};
use super::{bsd::mandoc_configured_manual_roots, config_directive, config_lines};
pub(super) fn linux_configured_manual_roots(
    environment: &HashMap<OsString, OsString>,
) -> Vec<PathBuf> {
    let user_config = environment_value(environment, "HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".manpath"));
    let system_configurations = [
        PathBuf::from("/etc/man_db.conf"),
        PathBuf::from("/etc/manpath.config"),
        PathBuf::from("/usr/local/etc/man_db.conf"),
    ];
    let configuration = user_config.filter(|path| path.is_file()).or_else(|| {
        system_configurations
            .into_iter()
            .find(|path| path.is_file())
    });
    if let Some(configuration) = configuration {
        let config = read_config(&configuration)
            .map(|text| parse_man_db_config(&text))
            .unwrap_or_default();
        let roots = man_db_manual_roots(environment, &config);
        if !roots.is_empty() {
            return roots;
        }
    }
    mandoc_configured_manual_roots(Path::new("/etc/man.conf"))
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ManDbConfig {
    pub(super) mappings: Vec<(PathBuf, PathBuf)>,
    pub(super) mandatory: Vec<PathBuf>,
}

pub(super) fn parse_man_db_config(text: &str) -> ManDbConfig {
    let mut configuration = ManDbConfig::default();
    for line in config_lines(text) {
        let Some((directive, value)) = config_directive(line) else {
            continue;
        };
        match directive {
            "MANPATH_MAP" => {
                let mut fields = value.split_whitespace();
                if let Some((binary, manual)) = fields.next().zip(fields.next()) {
                    configuration
                        .mappings
                        .push((PathBuf::from(binary), PathBuf::from(manual)));
                }
            }
            "MANDATORY_MANPATH" => {
                if let Some(manual) = value.split_whitespace().next() {
                    configuration.mandatory.push(PathBuf::from(manual));
                }
            }
            _ => {}
        }
    }
    configuration
}

pub(super) fn man_db_manual_roots(
    environment: &HashMap<OsString, OsString>,
    configuration: &ManDbConfig,
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(path) = environment_value(environment, "PATH") {
        for binary in env::split_paths(path) {
            let mapped = configuration
                .mappings
                .iter()
                .filter(|(configured, _)| paths_equivalent(configured, &binary))
                .map(|(_, manual)| manual.clone())
                .collect::<Vec<_>>();
            if mapped.is_empty() {
                roots.extend(
                    unmapped_man_db_roots(&binary)
                        .into_iter()
                        .filter(|candidate| candidate.is_dir()),
                );
            } else {
                roots.extend(mapped);
            }
        }
    }
    roots.extend(configuration.mandatory.iter().cloned());
    expand_man_db_systems(roots, environment)
}

pub(super) fn paths_equivalent(left: &Path, right: &Path) -> bool {
    left == right
        || fs::canonicalize(left)
            .ok()
            .zip(fs::canonicalize(right).ok())
            .is_some_and(|(left, right)| left == right)
}

pub(super) fn unmapped_man_db_roots(binary: &Path) -> Vec<PathBuf> {
    let mut roots = vec![binary.join("man"), binary.join("share/man")];
    if let Some(prefix) = binary.parent() {
        roots.insert(0, prefix.join("man"));
        roots.insert(2, prefix.join("share/man"));
    }
    roots
}

pub(super) fn expand_man_db_systems(
    roots: Vec<PathBuf>,
    environment: &HashMap<OsString, OsString>,
) -> Vec<PathBuf> {
    let Some(systems) = environment_value(environment, "SYSTEM") else {
        return deduplicate_paths(roots);
    };
    let systems_value = systems.to_string_lossy();
    let systems = systems_value
        .split([',', ':'])
        .filter(|system| !system.is_empty())
        .collect::<Vec<_>>();
    if systems.is_empty() {
        return deduplicate_paths(roots);
    }

    let mut expanded = Vec::new();
    for root in roots {
        for system in &systems {
            if *system == "man" {
                expanded.push(root.clone());
            } else {
                let candidate = root.join(system);
                if candidate.is_dir() {
                    expanded.push(candidate);
                }
            }
        }
    }
    deduplicate_paths(expanded)
}
