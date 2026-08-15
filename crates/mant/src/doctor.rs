//! Read-only installation diagnostics for the native CLI.

use std::{collections::BTreeSet, fmt::Write as _, fs, path::Path};

use anstyle::{AnsiColor, Style};
use mant_protocol::{DoctorCheck, DoctorCheckStatus, DoctorEnvironment, DoctorReport, Producer};
use mant_sources::{
    ConfiguredSourceInspection, DocumentPaths, RegisteredDocumentOrigin, SourceInstallationStatus,
    SourceTransport,
};

const OK_STYLE: Style = AnsiColor::Green.on_default().bold();
const INFO_STYLE: Style = AnsiColor::Cyan.on_default().bold();
const WARNING_STYLE: Style = AnsiColor::Yellow.on_default().bold();
const ERROR_STYLE: Style = AnsiColor::Red.on_default().bold();

struct DoctorBuilder {
    environment: DoctorEnvironment,
    checks: Vec<DoctorCheck>,
}

impl DoctorBuilder {
    fn new() -> Self {
        Self {
            environment: DoctorEnvironment {
                os: std::env::consts::OS.to_owned(),
                arch: std::env::consts::ARCH.to_owned(),
                data_root: None,
                config_path: None,
                documents_root: None,
                sources_root: None,
                manual_roots: Vec::new(),
                tldr_roots: Vec::new(),
            },
            checks: Vec::new(),
        }
    }

    fn push(
        &mut self,
        code: &str,
        status: DoctorCheckStatus,
        message: impl Into<String>,
    ) -> &mut DoctorCheck {
        self.checks.push(DoctorCheck {
            code: code.to_owned(),
            subject: None,
            status,
            message: message.into(),
            details: Vec::new(),
            remediation: None,
        });
        self.checks.last_mut().expect("doctor check was appended")
    }

    fn finish(self) -> DoctorReport {
        DoctorReport::new(
            Producer {
                name: "mant".to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                engine: None,
            },
            self.environment,
            self.checks,
        )
    }
}

/// Inspect the current installation without creating storage, acquiring
/// update locks, invoking external programs, or accessing the network.
pub(crate) fn inspect_system() -> DoctorReport {
    let mut builder = DoctorBuilder::new();
    builder.push(
        "runtime.platform",
        DoctorCheckStatus::Ok,
        format!(
            "ManT {} on {}-{}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::ARCH,
            std::env::consts::OS
        ),
    );
    inspect_libmandoc(&mut builder);
    inspect_sources(&mut builder);
    inspect_manuals(&mut builder);
    inspect_tldr(&mut builder);
    builder.finish()
}

fn inspect_libmandoc(builder: &mut DoctorBuilder) {
    let probe = b".TH MANT-DOCTOR 1\n.SH NAME\nmant-doctor \\- installation probe\n";
    match mant_engine::parse_manual_bytes(Path::new("mant-doctor.1"), probe) {
        Ok(document) if !document.sections.is_empty() => {
            builder.push(
                "runtime.libmandoc",
                DoctorCheckStatus::Ok,
                "libmandoc parsed the built-in roff probe",
            );
        }
        Ok(_) => {
            builder.push(
                "runtime.libmandoc",
                DoctorCheckStatus::Error,
                "libmandoc returned an empty document for the built-in roff probe",
            );
        }
        Err(error) => {
            let check = builder.push(
                "runtime.libmandoc",
                DoctorCheckStatus::Error,
                "libmandoc could not parse the built-in roff probe",
            );
            check.details.push(error.to_string());
        }
    }
}

