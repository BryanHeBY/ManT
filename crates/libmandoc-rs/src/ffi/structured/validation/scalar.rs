//! Bounded UTF-8 scalar-boundary lookup for one checked native handle.

use super::super::{
    BytesView, ContentAtomView, NativeStructuredError, alloc_error, relation_error,
};
use super::preflight::validate_utf8_view;

const STRIDE: usize = 64;

pub(super) struct ScalarBoundaryIndex {
    atom_offsets: Vec<usize>,
    checkpoints: Vec<u32>,
}

impl ScalarBoundaryIndex {
    pub(super) fn build(atoms: &[ContentAtomView]) -> Result<Self, NativeStructuredError> {
        let slots = atoms.len().checked_add(1).ok_or_else(relation_error)?;
        let mut atom_offsets = Vec::new();
        atom_offsets.try_reserve_exact(slots).map_err(alloc_error)?;
        let capacity = atoms.iter().try_fold(0_usize, |total, atom| {
            let length = usize::try_from(atom.text.len).map_err(|_| relation_error())?;
            total
                .checked_add(length / STRIDE)
                .ok_or_else(relation_error)
        })?;
        let mut checkpoints = Vec::new();
        checkpoints
            .try_reserve_exact(capacity)
            .map_err(alloc_error)?;
        for atom in atoms {
            atom_offsets.push(checkpoints.len());
            let length =
                usize::try_from(validate_utf8_view(atom.text)?).map_err(|_| relation_error())?;
            if length == 0 {
                continue;
            }
            // SAFETY: validate_utf8_view checked this handle-bound pointer
            // and length; no reference escapes the validation call.
            let bytes = unsafe { std::slice::from_raw_parts(atom.text.ptr, length) };
            let mut scalars = 0_u32;
            for (index, byte) in bytes.iter().enumerate() {
                if byte & 0xc0 != 0x80 {
                    scalars = scalars.checked_add(1).ok_or_else(relation_error)?;
                }
                if (index + 1) % STRIDE == 0 {
                    checkpoints.push(scalars);
                }
            }
        }
        atom_offsets.push(checkpoints.len());
        Ok(Self {
            atom_offsets,
            checkpoints,
        })
    }

    pub(super) fn prefix(&self, atom: usize, text: BytesView, end: u32) -> Option<u32> {
        let length = usize::try_from(text.len).ok()?;
        let end = end as usize;
        if end > length {
            return None;
        }
        let checkpoint = end / STRIDE;
        let prior = if checkpoint == 0 {
            0
        } else {
            *self
                .checkpoints
                .get(self.atom_offsets.get(atom)? + checkpoint - 1)?
        };
        let base = checkpoint * STRIDE;
        if base == end {
            return Some(prior);
        }
        if text.ptr.is_null() {
            return None;
        }
        // SAFETY: build validated this handle-bound view, and end is inside
        // its length.  We keep no borrowed data in the index.
        let bytes = unsafe { std::slice::from_raw_parts(text.ptr, length) };
        let additional = u32::try_from(
            bytes[base..end]
                .iter()
                .filter(|byte| **byte & 0xc0 != 0x80)
                .count(),
        )
        .ok()?;
        prior.checked_add(additional)
    }
}
