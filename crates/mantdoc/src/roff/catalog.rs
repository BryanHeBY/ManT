/// mandoc's small, read-only roff string compatibility catalog.
const PREDEFINED_STRINGS: &[(&[u8], &[u8])] = &[
    (b"Am", b"&"),
    (b"Ba", b"\\fR|\\fP"),
    (b"Ge", b"\\(>="),
    (b"Gt", b">"),
    (b"If", b"infinity"),
    (b"Le", b"\\(<="),
    (b"Lq", b"\\(lq"),
    (b"Lt", b"<"),
    (b"Na", b"NaN"),
    (b"Ne", b"\\(!="),
    (b"Pi", b"pi"),
    (b"Pm", b"\\(+-"),
    (b"Rq", b"\\(rq"),
    (b"left-bracket", b"["),
    (b"left-parenthesis", b"("),
    (b"lp", b"("),
    (b"left-singlequote", b"\\(oq"),
    (b"q", b"\\(dq"),
    (b"quote-left", b"\\(oq"),
    (b"quote-right", b"\\(cq"),
    (b"R", b"\\(rg"),
    (b"right-bracket", b"]"),
    (b"right-parenthesis", b")"),
    (b"rp", b")"),
    (b"right-singlequote", b"\\(cq"),
    (b"Tm", b"(Tm)"),
    (b"Px", b"POSIX"),
    (b"Ai", b"ANSI"),
    (b"'", b"\\'"),
    (b"aa", b"\\(aa"),
    (b"ga", b"\\(ga"),
    (b"`", b"\\`"),
    (b"lq", b"\\(lq"),
    (b"rq", b"\\(rq"),
    (b"ua", b"\\(ua"),
    (b"va", b"\\(va"),
    (b"<=", b"\\(<="),
    (b">=", b"\\(>="),
];

pub(super) fn predefined_string(name: &[u8]) -> Option<&'static [u8]> {
    PREDEFINED_STRINGS
        .iter()
        .find_map(|(candidate, value)| (*candidate == name).then_some(*value))
}

pub(super) fn predefined_register(name: &[u8]) -> Option<i32> {
    match name {
        b".A" | b".j" => Some(0),
        b".g" | b".T" => Some(1),
        b".H" => Some(24),
        b".V" => Some(40),
        _ => None,
    }
}
