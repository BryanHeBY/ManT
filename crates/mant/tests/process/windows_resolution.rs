use super::process_support::{executable, registered_data_root};
use super::support::{configure_registered_documents, registered_documents_dir};
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

#[test]
fn directory_bridge_rejects_implicit_drive_paths_and_accepts_relative_separators() {
    let root = std::env::temp_dir().join(format!(
        "mant-windows-directory-bridge-{}",
        std::process::id()
    ));
    let config = root.join("config");
    fs::create_dir_all(&config).unwrap();
    let run = || {
        let mut command = Command::new(executable());
        configure_registered_documents(&mut command, &root);
        command
            .env_remove("MANT_DATA_HOME")
            .args(["--installer-paths", "data"])
            .output()
            .unwrap()
    };
    for path in [
        r"C:manuals",
        r"\manuals",
        "/manuals",
        r"\\server",
        r"\\.\pipe\mant",
    ] {
        fs::write(
            config.join("mant.toml"),
            format!("[paths]\ndata_home = {path:?}\n"),
        )
        .unwrap();
        let output = run();
        assert_eq!(output.status.code(), Some(1), "{path}: {output:?}");
        assert_eq!(output.stdout, [] as [u8; 0]);
        assert!(String::from_utf8_lossy(&output.stderr).contains("configuration path"));
    }
    let expected = config.join("relative/data");
    for path in ["relative/data", r"relative\data"] {
        fs::write(
            config.join("mant.toml"),
            format!("[paths]\ndata_home = {path:?}\n"),
        )
        .unwrap();
        let output = run();
        assert!(output.status.success(), "{output:?}");
        let resolved = PathBuf::from(String::from_utf8(output.stdout).unwrap().trim());
        assert_eq!(
            mant_sources::settings::windows_path_key(&resolved),
            mant_sources::settings::windows_path_key(&expected)
        );
        assert!(!expected.exists());
    }
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
fn windows_suffix_fixture() -> PathBuf {
    let fixture_root = std::env::temp_dir().join(format!(
        "mant-windows-suffix-process-{}",
        std::process::id()
    ));
    let documents = registered_documents_dir(&fixture_root);
    let data_root = registered_data_root(&fixture_root);
    let sources = data_root.join("sources");
    let alpha = sources.join("alpha");
    let beta = sources.join("beta");
    fs::create_dir_all(&documents).expect("create personal documents");
    fs::create_dir_all(&alpha).expect("create alpha source");
    fs::create_dir_all(&beta).expect("create beta source");
    fs::create_dir_all(fixture_root.join("config")).expect("create configuration directory");
    fs::write(
        fixture_root.join("config/sources.toml"),
        "[alpha]\nrepo = \"https://example.invalid/alpha.git\"\nbranch = \"main\"\n\n[beta]\nrepo = \"https://example.invalid/beta.git\"\nbranch = \"main\"\n",
    )
    .expect("write source config");
    fs::write(alpha.join(".mant-source.toml"), "revision = \"alpha\"\n")
        .expect("mark alpha installed");
    fs::write(beta.join(".mant-source.toml"), "revision = \"beta\"\n")
        .expect("mark beta installed");

    let document = |name: &str, title: &str| {
        fs::write(documents.join(name), format!("# {title}\n\nBody.\n"))
            .expect("write root suffix fixture");
    };
    document("priority.md", "Exact");
    document("priority.vbs.md", "Priority VBS");
    for (suffix, title) in [
        ("vbs", "VBS"),
        ("msc", "MSC"),
        ("exe", "EXE"),
        ("com", "COM"),
    ] {
        document(&format!("ordered.{suffix}.md"), title);
    }
    document("defaulted.cmd.md", "Default CMD");
    fs::write(alpha.join("scoped.vbs.md"), "# Alpha VBS\n\nBody.\n")
        .expect("write alpha suffix fixture");
    fs::write(beta.join("foreign.vbs.md"), "# Beta VBS\n\nBody.\n")
        .expect("write beta suffix fixture");
    fixture_root
}

#[cfg(windows)]
fn query_windows_suffix(
    fixture_root: &std::path::Path,
    name: &str,
    pathext: Option<&str>,
    source: Option<&str>,
) -> std::process::Output {
    let mut command = Command::new(executable());
    configure_registered_documents(&mut command, fixture_root);
    match pathext {
        Some(value) => {
            command.env("PATHEXT", value);
        }
        None => {
            command.env_remove("PATHEXT");
        }
    }
    command.arg(name);
    if let Some(source) = source {
        command.args(["--source", source]);
    }
    command
        .args(["--format", "json", "--compact"])
        .output()
        .expect("query Windows suffix fixture")
}

#[cfg(windows)]
fn document_title(output: &std::process::Output) -> String {
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice::<serde_json::Value>(&output.stdout)
        .expect("suffix query JSON")["document"]["heading"]["content"][0]["value"]
        .as_str()
        .expect("document title")
        .to_owned()
}

#[cfg(windows)]
fn request_windows_suffix(fixture_root: &std::path::Path) -> std::process::Output {
    let mut child = Command::new(executable());
    configure_registered_documents(&mut child, fixture_root);
    let mut child = child
        .env("PATHEXT", ".MSC;.VBS;.EXE;.COM")
        .args(["--request-json", "--format", "json", "--compact"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start Windows suffix request");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            br#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"ordered"},"view":{"kind":"full"}}"#,
        )
        .expect("write Windows suffix request");
    child.wait_with_output().expect("wait for suffix request")
}

#[cfg(windows)]
#[test]
fn windows_frontends_follow_pathext_without_crossing_source_boundaries() {
    let fixture_root = windows_suffix_fixture();
    let query = |name, pathext, source| query_windows_suffix(&fixture_root, name, pathext, source);

    assert_eq!(
        document_title(&query("priority", Some(".VBS;.EXE"), None)),
        "Exact"
    );
    for (first, expected) in [
        (".VBS;.MSC;.EXE;.COM", "VBS"),
        (".MSC;.EXE;.COM;.VBS", "MSC"),
        (".EXE;.COM;.VBS;.MSC", "EXE"),
        (".COM;.VBS;.MSC;.EXE", "COM"),
    ] {
        assert_eq!(
            document_title(&query("ordered", Some(first), None)),
            expected
        );
    }
    assert_eq!(
        document_title(&query("ordered.EXE", Some(".VBS;.MSC"), None)),
        "EXE"
    );
    assert_eq!(
        document_title(&query("defaulted", None, None)),
        "Default CMD"
    );
    assert_eq!(
        document_title(&query("defaulted", Some(""), None)),
        "Default CMD"
    );
    assert_eq!(
        document_title(&query("scoped", Some(".VBS;.EXE"), Some("alpha"))),
        "Alpha VBS"
    );
    let foreign = query("foreign", Some(".VBS"), Some("alpha"));
    assert_eq!(foreign.status.code(), Some(1));

    assert_eq!(
        document_title(&request_windows_suffix(&fixture_root)),
        "MSC"
    );

    fs::remove_dir_all(fixture_root).expect("remove Windows suffix fixture");
}

#[cfg(windows)]
#[test]
fn windows_script_host_accepts_documented_double_slash_options() {
    let fixture =
        std::env::temp_dir().join(format!("mant-wsh-options-process-{}", std::process::id()));
    fs::create_dir_all(&fixture).expect("create WSH fixture");
    let script = fixture.join("check.vbs");
    let marker = fixture.join("executed.txt");
    fs::write(
        &script,
        concat!(
            "Set output = CreateObject(\"Scripting.FileSystemObject\")",
            ".CreateTextFile(WScript.Arguments(0), True)\r\n",
            "output.Write \"mant-wsh-double-slash\"\r\n",
            "output.Close\r\n",
        ),
    )
    .expect("write WSH fixture");
    let output = Command::new("cscript.exe")
        .args(["//B", "//Nologo"])
        .arg(&script)
        .arg(&marker)
        .output()
        .expect("run Windows Script Host");

    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read_to_string(&marker).expect("read WSH marker"),
        "mant-wsh-double-slash"
    );
    fs::remove_dir_all(fixture).expect("remove WSH fixture");
}
