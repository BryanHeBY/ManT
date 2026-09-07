//! End-to-end name, ownership and layout regression matrices.

#[test]
fn option_arguments_never_become_aliases() {
    for (head, aliases, missed) in [
        (".It Fl n Ar -NUM", vec!["-n"], "-NUM"),
        (".It Fl x Ar -1 | 1", vec!["-x"], "-1"),
        (
            ".It Fl n , Fl -number Ar NUM",
            vec!["-n", "--number"],
            "NUM",
        ),
        (".It Fl -arg Ns = Ns Ar VALUE", vec!["--arg"], "VALUE"),
    ] {
        let source = format!(
            ".Dd September 7, 2026\n.Dt PROBE 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n{head}\nDescription.\n.El\n"
        );
        assert_names(&source, &aliases, missed);
    }
    for head in [".BI \"-n \" -NUM", ".B -n\n.I -NUM", ".B \"-n -NUM\""] {
        assert_names(
            &format!(".TH PROBE 1\n.SH OPTIONS\n.TP\n{head}\nDescription.\n"),
            &["-n"],
            "-NUM",
        );
    }
}

fn assert_names(source: &str, names: &[&str], missed: &str) {
    let query = crate::query_roff_bytes(source.as_bytes()).unwrap();
    let index = mant_ir::SemanticIndex::build(query.document.as_ref().unwrap());
    let entries = index.section(&query.document.as_ref().unwrap().sections[0].id);
    assert_eq!(entries[0].aliases, names, "{source}");
    for name in names {
        assert!(
            crate::select_explanation(&query, name).is_ok(),
            "{source}: {name}"
        );
    }
    assert!(
        crate::select_explanation(&query, missed).is_err(),
        "{source}: {missed}"
    );
}