fn inspect_sources(builder: &mut DoctorBuilder) {
    let paths = match mant_sources::document_paths() {
        Ok(paths) => paths,
        Err(error) => {
            let check = builder.push(
                "paths.data-root",
                DoctorCheckStatus::Error,
                "the ManT data root could not be derived",
            );
            check.details.push(error.to_string());
            check.remediation = Some("set the platform user-data environment variable".to_owned());
            return;
        }
    };
    record_paths(builder, &paths);
    inspect_data_root(builder, &paths);

    let inspection = match mant_sources::inspect_document_sources() {
        Ok(inspection) => inspection,
        Err(error) => {
            let check = builder.push(
                "sources.configuration",
                DoctorCheckStatus::Error,
                "document-source configuration could not be loaded",
            );
            check.details.push(error.to_string());
            check.remediation = Some("fix or remove the reported sources.toml".to_owned());
            return;
        }
    };
    let configured = inspection.sources.len();
    builder.push(
        "sources.configuration",
        if inspection.config_exists {
            DoctorCheckStatus::Ok
        } else {
            DoctorCheckStatus::Info
        },
        if inspection.config_exists {
            format!("loaded {configured} configured source(s)")
        } else {
            "sources.toml is absent; no managed sources are configured".to_owned()
        },
    );
    inspect_registered_documents(builder);

    let git_required = inspection
        .sources
        .iter()
        .any(|source| source.transport == SourceTransport::Git);
    for source in &inspection.sources {
        push_configured_source(builder, source);
    }
    for source in &inspection.orphaned {
        let check = builder.push(
            "sources.orphaned",
            DoctorCheckStatus::Warning,
            if source.removable {
                "an updater-owned source is absent from sources.toml"
            } else {
                "an unconfigured source entry cannot be verified for pruning"
            },
        );
        check.subject = Some(source.source.clone());
        check.details.push(source.path.clone());
        if let Some(error) = &source.error {
            check.details.push(error.clone());
        }
        check.remediation = Some(if source.removable {
            "mant --prune-docs --dry-run".to_owned()
        } else {
            "inspect the reported entry manually".to_owned()
        });
    }
    if git_required {
        if let Some(path) = mant_engine::find_host_executable("git") {
            let check = builder.push(
                "tools.git",
                DoctorCheckStatus::Ok,
                "Git is available for configured sources",
            );
            check.details.push(path.to_string_lossy().into_owned());
        } else {
            let check = builder.push(
                "tools.git",
                DoctorCheckStatus::Warning,
                "Git is required by a configured source but was not found on PATH",
            );
            check.remediation = Some("install Git and ensure it is on PATH".to_owned());
        }
    }
}

fn record_paths(builder: &mut DoctorBuilder, paths: &DocumentPaths) {
    builder.environment.data_root = Some(paths.root.to_string_lossy().into_owned());
    builder.environment.config_path = Some(paths.config.to_string_lossy().into_owned());
    builder.environment.documents_root = Some(paths.documents.to_string_lossy().into_owned());
    builder.environment.sources_root = Some(paths.sources.to_string_lossy().into_owned());
}

fn inspect_data_root(builder: &mut DoctorBuilder, paths: &DocumentPaths) {
    match fs::symlink_metadata(&paths.root) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            builder.push(
                "paths.data-root",
                DoctorCheckStatus::Ok,
                "the ManT data root is a directory",
            );
        }
        Ok(_) => {
            let check = builder.push(
                "paths.data-root",
                DoctorCheckStatus::Error,
                "the ManT data root is not a directory",
            );
            check
                .details
                .push(paths.root.to_string_lossy().into_owned());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            builder.push(
                "paths.data-root",
                DoctorCheckStatus::Info,
                "the ManT data root has not been created yet",
            );
        }
        Err(error) => {
            let check = builder.push(
                "paths.data-root",
                DoctorCheckStatus::Error,
                "the ManT data root could not be inspected",
            );
            check.details.push(error.to_string());
        }
    }
}

fn inspect_registered_documents(builder: &mut DoctorBuilder) {
    match mant_sources::list_registered_documents() {
        Ok(documents) => {
            let personal = documents
                .iter()
                .filter(|document| document.origin == RegisteredDocumentOrigin::Documents)
                .count();
            let managed = documents.len().saturating_sub(personal);
            builder.push(
                "documents.registry",
                if documents.is_empty() {
                    DoctorCheckStatus::Info
                } else {
                    DoctorCheckStatus::Ok
                },
                format!("indexed {personal} personal and {managed} managed document(s)"),
            );
        }
        Err(error) => {
            let check = builder.push(
                "documents.registry",
                DoctorCheckStatus::Error,
                "the registered Markdown catalog could not be built",
            );
            check.details.push(error.to_string());
        }
    }
}

