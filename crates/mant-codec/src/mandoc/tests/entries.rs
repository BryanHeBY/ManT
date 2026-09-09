//! Codec-internal lowering contracts; no query or rendering dependencies.
use super::*;

#[test]
fn reads_incrementing_registers_only_from_ip_markers() {
    let context = LoweringContext::new(
        None,
        Some(".IP \\n+[step] 4\n.IP 1 \\n+[width]\n.IPX \\n+[other]\n'IP \"\\n+[quoted]\" 4\n"),
    );

    assert!(context.man_ip_uses_incrementing_register(1));
    assert!(!context.man_ip_uses_incrementing_register(2));
    assert!(!context.man_ip_uses_incrementing_register(3));
    assert!(context.man_ip_uses_incrementing_register(4));
}
