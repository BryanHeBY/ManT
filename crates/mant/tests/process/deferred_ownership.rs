//! Real CLI color policy never changes source membership or native hard rows.
use super::process_support::run_text_input;
use serde_json::Value;
#[path = "deferred_ownership/styles.rs"]
mod styles;

fn selected() -> Vec<Value> {
    let all: Value = serde_json::from_str(include_str!(
        "../../../mant-engine/tests/roff_lowering/deferred_ownership/cases.json"
    ))
    .unwrap();
    let selected: Value = serde_json::from_str(include_str!(
        "../../../mant-engine/tests/roff_lowering/deferred_ownership/consumers.json"
    ))
    .unwrap();
    let cases = selected["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|selected| {
            all["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|case| case["id"] == selected["id"])
                .unwrap()
                .clone()
        })
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 42);
    cases
}

#[test]
fn deferred_recovery_members_keep_exact_plain_and_ansi_cli_rows() {
    for case in selected() {
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
                case["source"].as_str().unwrap(),
            );
            assert!(result.status.success(), "{} {result:?}", case["id"]);
            let text = String::from_utf8(result.stdout).unwrap();
            if color == "always" {
                styles::assert_recovery_styles(&text, &case);
            }
            let text = if color == "always" {
                super::execution_scopes::without_sgr(&text)
            } else {
                text
            };
            for recovery in case["recoveries"].as_array().unwrap() {
                assert_eq!(
                    text.matches(recovery["text"].as_str().unwrap()).count(),
                    usize::try_from(recovery["occurrences"].as_u64().unwrap()).unwrap(),
                    "{} actual CLI recovery",
                    case["id"]
                );
            }
            if let Some(expected) = case["reading_rows"].as_array() {
                let lines = text.split('\n').collect::<Vec<_>>();
                let start = lines
                    .iter()
                    .position(|line| *line == "DESCRIPTION")
                    .unwrap()
                    + 1;
                let end = lines.iter().rposition(|line| *line == "NEXT").unwrap();
                let mut rows = lines[start..end].to_vec();
                if rows.last() == Some(&"") {
                    rows.pop();
                }
                assert_eq!(
                    rows,
                    expected
                        .iter()
                        .map(|v| v.as_str().unwrap())
                        .collect::<Vec<_>>(),
                    "{} native hard rows",
                    case["id"]
                );
            }
            assert!(!text.contains("mant:deferred-origin:"));
            outputs.push(text);
        }
        assert_eq!(
            outputs[0], outputs[1],
            "{} color must preserve full text",
            case["id"]
        );
    }
}

#[cfg(unix)]
#[test]
fn real_terminal_widths_preserve_recovery_and_original_label_order() {
    let ids = [
        "recovery-after-lk-fill-on",
        "recovery-after-lk-fill-off",
        "recovery-after-ur-nf-c-on",
        "recovery-after-mt-fill-off",
        "d2-same-recovery-both-owners",
        "d2-include-code-destination",
    ];
    let cases = selected()
        .into_iter()
        .filter(|case| ids.contains(&case["id"].as_str().unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 6);
    let mut child = std::process::Command::new("python3")
        .args([
            "-c",
            include_str!("../support/table_execution_pty.py"),
            super::process_support::executable(),
            &serde_json::to_string(&cases).unwrap(),
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
            panic!("deferred ownership PTY deadline");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
