//! Actual man macro offset assignments follow their own pre boundaries.

#[test]
fn man_macro_pre_resets_current_offset_without_restarting_temporary_indent() {
    // Each exact fixture ran pinned pristine ASCII/UTF-8/tree/lint first.
    // man_term.c assigns mt->offset in PP/HP BODY, IP/TP/SY HEAD/BODY,
    // and derived RS BODY after each BLOCK's row/vertical-space pre.
    // ti is a separate register: only an actual endline consumes it.
    let matrix: serde_json::Value = serde_json::from_str(include_str!("synopsis.json")).unwrap();
    assert_eq!(matrix["oracle"]["expectations_from_product"], false);
    assert_cases(&matrix, 32);
}

#[test]
fn hanging_nofill_origin_follows_first_child_native_exit() {
    // Exact sources ran pristine ASCII/UTF-8/tree/lint before writing
    // expected rows. print_man_node's common out:998 executes only for
    // nonempty TEXT and man handlers; empty TEXT, Roff, EQN/TBL return
    // earlier. A source comment is removed by the parser, so the native
    // first child in that source really is Alpha, not a synthetic Comment.
    let matrix: serde_json::Value = serde_json::from_str(include_str!("hanging.json")).unwrap();
    assert_eq!(matrix["oracle"]["expectations_from_product"], false);
    for case in matrix["cases"].as_array().unwrap() {
        let ast = libmandoc_rs::Parser::default()
            .parse_bytes(
                "hanging-first-child.7",
                case["source"].as_str().unwrap().as_bytes(),
            )
            .unwrap();
        let first = first_hp_child(&ast.document.root).expect("actual direct HP BODY child");
        let expected_kind = match case["first_kind"].as_str().unwrap() {
            "text" => libmandoc_rs::NodeKind::Text,
            "element" => libmandoc_rs::NodeKind::Element,
            "equation" => libmandoc_rs::NodeKind::Equation,
            "table" => libmandoc_rs::NodeKind::Table,
            _ => unreachable!("pristine tree fixture category"),
        };
        assert_eq!(
            first.kind, expected_kind,
            "{}: actual first child",
            case["id"]
        );
        assert_eq!(
            first.macro_token.as_deref(),
            case["first_token"].as_str(),
            "{}: actual first handler",
            case["id"]
        );
    }
    assert_cases(&matrix, 11);
}

fn first_hp_child(node: &libmandoc_rs::Node) -> Option<&libmandoc_rs::Node> {
    if node.kind == libmandoc_rs::NodeKind::Body
        && node.macro_token == Some(libmandoc_rs::MacroToken::Man(libmandoc_rs::ManMacro::Hp))
    {
        return node.children.first();
    }
    node.children.iter().find_map(first_hp_child)
}

fn assert_cases(matrix: &serde_json::Value, count: usize) {
    let cases = matrix["cases"].as_array().unwrap();
    assert_eq!(cases.len(), count);
    for case in cases {
        let query =
            mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap();
        let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
        let restored: mant_ir::ResolvedContent = restored.into();
        assert_eq!(query, restored);
        let expected: Vec<_> = case["profiles"]["utf8"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                let row = row.as_str().unwrap();
                let origin = row.bytes().take_while(|byte| *byte == b' ').count();
                format!("{}{}", " ".repeat(origin.saturating_sub(5)), &row[origin..])
            })
            .collect();
        let actual = super::body_rows(&mant_render::render_query_text_at_width(
            &restored,
            75,
            |_, text| text.to_owned(),
        ));
        assert_eq!(
            actual, expected,
            "{}: actual native rows and word origins",
            case["id"]
        );
    }
}
