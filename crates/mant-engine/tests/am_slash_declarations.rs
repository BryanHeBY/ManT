//! Slash-separated aliases retain their declaration owner and bindings.
use mant_ir::{EntryOwner, inline_plain_text, visit::Visit};
use mant_protocol::{EvidenceClass, ExplanationOptions, ExplanationQuery};

const MDOC: &str = ".Dd November 26, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd declaration probe\n.Sh OPTIONS\n.Bl -tag -width Ds\n";
const MAN: &str = ".TH PROBE 1 \"November 26, 2026\" \"ManT\"\n.SH NAME\nprobe \\- declaration probe\n.SH OPTIONS\n.TP\n";

fn check(source: &str, names: &[&str], form: &str, absent: &[&str]) {
    let mut content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    content.document = Some(
        serde_json::from_str(&serde_json::to_string(content.document.as_ref().unwrap()).unwrap())
            .unwrap(),
    );
    let document = content.document.as_ref().unwrap();
    assert_eq!(mant_ir::validate_document(document), []);
    let mut bindings = Bindings {
        expected: names,
        form,
        found: false,
    };
    bindings.visit_document(document);
    assert!(bindings.found, "missing public bindings: {source}");
    let mut path = None;
    for name in names.iter().chain(absent) {
        let result = mant_query::explain_query(
            &content,
            &ExplanationQuery {
                entry: (*name).into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        let direct = result
            .evidence
            .iter()
            .filter(|row| row.class == EvidenceClass::DirectEntry)
            .collect::<Vec<_>>();
        if absent.contains(name) {
            assert!(direct.is_empty(), "{name}: {source}");
            continue;
        }
        assert_eq!(direct.len(), 1, "{name}: {source}");
        let current = direct[0].outline.path().to_string();
        if let Some(expected) = &path {
            assert_eq!(&current, expected, "{name}: {source}");
        }
        path = Some(current);
        assert_eq!(direct[0].entry.as_ref().unwrap().names, names);
    }
}

struct Bindings<'a> {
    expected: &'a [&'a str],
    form: &'a str,
    found: bool,
}
impl Bindings<'_> {
    fn check(&mut self, owner: EntryOwner<'_>) {
        let Some(facts) = owner.facts() else { return };
        if facts.names.first().is_none_or(|name| name != "-x") {
            return;
        }
        assert_eq!(facts.names, self.expected);
        assert_eq!(facts.name_bindings.len(), self.expected.len());
        for binding in &facts.name_bindings {
            assert_ne!(binding.occurrences, []);
            for occurrence in &binding.occurrences {
                assert_eq!(
                    inline_plain_text(&owner.form(occurrence).unwrap()),
                    facts.names[binding.name]
                );
            }
        }
        self.found = true;
    }
}
impl<'a> Visit<'a> for Bindings<'_> {
    fn visit_definition_item(&mut self, item: &'a mant_ir::DefinitionItem) {
        if item
            .terms
            .iter()
            .any(|term| inline_plain_text(term) == self.form)
        {
            self.check(EntryOwner::Definition(item));
        }
        mant_ir::visit::walk_definition_item(self, item);
    }
    fn visit_list_item(&mut self, item: &'a mant_ir::ListItem) {
        self.check(EntryOwner::List(item));
        mant_ir::visit::walk_list_item(self, item);
    }
}

#[test]
fn independent_slashes_end_parameters_and_chain_aliases() {
    // Pinned CVS ASCII, UTF-8 and HTML ran on these exact sources first.
    // man_term.c::pre_alternate executes BI operands separately; mdoc_term.c::
    // termp_fl_pre supplies Fl dashes, and HTML places the aliases in one dt.
    for (source, names, form, absent) in [
        (
            format!("{MDOC}.It Fl x Ar file No / Fl Fl enable\nPAYLOAD.\n.El\n"),
            vec!["-x", "--enable"],
            "-x file / --enable",
            vec![],
        ),
        (
            format!("{MDOC}.It Fl x No / Fl Fl enable Ns No / Ns Fl Fl also\nPAYLOAD.\n.El\n"),
            vec!["-x", "--enable", "--also"],
            "-x / --enable/--also",
            vec![],
        ),
        (
            format!("{MDOC}.It Fl x No / Fl Fl enable Ar file No / Fl Fl also\nPAYLOAD.\n.El\n"),
            vec!["-x", "--enable", "--also"],
            "-x / --enable file / --also",
            vec![],
        ),
        (
            format!("{MAN}.BI \"\\-x \" \"file\" \" / \\-\\-enable\"\nPAYLOAD.\n"),
            vec!["-x", "--enable"],
            "-x file / --enable",
            vec![],
        ),
        (
            format!("{MAN}.BI \"\\-x \" \"file \" \"/ \\-\\-enable\"\nPAYLOAD.\n"),
            vec!["-x", "--enable"],
            "-x file / --enable",
            vec![],
        ),
        (
            format!("{MDOC}.It Fl x Ar \"/--fake/file\" No / Fl Fl enable\nPAYLOAD.\n.El\n"),
            vec!["-x", "--enable"],
            "-x /--fake/file / --enable",
            vec!["--fake"],
        ),
        (
            format!("{MAN}.BI \"\\-x \" \"'file / --fake'\" \" / \\-\\-enable\"\nPAYLOAD.\n"),
            vec!["-x", "--enable"],
            "-x 'file / --fake' / --enable",
            vec!["--fake"],
        ),
        (
            format!("{MDOC}.It Fl x Ar \"file / --fake\" , Fl Fl enable\nPAYLOAD.\n.El\n"),
            vec!["-x", "--enable"],
            "-x file / --fake, --enable",
            vec!["--fake"],
        ),
        (
            format!("{MAN}.BI \"\\-x \" \"file\" \" / --fake/file\"\nPAYLOAD.\n"),
            vec!["-x"],
            "-x file / --fake/file",
            vec!["--fake"],
        ),
        (
            format!("{MDOC}.It Fl x=foo/bar , Fl Fl enable\nPAYLOAD.\n.El\n"),
            vec!["-x", "--enable"],
            "-x=foo/bar, --enable",
            vec![],
        ),
        (
            format!("{MDOC}.It Fl x Ar \"file / --fake\"\nPAYLOAD.\n.El\n"),
            vec!["-x"],
            "-x file / --fake",
            vec!["--fake"],
        ),
    ] {
        check(&source, &names, form, &absent);
    }
}
