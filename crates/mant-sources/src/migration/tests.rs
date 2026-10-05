use super::*;
use std::collections::BTreeMap;

struct Fixture {
    root: PathBuf,
    old: PathBuf,
    settings: Settings,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "mant-migrate-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let old = root.join("legacy");
        fs::create_dir_all(old.join("documents")).unwrap();
        let environment = BTreeMap::from([("HOME".into(), root.clone().into_os_string())]);
        let settings = Settings::from_environment(&environment, cfg!(windows)).unwrap();
        Self {
            root,
            old,
            settings,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn copies_configuration_and_documents_preserves_originals_and_is_repeatable() {
    let fixture = Fixture::new();
    fs::write(fixture.old.join("sources.toml"), "# old configuration\n").unwrap();
    fs::write(fixture.old.join("man.conf"), "manpath /manuals\n").unwrap();
    fs::write(fixture.old.join("documents/user.md"), "# personal\n").unwrap();
    for _ in 0..2 {
        migrate_with(&fixture.old, fixture.settings.clone()).unwrap();
    }
    assert_eq!(
        fs::read_to_string(fixture.settings.config_home().unwrap().join("sources.toml")).unwrap(),
        "# old configuration\n"
    );
    assert!(
        fixture
            .settings
            .data_home()
            .unwrap()
            .join("documents/user.md")
            .is_file()
    );
    assert!(fixture.old.join("documents/user.md").is_file());
}

#[test]
fn existing_configuration_wins_and_data_conflicts_leave_old_installation_intact() {
    let fixture = Fixture::new();
    let config = fixture.settings.config_home().unwrap();
    let documents = fixture.settings.data_home().unwrap().join("documents");
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(&documents).unwrap();
    fs::write(config.join("sources.toml"), "# new\n").unwrap();
    fs::write(fixture.old.join("sources.toml"), "# old\n").unwrap();
    fs::write(documents.join("user.md"), "new").unwrap();
    fs::write(fixture.old.join("documents/user.md"), "old").unwrap();
    fs::write(fixture.old.join("man.conf"), "manpath /manuals\n").unwrap();
    assert!(migrate_with(&fixture.old, fixture.settings.clone()).is_err());
    assert_eq!(
        fs::read_to_string(config.join("sources.toml")).unwrap(),
        "# new\n"
    );
    assert!(!config.join("man.conf").exists());
    assert_eq!(
        fs::read_to_string(documents.join("user.md")).unwrap(),
        "new"
    );
    assert_eq!(
        fs::read_to_string(fixture.old.join("documents/user.md")).unwrap(),
        "old"
    );
}

#[test]
fn relative_git_identity_and_installed_source_metadata_survive_relocation() {
    let fixture = Fixture::new();
    let configuration = "[team]\nrepo = 'local-repository'\nbranch = 'main'\n";
    fs::write(fixture.old.join("sources.toml"), configuration).unwrap();
    let old = crate::config::parse_source_config(configuration, &fixture.old.join("sources.toml"))
        .unwrap();
    let source = old.get("team").unwrap();
    let directory = fixture.old.join("sources/team");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("tool.md"), "# tool\n").unwrap();
    let metadata = crate::metadata::SourceMetadata::git(
        "team",
        "local-repository",
        "main",
        "a".repeat(40),
        &crate::metadata::source_fingerprint(source),
        1,
    );
    fs::write(
        directory.join(crate::SOURCE_METADATA_FILE),
        toml::to_string(&metadata).unwrap(),
    )
    .unwrap();
    for _ in 0..2 {
        migrate_with(&fixture.old, fixture.settings.clone()).unwrap();
    }
    let config = crate::config::load_source_config_from(
        &fixture.settings.config_home().unwrap().join("sources.toml"),
    )
    .unwrap();
    let new = config.get("team").unwrap();
    assert_eq!(
        new.location,
        crate::SourceLocation::Git {
            repo: fixture
                .old
                .join("local-repository")
                .to_string_lossy()
                .into_owned(),
            branch: "main".into()
        }
    );
    let metadata = crate::metadata::read_source_metadata(
        &fixture.settings.data_home().unwrap().join("sources/team"),
    )
    .unwrap();
    assert!(metadata.matches("team", new, &crate::metadata::source_fingerprint(new)));
    assert!(!directory.join(".update.lock").exists());
}

#[test]
fn active_source_updates_prevent_migration() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.old.join("sources")).unwrap();
    fs::write(fixture.old.join("sources/.update.lock"), "active").unwrap();
    assert!(migrate_with(&fixture.old, fixture.settings.clone()).is_err());
    assert!(!fixture.settings.config_home().unwrap().exists());
}

#[test]
fn local_paths_with_colons_are_not_mistaken_for_remote_git_locations() {
    assert!(relative_local_repository("local-repository"));
    assert!(relative_local_repository("./local:repository"));
    assert!(relative_local_repository("repositories/local:repository"));
    assert!(!relative_local_repository(
        "git@example.org:team/repository.git"
    ));
    assert!(!relative_local_repository(
        "https://example.org/repository.git"
    ));
    assert!(!relative_local_repository(
        "ssh://git@example.org/repository.git"
    ));
}

#[test]
fn valid_target_configuration_wins_over_malformed_legacy_configuration() {
    let fixture = Fixture::new();
    let config = fixture.settings.config_home().unwrap();
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("sources.toml"), "# new\n").unwrap();
    fs::write(fixture.old.join("sources.toml"), "[broken").unwrap();
    migrate_with(&fixture.old, fixture.settings.clone()).unwrap();
    assert_eq!(
        fs::read_to_string(config.join("sources.toml")).unwrap(),
        "# new\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.old.join("sources.toml")).unwrap(),
        "[broken"
    );
}

#[test]
fn read_only_files_keep_permissions_without_leaving_staging_links() {
    let fixture = Fixture::new();
    let source = fixture.old.join("documents/readonly.md");
    fs::write(&source, "# Read only\n").unwrap();
    let original_permissions = fs::metadata(&source).unwrap().permissions();
    let mut read_only = original_permissions.clone();
    read_only.set_readonly(true);
    fs::set_permissions(&source, read_only).unwrap();
    migrate_with(&fixture.old, fixture.settings.clone()).unwrap();
    let target = fixture
        .settings
        .data_home()
        .unwrap()
        .join("documents/readonly.md");
    assert!(fs::metadata(&target).unwrap().permissions().readonly());
    assert_eq!(fs::read_dir(target.parent().unwrap()).unwrap().count(), 1);
    fs::set_permissions(&source, original_permissions.clone()).unwrap();
    fs::set_permissions(target, original_permissions).unwrap();
}

#[cfg(unix)]
#[test]
fn linked_source_store_is_rejected_before_lock_acquisition() {
    let fixture = Fixture::new();
    let outside = fixture.root.join("outside");
    fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, fixture.old.join("sources")).unwrap();
    assert!(migrate_with(&fixture.old, fixture.settings.clone()).is_err());
    assert!(!outside.join(".update.lock").exists());
    assert!(!fixture.settings.config_home().unwrap().exists());
}
