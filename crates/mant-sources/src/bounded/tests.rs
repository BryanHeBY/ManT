use super::read_file_utf8;
use std::fs;

#[test]
fn regular_files_are_size_and_encoding_bounded() {
    let path = std::env::temp_dir().join(format!("mant-source-bounded-{}", std::process::id()));
    fs::write(&path, b"abcd").unwrap();
    assert_eq!(read_file_utf8(&path, 4, "test", true).unwrap(), "abcd");
    assert!(read_file_utf8(&path, 3, "test", true).is_err());
    fs::write(&path, [0xff]).unwrap();
    assert!(read_file_utf8(&path, 4, "test", true).is_err());
    fs::remove_file(path).unwrap();
}

#[cfg(unix)]
#[test]
fn source_config_and_metadata_skip_fifos_without_waiting() {
    use std::{
        os::unix::fs::symlink,
        path::PathBuf,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    struct Guard(PathBuf, Option<std::process::Child>);
    impl Drop for Guard {
        fn drop(&mut self) {
            if let Some(child) = &mut self.1 {
                let _ = child.kill();
                let _ = child.wait();
            }
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    if let Some(root) = std::env::var_os("MANT_SOURCE_FIFO_PROBE") {
        let root = PathBuf::from(root);
        let fifo = root.join("sources.toml");
        assert!(crate::config::load_source_config_from(&fifo).is_err());
        assert!(read_file_utf8(&fifo, 1024, "metadata", false).is_err());
        assert!(super::open_checked(&fifo, 1024, true).is_err());
        assert!(super::open_checked(&fifo, 1024, false).is_err());
        let regular = root.join("regular");
        fs::write(&regular, b"# configuration\n").unwrap();
        let link = root.join("link");
        symlink(&regular, &link).unwrap();
        assert!(read_file_utf8(&link, 1024, "config", true).is_ok());
        assert!(read_file_utf8(&link, 1024, "metadata", false).is_err());
        assert!(super::open_checked(&link, 1024, false).is_err());
        assert!(read_file_utf8(&root, 1024, "config", true).is_err());
        fs::write(root.join("completed"), b"ok").unwrap();
        return;
    }
    let root = std::env::temp_dir().join(format!("mant-source-fifo-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let mut guard = Guard(root.clone(), None);
    assert!(
        Command::new("mkfifo")
            .arg(root.join("sources.toml"))
            .status()
            .unwrap()
            .success()
    );
    guard.1 = Some(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "bounded::tests::source_config_and_metadata_skip_fifos_without_waiting",
                "--nocapture",
            ])
            .env("MANT_SOURCE_FIFO_PROBE", &root)
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let start = Instant::now();
    loop {
        if let Some(status) = guard.1.as_mut().unwrap().try_wait().unwrap() {
            assert!(status.success());
            assert_eq!(fs::read(root.join("completed")).unwrap(), b"ok");
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "source configuration FIFO blocked"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
