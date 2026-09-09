use super::*;
use std::fs;

fn absolute(name: &str) -> PathBuf {
    std::env::current_dir().unwrap().join(name)
}

#[test]
fn both_dialects_report_work_exhaustion_and_keep_only_completed_roots() {
    let root = std::env::temp_dir().join(format!("mant-bsd-budget-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    for i in 0..80 {
        fs::write(root.join(format!("{i}.conf")), "").unwrap();
    }
    let config = root.join("man.conf");
    for bsd in [false, true] {
        let directive = if bsd { "MANPATH" } else { "manpath" };
        for pattern in [
            root.join("*.missing"),
            PathBuf::from("segment/".repeat(50) + "final"),
        ] {
            fs::write(
                &config,
                format!(
                    "{directive} first\n{directive} {}\n{directive} omitted\n",
                    pattern.display()
                ),
            )
            .unwrap();
            let result = load(&config, bsd, 40);
            assert_eq!(result.roots, vec![absolute("first")]);
            assert_eq!(result.diagnostics.len(), 1);
            assert_eq!(result.diagnostics[0].config_path, config);
            assert!(result.diagnostics[0].message.contains("budget"));
        }
        fs::write(
            &config,
            format!(
                "{directive} first\n{directive} second\nMANCONFIG {}/absent*.fragment\n",
                root.display()
            ),
        )
        .unwrap();
        let result = load(&config, bsd, 4096);
        assert_eq!(result.roots, vec![absolute("first"), absolute("second")]);
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn macos_fragments_share_work_budget_and_report_fragment_limit() {
    let root = std::env::temp_dir().join(format!("mant-bsd-fragments-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let config = root.join("man.conf");
    fs::write(
        &config,
        format!("MANPATH first\nMANCONFIG {}/*.fragment\n", root.display()),
    )
    .unwrap();
    for i in 0..257 {
        fs::write(
            root.join(format!("{i:03}.fragment")),
            format!("MANPATH root-{i}\n"),
        )
        .unwrap();
    }
    let result = load(&config, true, 40);
    assert_eq!(result.roots, vec![absolute("first")]);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("MANCONFIG expansion"))
    );
    let result = load(&config, true, 4096);
    assert_eq!(result.roots.len(), 257); // primary plus 256 fragments
    assert!(!result.roots.contains(&absolute("root-256")));
    assert_eq!(result.diagnostics.len(), 1);
    assert!(result.diagnostics[0].message.contains("256-fragment"));
    // Expansion fits, but the first fragment's directives exhaust the *same*
    // budget. Its finding must identify that fragment, not only the parent.
    fs::write(root.join("000.fragment"), "MANPATH kept\n".repeat(4096)).unwrap();
    let result = load(&config, true, 4096);
    assert!(result.roots.contains(&absolute("kept")));
    assert!(!result.roots.contains(&absolute("root-1")));
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.config_path == root.join("000.fragment") && d.message.contains("budget"))
    );
    fs::remove_dir_all(root).unwrap();
}
