use std::ops::{Deref, DerefMut};

use super::{RenderError, RenderErrorKind};

/// One renderer-owned buffer with a complete-output byte budget.
///
/// Device helpers may still borrow the inner `String` directly for in-place
/// layout operations.  The owning boundary performs the final invariant
/// check, while `append_checked` rejects growth before copying bytes.
pub(super) struct BoundedOutput {
    buffer: String,
    maximum: usize,
}

impl BoundedOutput {
    pub(super) const fn new(maximum: usize) -> Self {
        Self {
            buffer: String::new(),
            maximum,
        }
    }

    pub(super) fn finish(self) -> Result<String, RenderError> {
        ensure_length(self.buffer.len(), self.maximum)?;
        Ok(self.buffer)
    }

    pub(super) fn finish_trimmed(self) -> Result<String, RenderError> {
        ensure_length(self.buffer.len(), self.maximum)?;
        Ok(self.buffer.trim_end().to_owned())
    }
}

impl Deref for BoundedOutput {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.buffer
    }
}

impl DerefMut for BoundedOutput {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.buffer
    }
}

pub(super) fn append_checked(
    output: &mut String,
    value: &str,
    maximum: usize,
) -> Result<(), RenderError> {
    let length = output.len().saturating_add(value.len());
    ensure_length(length, maximum)?;
    output.push_str(value);
    Ok(())
}

pub(super) fn ensure_length(length: usize, maximum: usize) -> Result<(), RenderError> {
    if length > maximum {
        return Err(output_limit(maximum));
    }
    Ok(())
}

pub(super) fn output_limit(maximum: usize) -> RenderError {
    RenderError {
        kind: RenderErrorKind::OutputLimit,
        message: format!("rendered output exceeds {maximum} bytes").into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{BoundedOutput, append_checked};

    #[test]
    fn checked_append_and_final_guard_share_the_same_byte_budget() {
        let mut checked = String::new();
        append_checked(&mut checked, "é", 2).expect("two UTF-8 bytes fit");
        assert!(append_checked(&mut checked, "x", 2).is_err());

        let mut guarded = BoundedOutput::new(2);
        guarded.push_str("éx");
        assert!(guarded.finish().is_err());
    }
}
