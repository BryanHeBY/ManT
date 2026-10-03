//! Exercise a real pointer activation through the application's loading policy.
use super::*;
use crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use mant_ui::{App, ReaderServices};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

fn screen(app: &mut App) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(140, 32)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    terminal.backend().buffer().clone()
}

fn click(app: &mut App, label: &str) {
    let buffer = screen(app);
    assert!(label.is_ascii());
    let (column, row) = (0..buffer.area.height)
        .find_map(|row| {
            (0..buffer.area.width)
                .rev()
                .find(|column| {
                    column
                        .checked_add(u16::try_from(label.len()).unwrap())
                        .filter(|end| *end <= buffer.area.width)
                        .is_some_and(|end| {
                            (*column..end).zip(label.as_bytes()).all(|(column, byte)| {
                                buffer[(column, row)].symbol().as_bytes()
                                    == std::slice::from_ref(byte)
                            })
                        })
                })
                .map(|column| (column, row))
        })
        .expect("ASCII link is visible in the body");
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
}

#[test]
fn clicked_manual_links_keep_the_quick_reference_and_native_document() {
    for (href, expected_target) in [
        (
            "man:git(1)",
            mant_protocol::DocumentOpenTarget::Address {
                address: DocumentAddress::Manual {
                    name: "git".into(),
                    manual_section: "1".into(),
                },
            },
        ),
        (
            "man:git",
            mant_protocol::DocumentOpenTarget::Manual {
                name: "git".into(),
                manual_section: None,
            },
        ),
    ] {
        for has_cache in [false, true] {
            let origin =
                mant_loader::load_markdown_text(&format!("# ORIGIN\n\n[git(1)]({href})\n"), None)
                    .unwrap();
            let mut app = App::new(&origin);
            let mut host = if has_cache {
                FakeHost::with_manual_and_tldr()
            } else {
                FakeHost::with_manual()
            };
            if let Some(reference) = &mut host.tldr {
                reference.title = "git".into();
                reference.description = vec!["LOADED_QUICK_REFERENCE".into()];
            }
            click(&mut app, "git(1)");
            let mut open = |target: &mant_protocol::DocumentOpenTarget| {
                assert_eq!(target, &expected_target, "preserve section authority");
                let (request, policy) = crate::application::request_for_navigation(target);
                let content = crate::application::read_full(&request, policy, &host)
                    .map_err(|error| error.message().to_owned())?;
                assert_eq!(host.last_policy.get(), LoadPolicy::ManualWithTldr);
                assert_eq!(content.tldr.is_some(), has_cache);
                Ok(content)
            };
            let mut services = ReaderServices {
                open_document: Some(&mut open),
                ..ReaderServices::default()
            };
            assert!(app.service_pending(&mut services));
            assert!(!app.service_pending(&mut services));
            assert_eq!(host.query_calls.get(), 1);
            assert_eq!(host.update_calls.get(), 0);
            let buffer = screen(&mut app);
            let text: String = buffer
                .content()
                .iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect();
            assert!(
                text.contains("demo - a test"),
                "manual body remains visible"
            );
            assert_eq!(text.contains("LOADED_QUICK_REFERENCE"), has_cache);
        }
    }
}
