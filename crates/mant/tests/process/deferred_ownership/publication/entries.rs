use super::{Case, array, inline_text};
use serde_json::{Value, json};

#[path = "../../../../../mant-engine/tests/support/explanation_ranges.rs"]
mod explanation_ranges;

fn names(case: &Case<'_>) -> &'static [&'static str] {
    match (case.axes[0], case.axes[1], case.axes[2]) {
        (_, "digit", _) => &["-1"],
        (_, "flag", _) => &["-f"],
        ("OPTIONS" | "ENVIRONMENT", "command", "sy") => &["run FILE"],
        (_, "command", _) => &["run"],
        (_, "env", "xr" | "sy") => &[],
        (_, "env", _) => &["DEMO_HOME"],
        _ => panic!("unknown head {}", case.id),
    }
}

pub(super) fn head(case: &Case<'_>, blocks: &[Value]) {
    // CVS mdoc_term.c termp_fl_pre/termp_ev_pre execute the role-bearing
    // first operand. The entry role and admissible lexical names follow
    // the documented ManT classification contract for those operands.
    assert_eq!(blocks.len(), 1, "{}", case.id);
    assert_eq!(blocks[0]["type"], "definition-list", "{}", case.id);
    let items = array(&blocks[0]["items"]);
    assert_eq!(items.len(), 1, "{}", case.id);
    let item = &items[0];
    let kind = match case.axes[1] {
        "digit" | "flag" => json!({"kind":"parameter","parameterKind":"option"}),
        "command" => json!({"kind":"term"}),
        "env" => json!({"kind":"environment-variable"}),
        _ => unreachable!(),
    };
    assert_eq!(item["entry"]["kind"], kind, "{} classification", case.id);
    assert_eq!(
        item["entry"]["names"],
        json!(names(case)),
        "{} names",
        case.id
    );
    assert_eq!(
        item["source"],
        json!({"line":9,"column":2}),
        "{} source",
        case.id
    );
    assert_eq!(
        inline_text(&item["description"][0]["children"]),
        "BODY",
        "{}",
        case.id
    );
}

fn shares(case: &Case<'_>) -> bool {
    case.family == "X-head-marker" || matches!(case.axes[1], "adjacent" | "if0")
}

pub(super) fn group(case: &Case<'_>, blocks: &[Value]) {
    let block = &blocks[0];
    let items = array(&block["items"]);
    assert_eq!(items.len(), 2, "{} declaration owners", case.id);
    assert_eq!(items[0]["entry"]["names"], json!(["-a"]), "{}", case.id);
    assert_eq!(items[1]["entry"]["names"], json!(["-b"]), "{}", case.id);
    assert_ne!(
        items[0]["source"], items[1]["source"],
        "{} owner sources",
        case.id
    );
    let groups = &block["declarationGroups"];
    if shares(case) {
        // CVS tree confirms .if 0 .Pp executes no paragraph. A real .Pp
        // and text in the first BODY are ManT declaration-group boundaries.
        assert_eq!(groups, &json!([{"startItem":0,"endItem":2}]), "{}", case.id);
    } else {
        assert!(groups.as_array().is_none_or(Vec::is_empty), "{}", case.id);
    }
}

pub(super) fn hanging(case: &Case<'_>, blocks: &[Value]) {
    if case.axes[1] != "headless" {
        return;
    }
    assert_eq!(blocks.len(), 1, "{} hanging owner blocks", case.id);
    let block = &blocks[0];
    assert_eq!(block["type"], "list", "{}", case.id);
    let items = array(&block["items"]);
    assert_eq!(items.len(), 1, "{} hanging owner", case.id);
    assert_eq!(items[0]["entry"]["names"], json!(["--one"]), "{}", case.id);
    let nested = array(&items[0]["blocks"]);
    assert_eq!(nested.len(), 2, "{} HP and receiving IP", case.id);
    assert_eq!(
        inline_text(&nested[1]["items"][0]["description"][0]["children"]),
        "SHAREDBODY",
        "{} receiving BODY",
        case.id
    );
}

pub(super) fn query(
    case: &Case<'_>,
    binary: &str,
    query: &str,
    blocks: &[Value],
    response: &Value,
    explanation: &mant_protocol::QueryExplanation,
) {
    let direct = match case.family {
        "N1a" => names(case).contains(&query),
        "N1c" => case.axes[1] == "headless",
        "X-head-marker" => query != "printf",
        _ => true,
    };
    assert_eq!(
        response["counts"]["directEntry"]["total"],
        u32::from(direct),
        "{} {query} direct owners",
        case.id
    );
    if !direct {
        return;
    }
    let evidence = &response["evidence"][0];
    assert_eq!(evidence["class"], "direct-entry", "{} {query}", case.id);
    assert!(
        array(&evidence["bases"])
            .iter()
            .any(|basis| basis["kind"] == "name"),
        "{} {query} name basis",
        case.id
    );
    assert!(
        explanation_ranges::assert_ranges(explanation, query) > 0,
        "{} {query}",
        case.id
    );

    if query == "-a" {
        let member = &evidence["content"];
        let shared = shares(case);
        if shared {
            shared_support(case, &blocks[0], response);
        } else {
            assert_ne!(
                member["kind"], "declaration-member",
                "{} spurious shared owner",
                case.id
            );
        }
        let flag = format!("--explain={query}");
        let rendered = String::from_utf8(case.output(binary, "text", &[&flag])).unwrap();
        assert_eq!(
            rendered.matches("SHAREDBODY").count(),
            usize::from(shared),
            "{} actual explanation reading",
            case.id
        );
    }
}

pub(super) fn shared_support(case: &Case<'_>, original: &Value, response: &Value) {
    let evidence = &response["evidence"][0];
    let member = &evidence["content"];
    assert_eq!(
        member["kind"], "declaration-member",
        "{} support kind",
        case.id
    );
    assert_eq!(member["itemIndex"], 0, "{} member index", case.id);
    let support_index = usize::try_from(member["support"].as_u64().unwrap()).unwrap();
    let support = &response["supports"][support_index];
    assert_eq!(support["kind"], "declaration-group", "{}", case.id);
    assert_eq!(
        support["group"],
        json!({"startItem":0,"endItem":2}),
        "{} group range",
        case.id
    );
    assert_eq!(
        support["members"][0], evidence["outline"],
        "{} requesting owner",
        case.id
    );
    assert_eq!(
        support["members"][1]["node"]["names"],
        json!(["-b"]),
        "{} provider name",
        case.id
    );
    assert!(
        inline_text(&support["block"]["items"][1]["description"][0]["children"])
            .starts_with("SHAREDBODY"),
        "{} provider content",
        case.id
    );
    assert_eq!(&support["block"], original, "{} source owner", case.id);
}
