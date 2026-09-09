//! Optional source-bound explanation probes; not another declaration classifier.
use libmandoc_rs::Node;
use mant_ir::ResolvedContent;
use mant_protocol::{ExplanationOptions, ExplanationQuery};
use serde_json::{Value, json};

pub(super) fn profile(
    root: Option<&Node>,
    content: &ResolvedContent,
    queries: &[Value],
) -> Result<Vec<Value>, String> {
    if queries.len() > 16 {
        return Err("at most 16 query probes per page".into());
    }
    queries.iter().map(|query| {
        let query = query.as_str().ok_or("query probe must be a string")?;
        let explanation = mant_query::explain_query(content, &ExplanationQuery {
            entry: query.into(), options: ExplanationOptions { limit: 256, content_bytes: 4 * 1024 * 1024, ..Default::default() },
        }).map_err(|error| error.to_string())?;
        let witnesses: Vec<_> = explanation.evidence.iter().filter_map(|evidence| {
            let source = evidence.source?;
            let mut candidates = Vec::new();
            if let Some(root) = root {
                source_candidates(root, source.line, source.column, &mut Vec::new(), &mut candidates);
            }
            Some(json!({"owner": evidence.outline.node.id(), "source": source,
                "status": if candidates.is_empty() {"unresolved"} else {"source-candidates"},
                "candidates": candidates}))
        }).collect();
        Ok(json!({"query": query, "explanation": explanation, "nativeSourceWitnesses": witnesses}))
    }).collect()
}

fn source_candidates(
    node: &Node,
    line: u32,
    column: u32,
    path: &mut Vec<usize>,
    output: &mut Vec<Value>,
) {
    if node.line == line && node.column == column {
        output.push(json!({"astPath": path, "macro": node.macro_name, "nodeKind": format!("{:?}", node.kind), "text": node.text}));
    }
    for (index, child) in node.children.iter().enumerate() {
        path.push(index);
        source_candidates(child, line, column, path, output);
        path.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn probes_retain_distinct_same_name_owners_and_original_source_coordinates() {
        let raw = b".TH TEST 1\n.SH OPTIONS\n.TP\n.B -x\n.TP\n.B -x ARG\nSECOND_BODY\n";
        let report = libmandoc_rs::Parser::new(Default::default())
            .parse_bytes("probe.1", raw)
            .unwrap();
        let content = mant_loader::load_roff_bytes(raw).unwrap();
        let rows = profile(Some(&report.document.root), &content, &[json!("-x")]).unwrap();
        assert_eq!(rows[0]["explanation"]["counts"]["directEntry"]["total"], 2);
        let evidence = rows[0]["explanation"]["evidence"].as_array().unwrap();
        assert_ne!(evidence[0]["source"], evidence[1]["source"]);
        assert_eq!(
            rows[0]["nativeSourceWitnesses"].as_array().unwrap().len(),
            2
        );
        assert!(profile(Some(&report.document.root), &content, &vec![json!("x"); 17]).is_err());
        let replay = profile(None, &content, &[json!("-x")]).unwrap();
        assert_eq!(replay[0]["explanation"], rows[0]["explanation"]);
        assert!(
            replay[0]["nativeSourceWitnesses"]
                .as_array()
                .unwrap()
                .iter()
                .all(|w| w["status"] == "unresolved"
                    && w["candidates"].as_array().unwrap().is_empty())
        );
    }
}
