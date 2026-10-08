use super::*;
use std::{
    fmt::Write as _,
    fs,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = env::temp_dir().join(format!(
            "mant-settings-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join(".config/mant")).unwrap();
        Self(root)
    }
    fn environment(&self) -> BTreeMap<OsString, OsString> {
        BTreeMap::from([(OsString::from("HOME"), self.0.clone().into_os_string())])
    }
    fn configure(&self, text: &str) {
        fs::write(self.0.join(".config/mant/mant.toml"), text).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn portable_defaults_and_no_write_side_effects() {
    let fixture = Fixture::new();
    for windows in [false, true] {
        let settings = Settings::from_environment(&fixture.environment(), windows).unwrap();
        assert_eq!(
            settings.config_home().unwrap(),
            fixture.0.join(".config/mant")
        );
        assert_eq!(
            settings.data_home().unwrap(),
            fixture.0.join(".local/share/mant")
        );
        assert_eq!(
            settings.cache_home().unwrap(),
            fixture.0.join(".cache/mant")
        );
        assert!(settings.discover_manuals());
        assert!(settings.inherit_manual_paths());
        assert!(!settings.data_home().unwrap().exists());
        assert!(!settings.cache_home().unwrap().exists());
    }
}

#[test]
fn manual_declarations_preserve_omission_and_explicit_empty_paths() {
    for text in ["", "[paths]\n", "[paths]\ndata_home = 'data'\n"] {
        let declaration: Declaration = toml::from_str(text).unwrap();
        assert!(declaration.man.is_none());
    }
    let declaration: Declaration = toml::from_str("[man]\n").unwrap();
    let man = declaration.man.unwrap();
    assert!(man.paths.is_none());
    assert!(man.inherit_paths.is_none());
    assert!(man.discover.is_none());

    let declaration: Declaration =
        toml::from_str("[man]\npaths = []\ninherit_paths = true\ndiscover = false\n").unwrap();
    let man = declaration.man.unwrap();
    assert_eq!(man.paths, Some(Vec::new()));
    assert_eq!(man.inherit_paths, Some(true));
    assert_eq!(man.discover, Some(false));
}

#[test]
fn manual_flags_default_to_true_and_remain_independent() {
    let fixture = Fixture::new();
    let environment = fixture.environment();
    for inherit in [None, Some(true), Some(false)] {
        for discover in [None, Some(true), Some(false)] {
            let mut text = "[man]\npaths = ['~/manuals']\n".to_owned();
            if let Some(value) = inherit {
                writeln!(text, "inherit_paths = {value}").unwrap();
            }
            if let Some(value) = discover {
                writeln!(text, "discover = {value}").unwrap();
            }
            fixture.configure(&text);
            let settings = Settings::from_environment(&environment, cfg!(windows)).unwrap();
            assert_eq!(settings.inherit_manual_paths(), inherit.unwrap_or(true));
            assert_eq!(settings.discover_manuals(), discover.unwrap_or(true));
            assert_eq!(
                settings.manual_paths().unwrap(),
                [fixture.0.join("manuals")]
            );
            let man = settings.declaration.man.as_ref().unwrap();
            assert_eq!(man.inherit_paths, inherit);
            assert_eq!(man.discover, discover);
            assert_eq!(
                fs::read_to_string(fixture.0.join(".config/mant/mant.toml")).unwrap(),
                text
            );
            assert!(!fixture.0.join("manuals").exists());
            assert!(!settings.data_home().unwrap().exists());
            assert!(!settings.cache_home().unwrap().exists());
        }
    }
    for text in ["", "[man]\n", "[man]\npaths = []\n"] {
        fixture.configure(text);
        let settings = Settings::from_environment(&environment, cfg!(windows)).unwrap();
        assert!(settings.inherit_manual_paths());
        assert!(settings.discover_manuals());
        assert_eq!(settings.manual_paths().unwrap(), [] as [PathBuf; 0]);
    }
}

#[test]
fn overrides_take_precedence_without_an_extra_application_component() {
    let fixture = Fixture::new();
    fixture.configure("[paths]\ndata_home = '~/configured'\ncache_home = '~/configured-cache'\n");
    let mut environment = fixture.environment();
    environment.insert(
        "XDG_DATA_HOME".into(),
        fixture.0.join("xdg-data").into_os_string(),
    );
    let settings = Settings::from_environment(&environment, false).unwrap();
    assert_eq!(settings.data_home().unwrap(), fixture.0.join("configured"));
    environment.insert(
        "MANT_DATA_HOME".into(),
        fixture.0.join("explicit").into_os_string(),
    );
    let settings = Settings::from_environment(&environment, false).unwrap();
    assert_eq!(settings.data_home().unwrap(), fixture.0.join("explicit"));
    assert_eq!(
        settings.cache_home().unwrap(),
        fixture.0.join("configured-cache")
    );
}

#[test]
fn windows_names_and_userprofile_fallback() {
    let fixture = Fixture::new();
    let environment = BTreeMap::from([
        ("UserProfile".into(), fixture.0.clone().into_os_string()),
        (
            "Mant_Data_Home".into(),
            fixture.0.join("data").into_os_string(),
        ),
    ]);
    let settings = Settings::from_environment(&environment, true).unwrap();
    assert_eq!(
        settings.config_home().unwrap(),
        fixture.0.join(".config/mant")
    );
    assert_eq!(settings.data_home().unwrap(), fixture.0.join("data"));
}

#[test]
fn paths_use_configuration_directory_not_current_directory() {
    let fixture = Fixture::new();
    fixture.configure("[paths]\ndata_home = '../../data'\n[man]\npaths = ['~/manuals', 'local manuals']\ndiscover = false\n");
    let settings = Settings::from_environment(&fixture.environment(), false).unwrap();
    assert_eq!(
        settings.data_home().unwrap(),
        fixture.0.join(".config/mant/../../data")
    );
    assert_eq!(
        settings.manual_paths().unwrap(),
        [
            fixture.0.join("manuals"),
            fixture.0.join(".config/mant/local manuals")
        ]
    );
    assert!(!settings.discover_manuals());
}

#[test]
fn toml_path_fields_share_expansion_and_read_only_resolution() {
    let fixture = Fixture::new();
    fixture.configure("[paths]\ndata_home = '%ROOT%/data'\ncache_home = '~/cache/100%%'\n[man]\npaths = ['~', '%ROOT%/manuals', '%RELATIVE%']\n");
    let before = fs::read(fixture.0.join(".config/mant/mant.toml")).unwrap();
    let mut environment = fixture.environment();
    environment.insert("ROOT".into(), fixture.0.clone().into_os_string());
    environment.insert("RELATIVE".into(), "local manuals".into());
    for windows in [false, true] {
        let settings = Settings::from_environment(&environment, windows).unwrap();
        assert_eq!(settings.data_home().unwrap(), fixture.0.join("data"));
        assert_eq!(settings.cache_home().unwrap(), fixture.0.join("cache/100%"));
        assert_eq!(
            settings.manual_paths().unwrap(),
            [
                fixture.0.clone(),
                fixture.0.join("manuals"),
                fixture.0.join(".config/mant/local manuals"),
            ]
        );
        assert!(!settings.data_home().unwrap().exists());
        assert!(!settings.cache_home().unwrap().exists());
    }
    assert_eq!(
        fs::read(fixture.0.join(".config/mant/mant.toml")).unwrap(),
        before
    );
}

#[test]
fn invalid_configuration_does_not_silently_fall_back() {
    let fixture = Fixture::new();
    for text in [
        "[paths]\nconfig_home = '~/elsewhere'",
        "[man]\ndiscover = 'yes'",
        "[man]\ninherit_paths = 'yes'",
        "[man]\ninherit_paths = 1",
        "[other]",
    ] {
        fixture.configure(text);
        assert!(Settings::from_environment(&fixture.environment(), false).is_err());
    }
}

#[test]
fn explicit_directories_work_without_home_and_relative_overrides_fail() {
    let fixture = Fixture::new();
    let mut environment = BTreeMap::from([
        (
            "MANT_CONFIG_HOME".into(),
            fixture.0.join("absent").into_os_string(),
        ),
        (
            "MANT_DATA_HOME".into(),
            fixture.0.join("data").into_os_string(),
        ),
        (
            "MANT_CACHE_HOME".into(),
            fixture.0.join("cache").into_os_string(),
        ),
    ]);
    let settings = Settings::from_environment(&environment, false).unwrap();
    assert_eq!(settings.data_home().unwrap(), fixture.0.join("data"));
    assert_eq!(settings.cache_home().unwrap(), fixture.0.join("cache"));
    environment.insert("MANT_DATA_HOME".into(), "relative".into());
    assert!(
        Settings::from_environment(&environment, false)
            .unwrap()
            .data_home()
            .is_err()
    );
}

#[test]
fn xdg_bases_append_mant_and_ignore_empty_or_relative_values() {
    let fixture = Fixture::new();
    let mut environment = fixture.environment();
    environment.insert(
        "XDG_CONFIG_HOME".into(),
        fixture.0.join("xdg-config").into_os_string(),
    );
    environment.insert(
        "XDG_DATA_HOME".into(),
        fixture.0.join("xdg-data").into_os_string(),
    );
    environment.insert(
        "XDG_CACHE_HOME".into(),
        fixture.0.join("xdg-cache").into_os_string(),
    );
    let settings = Settings::from_environment(&environment, cfg!(windows)).unwrap();
    assert_eq!(
        settings.config_home().unwrap(),
        fixture.0.join("xdg-config/mant")
    );
    assert_eq!(
        settings.data_home().unwrap(),
        fixture.0.join("xdg-data/mant")
    );
    assert_eq!(
        settings.cache_home().unwrap(),
        fixture.0.join("xdg-cache/mant")
    );
    environment.insert("XDG_CONFIG_HOME".into(), "relative".into());
    environment.insert("XDG_DATA_HOME".into(), "".into());
    environment.insert("XDG_CACHE_HOME".into(), "relative".into());
    environment.insert("MANT_CONFIG_HOME".into(), "".into());
    let settings = Settings::from_environment(&environment, cfg!(windows)).unwrap();
    assert_eq!(
        settings.config_home().unwrap(),
        fixture.0.join(".config/mant")
    );
    assert_eq!(
        settings.data_home().unwrap(),
        fixture.0.join(".local/share/mant")
    );
    assert_eq!(
        settings.cache_home().unwrap(),
        fixture.0.join(".cache/mant")
    );
}
