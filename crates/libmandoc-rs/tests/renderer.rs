#![cfg(feature = "render")]

use std::{
    process::Command,
    sync::{Arc, Barrier},
};

use libmandoc_rs::{RenderErrorKind, RenderFormat, Renderer, SourceBundle};

const MAN_SOURCE: &[u8] = b".TH HELLO 1 \"August 23, 2026\" \"ManT\" \"User Commands\"\n\
.SH NAME\nhello \\- render fixture\n\
.SH SYNOPSIS\n.B hello\n.RI [ option ]\n\
.SH DESCRIPTION\nRender a small manual.\n";

const MDOC_SOURCE: &[u8] = b".Dd August 23, 2026\n\
.Dt HELLO 1\n\
.Os ManT\n\
.Sh NAME\n.Nm hello\n.Nd render fixture\n\
.Sh SYNOPSIS\n.Nm\n.Op Fl v\n\
.Sh DESCRIPTION\nRender a small manual.\n";

const MAN_ASCII: &str = concat!(
    "HELLO(1)                         User Commands                        HELLO(1)\n",
    "\n",
    "N\x08NA\x08AM\x08ME\x08E\n",
    "       hello - render fixture\n",
    "\n",
    "S\x08SY\x08YN\x08NO\x08OP\x08PS\x08SI\x08IS\x08S\n",
    "       h\x08he\x08el\x08ll\x08lo\x08o [_\x08o_\x08p_\x08t_\x08i_\x08o_\x08n]\n",
    "\n",
    "D\x08DE\x08ES\x08SC\x08CR\x08RI\x08IP\x08PT\x08TI\x08IO\x08ON\x08N\n",
    "       Render a small manual.\n",
    "\n",
    "ManT                            August 23, 2026                       HELLO(1)\n",
);

#[test]
fn ascii_man_output_matches_the_pinned_renderer_golden() {
    let report = Renderer::new(RenderFormat::Ascii)
        .render_bytes("hello.1", MAN_SOURCE)
        .expect("render man fixture");
    assert_eq!(report.output, MAN_ASCII);
}

#[test]
fn html_mdoc_output_matches_the_pinned_renderer_golden() {
    let report = Renderer::new(RenderFormat::Html)
        .with_html_fragment(true)
        .render_bytes("hello.1", MDOC_SOURCE)
        .expect("render mdoc fixture");
    assert_eq!(
        report.output,
        include_str!("fixtures/render-mdoc-fragment.html")
    );
}

#[test]
fn utf8_output_preserves_unicode_scalars() {
    let report = Renderer::new(RenderFormat::Utf8)
        .render_bytes(
            "unicode.1",
            ".TH UNICODE 1\n.SH NAME\ncafé \\(em 日本 😀\n".as_bytes(),
        )
        .expect("render deterministic UTF-8");
    assert!(report.output.contains("café — 日本 😀"));
    assert!(report.output.is_char_boundary(report.output.len()));
}

#[test]
fn renderer_address_safety_boundaries() {
    let sources: &[(&str, &[u8])] = &[
        ("no-newline.1", b".TH NO-NEWLINE 1\n.SH NAME\nno-newline"),
        ("modeline-tail.1", b".\\\" -*- mode: nroff -*-"),
        ("encoding-tail.1", b".\\\" coding: UTF-8"),
        ("utf8-tail.1", b".TH UTF8-TAIL 1\n\xe2\x82\xac"),
    ];
    for format in [RenderFormat::Ascii, RenderFormat::Utf8, RenderFormat::Html] {
        for (path, source) in sources {
            Renderer::new(format)
                .render_bytes(path, source)
                .expect("render exact caller-owned buffer boundary");
        }
    }

    let full = Renderer::new(RenderFormat::Html)
        .render_bytes("limit.1", MAN_SOURCE)
        .expect("measure complete output");
    Renderer::new(RenderFormat::Html)
        .with_max_output_bytes(full.output.len())
        .render_bytes("limit.1", MAN_SOURCE)
        .expect("accept an exact output limit");
    let error = Renderer::new(RenderFormat::Html)
        .with_max_output_bytes(full.output.len() - 1)
        .render_bytes("limit.1", MAN_SOURCE)
        .expect_err("reject one byte below complete output");
    assert_eq!(error.kind, RenderErrorKind::OutputLimit);
}

