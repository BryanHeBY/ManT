//! Narrow wire migration for the former collapsed lower-decoration cache.
use super::{EquationExpression, EquationValidationError, EquationValidationLimits};

impl EquationExpression {
    /// Accept the canonical cache or its exact historical lower-decoration
    /// projection, then publish the canonical reading. This is only used by
    /// wire owners; in-memory projection validation remains strict.
    pub(crate) fn normalize_readable_cache(
        &self,
        value: &mut String,
    ) -> Result<(), EquationValidationError> {
        self.validate(EquationValidationLimits::default())?;
        let canonical = self.readable_text();
        if *value == canonical {
            return Ok(());
        }
        if *value != self.legacy_bottom_readable_text() {
            return Err(EquationValidationError::InvalidNode(
                "equation text does not match its owned expression",
            ));
        }
        *value = canonical;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
