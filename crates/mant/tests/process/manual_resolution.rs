//! Hermetic native category selection through the real process boundary.

use super::process_support::executable;
use super::support::{configure_registered_documents, registered_documents_dir};
use std::{fs, path::PathBuf, process::Command};

// The exact sources were run through the pinned pristine ASCII/UTF-8/HTML
// reference before adding assertions. main.c lookup keeps section identities;
// expanding a main-category query is a loader policy, not a formatter rewrite.
const REFERENCE: &str =
    ".TH REGEX 7\n.SH NAME\nregex \\- reference probe\n.SH SEE ALSO\n.BR ed (1)\n";

struct Fixture(PathBuf);

impl Fixture {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "mant-manual-query-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(root.join("manuals/man7")).unwrap();
        fs::create_dir_all(root.join("tldr/pages/common")).unwrap();
        fs::create_dir_all(registered_documents_dir(&root)).unwrap();
        fs::write(root.join("manuals/man7/regex.7"), REFERENCE).unwrap();
        let fixture = Self(root);
        fixture.page("1p");
        fixture
    }

    fn page(&self, section: &str) {
        let directory = self.0.join("manuals").join(format!("man{section}"));
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join(format!("ed.{section}")),
            format!(".TH ED {section}\n.SH NAME\ned \\- selected native manual\n.SH DESCRIPTION\nSELECTED_MANUAL_BODY\n"),
        ).unwrap();
    }

    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = Command::new(executable());
        configure_registered_documents(&mut command, &self.0);
        command
            .args(arguments)
            .env("MANT_MANPATH", self.0.join("manuals"))
            .env("MANT_TLDR_DIR", self.0.join("tldr"))
            .env("MANSECT", "1p:1m:1:7:3");
        command
    }

    fn json(&self, arguments: &[&str]) -> serde_json::Value {
        let output = self
            .command(arguments)
            .args(["--format", "json", "--compact"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{arguments:?}: {output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn manual_queries_share_category_resolution_but_catalog_filters_and_addresses_stay_exact() {
    let fixture = Fixture::new("selectors");
    for arguments in [
        vec!["1", "ed", "--manual"],
        vec!["ed(1)", "--manual"],
        vec!["ed", "--man-section", "1", "--manual"],
        vec!["1p", "ed", "--manual"],
        vec!["manual/1p/ed", "--manual"],
    ] {
        let result = fixture.json(&arguments);
        assert_eq!(result["address"]["manualSection"], "1p", "{arguments:?}");
        assert_eq!(result["document"]["meta"]["manualSection"], "1p");
        let decoded: mant_protocol::QueryBundle = serde_json::from_value(result).unwrap();
        let encoded = serde_json::to_string(&decoded).unwrap();
        serde_json::from_str::<mant_protocol::QueryBundle>(&encoded).unwrap();
    }
    for arguments in [
        vec!["manual/1/ed", "--manual"],
        vec!["1m", "ed", "--manual"],
    ] {
        let output = fixture
            .command(&arguments)
            .args(["--format", "json"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert_eq!(output.stdout.len(), 0);
        assert!(String::from_utf8_lossy(&output.stderr).contains("available sections: 1p"));
    }
    let catalog = fixture.json(&[
        "--find",
        "^ed$",
        "--regex",
        "--kind",
        "manual",
        "--man-section",
        "1",
    ]);
    assert_eq!(catalog["total"], 0, "catalog filters are exact categories");
    fixture.page("1");
    let result = fixture.json(&["1", "ed", "--manual"]);
    assert_eq!(
        result["address"]["manualSection"], "1",
        "exact wins over MANSECT"
    );
}

#[test]
fn source_references_and_scope_edges_keep_authored_labels_and_actual_page_identity() {
    let fixture = Fixture::new("references");
    let source = fixture.json(&["7", "regex", "--manual"]);
    let bundle: mant_protocol::QueryBundle = serde_json::from_value(source).unwrap();
    let document: mant_ir::Document = bundle.document.unwrap().into();
    let mut references = Vec::new();
    mant_ir::scan_references(&document, mant_ir::ReferenceScanLimits::default(), |link| {
        references.push((link.target.clone(), mant_ir::inline_plain_text(link.label)));
        std::ops::ControlFlow::Continue(())
    });
    assert!(
        references.iter().any(|(target, label)| {
            label == "ed(1)"
                && matches!(target, mant_ir::LinkTarget::Manual {
            name, manual_section: Some(section)
        } if name == "ed" && section == "1")
        }),
        "{references:?}"
    );
    let scope = fixture.json(&[
        "7",
        "regex",
        "--follow-links",
        "--max-documents",
        "3",
        "--search",
        "SELECTED_MANUAL_BODY",
    ]);
    assert_eq!(scope["scope"]["documents"].as_array().unwrap().len(), 2);
    assert_eq!(
        scope["scope"]["documents"][1]["address"]["manualSection"],
        "1p"
    );
    assert_eq!(scope["scope"]["edges"][0]["to"]["manualSection"], "1p");
    assert_eq!(scope["result"]["search"]["total"], 1);
}

#[test]
fn exact_domain_failure_does_not_poison_a_later_compatible_citation() {
    let fixture = Fixture::new("scope-intent");
    fs::write(registered_documents_dir(&fixture.0).join("index.md"),
        "# Index\n\n<!-- mant:entries role=option case=sensitive -->\n- `--key`: Description.\n\n  <!-- mant:domain entries=manual/1/ed roles=command -->\n\n[ed(1)](man:ed(1)) [ed(1p)](man:ed(1p))\n").unwrap();
    let scope = fixture.json(&[
        "index",
        "--follow-links",
        "--max-documents",
        "4",
        "--search",
        "SELECTED_MANUAL_BODY",
    ]);
    let graph = &scope["scope"];
    assert_eq!(graph["documents"].as_array().unwrap().len(), 2);
    assert_eq!(graph["documents"][1]["address"]["manualSection"], "1p");
    assert_eq!(
        graph["edges"].as_array().unwrap().len(),
        1,
        "queries deduplicate actual destinations"
    );
    assert_eq!(graph["unresolved"].as_array().unwrap().len(), 1);
    assert_eq!(
        graph["unresolved"][0]["selector"]["selector"],
        "manual/1/ed"
    );
    assert_eq!(scope["result"]["search"]["total"], 1);
}

#[cfg(feature = "tui")]
#[test]
fn native_citation_click_loads_the_selected_extension_through_the_request_boundary() {
    use crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use mant_ui::{App, ReaderServices};
    use ratatui::{Terminal, backend::TestBackend};

    let fixture = Fixture::new("native-click");
    let origin: mant_protocol::QueryBundle =
        serde_json::from_value(fixture.json(&["7", "regex", "--manual"])).unwrap();
    let mut app = App::new(&origin.into());
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    let (column, row) = (0..24)
        .find_map(|row| {
            (0..95)
                .find(|column| {
                    b"ed(1)".iter().enumerate().all(|(index, byte)| {
                        buffer[(*column + u16::try_from(index).unwrap(), row)]
                            .symbol()
                            .as_bytes()
                            == std::slice::from_ref(byte)
                    })
                })
                .map(|column| (column, row))
        })
        .expect("native ed(1) reference is visible");
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
    let mut opened = false;
    let mut open = |target: &mant_protocol::DocumentOpenTarget| {
        let mant_protocol::DocumentOpenTarget::Manual {
            name,
            manual_section,
        } = target
        else {
            panic!("source citation must remain a query: {target:?}");
        };
        assert_eq!(name, "ed");
        assert_eq!(manual_section.as_deref(), Some("1"));
        let request = mant_protocol::QueryRequest {
            schema: mant_protocol::RequestSchema::V0Dot13,
            input: mant_protocol::QueryInput::Document {
                selector: name.clone(),
                source: None,
                manual_section: manual_section.clone(),
            },
            view: mant_protocol::QueryView::Full {},
        };
        let output = super::process_support::run_text_command(
            fixture.command(&["--request-json", "--format", "json", "--compact"]),
            &serde_json::to_string(&request).unwrap(),
        );
        assert!(output.status.success(), "{output:?}");
        let loaded: mant_protocol::QueryBundle = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            loaded.address,
            Some(mant_ir::DocumentAddress::Manual {
                name: "ed".into(),
                manual_section: "1p".into(),
            })
        );
        opened = true;
        Ok(loaded.into())
    };
    assert!(app.service_pending(&mut ReaderServices {
        open_document: Some(&mut open),
        ..ReaderServices::default()
    }));
    assert!(opened);
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect();
    assert!(text.contains("SELECTED_MANUAL_BODY"), "{text}");
}
