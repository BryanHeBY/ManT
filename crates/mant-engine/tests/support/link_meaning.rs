//! Historical link-source controls and the shared independent witness.
pub use crate::link_evidence::Meaning;

pub const SOURCES: &[&str] = &[
    r"[_! ***a***_&#32;](https://a.test)",
    r#"[LinkWord](https://a.test "say \\")"#,
    r#"[LinkWord](https://a.test "say \\\\")"#,
    r#"[LinkWord](https://a.test/&amp;amp; "&amp;amp;")"#,
    r#"[LinkWord](https://a.test/&#38;amp; "&#38;amp;")"#,
    "[LinkWord](https://a.test \"one\ntwo\")",
    "[LinkWord](https://a.test \"one\r\ntwo\")",
    r#"[LinkWord](https://a.test "quote \" and \\!")"#,
    r"[`a*b`](https://a.test/a(b)%25)",
    "[LINK_A  \nLINK_B](https://a.test)",
];
