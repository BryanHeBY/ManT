use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use super::{open_regular_file, read_text};

struct Fixture(PathBuf);

impl Fixture {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "mant-config-{label}-{}-{nonce}",
            std::process::id(),
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn regular_config_reads_enforce_type_size_and_utf8() {
    let fixture = Fixture::new("regular");
    let path = fixture.0.join("man.conf");
    fs::write(&path, b"abcd").unwrap();
    assert_eq!(read_text(&path, 4).unwrap(), "abcd");
    assert!(read_text(&path, 3).is_err());
    // Also enforce size/type at the open stage, after any path precheck.
    assert!(open_regular_file(&path, 3).is_err());
    assert!(open_regular_file(&fixture.0, 16).is_err());
    assert!(read_text(&fixture.0, 16).is_err());
    assert!(read_text(&fixture.0.join("missing"), 16).is_err());
    fs::write(&path, []).unwrap();
    assert_eq!(read_text(&path, 0).unwrap(), "");
    fs::write(&path, [0xff]).unwrap();
    assert!(read_text(&path, 16).is_err());
    fs::File::create(&path)
        .unwrap()
        .set_len(super::super::MAX_MANUAL_PATH_CONFIG_BYTES + 1)
        .unwrap();
    assert!(
        read_text(&path, u64::MAX).is_err(),
        "per-file cap is always enforced"
    );
}

#[cfg(unix)]
#[test]
fn fifo_configs_are_skipped_without_blocking_discovery() {
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let fixture = Fixture::new("fifo");
    let fifo = fixture.0.join("pipe.conf");
    // Test fixture construction only; ordinary manual discovery spawns nothing.
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let mut child = ChildGuard(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "manual_paths::config_file::tests::fifo_config_probe",
                "--ignored",
                "--nocapture",
            ])
            .env("MANT_CONFIG_FIFO_PROBE", &fixture.0)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let started = Instant::now();
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success(), "FIFO discovery probe failed: {status}");
            assert_eq!(
                fs::read(fixture.0.join("completed")).unwrap(),
                b"ok",
                "the isolated probe must actually execute, not match zero tests"
            );
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "configuration discovery blocked on a FIFO without a writer"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
#[test]
#[ignore = "isolated probe invoked by the timeout-protected parent test"]
fn fifo_config_probe() {
    use std::os::unix::fs::symlink;

    let root = PathBuf::from(std::env::var_os("MANT_CONFIG_FIFO_PROBE").expect("probe root"));
    let fifo = root.join("pipe.conf");
    assert!(read_text(&fifo, 1024).is_err());
    // Deterministically exercise the state seen if a regular path becomes a
    // FIFO after its precheck. Removing O_NONBLOCK must trip the parent timeout.
    assert!(open_regular_file(&fifo, 1024).is_err());
    let fifo_link = root.join("pipe-link.conf");
    symlink(&fifo, &fifo_link).unwrap();
    assert!(read_text(&fifo_link, 1024).is_err());
    assert!(open_regular_file(&fifo_link, 1024).is_err());

    let primary = root.join("primary");
    let port = root.join("port");
    let fragments = root.join("man.d");
    for path in [&primary, &port, &fragments] {
        fs::create_dir(path).unwrap();
    }
    fs::rename(&fifo, fragments.join("00-pipe.conf")).unwrap();
    let regular = fragments.join("99-tool.conf");
    let contents = format!("MANPATH {}\n", port.display());
    fs::write(&regular, &contents).unwrap();
    let link = fragments.join("98-tool-link.conf");
    symlink(&regular, &link).unwrap();
    assert_eq!(read_text(&link, 4096).unwrap(), contents);
    fs::create_dir(fragments.join("01-directory.conf")).unwrap();
    let config = root.join("man.conf");
    fs::write(
        &config,
        format!(
            "MANPATH {}\nMANCONFIG {}/*.conf\n",
            primary.display(),
            fragments.display()
        ),
    )
    .unwrap();
    assert_eq!(
        super::super::macos_configuration_roots(&config).roots,
        vec![primary, port]
    );
    assert!(super::super::read_config(&fragments.join("00-pipe.conf")).is_none());
    fs::write(root.join("completed"), b"ok").unwrap();
}
