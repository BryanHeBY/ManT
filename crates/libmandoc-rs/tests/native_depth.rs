//! Run native stack-safety regressions in children so an abort is observable.

use libmandoc_rs::Parser;

#[test]
fn native_nesting_is_rejected_without_aborting_or_poisoning_sessions() {
    if std::env::var_os("MANT_NATIVE_DEPTH_CHILD").is_some() {
        let man = format!(
            ".TH DEEP 1\n.SH BODY\n{}x\n{}",
            ".RS 0\n".repeat(40_000),
            ".RE\n".repeat(40_000)
        );
        let mdoc = format!(
            ".Dd September 7, 2026\n.Dt DEEP 1\n.Os\n.Sh NAME\n.Nm deep\n.Nd test\n.Sh BODY\n{}x\n{}",
            ".Bd -ragged\n".repeat(10_000),
            ".Ed\n".repeat(10_000)
        );
        for source in [&man, &mdoc] {
            let error = Parser::default()
                .parse_bytes("deep.1", source.as_bytes())
                .unwrap_err();
            assert!(error.to_string().contains("nesting limit"), "{error}");
        }
        #[cfg(feature = "render")]
        for format in [
            libmandoc_rs::RenderFormat::Ascii,
            libmandoc_rs::RenderFormat::Utf8,
            libmandoc_rs::RenderFormat::Html,
        ] {
            let equation = format!(
                ".TH EQN 1\n.SH BODY\n.EQ\n{}x{}\n.EN\n",
                "sqrt { ".repeat(5_000),
                " }".repeat(5_000)
            );
            let copy_truncated = format!(
                ".TH COPY 1\n.SH BODY\n{}x\n{}",
                ".RS 0\n".repeat(180),
                ".RE\n".repeat(180)
            );
            for source in [&man, &mdoc, &equation, &copy_truncated] {
                let error = libmandoc_rs::Renderer::new(format)
                    .with_max_output_bytes(4096)
                    .render_bytes("deep.1", source.as_bytes())
                    .unwrap_err();
                assert!(error.to_string().contains("nesting limit"), "{error}");
            }
            assert!(
                libmandoc_rs::Renderer::new(format)
                    .render_bytes("ok.1", b".TH OK 1\n.SH NAME\nok\n")
                    .is_ok()
            );
        }
        assert!(
            Parser::default()
                .parse_bytes("ok.1", b".TH OK 1\n.SH NAME\nok\n")
                .is_ok()
        );
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "native_nesting_is_rejected_without_aborting_or_poisoning_sessions",
            "--nocapture",
        ])
        .env("MANT_NATIVE_DEPTH_CHILD", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
