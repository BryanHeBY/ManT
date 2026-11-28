//! The normal native file-selector entry plus real application clicks.
use super::*;
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::{Terminal, backend::TestBackend};

struct Fixture(std::path::PathBuf);

impl Fixture {
    fn create() -> Self {
        let serial = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "mant-native-reader-{}-{serial}.1",
            std::process::id()
        ));
        std::fs::write(&path, ".Dd November 28, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd use\n.Sh DESCRIPTION\n.No HEAD\nY\n.No \"\\h'|1n'R\"\n.Lk https://z.test Z U\nEND\n.br\n.No OTHER\n.Lk https://z.test ZYU\n.br\n.Xr printf 3\n.Sh NEXT\nDone.\n").unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn native_interactive_file_selector_delivers_opaque_click_regions() {
    let file = Fixture::create();
    let args = [
        "--input",
        file.0.to_str().unwrap(),
        "--input-format",
        "roff",
        "--display",
        "tui",
    ]
    .map(str::to_owned);
    let Command::Query { source, policy, .. } = arguments::parse_process(&args).unwrap() else {
        panic!("interactive file selector");
    };
    let host = SystemHost::default();
    let prepared = interactive_sources(source, policy, &host).unwrap();
    let snapshot = prepared
        .native
        .expect("normal roff+tui feature selects one paired decode");
    assert!(Arc::ptr_eq(&prepared.scope[0], &snapshot.content_arc()));
    assert_eq!(snapshot.receipts().len(), 1);

    let mut app =
        mant_ui::App::from_native_snapshot(&snapshot, mant_protocol::DocumentCatalog::default());
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let cells = terminal.backend().buffer();
    let hits: Vec<_> = (0..cells.area.height)
        .flat_map(|row| (0..cells.area.width.saturating_sub(2)).map(move |column| (column, row)))
        .filter(|&(column, row)| {
            ["Z", "Y", "U"]
                .into_iter()
                .enumerate()
                .all(|(offset, glyph)| {
                    cells[(column + u16::try_from(offset).unwrap(), row)].symbol() == glyph
                })
        })
        .collect();
    let [mixed, authored] = hits.as_slice() else {
        panic!("both visible labels: {hits:?}");
    };
    for (&(column, row), expected) in [(mixed, [true, false, true]), (authored, [true, true, true])]
    {
        for (offset, linked) in expected.into_iter().enumerate() {
            for kind in [
                MouseEventKind::Down(MouseButton::Left),
                MouseEventKind::Up(MouseButton::Left),
            ] {
                app.handle_event(&Event::Mouse(MouseEvent {
                    kind,
                    column: column + u16::try_from(offset).unwrap(),
                    row,
                    modifiers: KeyModifiers::NONE,
                }));
            }
            let mut opens = Vec::new();
            let mut open = |uri: &mant_ui::ExternalUri| {
                opens.push(uri.as_str().to_owned());
                Ok(())
            };
            app.service_pending(&mut mant_ui::ReaderServices {
                open_external: Some(&mut open),
                ..Default::default()
            });
            assert_eq!(
                opens,
                if linked {
                    vec!["https://z.test"]
                } else {
                    vec![]
                }
            );
        }
    }

    assert_returned_file_keeps_click_members(&mut app, &mut terminal);
}

fn assert_returned_file_keeps_click_members(
    app: &mut mant_ui::App,
    terminal: &mut Terminal<TestBackend>,
) {
    let presented = |terminal: &Terminal<TestBackend>, word: &str| {
        let cells = terminal.backend().buffer();
        (0..cells.area.height)
            .flat_map(|row| (0..cells.area.width).map(move |column| (column, row)))
            .find(|&(column, row)| {
                word.chars().enumerate().all(|(offset, glyph)| {
                    let x = column + u16::try_from(offset).unwrap();
                    cells
                        .cell((x, row))
                        .is_some_and(|cell| cell.symbol().starts_with(glyph))
                })
            })
            .expect("word is rendered")
    };
    let click = |app: &mut mant_ui::App, (column, row)| {
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            app.handle_event(&Event::Mouse(MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            }));
        }
    };
    let mut manual =
        mant_loader::load_roff_bytes(b".TH PRINTF 3\n.SH NAME\nprintf - destination fixture\n")
            .unwrap();
    manual.address = Some(mant_protocol::DocumentAddress::Manual {
        name: "printf".into(),
        manual_section: "3".into(),
    });
    click(app, presented(terminal, "printf(3)"));
    assert!(
        app.service_pending_with_native_links(&mut mant_ui::ReaderServices::default(), |_| {
            Ok(mant_loader::ResolvedWithLinkLayout::without_native_links(
                manual.clone(),
            ))
        })
    );
    app.handle_event(&Event::Key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT)));
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let (column, row) = presented(terminal, "ZYU");
    for offset in 0..3 {
        click(app, (column + offset, row));
        let mut opens = Vec::new();
        let mut open = |uri: &mant_ui::ExternalUri| {
            opens.push(uri.as_str().to_owned());
            Ok(())
        };
        app.service_pending(&mut mant_ui::ReaderServices {
            open_external: Some(&mut open),
            ..Default::default()
        });
        assert_eq!(
            opens,
            if offset == 1 {
                vec![]
            } else {
                vec!["https://z.test"]
            }
        );
    }
}
