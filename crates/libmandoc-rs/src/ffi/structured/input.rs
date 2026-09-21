//! Bundle input descriptors and their borrowed source lifetime.

use super::{
    BytesView, FORMAT_MAN, FORMAT_MDOC, IDENTITY_BUNDLE_MEMBER, InputSourceView, InputView, Limits,
    NativeStructuredError, STATUS_BUDGET, STATUS_BUILDER_ALLOC, STATUS_INVALID_INPUT, SliceView,
};
use crate::{InputFormat, SourceBundle};

pub(super) struct InputStorage<'a> {
    _bundle: &'a SourceBundle,
    pub(super) descriptors: Vec<InputSourceView>,
    source_count: u32,
    pub(super) root_input: u32,
}

impl<'a> InputStorage<'a> {
    pub(super) fn new(
        root: &str,
        bundle: &'a SourceBundle,
        format: InputFormat,
        limits: &Limits,
    ) -> Result<Self, NativeStructuredError> {
        let invalid = || NativeStructuredError {
            status: STATUS_INVALID_INPUT,
            stage: 1,
            limit_kind: 0,
            observed: 0,
            allowed: 0,
        };
        let budget = |limit_kind, observed, allowed| NativeStructuredError {
            status: STATUS_BUDGET,
            stage: 1,
            limit_kind,
            observed,
            allowed,
        };
        if !limits.is_valid() {
            return Err(invalid());
        }
        let format = match format {
            InputFormat::Man => FORMAT_MAN,
            InputFormat::Mdoc => FORMAT_MDOC,
            InputFormat::Auto => return Err(invalid()),
        };
        let source_count = u64::try_from(bundle.sources().count()).map_err(|_| invalid())?;
        if source_count > limits.max_input_sources {
            return Err(budget(1, source_count, limits.max_input_sources));
        }
        if source_count > limits.max_source_map_entries {
            return Err(budget(6, source_count, limits.max_source_map_entries));
        }
        let mut root_input = None;
        let mut path_bytes = 0_u64;
        let mut decoded_bytes = 0_u64;
        for (index, (name, bytes)) in bundle.sources().enumerate() {
            if name == root {
                root_input = Some(u32::try_from(index + 1).map_err(|_| invalid())?);
            }
            let name_len = u64::try_from(name.len()).map_err(|_| invalid())?;
            path_bytes = path_bytes.checked_add(name_len).ok_or_else(invalid)?;
            if path_bytes > limits.max_source_path_bytes {
                return Err(budget(3, path_bytes, limits.max_source_path_bytes));
            }
            let source_len = u64::try_from(bytes.len()).map_err(|_| invalid())?;
            if source_len > limits.max_decoded_source_bytes_per_source {
                return Err(budget(
                    4,
                    source_len,
                    limits.max_decoded_source_bytes_per_source,
                ));
            }
            decoded_bytes = decoded_bytes.checked_add(source_len).ok_or_else(invalid)?;
            if decoded_bytes > limits.max_decoded_source_bytes_total {
                return Err(budget(
                    5,
                    decoded_bytes,
                    limits.max_decoded_source_bytes_total,
                ));
            }
        }
        let root_input = root_input.ok_or_else(invalid)?;
        let source_count = u32::try_from(source_count).map_err(|_| invalid())?;
        let mut descriptors = Vec::new();
        descriptors
            .try_reserve_exact(source_count as usize)
            .map_err(|_| NativeStructuredError {
                status: STATUS_BUILDER_ALLOC,
                stage: 1,
                limit_kind: 0,
                observed: u64::from(source_count),
                allowed: limits.max_input_sources,
            })?;
        for (name, bytes) in bundle.sources() {
            descriptors.push(InputSourceView {
                identity_kind: IDENTITY_BUNDLE_MEMBER,
                format,
                logical_name: BytesView {
                    ptr: name.as_ptr(),
                    len: name.len() as u64,
                },
                resolver_name: BytesView {
                    ptr: name.as_ptr(),
                    len: name.len() as u64,
                },
                source_bytes: if bytes.is_empty() {
                    BytesView::default()
                } else {
                    BytesView {
                        ptr: bytes.as_ptr(),
                        len: bytes.len() as u64,
                    }
                },
                reserved: 0,
            });
        }
        Ok(Self {
            _bundle: bundle,
            descriptors,
            source_count,
            root_input,
        })
    }

    pub(super) fn view(&self, width: u32, profile: u32) -> InputView {
        InputView {
            sources: SliceView {
                ptr: self.descriptors.as_ptr().cast(),
                count: self.source_count,
                stride: u32::try_from(std::mem::size_of::<InputSourceView>())
                    .expect("input-source ABI size fits u32"),
            },
            root_input: self.root_input,
            profile,
            width,
            resolve: None,
            resolve_context: std::ptr::null_mut(),
            reserved: 0,
        }
    }
}