#[test]
fn repository_real_fixture_renders_when_available() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/roff/real/archlinux/archive_entry_stat.3");
    if !path.exists() {
        // Published crate tests are intentionally self-contained; the tagged
        // repository supplies the separately licensed shared fixture corpus.
        return;
    }
    for format in [RenderFormat::Ascii, RenderFormat::Utf8, RenderFormat::Html] {
        let report = Renderer::new(format)
            .render_file(&path)
            .expect("render licensed archive_entry_stat(3) fixture");
        assert!(report.output.contains("ARCHIVE_ENTRY_STAT"));
    }
}

#[test]
fn output_limit_rejects_partial_results() {
    let error = Renderer::new(RenderFormat::Ascii)
        .with_max_output_bytes(16)
        .render_bytes("limited.1", MAN_SOURCE)
        .expect_err("reject output larger than the configured cap");
    assert_eq!(error.kind, RenderErrorKind::OutputLimit);
}

#[test]
fn renderer_resolves_virtual_includes() {
    let mut bundle = SourceBundle::new();
    bundle
        .insert("man1/alias.1", b".so target.1\n".to_vec())
        .expect("insert alias");
    bundle
        .insert("man1/target.1", MAN_SOURCE.to_vec())
        .expect("insert target");
    let report = Renderer::new(RenderFormat::Ascii)
        .render_bundle("man1/alias.1", &bundle)
        .expect("render virtual include");
    assert!(report.output.contains("render fixture"));
}

#[test]
fn concurrent_render_formats_isolate_output_state() {
    const WORKERS: usize = 8;
    let start = Arc::new(Barrier::new(WORKERS));
    let workers: Vec<_> = (0..WORKERS)
        .map(|worker| {
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                let identity = format!("render-worker-{worker}");
                let source =
                    format!(".TH WORKER-{worker} 1\n.SH NAME\n{identity} \\- isolated output\n");
                let format = match worker % 3 {
                    0 => RenderFormat::Ascii,
                    1 => RenderFormat::Utf8,
                    _ => RenderFormat::Html,
                };
                start.wait();
                for _ in 0..16 {
                    let report = Renderer::new(format)
                        .render_bytes(format!("worker-{worker}.1"), source.as_bytes())
                        .expect("render isolated worker source");
                    assert!(report.output.contains(&identity));
                    for other in 0..WORKERS {
                        if other != worker {
                            assert!(!report.output.contains(&format!("render-worker-{other}")));
                        }
                    }
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().expect("renderer worker must not panic");
    }
}

#[test]
fn stdio_child_render() {
    if std::env::var_os("LIBMANDOC_RS_STDIO_CHILD").is_none() {
        return;
    }
    Renderer::new(RenderFormat::Html)
        .render_bytes(
            "stdio.1",
            b".TH STDIO 1\n.SH NAME\nNATIVE-RENDER-MUST-NOT-LEAK \\- isolated\n",
        )
        .expect("render without stdout");
}

#[test]
fn native_rendering_does_not_write_to_process_stdout() {
    let output = Command::new(std::env::current_exe().expect("current test executable"))
        .args(["--exact", "stdio_child_render", "--nocapture"])
        .env("LIBMANDOC_RS_STDIO_CHILD", "1")
        .output()
        .expect("run isolated renderer child");
    assert!(output.status.success(), "child failed: {output:?}");
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("NATIVE-RENDER-MUST-NOT-LEAK"),
        "native renderer leaked document content to stdout"
    );
}
