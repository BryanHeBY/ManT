//! Actual App actions on source-gold label cells, including authored blanks.
use super::*;
use serde_json::Value;

fn fixtures() -> Vec<Value> {
    let boundaries: Value = serde_json::from_str(include_str!(
        "../../../mant-codec/src/mandoc/tests/annotation_ownership/boundaries.json"
    ))
    .unwrap();
    boundaries["cases"].as_array().unwrap().clone()
}

#[test]
fn annotation_source_members_keep_search_and_painted_styles_after_resize() {
    for case in fixtures() {
        let content = json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap(),
        );
        for width in [40, 78, 12, 16] {
            let rendered = DocumentView::new(&content).render(width);
            let buffer = terminal_cells::paint(&rendered, width);
            if case["family"] == "HeldStyle" {
                check_held_body(&rendered, &buffer, &case, width);
            }
            for link in case["expected"]["links"].as_array().unwrap() {
                for span in link["scalar_styles"].as_array().unwrap() {
                    let spelling = span["text"].as_str().unwrap();
                    // LF has no painted glyph; source blanks are checked by
                    // actual pointer activation in the following test.
                    let Some(word) = spelling
                        .split(|c: char| !c.is_alphanumeric())
                        .find(|word| !word.is_empty())
                    else {
                        continue;
                    };
                    let hits = rendered.search(word);
                    assert!(!hits.is_empty(), "{} {width} {word}", case["id"]);
                    let expected = span["style"].as_str().unwrap();
                    assert!(
                        hits.iter().any(|hit| {
                            let cell = &buffer[(
                                u16::try_from(hit.start_column).unwrap(),
                                u16::try_from(hit.row).unwrap(),
                            )];
                            match expected {
                                "strong" => cell.modifier.contains(Modifier::BOLD),
                                "emphasis" => cell.modifier.contains(Modifier::ITALIC),
                                _ => true,
                            }
                        }),
                        "{} {width} {word}: {expected}",
                        case["id"]
                    );
                }
            }
        }
    }
}

#[test]
fn authored_blank_labels_activate_but_padding_tail_and_invalid_targets_do_not() {
    for case in fixtures().into_iter().filter(|case| {
        case["family"] == "Prefix" || case["family"] == "Named" || case["family"] == "OwnerCut"
    }) {
        let content = json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap(),
        );
        let mut app = App::new(&content);
        key(&mut app, KeyCode::Char('B'));
        let mut terminal = Terminal::new(TestBackend::new(78, 40)).unwrap();
        for width in [40, 78, 12, 16] {
            terminal.backend_mut().resize(width, 40);
            terminal.draw(|frame| app.draw(frame)).unwrap();
            key(&mut app, KeyCode::Home);
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let buffer = terminal.backend().buffer();
            for link in case["expected"]["links"].as_array().unwrap() {
                let target: LinkTarget = serde_json::from_value(link["target"].clone()).unwrap();
                let Some(uri) = external(&target) else {
                    continue;
                };
                let label = link["label"].as_str().unwrap();
                let word = label
                    .trim_start_matches([' ', '\u{a0}'])
                    .split('\n')
                    .next()
                    .unwrap();
                if word.is_empty() || word.contains(' ') || word.len() > 8 {
                    continue;
                }
                let positions = reader_actions::positions(buffer, word);
                // A narrow visual wrap is verified by the search test above.
                if positions.is_empty() && width < 40 {
                    continue;
                }
                assert!(!positions.is_empty(), "{} {width} {word}", case["id"]);
                let (column, row) = positions[0];
                let valid = target
                    .to_uri()
                    .as_deref()
                    .and_then(mant_ui::ExternalUri::parse)
                    .is_some();
                reader_actions::click(&mut app, column, row);
                assert_eq!(
                    reader_actions::opened_targets(&mut app),
                    if valid { vec![uri.clone()] } else { vec![] },
                    "{} {width}",
                    case["id"]
                );
                // Only a genuine authored prefix receives a hit range. The
                // preceding device padding remains outside that same link.
                let prefix = label
                    .chars()
                    .take_while(|ch| matches!(ch, ' ' | '\u{a0}'))
                    .count();
                // At a soft-wrap seam the source blank has no painted
                // cell. The left cells are structural padding, not its label.
                let painted_prefix = width >= 40
                    || (1..=prefix).all(|offset| {
                        buffer[(column - u16::try_from(offset).unwrap(), row)]
                            .modifier
                            .contains(Modifier::UNDERLINED)
                    });
                for offset in 1..=if painted_prefix { prefix } else { 0 } {
                    reader_actions::click(&mut app, column - u16::try_from(offset).unwrap(), row);
                    assert_eq!(
                        reader_actions::opened_targets(&mut app),
                        if valid { vec![uri.clone()] } else { vec![] },
                        "{} authored prefix",
                        case["id"]
                    );
                }
                if prefix > 0 && !painted_prefix {
                    reader_actions::click(&mut app, column - 1, row);
                    assert!(
                        reader_actions::opened_targets(&mut app).is_empty(),
                        "{} soft seam padding",
                        case["id"]
                    );
                }
                if column > u16::try_from(prefix).unwrap() + 1 {
                    reader_actions::click(
                        &mut app,
                        column - u16::try_from(prefix).unwrap() - 1,
                        row,
                    );
                    assert!(
                        reader_actions::opened_targets(&mut app).is_empty(),
                        "{} padding",
                        case["id"]
                    );
                }
            }
            check_outside_cells(&mut app, buffer, &case);
        }
    }
}

