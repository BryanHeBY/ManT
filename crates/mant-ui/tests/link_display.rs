//! Typed targets never synthesize body text. Real terminal cells, pointer
//! activation and visual copy all consume the executed link-label range.

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use mant_ir::ResolvedContent;
use mant_ui::{App, CopyRequest, DocumentView, ReaderServices};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

const PRE: &str =
    ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";

fn roundtrip(query: &ResolvedContent) -> ResolvedContent {
    let json = mant_render::render_query_json(query, false).unwrap();
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    decoded.into()
}

fn pointer(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
    app.handle_mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    });
}

fn body_position(buffer: &Buffer, label: &str) -> (u16, u16) {
    // Find the rightmost exact text in actual cells: reference/navigation
    // labels live to its left and do not determine document hit geometry.
    let mut positions = Vec::new();
    for row in 1..buffer.area.height.saturating_sub(1) {
        for column in 0..buffer
            .area
            .width
            .saturating_sub(u16::try_from(label.len()).unwrap())
        {
            let text: String = (column..column + u16::try_from(label.len()).unwrap())
                .map(|x| buffer[(x, row)].symbol())
                .collect();
            if text == label {
                positions.push((column, row));
            }
        }
    }
    positions
        .into_iter()
        .max_by_key(|(column, _)| *column)
        .expect("visible linked label")
}

