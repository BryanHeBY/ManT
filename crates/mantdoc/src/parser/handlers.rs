#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub(in crate::parser) enum RequestTransition {
    /// The handler consumed the control event; source dispatch resumes with
    /// the next event.
    Consumed,
    /// The handler deliberately left the event for package or user-macro
    /// dispatch.
    Continue,
}

pub(super) mod environment;
pub(super) mod transparent;
