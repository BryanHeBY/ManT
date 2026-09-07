//! Work-bounded path expansion shared by native configuration dialects.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

/// Final paths and whether this expansion exhausted its work budget.
pub(super) struct ExpansionOutcome {
    pub(super) paths: Vec<PathBuf>,
    pub(super) exhausted: bool,
}

/// Shared across every component and pattern in one configuration expansion.
pub(super) struct ScanBudget {
    pub(super) remaining: usize,
    matcher_cells: usize,
}

impl ScanBudget {
    pub(super) const fn new(remaining: usize) -> Self {
        Self {
            remaining,
            matcher_cells: 4 * 1024 * 1024,
        }
    }

    pub(super) fn charge(&mut self) -> bool {
        if self.remaining == 0 {
            return false;
        }
        self.remaining -= 1;
        true
    }
}

pub(super) fn expand_path_pattern_bounded(
    pattern: &Path,
    budget: &mut ScanBudget,
) -> ExpansionOutcome {
    // Bound both component count and wildcard-matcher work for a single path.
    if pattern.as_os_str().len() > 4096 || !budget.charge() {
        return ExpansionOutcome {
            paths: Vec::new(),
            exhausted: true,
        };
    }
    let mut candidates = vec![PathBuf::new()];
    for component in pattern.components() {
        if candidates.is_empty() {
            break;
        }
        let mut expanded = Vec::new();
        for mut candidate in candidates {
            if !budget.charge() {
                return ExpansionOutcome {
                    paths: Vec::new(),
                    exhausted: true,
                };
            }
            match component {
                Component::RootDir => {
                    candidate.push(std::path::MAIN_SEPARATOR.to_string());
                    expanded.push(candidate);
                }
                Component::Normal(name) if name.to_string_lossy().contains(['*', '?']) => {
                    let Ok(entries) = fs::read_dir(&candidate) else {
                        continue;
                    };
                    // Charge enumeration, including nonmatches and failed entries,
                    // before retaining or sorting anything. Discard a truncated
                    // pattern: intermediate prefixes are never final roots.
                    for entry in entries {
                        if !budget.charge() {
                            return ExpansionOutcome {
                                paths: Vec::new(),
                                exhausted: true,
                            };
                        }
                        let Ok(entry) = entry else {
                            continue;
                        };
                        let filename = entry.file_name();
                        let Some(value) = filename.to_str() else {
                            continue;
                        };
                        let Some(matches) = wildcard_matches_bounded(
                            &name.to_string_lossy(),
                            value,
                            &mut budget.matcher_cells,
                        ) else {
                            budget.remaining = 0;
                            return ExpansionOutcome {
                                paths: Vec::new(),
                                exhausted: true,
                            };
                        };
                        if matches {
                            expanded.push(entry.path());
                        }
                    }
                }
                _ => {
                    candidate.push(component.as_os_str());
                    expanded.push(candidate);
                }
            }
        }
        expanded.sort_unstable();
        expanded.dedup();
        candidates = expanded;
    }
    ExpansionOutcome {
        paths: candidates,
        exhausted: false,
    }
}

#[cfg(test)]
pub(super) fn wildcard_matches(pattern: &str, value: &str) -> bool {
    let mut cells = usize::MAX;
    wildcard_matches_bounded(pattern, value, &mut cells).unwrap()
}

fn wildcard_matches_bounded(pattern: &str, value: &str, cells: &mut usize) -> Option<bool> {
    let value = value.chars().collect::<Vec<_>>();
    let mut previous = vec![false; value.len() + 1];
    previous[0] = true;
    for token in pattern.chars() {
        *cells = cells.checked_sub(value.len() + 1)?;
        let mut current = vec![false; value.len() + 1];
        match token {
            '*' => {
                current[0] = previous[0];
                for index in 1..=value.len() {
                    current[index] = previous[index] || current[index - 1];
                }
            }
            '?' => current[1..].copy_from_slice(&previous[..value.len()]),
            token => {
                for index in 1..=value.len() {
                    current[index] = previous[index - 1] && value[index - 1] == token;
                }
            }
        }
        previous = current;
    }
    Some(previous[value.len()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_question_mark_counts_unicode_scalars() {
        assert!(wildcard_matches("?.conf", "é.conf"));
        assert!(wildcard_matches("?.conf", "日.conf"));
        assert!(!wildcard_matches("?.conf", "ab.conf"));
        let mut cells = 8;
        assert_eq!(wildcard_matches_bounded("***", "abc", &mut cells), None);
        assert_eq!(cells, 0);
    }

    #[test]
    fn enumeration_and_components_share_one_budget_without_partial_paths() {
        let root = std::env::temp_dir().join(format!("mant-scan-budget-{}", std::process::id()));
        fs::create_dir_all(root.join("child")).unwrap();
        for index in 0..30 {
            fs::write(root.join(format!("{index}.conf")), "").unwrap();
        }
        let mut budget = ScanBudget::new(20);
        let ExpansionOutcome {
            paths,
            exhausted: truncated,
        } = expand_path_pattern_bounded(&root.join("*.missing"), &mut budget);
        assert!(paths.is_empty() && truncated);
        assert_eq!(budget.remaining, 0);
        assert!(expand_path_pattern_bounded(&root.join("*.conf"), &mut budget).exhausted);
        let mut budget = ScanBudget::new(90);
        let ExpansionOutcome {
            paths,
            exhausted: truncated,
        } = expand_path_pattern_bounded(&root.join("*/../*/../*/../final"), &mut budget);
        assert!(paths.is_empty() && truncated);
        fs::remove_dir_all(root).unwrap();
    }
}