fn push_configured_source(builder: &mut DoctorBuilder, source: &ConfiguredSourceInspection) {
    let (status, message, remediation) = match source.status {
        SourceInstallationStatus::Ready => (
            DoctorCheckStatus::Ok,
            "configured source is installed and current",
            None,
        ),
        SourceInstallationStatus::Missing => (
            DoctorCheckStatus::Warning,
            "configured source is not installed",
            Some("mant --update-docs"),
        ),
        SourceInstallationStatus::Stale => (
            DoctorCheckStatus::Warning,
            "installed source does not match its active configuration",
            Some("mant --update-docs"),
        ),
        SourceInstallationStatus::Invalid => (
            DoctorCheckStatus::Warning,
            "installed source is invalid or unreadable",
            Some("mant --update-docs"),
        ),
    };
    let check = builder.push("sources.installation", status, message);
    check.subject = Some(source.source.clone());
    check.details.push(format!(
        "transport={}, priority={}",
        match source.transport {
            SourceTransport::Git => "git",
            SourceTransport::Archive => "archive",
        },
        source.priority
    ));
    if let Some(revision) = &source.revision {
        check.details.push(format!("revision={revision}"));
    }
    if let Some(documents) = source.documents {
        check.details.push(format!("documents={documents}"));
    }
    if let Some(detail) = &source.detail {
        check.details.push(detail.clone());
    }
    check.remediation = remediation.map(ToOwned::to_owned);
}

fn inspect_manuals(builder: &mut DoctorBuilder) {
    let roots = mant_engine::discover_manual_roots();
    builder.environment.manual_roots = roots
        .iter()
        .map(|root| root.to_string_lossy().into_owned())
        .collect();
    let existing = roots.iter().filter(|root| root.is_dir()).count();
    let index = mant_engine::ManualIndex::from_roots(roots);
    let pages = index.pages().len();
    let sections = index
        .pages()
        .iter()
        .map(|page| page.section.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let status = if pages > 0 {
        DoctorCheckStatus::Ok
    } else if cfg!(windows) {
        DoctorCheckStatus::Info
    } else {
        DoctorCheckStatus::Warning
    };
    let check = builder.push(
        "manuals.index",
        status,
        format!(
            "indexed {pages} native manual page(s) in {sections} section(s) from {existing} existing root(s)"
        ),
    );
    if pages == 0 && !cfg!(windows) {
        check.remediation = Some("install manual pages or set MANT_MANPATH".to_owned());
    }
}

fn inspect_tldr(builder: &mut DoctorBuilder) {
    let environment = std::env::vars().collect::<std::collections::BTreeMap<_, _>>();
    let platform = match mant_engine::HostPlatform::current() {
        Ok(platform) => platform,
        Err(error) => {
            let check = builder.push(
                "tldr.cache",
                DoctorCheckStatus::Info,
                "tldr cache conventions are unavailable on this platform",
            );
            check.details.push(error.to_string());
            return;
        }
    };
    let client = mant_engine::find_host_executable("tldr");
    let roots =
        match mant_engine::get_tldr_read_cache_dirs(&environment, platform, client.is_some()) {
            Ok(roots) => roots,
            Err(error) => {
                let check = builder.push(
                    "tldr.cache",
                    DoctorCheckStatus::Warning,
                    "tldr cache paths could not be derived",
                );
                check.details.push(error.to_string());
                return;
            }
        };
    builder.environment.tldr_roots = roots
        .iter()
        .map(|root| root.to_string_lossy().into_owned())
        .collect();
    let readable = roots.iter().filter(|root| root.is_dir()).count();
    let explicit_override = environment
        .keys()
        .any(|name| name.eq_ignore_ascii_case("MANT_TLDR_DIR"));
    let status = if readable > 0 {
        DoctorCheckStatus::Ok
    } else if explicit_override {
        DoctorCheckStatus::Warning
    } else {
        DoctorCheckStatus::Info
    };
    let check = builder.push(
        "tldr.cache",
        status,
        if readable > 0 {
            format!("found {readable} readable tldr cache root(s)")
        } else {
            "no readable tldr cache root was found".to_owned()
        },
    );
    if let Some(client) = client {
        check
            .details
            .push(format!("client={}", client.to_string_lossy()));
    }
    if readable == 0 {
        check.remediation = Some("mant --update-tldr".to_owned());
    }
}

/// Render a bounded copy-friendly report. Only fixed status labels receive
/// terminal styling; inspected values remain ordinary text.
pub(crate) fn render_text(report: &DoctorReport, color: bool) -> String {
    let mut output = String::from("ManT doctor\n\n");
    for check in &report.checks {
        let label = match check.status {
            DoctorCheckStatus::Ok => "ok",
            DoctorCheckStatus::Info => "info",
            DoctorCheckStatus::Warning => "warning",
            DoctorCheckStatus::Error => "error",
        };
        let subject = check
            .subject
            .as_deref()
            .map_or_else(String::new, |subject| {
                format!(" [{}]", terminal_safe(subject))
            });
        let code = terminal_safe(&check.code);
        let message = terminal_safe(&check.message);
        if color {
            let style = match check.status {
                DoctorCheckStatus::Ok => OK_STYLE,
                DoctorCheckStatus::Info => INFO_STYLE,
                DoctorCheckStatus::Warning => WARNING_STYLE,
                DoctorCheckStatus::Error => ERROR_STYLE,
            };
            writeln!(
                output,
                "{style}[{label}]{style:#} {code}{subject}: {message}"
            )
            .expect("writing to String cannot fail");
        } else {
            writeln!(output, "[{label}] {code}{subject}: {message}")
                .expect("writing to String cannot fail");
        }
        for detail in &check.details {
            writeln!(output, "       {}", terminal_safe(detail))
                .expect("writing to String cannot fail");
        }
        if let Some(remediation) = &check.remediation {
            let remediation = terminal_safe(remediation);
            if color {
                writeln!(
                    output,
                    "       {INFO_STYLE}hint:{INFO_STYLE:#} {remediation}"
                )
                .expect("writing to String cannot fail");
            } else {
                writeln!(output, "       hint: {remediation}")
                    .expect("writing to String cannot fail");
            }
        }
    }
    writeln!(
        output,
        "\n{} ok, {} info, {} warning(s), {} error(s)",
        report.summary.ok, report.summary.info, report.summary.warnings, report.summary.errors
    )
    .expect("writing to String cannot fail");
    output
}

fn terminal_safe(value: &str) -> String {
    let mut safe = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_control() {
            safe.extend(character.escape_default());
        } else {
            safe.push(character);
        }
    }
    safe
}

