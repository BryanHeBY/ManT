use crate::delivery::pager::native::Pager;
use crate::delivery::pager::native::error::MinusError;
use crate::delivery::pager::native::minus_core::init;

/// Starts a asynchronously running pager
///
/// This means that data and configuration can be fed into the pager while it is running.
///
/// See [examples](../index.html#examples) on how to use this function.
///
/// # Panics
/// This function will panic if another instance of minus is already running.
///
/// # Errors
/// The function will return with an error if it encounters a error during paging.
#[allow(clippy::needless_pass_by_value)]
pub fn dynamic_paging(pager: Pager) -> Result<(), MinusError> {
    init::init_core(&pager, crate::delivery::pager::native::RunMode::Dynamic)
}
