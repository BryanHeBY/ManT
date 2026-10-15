//! Query compatibility is category selection, independent of roff execution.

use super::*;
use crate::ManualLookupMode;

struct Fixture(PathBuf);

impl Fixture {
    fn new(label: &str) -> Self {
        Self(temporary_root(label))
    }

    fn page(&self, relative: &str) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().expect("manual parent")).expect("manual directory");
        fs::write(&path, b"index placeholder, never parsed").expect("index source");
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove index fixture");
    }
}

#[test]
fn query_major_categories_prefer_exact_then_alphabetic_extensions() {
    let fixture = Fixture::new("query-category-matrix");
    for major in 0..=9 {
        for section in [
            format!("{major}"),
            format!("{major}p"),
            format!("{major}type"),
        ] {
            fixture.page(&format!("man{section}/both.{section}"));
            if section != major.to_string() {
                fixture.page(&format!("man{section}/extensions.{section}"));
            }
        }
    }
    let order = (0..=9)
        .flat_map(|major| {
            [
                format!("{major}type"),
                format!("{major}p"),
                major.to_string(),
            ]
        })
        .collect::<Vec<_>>();
    let index =
        ManualIndex::from_roots_with_locale_and_sections(vec![fixture.0.clone()], None, &order);
    for major in 0..=9 {
        let category = major.to_string();
        for name in ["both", "extensions"] {
            let query = ManualRequest::query(name, Some(category.clone()));
            assert_eq!(query.lookup_mode, ManualLookupMode::Compatible);
            let selected = locate_manual_source_in(&query, &index).expect("major query");
            assert_eq!(
                selected.section,
                if name == "both" {
                    category.clone()
                } else {
                    format!("{major}type")
                }
            );
        }
        assert!(index.find("extensions", Some(&category)).is_none());
        assert!(
            locate_manual_source_in(&ManualRequest::new("extensions", Some(category)), &index,)
                .is_err()
        );
        for suffix in ["p", "type"] {
            let category = format!("{major}{suffix}");
            assert_eq!(
                index.find_query("both", Some(&category)).unwrap().section,
                category
            );
        }
    }
}

#[test]
fn query_is_strict_for_suffix_letter_and_other_major_categories() {
    let fixture = Fixture::new("query-strict-matrix");
    for section in ["1", "1p", "1px", "1p2", "10", "2", "3p", "l", "n"] {
        fixture.page(&format!("man{section}/only-{section}.{section}"));
    }
    let index = ManualIndex::from_roots_with_locale(vec![fixture.0.clone()], None);
    for request in [
        ManualRequest::query("only-1", Some("1p".into())),
        ManualRequest::query("only-1px", Some("1p".into())),
        ManualRequest::query("only-1p2", Some("1".into())),
        ManualRequest::query("only-10", Some("1".into())),
        ManualRequest::query("only-2", Some("1".into())),
        ManualRequest::query("only-3p", Some("1".into())),
        ManualRequest::query("only-l", Some("n".into())),
        ManualRequest::query("only-n", Some("l".into())),
    ] {
        assert!(
            matches!(
                locate_manual_source_in(&request, &index),
                Err(LocateError::NotFound { .. })
            ),
            "{request:?}"
        );
    }
    for section in ["1", "1p", "1px", "1p2", "10", "2", "3p", "l", "n"] {
        let page = locate_manual_source_in(
            &ManualRequest::query(format!("only-{section}"), Some(section.into())),
            &index,
        )
        .expect("exact query");
        assert_eq!(page.section, section);
    }
}

#[test]
fn query_uses_snapshot_section_order_and_preserves_root_and_locale_precedence() {
    let fixture = Fixture::new("query-precedence");
    let first = fixture.0.join("first");
    let second = fixture.0.join("second");
    fixture.page("first/man1p/ed.1p");
    let localized = fixture.page("first/zh_CN/man1p/ed.1p");
    fixture.page("second/zh_CN/man1p/ed.1p");
    let m_page = fixture.page("second/man1m/ed.1m");
    let exact_page = fixture.page("second/man1/exact.1");
    fixture.page("first/zh_CN/man1p/exact.1p");
    let roots = vec![first.clone(), second.clone(), first];
    let order = parse_manual_section_order("1m:1p:1").unwrap();
    let index =
        ManualIndex::from_roots_with_locale_and_sections(roots.clone(), Some("zh_CN"), &order);
    assert_eq!(index.roots().len(), 2);
    assert_eq!(index.find_query("ed", Some("1")).unwrap().path, m_page);
    assert_eq!(
        index.find_query("exact", Some("1")).unwrap().path,
        exact_page
    );
    assert_eq!(index.find_query("ed", Some("1p")).unwrap().path, localized);
    let default = ManualIndex::from_roots_with_locale(roots, Some("zh_CN"));
    assert_eq!(default.find_query("ed", Some("1")).unwrap().path, localized);
    assert_eq!(default.find_query("ed", None), default.find("ed", None));
}

#[test]
fn query_rejects_invalid_selectors_before_fallback() {
    let index = ManualIndex::default();
    assert_eq!(
        locate_manual_source_in(&ManualRequest::query(" ", Some("1".into())), &index),
        Err(LocateError::EmptyName)
    );
    for section in ["", "  ", "1/../3", "1*", "DESCRIPTION"] {
        assert_eq!(
            locate_manual_source_in(&ManualRequest::query("ed", Some(section.into())), &index),
            Err(LocateError::InvalidManualSection)
        );
    }
    assert_ne!(
        ManualRequest::new("ed", Some("1".into())),
        ManualRequest::query("ed", Some("1".into()))
    );
}
