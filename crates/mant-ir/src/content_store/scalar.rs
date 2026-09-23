//! Small scalar-prefix checkpoints for bounded UTF-8 relation validation.

use super::ContentStore;

const STRIDE: usize = 64;

pub(super) struct ScalarBoundaryIndex {
    atom_offsets: Vec<usize>,
    checkpoints: Vec<u32>,
}

impl ScalarBoundaryIndex {
    pub(super) fn build(store: &ContentStore) -> Option<Self> {
        let mut atom_offsets = Vec::new();
        atom_offsets
            .try_reserve_exact(store.atoms.len().checked_add(1)?)
            .ok()?;
        let capacity = store.atoms.iter().try_fold(0_usize, |total, atom| {
            total.checked_add(atom.kind.text().map_or(0, |text| text.len() / STRIDE))
        })?;
        let mut checkpoints = Vec::new();
        checkpoints.try_reserve_exact(capacity).ok()?;
        for atom in &store.atoms {
            atom_offsets.push(checkpoints.len());
            if let Some(text) = atom.kind.text() {
                let mut scalars = 0_u32;
                for (index, byte) in text.bytes().enumerate() {
                    if byte & 0xc0 != 0x80 {
                        scalars = scalars.checked_add(1)?;
                    }
                    if (index + 1) % STRIDE == 0 {
                        checkpoints.push(scalars);
                    }
                }
            }
        }
        atom_offsets.push(checkpoints.len());
        Some(Self {
            atom_offsets,
            checkpoints,
        })
    }

    pub(super) fn prefix(&self, atom: usize, text: &str, end: usize) -> Option<u32> {
        if !text.is_char_boundary(end) {
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
        let additional = u32::try_from(
            text.as_bytes()[base..end]
                .iter()
                .filter(|byte| **byte & 0xc0 != 0x80)
                .count(),
        )
        .ok()?;
        prior.checked_add(additional)
    }
}
