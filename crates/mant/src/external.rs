//! Host-owned external URI activation.

use std::{
    io,
    process::{Child, Command, Stdio},
    thread,
};

/// Hand a previously validated external URI to the platform URL handler.
pub(super) fn open_uri(uri: &str) -> Result<(), String> {
    let command = platform_command(uri);
    let child = spawn_silenced(command)
        .map_err(|error| format!("could not start the system link opener: {error}"))?;

    // URL handlers are intentionally asynchronous: some desktop launchers
    // remain alive for as long as the browser does. Reap the short-lived
    // helpers without blocking the terminal event loop.
    let _ = thread::Builder::new()
        .name("mant-link-opener".to_owned())
        .spawn(move || {
            let mut child = child;
            let _ = child.wait();
        });
    Ok(())
}

fn spawn_silenced(mut command: Command) -> io::Result<Child> {
    // A child inheriting these streams can consume TUI input or write ordinary
    // process diagnostics directly into the alternate screen.
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

fn platform_command(uri: &str) -> Command {
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("rundll32.exe");
        command.args(["url.dll,FileProtocolHandler", uri]);
        command
    }

    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("open");
        command.arg(uri);
        command
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let wsl_handler = running_under_wsl().then(wsl_windows_url_handler).flatten();
        linux_command(uri, wsl_handler.as_deref())
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_command(uri: &str, wsl_handler: Option<&std::path::Path>) -> Command {
    if let Some(program) = wsl_handler {
        let mut command = Command::new(program);
        command.args(["url.dll,FileProtocolHandler", uri]);
        return command;
    }

    let mut command = Command::new("xdg-open");
    command.arg(uri);
    command
}

#[cfg(all(unix, not(target_os = "macos")))]
fn running_under_wsl() -> bool {
    std::env::var_os("WSL_INTEROP").is_some()
        || std::env::var_os("WSL_DISTRO_NAME").is_some()
        || std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .is_ok_and(|release| release_identifies_wsl(&release))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn release_identifies_wsl(release: &str) -> bool {
    let release = release.to_ascii_lowercase();
    release.contains("microsoft") || release.contains("wsl")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn wsl_windows_url_handler() -> Option<std::path::PathBuf> {
    const DEFAULT_HANDLER: &str = "/mnt/c/Windows/System32/rundll32.exe";

    let default = std::path::PathBuf::from(DEFAULT_HANDLER);
    if default.is_file() {
        return Some(default);
    }

    // `wslpath` is supplied by WSL itself and accounts for a customized drive
    // mount root. Keep its diagnostics away from the active terminal too.
    let output = Command::new("wslpath")
        .args(["-u", r"C:\Windows\System32\rundll32.exe"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = std::str::from_utf8(&output.stdout).ok()?.trim();
    let path = std::path::PathBuf::from(path);
    path.is_file().then_some(path)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    const STREAM_TEST_MODE: &str = "MANT_EXTERNAL_STREAM_TEST";
    const STREAM_SENTINEL: &str = "mant-external-stream-sentinel";
    const STREAM_TEST_NAME: &str =
        "external::tests::system_opener_does_not_inherit_process_streams";

    #[test]
    fn system_opener_does_not_inherit_process_streams() {
        match std::env::var(STREAM_TEST_MODE).as_deref() {
            Ok("emit") => {
                println!("{STREAM_SENTINEL}-stdout");
                eprintln!("{STREAM_SENTINEL}-stderr");
                std::io::stdout().flush().expect("flush stdout");
                std::io::stderr().flush().expect("flush stderr");
            }
            Ok("launch") => {
                let mut command = Command::new(std::env::current_exe().expect("test binary"));
                command
                    .args(["--exact", STREAM_TEST_NAME, "--nocapture"])
                    .env(STREAM_TEST_MODE, "emit");
                let mut child = spawn_silenced(command).expect("launch emitting child");
                assert!(child.wait().expect("wait for emitting child").success());
            }
            _ => {
                let output = Command::new(std::env::current_exe().expect("test binary"))
                    .args(["--exact", STREAM_TEST_NAME, "--nocapture"])
                    .env(STREAM_TEST_MODE, "launch")
                    .output()
                    .expect("run stream-isolation driver");
                assert!(output.status.success());
                assert!(!String::from_utf8_lossy(&output.stdout).contains(STREAM_SENTINEL));
                assert!(!String::from_utf8_lossy(&output.stderr).contains(STREAM_SENTINEL));
            }
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn wsl_detection_accepts_both_kernel_release_conventions() {
        assert!(release_identifies_wsl("6.18.33.2-microsoft-standard-WSL2"));
        assert!(release_identifies_wsl("4.4.0-19041-Microsoft"));
        assert!(!release_identifies_wsl("6.12.10-arch1-1"));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_command_prefers_the_supplied_wsl_host_handler() {
        let program = std::path::Path::new("/windows/System32/rundll32.exe");
        let command = linux_command("https://example.test/docs", Some(program));

        assert_eq!(command.get_program(), program.as_os_str());
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["url.dll,FileProtocolHandler", "https://example.test/docs"]
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn ordinary_linux_command_uses_xdg_open() {
        let command = linux_command("https://example.test/docs", None);

        assert_eq!(command.get_program(), "xdg-open");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["https://example.test/docs"]
        );
    }
}