fn assert_pointer_and_copy(query: &ResolvedContent, label: &str, target: &str) {
    for width in [80, 128] {
        let mut app = App::new(query);
        let mut terminal = Terminal::new(TestBackend::new(width, 32)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let (column, row) = body_position(terminal.backend().buffer(), label);
        let mut activated = Vec::new();
        let mut open = |uri: &mant_ui::ExternalUri| {
            activated.push(uri.as_str().to_owned());
            Ok(())
        };
        pointer(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            column,
            row,
        );
        pointer(&mut app, MouseEventKind::Up(MouseButton::Left), column, row);
        app.service_pending(&mut ReaderServices {
            open_external: Some(&mut open),
            ..ReaderServices::default()
        });
        assert_eq!(activated, [target], "width={width}, label={label}");

        let last = column + u16::try_from(label.len()).unwrap() - 1;
        for (kind, x) in [
            (MouseEventKind::Down(MouseButton::Left), column),
            (MouseEventKind::Drag(MouseButton::Left), last),
            (MouseEventKind::Up(MouseButton::Left), last),
        ] {
            pointer(&mut app, kind, x, row);
        }
        let mut copied = Vec::new();
        let mut copy = |request| {
            if let CopyRequest::Selection { text } = request {
                copied.push(text);
            } else {
                panic!("visual selection must issue a selection copy");
            }
            Ok(())
        };
        app.service_pending(&mut ReaderServices {
            copy_to_clipboard: Some(&mut copy),
            ..ReaderServices::default()
        });
        assert_eq!(copied, [label], "width={width}: copy escaped the label");
    }
}

#[test]
fn native_and_markdown_link_labels_keep_pointer_and_copy_ranges() {
    // Exact roff inputs were run with pristine CVS -Tutf8/-Thtml first:
    // man_UR_pre/post, termp_lk_pre and mdoc__x_pre own their distinct display
    // rules. The common visitor must never append a target on its own.
    let markdown = mant_loader::load_markdown_text(
        "# Title\n\n[LINKLABEL](https://example.com/x) outside\n",
        Some("link.md".into()),
    )
    .unwrap();
    let ur = mant_loader::load_roff_bytes(
        b".TH TEST 1\n.SH DESCRIPTION\n.UR https://example.com/x\nLINKLABEL\n.UE\noutside\n",
    )
    .unwrap();
    let lk = mant_loader::load_roff_bytes(
        format!("{PRE}.Lk https://example.com/x LINKLABEL\n.No outside\n").as_bytes(),
    )
    .unwrap();
    let rfc = mant_loader::load_roff_bytes(
        format!("{PRE}.Rs\n.%R RFC 1149\n.Re\n.No outside\n").as_bytes(),
    )
    .unwrap();
    let uri = mant_loader::load_roff_bytes(
        format!("{PRE}.Rs\n.%U https://example.com/a\\&b\n.Re\n.No outside\n").as_bytes(),
    )
    .unwrap();
    for (query, label, target, displayed_target) in [
        (markdown, "LINKLABEL", "https://example.com/x", false),
        (ur, "LINKLABEL", "https://example.com/x", true),
        (lk, "LINKLABEL", "https://example.com/x", true),
        (
            rfc,
            "RFC 1149",
            "https://www.rfc-editor.org/rfc/rfc1149.html",
            false,
        ),
        (
            uri,
            "https://example.com/ab",
            "https://example.com/ab",
            true,
        ),
    ] {
        let query = roundtrip(&query);
        let view = DocumentView::new(&query);
        for width in [40, 80, 128] {
            let rendered = view.render(width);
            assert_eq!(rendered.search(label).len(), 1, "{label}, width={width}");
            assert_eq!(
                rendered.search(target).len(),
                usize::from(displayed_target),
                "implicit target insertion: {label}, width={width}"
            );
        }
        assert_pointer_and_copy(&query, label, target);
    }
}

#[test]
fn delayed_mail_glyph_is_visible_and_copied_without_becoming_a_hit_target() {
    // Exact complete roff source ran through pristine CVS first. The first
    // Mt operand owns X; the second operand settles it at its ordinary word
    // separator, but only the address is the typed mail activation range.
    let query = roundtrip(
        &mant_loader::load_roff_bytes(
            format!("{PRE}.Mt \\zX a@example.org\n.No outside\n").as_bytes(),
        )
        .unwrap(),
    );
    for width in [80, 128] {
        let mut app = App::new(&query);
        let mut terminal = Terminal::new(TestBackend::new(width, 32)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let (column, row) = body_position(terminal.backend().buffer(), "Xa@example.org");
        pointer(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            column,
            row,
        );
        pointer(&mut app, MouseEventKind::Up(MouseButton::Left), column, row);
        let mut activated = Vec::new();
        let mut open = |uri: &mant_ui::ExternalUri| {
            activated.push(uri.as_str().to_owned());
            Ok(())
        };
        app.service_pending(&mut ReaderServices {
            open_external: Some(&mut open),
            ..ReaderServices::default()
        });
        assert!(
            activated.is_empty(),
            "cached X became clickable: width={width}"
        );
    }
    assert_pointer_and_copy(&query, "a@example.org", "mailto:a@example.org");
}

#[test]
fn man_head_identity_not_its_terminal_word_controls_pointer_activation() {
    // Every exact source ran pristine first. print_encode(norecurse=1)
    // derives the href; the terminal target word is separate post_UR output.
    // Actual cells, clicking and copy must follow the unchanged BODY label.
    for (start, end, prefix) in [
        ("UR", "UE", "https://example.org/"),
        ("MT", "ME", "user@example.org"),
    ] {
        for (spelling, suffix) in [
            ("", ""),
            (r"\zX", ""),
            (r"\z\fBX\fP", ""),
            (r"\&X", "X"),
            (r"\*[.T]", "html"),
            (r"\o'BC'", "C"),
            (r"\z\&X", "X"),
        ] {
            let source = format!(
                ".TH TEST 1 \"2026-10-01\"\n.SH DESCRIPTION\n.{start} \"{prefix}{spelling}\"\nLINKLABEL\n.{end}\nafter\n"
            );
            let query = roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
            let address = format!("{prefix}{suffix}");
            let target = if start == "MT" {
                format!("mailto:{address}")
            } else {
                address
            };
            assert_pointer_and_copy(&query, "LINKLABEL", &target);
        }
    }
}

#[test]
fn invalid_decoded_man_targets_keep_labels_visible_without_host_activation() {
    // Both exact sources ran pristine first. The HTML overstrike identity
    // keeps its final source space; URI/email validation rejects activation
    // without erasing the native BODY label or terminal C target glyph.
    for (start, end, prefix) in [
        ("UR", "UE", "https://example.org/"),
        ("MT", "ME", "user@example.org"),
    ] {
        let source = format!(
            ".TH TEST 1 \"2026-10-01\"\n.SH DESCRIPTION\n.{start} \"{prefix}\\o'BC '\"\nLINKLABEL\n.{end}\nafter\n"
        );
        let query = roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        for width in [80, 128] {
            let mut app = App::new(&query);
            let mut terminal = Terminal::new(TestBackend::new(width, 32)).unwrap();
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let (column, row) = body_position(terminal.backend().buffer(), "LINKLABEL");
            pointer(
                &mut app,
                MouseEventKind::Down(MouseButton::Left),
                column,
                row,
            );
            pointer(&mut app, MouseEventKind::Up(MouseButton::Left), column, row);
            let mut activated = Vec::new();
            let mut open = |uri: &mant_ui::ExternalUri| {
                activated.push(uri.as_str().to_owned());
                Ok(())
            };
            app.service_pending(&mut ReaderServices {
                open_external: Some(&mut open),
                ..ReaderServices::default()
            });
            assert!(activated.is_empty(), "{source}\nwidth={width}");
        }
    }
}
