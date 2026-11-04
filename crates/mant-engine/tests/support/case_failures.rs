//! Run each independent fixture case before reporting all failures together.

use std::panic::{RefUnwindSafe, catch_unwind};

pub fn verify_cases<Case: RefUnwindSafe>(
    cases: impl IntoIterator<Item = Case>,
    id: impl Fn(&Case) -> &str,
    check: impl Fn(&Case) + RefUnwindSafe,
) {
    let mut failures = Vec::new();
    for case in cases {
        if let Err(error) = catch_unwind(|| check(&case)) {
            let message = error
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| error.downcast_ref::<&str>().copied())
                .unwrap_or("non-string test panic");
            failures.push(format!("{}: {message}", id(&case)));
        }
    }
    assert!(
        failures.is_empty(),
        "{} case failures:\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