fn check_held_body(
    rendered: &mant_ui::RenderedDocument,
    buffer: &ratatui::buffer::Buffer,
    case: &Value,
    width: u16,
) {
    let body = case["expected"]["body_text"].as_str().unwrap();
    let hits = rendered.search(body);
    assert_eq!(hits.len(), 1, "{} {width}: complete held body", case["id"]);
    let hit = &hits[0];
    let mut column = u16::try_from(hit.start_column).unwrap();
    let row = u16::try_from(hit.row).unwrap();
    for span in case["expected"]["body_scalar_styles"].as_array().unwrap() {
        let style = span["style"].as_str().unwrap();
        let first = buffer[(column, row)].style();
        for ch in span["text"].as_str().unwrap().chars() {
            let cell = &buffer[(column, row)];
            assert_eq!(cell.symbol(), ch.to_string(), "{} {width}", case["id"]);
            assert_eq!(cell.modifier.contains(Modifier::BOLD), style == "strong");
            assert_eq!(
                cell.modifier.contains(Modifier::ITALIC),
                style == "emphasis"
            );
            if style == "code" {
                assert_eq!(cell.style(), first, "{} held code blank", case["id"]);
            }
            column += 1;
        }
    }
}

#[test]
fn held_text_copy_and_nonactivatable_mail_survive_real_app_resizing() {
    for case in fixtures()
        .into_iter()
        .filter(|c| c["family"] == "HeldStyle" || c["id"] == "Named-0006")
    {
        let content =
            mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap();
        let body = if case["family"] == "HeldStyle" {
            case["expected"]["body_text"].as_str().unwrap()
        } else {
            "a@example.testX"
        };
        let mut app = App::new(&content);
        key(&mut app, KeyCode::Char('B'));
        let mut terminal = Terminal::new(TestBackend::new(78, 40)).unwrap();
        for width in [78, 40, 12, 16, 78] {
            terminal.backend_mut().resize(width, 40);
            terminal.draw(|frame| app.draw(frame)).unwrap();
            key(&mut app, KeyCode::Home);
            key(&mut app, KeyCode::Char('/'));
            for ch in regex_syntax::escape(body).chars() {
                key(&mut app, KeyCode::Char(ch));
            }
            key(&mut app, KeyCode::Enter);
            settle(&mut app);
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let selected = if width < 40 {
                if case["id"] == "Named-0006" {
                    "a@"
                } else {
                    body.split_whitespace().next().unwrap()
                }
            } else {
                body
            };
            let points = reader_actions::positions(terminal.backend().buffer(), selected);
            assert!(
                !points.is_empty(),
                "{} {width} search/reveal body",
                case["id"]
            );
            let (column, row) = points[0];
            if case["family"] == "HeldStyle" {
                let cells: Vec<_> = selected
                    .chars()
                    .enumerate()
                    .map(|(i, _)| (column + u16::try_from(i).unwrap(), row))
                    .collect();
                let mut copy_app = App::new(&content);
                key(&mut copy_app, KeyCode::Char('B'));
                let mut copy_terminal = Terminal::new(TestBackend::new(78, 40)).unwrap();
                copy_terminal.backend_mut().resize(width, 40);
                copy_terminal.draw(|frame| copy_app.draw(frame)).unwrap();
                key(&mut copy_app, KeyCode::Home);
                copy_terminal.draw(|frame| copy_app.draw(frame)).unwrap();
                let (column, row) =
                    reader_actions::positions(copy_terminal.backend().buffer(), selected)[0];
                let cells: Vec<_> = cells
                    .iter()
                    .enumerate()
                    .map(|(i, _)| (column + u16::try_from(i).unwrap(), row))
                    .collect();
                check_copy(
                    &mut copy_app,
                    &cells,
                    selected,
                    case["id"].as_str().unwrap(),
                );
            } else {
                reader_actions::click(&mut app, column, row);
                assert_eq!(
                    reader_actions::opened_targets(&mut app),
                    Vec::<String>::new()
                );
            }
        }
    }
}

