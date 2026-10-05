//! Provides platform-aware filesystem fixtures shared by black-box tests.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

/// Return the registered-document directory selected by the production
/// resolver for a test-owned home directory.
pub fn registered_documents_dir(home: &Path) -> PathBuf {
    home.join("data").join("mant").join("documents")
}

/// Isolate document discovery and point it at a test-owned home directory.
///
/// Use the unified layout with native path separators and isolate legacy
/// discovery roots so each child process stays hermetic on every target OS.
pub fn configure_registered_documents(command: &mut Command, home: &Path) {
    command
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("MANT_CONFIG_HOME", home.join("config"))
        .env("MANT_DATA_HOME", home.join("data").join("mant"))
        .env("MANT_CACHE_HOME", home.join("cache").join("mant"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_DATA_DIRS", home.join("empty-system-data"))
        .env("APPDATA", home.join("AppData").join("Roaming"))
        .env("LOCALAPPDATA", home.join("AppData").join("Local"))
        .env("PROGRAMDATA", home.join("ProgramData"));
}
