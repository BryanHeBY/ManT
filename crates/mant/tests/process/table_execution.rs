//! Real process stdout is checked independently of the rendering library.
use super::process_support::run_text_input;
#[path = "../../../mant-engine/tests/support/table_execution.rs"]
mod cases;

#[test]
fn table_followers_reach_the_actual_plain_and_ansi_cli() {
    for case in cases::cases() {
        let mut outputs = Vec::new();
        for color in ["never", "always"] {
            let result = run_text_input(
                &[
                    "--input",
                    "-",
                    "--input-format",
                    "roff",
                    "--format",
                    "text",
                    "--display",
                    "direct",
                    "--color",
                    color,
                ],
                &case.source,
            );
            assert!(result.status.success(), "{}: {result:?}", case.id);
            let text = String::from_utf8(result.stdout).unwrap();
            let accepted = if color == "always" {
                super::execution_scopes::without_sgr(&text)
            } else {
                text.clone()
            };
            cases::assert_follower(&accepted, &case);
            outputs.push(accepted);
        }
        assert_eq!(outputs[0], outputs[1], "{}", case.id);
    }
}

#[cfg(unix)]
#[test]
fn actual_40_and_78_column_terminals_keep_positive_and_surviving_controls() {
    let selected=cases::cases().into_iter().filter(|case|case.id.starts_with("T1-") && ((case.id.contains("Q01") || case.id.contains("Q03"))&&case.id.ends_with("-nofill") || case.id.contains("Q02")&&case.id.ends_with("-link"))).map(|case|serde_json::json!({"id":case.id,"source":case.source,"expected_label":case.expected_label})).collect::<Vec<_>>();
    assert_eq!(selected.len(), 6);
    let mut child = std::process::Command::new("python3")
        .args([
            "-c",
            include_str!("../support/table_execution_pty.py"),
            super::process_support::executable(),
            &serde_json::to_string(&selected).unwrap(),
        ])
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(45);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("table PTY deadline");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