#[test]
fn hard_line_section_label_and_authored_blank_reach_the_original_target() {
    let case = fixtures()
        .into_iter()
        .find(|case| case["id"] == "Named-0008")
        .unwrap();
    let content =
        mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap();
    for blank in [false, true] {
        for width in [40, 78, 12, 16] {
            let mut app = App::new(&content);
            key(&mut app, KeyCode::Char('B'));
            let mut terminal = Terminal::new(TestBackend::new(width, 7)).unwrap();
            terminal.draw(|frame| app.draw(frame)).unwrap();
            key(&mut app, KeyCode::Home);
            key(&mut app, KeyCode::Char('/'));
            for ch in "NEXTX".chars() {
                key(&mut app, KeyCode::Char(ch));
            }
            key(&mut app, KeyCode::Enter);
            settle(&mut app);
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let buffer = terminal.backend().buffer();
            let source = reader_actions::positions(buffer, "NEXTX")
                .into_iter()
                .find(|point| buffer[*point].modifier.contains(Modifier::ITALIC))
                .unwrap();
            let point = if blank {
                (source.1 + 1..buffer.area.height - 1)
                    .find_map(|row| {
                        (0..width).find_map(|column| {
                            let cell = &buffer[(column, row)];
                            (cell.symbol() == " "
                                && cell
                                    .modifier
                                    .contains(Modifier::UNDERLINED | Modifier::ITALIC))
                            .then_some((column, row))
                        })
                    })
                    .expect("authored blank after a hard line has a source hit cell")
            } else {
                source
            };
            if width >= 40 {
                assert!(terminal.backend().to_string().contains("2/3"));
            }
            assert_eq!(
                reader_actions::positions(terminal.backend().buffer(), "body"),
                Vec::<(u16, u16)>::new()
            );
            reader_actions::click(&mut app, point.0, point.1);
            terminal.draw(|frame| app.draw(frame)).unwrap();
            if width >= 40 {
                assert!(terminal.backend().to_string().contains("3/3"));
            }
            assert!(
                !reader_actions::positions(terminal.backend().buffer(), "body").is_empty(),
                "{width} {blank}: {}",
                terminal.backend()
            );
            assert_eq!(
                reader_actions::opened_targets(&mut app),
                Vec::<String>::new()
            );
        }
    }
}

fn check_outside_cells(app: &mut App, buffer: &ratatui::buffer::Buffer, case: &Value) {
    for literal in case["expected"]["outside_link_literals"]
        .as_array()
        .unwrap()
    {
        for (column, row) in reader_actions::positions(buffer, literal.as_str().unwrap()) {
            reader_actions::click(app, column, row);
            assert!(
                reader_actions::opened_targets(app).is_empty(),
                "{} outside member",
                case["id"]
            );
        }
    }
}
