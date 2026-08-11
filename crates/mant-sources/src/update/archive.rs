//! Conditional HTTP archive acquisition for one configured source.

use crate::{
    archive::extract_archive,
    download::{DownloadOutcome, download_archive},
};

use super::{
    SourceMetadata, SourceUpdateContext, SourceUpdateResult, UpdateWorkspace, activate_source,
    install_selected_documents,
};

pub(super) fn update(
    context: &SourceUpdateContext<'_>,
    url: &str,
) -> Result<SourceUpdateResult, String> {
    let workspace = UpdateWorkspace::new(&context.paths.sources, context.name);
    let validators = context
        .metadata
        .as_ref()
        .and_then(SourceMetadata::validators);
    match download_archive(url, &workspace.download, validators.as_ref())? {
        DownloadOutcome::NotModified => {
            let metadata = context.metadata.as_ref().ok_or_else(|| {
                "archive server returned not-modified without installed metadata".to_owned()
            })?;
            Ok(context.unchanged(metadata.revision().to_owned()))
        }
        DownloadOutcome::Downloaded {
            revision,
            validators,
        } => {
            if let Some(metadata) = &context.metadata
                && metadata.revision() == revision
            {
                return Ok(context.unchanged(revision));
            }
            extract_archive(&workspace.download, &workspace.checkout)?;
            workspace.create_staging()?;
            let documents = install_selected_documents(
                &workspace.checkout,
                &workspace.staging,
                context.configured,
            )?;
            let document_count = u32::try_from(documents).unwrap_or(u32::MAX);
            let metadata = SourceMetadata::archive(
                context.name,
                url,
                revision.clone(),
                &context.fingerprint,
                document_count,
                validators,
            );
            activate_source(&workspace.staging, &context.target, &metadata)?;
            Ok(context.updated(revision, document_count))
        }
    }
}