#[cfg(test)]
mod tests {
    use mant_protocol::{
        DoctorCheck, DoctorCheckStatus, DoctorEnvironment, DoctorReport, Producer,
    };

    use super::{render_text, terminal_safe};

    fn report() -> DoctorReport {
        DoctorReport::new(
            Producer {
                name: "mant".to_owned(),
                version: "0.7.1".to_owned(),
                engine: None,
            },
            DoctorEnvironment {
                os: "linux".to_owned(),
                arch: "x86_64".to_owned(),
                data_root: None,
                config_path: None,
                documents_root: None,
                sources_root: None,
                manual_roots: Vec::new(),
                tldr_roots: Vec::new(),
            },
            vec![DoctorCheck {
                code: "sources.installation".to_owned(),
                subject: Some("team".to_owned()),
                status: DoctorCheckStatus::Warning,
                message: "configured source is not installed".to_owned(),
                details: vec!["transport=git, priority=1".to_owned()],
                remediation: Some("mant --update-docs".to_owned()),
            }],
        )
    }

    #[test]
    fn text_report_is_copy_friendly_and_colors_only_terminal_labels() {
        let plain = render_text(&report(), false);
        assert!(plain.contains("[warning] sources.installation [team]"));
        assert!(plain.contains("hint: mant --update-docs"));
        assert!(!plain.contains('\u{1b}'));

        let colored = render_text(&report(), true);
        assert!(colored.contains('\u{1b}'));
        assert!(colored.contains("sources.installation [team]"));
    }

    #[test]
    fn terminal_report_escapes_dynamic_control_characters() {
        assert_eq!(
            terminal_safe("path\u{1b}[2J\nnext"),
            "path\\u{1b}[2J\\nnext"
        );
    }
}
