//! System clipboard integration for the interactive reader.

use std::{ffi::OsStr, io::Write};

use base64::Engine as _;
use mant_ui::{CopyFormat, CopyRequest, MAX_COPY_BYTES};

#[derive(Default)]
pub(super) struct SystemClipboard {
    clipboard: Option<arboard::Clipboard>,
}

impl SystemClipboard {
    pub(super) fn copy(&mut self, request: CopyRequest) -> Result<(), String> {
        let text = render_copy_request(request)?;
        if text.len() > MAX_COPY_BYTES {
            return Err("clipboard content exceeds the 4 MiB limit".to_owned());
        }

        deliver_clipboard(
            should_prefer_terminal_clipboard(),
            || self.copy_native(&text),
            || write_terminal_clipboard(&text),
        )
    }

    fn copy_native(&mut self, text: &str) -> Result<(), String> {
        let clipboard = match &mut self.clipboard {
            Some(clipboard) => clipboard,
            None => self.clipboard.insert(
                arboard::Clipboard::new()
                    .map_err(|error| format!("could not access the native clipboard: {error}"))?,
            ),
        };
        clipboard
            .set_text(text.to_owned())
            .map_err(|error| format!("could not write the native clipboard: {error}"))
    }
}

fn deliver_clipboard<N, T>(
    prefer_terminal: bool,
    mut copy_native: N,
    mut copy_through_terminal: T,
) -> Result<(), String>
where
    N: FnMut() -> Result<(), String>,
    T: FnMut() -> Result<(), String>,
{
    if prefer_terminal {
        return copy_through_terminal();
    }

    match copy_native() {
        Ok(()) => Ok(()),
        Err(native_error) => copy_through_terminal().map_err(|terminal_error| {
            format!("{native_error}; terminal clipboard fallback failed: {terminal_error}")
        }),
    }
}

fn write_terminal_clipboard(text: &str) -> Result<(), String> {
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    write_osc52(&mut stdout, text.as_bytes())
        .map_err(|error| format!("could not send clipboard content to the terminal: {error}"))
}

fn write_osc52(writer: &mut impl Write, bytes: &[u8]) -> std::io::Result<()> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    writer.write_all(b"\x1b]52;c;")?;
    writer.write_all(encoded.as_bytes())?;
    writer.write_all(b"\x07")?;
    writer.flush()
}

#[cfg(any(target_os = "linux", test))]
fn contains_wsl_marker(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("microsoft") || lower.contains("wsl2") || lower.contains("-wsl")
}

#[cfg(any(target_os = "linux", test))]
fn is_wsl_for_env(
    os_release: Option<&str>,
    proc_version: Option<&str>,
    wsl_distro_name: Option<&OsStr>,
    wsl_interop: Option<&OsStr>,
    runtime_marker_exists: bool,
) -> bool {
    wsl_distro_name.is_some()
        || wsl_interop.is_some()
        || os_release.is_some_and(contains_wsl_marker)
        || proc_version.is_some_and(contains_wsl_marker)
        || runtime_marker_exists
}

#[cfg(target_os = "linux")]
fn is_wsl() -> bool {
    let os_release = std::fs::read_to_string("/proc/sys/kernel/osrelease").ok();
    let proc_version = std::fs::read_to_string("/proc/version").ok();
    is_wsl_for_env(
        os_release.as_deref(),
        proc_version.as_deref(),
        std::env::var_os("WSL_DISTRO_NAME").as_deref(),
        std::env::var_os("WSL_INTEROP").as_deref(),
        std::path::Path::new("/run/WSL").exists()
            || std::path::Path::new("/proc/sys/fs/binfmt_misc/WSLInterop").exists(),
    )
}

#[cfg(not(target_os = "linux"))]
const fn is_wsl() -> bool {
    false
}

fn should_prefer_terminal_clipboard_for_env(
    ssh_connection: Option<&OsStr>,
    ssh_tty: Option<&OsStr>,
    vscode_ipc_hook_cli: Option<&OsStr>,
    wsl: bool,
) -> bool {
    ssh_connection.is_some() || ssh_tty.is_some() || vscode_ipc_hook_cli.is_some() || wsl
}

fn should_prefer_terminal_clipboard() -> bool {
    should_prefer_terminal_clipboard_for_env(
        std::env::var_os("SSH_CONNECTION").as_deref(),
        std::env::var_os("SSH_TTY").as_deref(),
        std::env::var_os("VSCODE_IPC_HOOK_CLI").as_deref(),
        is_wsl(),
    )
}

