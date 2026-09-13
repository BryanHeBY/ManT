#![cfg(feature = "execute")]
#![allow(unsafe_code)]

#[link(name = "mant_mandoc", kind = "static")]
unsafe extern "C" {
    fn mant_mandoc_execution_validation_selftest() -> u32;
}

#[test]
fn native_seal_rejects_invalid_origins_wrappers_and_table_pairing() {
    // Pinned term.c wraps print_*_node() in strict enter/leave scopes and
    // pinned tbl_term.c brackets one tbl_data() call with the same cell facts.
    // The C-side test deliberately violates those private execution facts.
    let failures = unsafe { mant_mandoc_execution_validation_selftest() };
    assert_eq!(
        failures, 0,
        "native validation self-test bits: {failures:#x}"
    );
}
