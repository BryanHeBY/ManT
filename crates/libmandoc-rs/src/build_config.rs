//! Target feature configurations shared by the build hook and its unit tests.

const LINUX_COMPAT_SOURCES: &[&str] = &[
    "compat_ohash.c",
    "compat_progname.c",
    "compat_recallocarray.c",
    "compat_strlcat.c",
    "compat_strlcpy.c",
    "compat_strtonum.c",
];

const MACOS_COMPAT_SOURCES: &[&str] = &[
    "compat_ohash.c",
    "compat_reallocarray.c",
    "compat_recallocarray.c",
];

const WINDOWS_MSVC_COMPAT_SOURCES: &[&str] = &[
    "compat_err.c",
    "compat_ohash.c",
    "compat_progname.c",
    "compat_reallocarray.c",
    "compat_recallocarray.c",
    "compat_strlcat.c",
    "compat_strlcpy.c",
    "compat_strndup.c",
    "compat_strtonum.c",
    "compat_vasprintf.c",
];

pub(crate) fn target_configuration(
    target_os: &str,
    target_env: &str,
) -> (&'static str, &'static [&'static str]) {
    match (target_os, target_env) {
        ("linux", "gnu") => ("config/linux-gnu.h", LINUX_COMPAT_SOURCES),
        ("macos", _) => ("config/macos.h", MACOS_COMPAT_SOURCES),
        ("windows", "msvc") => ("config/windows-msvc.h", WINDOWS_MSVC_COMPAT_SOURCES),
        ("linux", env) => {
            panic!("libmandoc-rs does not yet provide a checked configuration for Linux/{env}")
        }
        (os, env) => panic!(
            "libmandoc-rs only supports Linux/glibc, macOS, and Windows/MSVC, not {os}/{env}"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LINUX_COMPAT_SOURCES, MACOS_COMPAT_SOURCES, WINDOWS_MSVC_COMPAT_SOURCES,
        target_configuration,
    };

    #[test]
    fn target_families_select_explicit_compatibility_sources() {
        assert_eq!(
            target_configuration("linux", "gnu"),
            ("config/linux-gnu.h", LINUX_COMPAT_SOURCES)
        );
        assert_eq!(
            target_configuration("macos", ""),
            ("config/macos.h", MACOS_COMPAT_SOURCES)
        );
        assert_eq!(
            target_configuration("windows", "msvc"),
            ("config/windows-msvc.h", WINDOWS_MSVC_COMPAT_SOURCES)
        );
    }

    #[test]
    fn linux_does_not_depend_on_new_glibc_string_extensions() {
        let (_, sources) = target_configuration("linux", "gnu");
        let config = include_str!("../config/linux-gnu.h");

        assert!(sources.contains(&"compat_strlcat.c"));
        assert!(sources.contains(&"compat_strlcpy.c"));
        assert!(config.contains("#define HAVE_STRLCAT 0"));
        assert!(config.contains("#define HAVE_STRLCPY 0"));
    }

    #[test]
    fn every_target_locks_roff_character_classes_to_ascii() {
        let configs = [
            include_str!("../config/linux-gnu.h"),
            include_str!("../config/macos.h"),
            include_str!("../config/windows-msvc.h"),
        ];
        for config in configs {
            assert!(config.contains("#include \"mant_ascii_ctype.h\""));
        }

        let ascii = include_str!("../config/mant_ascii_ctype.h");
        for class in [
            "isalnum", "isalpha", "isdigit", "isgraph", "islower", "isspace", "isupper", "tolower",
            "toupper",
        ] {
            assert!(
                ascii.contains(&format!("#define {class}(c) mant_ascii_{class}(c)")),
                "missing deterministic override for {class}"
            );
        }
    }

    #[test]
    #[should_panic(expected = "Linux/musl")]
    fn unconfigured_linux_libc_is_rejected() {
        target_configuration("linux", "musl");
    }
}
