//! Limits in the shared declaration scanner.
use super::*;

#[test]
fn name_limit_is_distinct_from_an_unsupported_weak_head() {
    for (count, expected) in [
        (255, None),
        (256, None),
        (257, Some(DeclarationLimit::Names)),
    ] {
        let value = (0..count)
            .map(|index| format!("--flag{index}"))
            .collect::<Vec<_>>()
            .join(" ");
        let nodes = [strong(vec![text(&value)])];
        let result = option_head(&nodes, &[]);
        assert_eq!(result.limit, expected);
        assert_eq!(
            result.names.len(),
            if expected.is_some() { 0 } else { count }
        );
        let item = mant_ir::DefinitionItem {
            head_body_relation: mant_ir::HeadBodyRelation::Separate,
            terms: (vec![nodes.to_vec()]).into_iter().map(Into::into).collect(),
            description: Vec::new(),
            entry: None,
            layout: mant_ir::DefinitionLayout::default(),
            source: None,
        };
        // A constructed leading role has no accepted native operand proof.
        let inferred = super::super::super::infer_identity(
            &item,
            crate::definitions::context::DefinitionContext::Generic,
            Some(crate::definitions::NativeHeadRole::Option),
            None,
        );
        assert_eq!(inferred.limit, None);
    }
    let opaque = [strong(vec![text("--pattern [unterminated,--fake")])];
    assert_eq!(option_head(&opaque, &[]).limit, None);
}

#[test]
fn one_long_argument_whitespace_run_is_consumed_without_suffix_restarts() {
    let value = format!("--opt VALUE{}ordinary", " ".repeat(65_536));
    let view = HeadView::new(&[text(&value)], &[]);
    let mut scanner = Scanner::new(&view);
    scanner.cursor = "--opt VALUE".len();
    scanner.argument_start = Some(Argument {
        bytes: "--opt ".len().."--opt ".len(),
        attached_to_name: false,
    });
    scanner.consume_argument(' ');
    assert_eq!(scanner.cursor, value.find("ordinary").unwrap());
}

#[test]
fn bare_parameter_heads_keep_monotone_whitespace_and_existing_limits() {
    for size in [128, 1024, 8192] {
        for parameter in ["a".repeat(size), format!("script{}", " ".repeat(size))] {
            let nodes = [
                strong(vec![text("-e")]),
                text(&format!(" {parameter}, ")),
                strong(vec![text("--expression=script")]),
            ];
            assert_names(&nodes, &[], &["-e", "--expression"]);
            assert!(option_head(&nodes, &[]).inferred_complete);
        }
        let nodes = (0..size)
            .flat_map(|index| {
                [
                    strong(vec![text(&format!("--flag{index}"))]),
                    text(" arg, "),
                ]
            })
            .collect::<Vec<_>>();
        let result = option_head(&nodes, &[]);
        assert_eq!(
            result.limit,
            (size > MAX_NAMES).then_some(DeclarationLimit::Names)
        );
        assert_eq!(result.names.len(), if size > MAX_NAMES { 0 } else { size });
    }
}