fn render_copy_request(request: CopyRequest) -> Result<String, String> {
    match request {
        CopyRequest::Selection { text } => Ok(text),
        CopyRequest::Node {
            content,
            selector,
            format,
        } => {
            let excerpt = mant_engine::select_excerpt(content.as_ref(), &[selector])
                .map_err(|error| format!("could not select the current node: {error}"))?;
            Ok(match format {
                CopyFormat::Text => mant_engine::render_excerpt_text(&excerpt),
                CopyFormat::Markdown => mant_engine::render_excerpt_markdown(&excerpt),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, ffi::OsStr, io, sync::Arc};

    use mant_ir::ResolvedContent;
    use mant_protocol::NodeSelector;

    use super::{
        CopyFormat, CopyRequest, deliver_clipboard, is_wsl_for_env, render_copy_request,
        should_prefer_terminal_clipboard_for_env, write_osc52,
    };

    #[test]
    fn requests_reuse_deterministic_semantic_renderers() {
        let content = Arc::new(
            mant_engine::query_markdown_text(
                "# Demo\n\n## Options\n\nUse `--help` for details.\n",
                Some("demo.md".to_owned()),
            )
            .expect("Markdown fixture"),
        );

        let text = render_copy_request(CopyRequest::Node {
            content: Arc::clone(&content),
            selector: NodeSelector::new("options"),
            format: CopyFormat::Text,
        })
        .expect("text node");
        let markdown = render_copy_request(CopyRequest::Node {
            content,
            selector: NodeSelector::new("options"),
            format: CopyFormat::Markdown,
        })
        .expect("Markdown node");

        assert!(text.contains("Options"));
        assert!(text.contains("--help"));
        assert!(markdown.contains("## Options"));
        assert!(markdown.contains("`--help`"));
    }

    #[test]
    fn visual_requests_are_not_reinterpreted() {
        let text = "rendered  text\nwithout Markdown reconstruction".to_owned();
        assert_eq!(
            render_copy_request(CopyRequest::Selection { text: text.clone() }).expect("selection"),
            text
        );
    }

    #[test]
    fn an_unknown_semantic_node_fails_before_clipboard_access() {
        let content = Arc::new(ResolvedContent {
            address: None,
            label: "empty".to_owned(),
            document: None,
            tldr: None,
        });
        let error = render_copy_request(CopyRequest::Node {
            content,
            selector: NodeSelector::new("missing"),
            format: CopyFormat::Text,
        })
        .expect_err("unknown node");

        assert!(error.starts_with("could not select the current node:"));
    }

    #[test]
    fn osc52_uses_the_terminal_clipboard_and_bel_terminator() {
        let mut output = Vec::new();
        write_osc52(&mut output, "café 日本 😀".as_bytes()).expect("OSC 52 write");

        assert_eq!(output, b"\x1b]52;c;Y2Fmw6kg5pel5pysIPCfmIA=\x07");
    }

    #[test]
    fn remote_routes_skip_native_clipboard_access() {
        let native_called = Cell::new(false);
        let terminal_called = Cell::new(false);

        deliver_clipboard(
            true,
            || {
                native_called.set(true);
                Ok(())
            },
            || {
                terminal_called.set(true);
                Ok(())
            },
        )
        .expect("terminal delivery");

        assert!(!native_called.get());
        assert!(terminal_called.get());
    }

    #[test]
    fn local_routes_fall_back_only_after_native_failure() {
        let terminal_called = Cell::new(false);
        deliver_clipboard(
            false,
            || Ok(()),
            || {
                terminal_called.set(true);
                Ok(())
            },
        )
        .expect("native delivery");
        assert!(!terminal_called.get());

        deliver_clipboard(false, || Err("native unavailable".to_owned()), || Ok(()))
            .expect("terminal fallback");
    }

    #[test]
    fn delivery_preserves_both_failures() {
        let error = deliver_clipboard(
            false,
            || Err("native unavailable".to_owned()),
            || Err(io::Error::other("terminal closed").to_string()),
        )
        .expect_err("both routes fail");

        assert!(error.contains("native unavailable"));
        assert!(error.contains("terminal clipboard fallback failed: terminal closed"));
    }

    #[test]
    fn remote_and_wsl_environments_prefer_terminal_delivery() {
        assert!(should_prefer_terminal_clipboard_for_env(
            Some(OsStr::new("1 2 3 4")),
            None,
            None,
            false,
        ));
        assert!(should_prefer_terminal_clipboard_for_env(
            None,
            Some(OsStr::new("/dev/pts/1")),
            None,
            false,
        ));
        assert!(should_prefer_terminal_clipboard_for_env(
            None,
            None,
            Some(OsStr::new("/tmp/vscode-remote.sock")),
            false,
        ));
        assert!(should_prefer_terminal_clipboard_for_env(
            None, None, None, true,
        ));
        assert!(!should_prefer_terminal_clipboard_for_env(
            None, None, None, false,
        ));
    }

    #[test]
    fn wsl_detection_accepts_environment_kernel_and_runtime_markers() {
        assert!(is_wsl_for_env(
            None,
            None,
            Some(OsStr::new("Ubuntu")),
            None,
            false,
        ));
        assert!(is_wsl_for_env(
            Some("5.15.167.4-microsoft-standard-WSL2"),
            None,
            None,
            None,
            false,
        ));
        assert!(is_wsl_for_env(None, None, None, None, true));
        assert!(!is_wsl_for_env(None, None, None, None, false));
    }
}
