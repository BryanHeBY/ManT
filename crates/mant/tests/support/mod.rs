//! Provides platform-aware filesystem fixtures shared by black-box tests.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

/// Return the registered-document directory selected by the production
/// resolver for a test-owned home directory.
pub fn registered_documents_dir(home: &Path) -> PathBuf {
    if cfg!(windows) {
        return home
            .join("AppData")
            .join("Roaming")
            .join("ManT")
            .join("documents");
    }
    if cfg!(target_os = "macos") {
        return home.join("Library/Application Support/ManT/documents");
    }
    home.join("data/mant/documents")
}

/// Isolate document discovery and point it at a test-owned home directory.
///
/// Setting both families keeps each child process hermetic while allowing the
/// production resolver to choose the native convention for its target OS.
pub fn configure_registered_documents(command: &mut Command, home: &Path) {
    command
        .env("HOME", home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_DATA_DIRS", home.join("empty-system-data"))
        .env("APPDATA", home.join("AppData").join("Roaming"))
        .env("LOCALAPPDATA", home.join("AppData").join("Local"))
        .env("PROGRAMDATA", home.join("ProgramData"));
}
