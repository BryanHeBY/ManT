//! Private artifact finalization keeps exact ownership and lazy anchor ranges.
use super::MarkdownNode;

#[test]
fn final_artifact_owns_only_real_anchor_ranges() {
    let mut builder = super::ArtifactBuilder {
        track: true,
        ..super::ArtifactBuilder::default()
    };
    builder.begin_root(0);
    builder.push("<a id=\"real\"></a>\n`<a id=\"literal\"></a>`\n\n ");
    let artifact = builder.finish();
    assert!(artifact.anchors.get().is_none());
    let ranges = artifact.anchor_ranges();
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0], 0.."<a id=\"real\"></a>".len());
    assert_eq!(&artifact.text()[ranges[0].clone()], "<a id=\"real\"></a>");
    assert!(std::ptr::eq(ranges, artifact.anchor_ranges()));
    assert!(artifact.text().ends_with('`'));
    assert_eq!(artifact.nodes().len(), 1);
    assert_eq!(artifact.nodes()[0].range, 0..artifact.text().len());
    assert!(matches!(
        artifact.nodes()[0].node,
        MarkdownNode::DocumentRoot
    ));
    let final_text = artifact.text().to_owned();
    assert_eq!(artifact.into_text(), final_text);
}
