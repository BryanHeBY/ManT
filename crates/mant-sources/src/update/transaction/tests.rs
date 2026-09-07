use super::*;

struct FailingIo {
    step: usize,
    failures: Vec<usize>,
}
impl FailingIo {
    fn checkpoint(&mut self) -> std::io::Result<()> {
        self.step += 1;
        if self.failures.contains(&self.step) {
            Err(std::io::Error::other(format!(
                "injected failure {}",
                self.step
            )))
        } else {
            Ok(())
        }
    }
}
impl ActivationIo for FailingIo {
    fn rename(&mut self, from: &Path, to: &Path) -> std::io::Result<()> {
        self.checkpoint()?;
        fs::rename(from, to)
    }
    fn sync_parent(&mut self, path: &Path) -> Result<(), String> {
        self.checkpoint().map_err(|e| e.to_string())?;
        sync_parent_directory(path)
    }
}

#[test]
fn every_activation_boundary_leaves_a_complete_recoverable_installation() {
    let root = std::env::temp_dir().join(format!("mant-activation-faults-{}", std::process::id()));
    // 1: preserve; 2: sync backup; 3: activate; 4: sync activation;
    // 5: sync cleanup. After activation, an error may still expose the complete
    // new tree: an fsync error cannot truthfully promise rollback/durability.
    for failures in [
        vec![1],
        vec![2],
        vec![3],
        vec![4],
        vec![5],
        vec![3, 4],
        vec![3, 5],
        vec![],
    ] {
        let case = root.join(format!("{failures:?}"));
        let target = case.join("installed");
        let staging = case.join("staging");
        let backup = target.with_extension("backup");
        for (path, value) in [(&target, "old"), (&staging, "new")] {
            fs::create_dir_all(path).unwrap();
            fs::write(path.join("document.md"), value).unwrap();
            fs::write(path.join(SOURCE_METADATA_FILE), value).unwrap();
        }
        let mut ops = FailingIo {
            step: 0,
            failures: failures.clone(),
        };
        let result = replace_with(&staging, &target, &mut ops);
        assert_eq!(result.is_ok(), failures.is_empty());
        if failures.first() == Some(&3) {
            assert!(
                result
                    .unwrap_err()
                    .contains("could not activate updated source")
            );
        }
        let expected = if failures.is_empty() || failures[0] >= 4 {
            "new"
        } else {
            "old"
        };
        assert!(target.exists() || backup.exists());
        recover_directory(&target).unwrap();
        assert_eq!(
            fs::read_to_string(target.join("document.md")).unwrap(),
            expected
        );
        assert_eq!(
            fs::read_to_string(target.join(SOURCE_METADATA_FILE)).unwrap(),
            expected
        );
        assert!(!backup.exists());
        // Retrying recovery is idempotent after every failure point.
        recover_directory(&target).unwrap();
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn recovery_rename_failure_keeps_backup_for_retry() {
    let root = std::env::temp_dir().join(format!("mant-recovery-fault-{}", std::process::id()));
    let target = root.join("installed");
    let backup = target.with_extension("backup");
    fs::create_dir_all(&backup).unwrap();
    fs::write(backup.join("document.md"), "old").unwrap();
    let mut ops = FailingIo {
        step: 0,
        failures: vec![1],
    };
    assert!(
        recover_with(&target, &mut ops)
            .unwrap_err()
            .contains("recover previous source")
    );
    assert!(!target.exists());
    assert_eq!(
        fs::read_to_string(backup.join("document.md")).unwrap(),
        "old"
    );
    recover_directory(&target).unwrap();
    assert_eq!(
        fs::read_to_string(target.join("document.md")).unwrap(),
        "old"
    );
    fs::remove_dir_all(root).unwrap();
}
