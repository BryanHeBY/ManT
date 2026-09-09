//! Discovers and resolves local manual sources without invoking a man program.

mod ordering;
mod scan;
#[cfg(test)]
mod tests;

use std::{collections::BTreeMap, env, fmt, path::PathBuf};

use ordering::{
    compare_manual_sections, default_manual_section_order, manual_name_key, manual_names_equal,
    parse_manual_section_order,
};
pub(crate) use scan::deduplicate_paths;
#[cfg(test)]
use scan::normalize_locale;
use scan::{current_locale, scan_manual_root};

/// One validated manual lookup independent from CLI token syntax.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManualRequest {
    /// Manual topic without its category suffix.
    pub name: String,
    /// Optional exact native category such as `1` or `3p`.
    pub manual_section: Option<String>,
}

impl ManualRequest {
    /// Construct a normalized lookup request without performing I/O.
    #[must_use]
    pub fn new(name: impl Into<String>, manual_section: Option<String>) -> Self {
        Self {
            name: name.into(),
            manual_section,
        }
    }
}

/// One effective local manual page after path and locale precedence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManualPage {
    /// Indexed manual topic.
    pub name: String,
    /// Native manual category derived from the containing `man<section>` tree.
    pub section: String,
    /// Physical source path, possibly compressed.
    pub path: PathBuf,
    /// Approved hierarchy root used to resolve this page's `.so` redirects.
    ///
    /// The indexed leaf itself may be a file symlink whose target is outside
    /// this root. Redirect targets must still remain inside it.
    pub manual_root: PathBuf,
}

/// Immutable index shared by discovery and exact manual lookup.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ManualIndex {
    roots: Vec<PathBuf>,
    pages: Vec<ManualPage>,
}

impl ManualIndex {
    /// Scan explicit roots in precedence order.
    #[must_use]
    pub fn from_roots(roots: Vec<PathBuf>) -> Self {
        let locale = current_locale();
        let section_order = current_manual_section_order();
        Self::from_roots_with_locale_and_sections(roots, locale.as_deref(), &section_order)
    }

    #[cfg(test)]
    fn from_roots_with_locale(roots: Vec<PathBuf>, locale: Option<&str>) -> Self {
        Self::from_roots_with_locale_and_sections(roots, locale, &default_manual_section_order())
    }

    fn from_roots_with_locale_and_sections(
        roots: Vec<PathBuf>,
        locale: Option<&str>,
        section_order: &[String],
    ) -> Self {
        let roots = deduplicate_paths(roots);
        let mut effective = BTreeMap::<(String, String), ManualPage>::new();
        for root in &roots {
            for page in scan_manual_root(root, locale) {
                effective
                    .entry((manual_name_key(&page.name), page.section.clone()))
                    .or_insert(page);
            }
        }
        let mut pages = effective.into_values().collect::<Vec<_>>();
        pages.sort_by(|left, right| {
            manual_name_key(&left.name)
                .cmp(&manual_name_key(&right.name))
                .then_with(|| compare_manual_sections(&left.section, &right.section, section_order))
        });
        Self { roots, pages }
    }

    /// Roots searched by this index, in precedence order.
    #[must_use]
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Effective pages sorted by name and section.
    #[must_use]
    pub fn pages(&self) -> &[ManualPage] {
        &self.pages
    }

    /// Resolve one page using an optional exact manual category.
    #[must_use]
    pub fn find(&self, name: &str, section: Option<&str>) -> Option<&ManualPage> {
        let name = name.trim();
        let section = section.map(str::trim);
        self.pages.iter().find(|page| {
            manual_names_equal(&page.name, name)
                && section.is_none_or(|section| page.section == section)
        })
    }

    /// Exact manual categories available for one logical page name.
    #[must_use]
    pub fn available_manual_sections(&self, name: &str) -> Vec<String> {
        let name = name.trim();
        self.pages
            .iter()
            .filter(|page| manual_names_equal(&page.name, name))
            .map(|page| page.section.clone())
            .collect()
    }
}

fn current_manual_section_order() -> Vec<String> {
    env::var("MANSECT")
        .ok()
        .and_then(|value| parse_manual_section_order(&value))
        .unwrap_or_else(default_manual_section_order)
}

/// Expected source-discovery failures suitable for a user-facing CLI error.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum LocateError {
    /// The requested manual name was empty.
    EmptyName,
    /// An explicitly requested manual category was malformed.
    InvalidManualSection,
    /// No indexed page satisfied the request.
    NotFound {
        /// Requested manual topic.
        name: String,
        /// Exact requested native category, when supplied.
        requested_manual_section: Option<String>,
        /// Other indexed categories available for the same topic.
        available_manual_sections: Vec<String>,
    },
}

impl LocateError {
    /// Render this error as detail nested below an already identified topic.
    pub(crate) fn load_detail(&self) -> String {
        match self {
            Self::NotFound {
                requested_manual_section: Some(requested),
                available_manual_sections,
                ..
            } if !available_manual_sections.is_empty() => format!(
                "manual section '{requested}' is unavailable; available sections: {}",
                available_manual_sections.join(", ")
            ),
            Self::NotFound {
                requested_manual_section: Some(requested),
                ..
            } => format!("no source was found in manual section '{requested}'"),
            Self::NotFound { .. } => "no local manual source was found".to_owned(),
            Self::EmptyName | Self::InvalidManualSection => self.to_string(),
        }
    }
}

impl fmt::Display for LocateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => formatter.write_str("manual page name must not be empty"),
            Self::InvalidManualSection => formatter.write_str(
                "manual section must be a conventional number or the single letter 'l' or 'n'",
            ),
            Self::NotFound {
                name,
                requested_manual_section: Some(requested),
                available_manual_sections,
            } if !available_manual_sections.is_empty() => write!(
                formatter,
                "requested manual section '{requested}' is unavailable for '{name}'; available manual sections: {}",
                available_manual_sections.join(", ")
            ),
            Self::NotFound {
                name,
                requested_manual_section: Some(requested),
                ..
            } => write!(
                formatter,
                "no local manual source was found for '{name}' in manual section '{requested}'"
            ),
            Self::NotFound { name, .. } => {
                write!(formatter, "no local manual source was found for '{name}'")
            }
        }
    }
}

impl std::error::Error for LocateError {}

/// Locate a manual in an explicit immutable index.
///
/// # Errors
///
/// Returns [`LocateError`] for invalid requests and missing local sources.
pub fn locate_manual_source_in(
    request: &ManualRequest,
    index: &ManualIndex,
) -> Result<ManualPage, LocateError> {
    let name = request.name.trim();
    if name.is_empty() {
        return Err(LocateError::EmptyName);
    }
    let section = request.manual_section.as_deref().map(str::trim);
    if section.is_some_and(|section| !crate::is_manual_section(section)) {
        return Err(LocateError::InvalidManualSection);
    }
    index
        .find(name, section)
        .cloned()
        .ok_or_else(|| LocateError::NotFound {
            name: name.to_owned(),
            requested_manual_section: section.map(ToOwned::to_owned),
            available_manual_sections: index.available_manual_sections(name),
        })
}
