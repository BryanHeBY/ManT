//! Resolves logical document names across registered sources, manuals, and tldr.

use super::{
    DocumentAddress, FullDocumentMode, LoadedManual, ManualLoadError, ManualRequest,
    MarkdownOrigin, QueryError, QueryHost, QueryPolicy, QuickReferenceMode, RegisteredLookupPhase,
    RegisteredSelection, RegisteredSelectionGroup, ResolvedContent, TldrDocument,
    query_markdown_text,
};

pub(super) fn query_named_document(
    name: &str,
    requested_source: Option<&str>,
    requested_manual_section: Option<&str>,
    policy: QueryPolicy,
    host: &dyn QueryHost,
) -> Result<ResolvedContent, QueryError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(QueryError::EmptyName);
    }
    if let Some(address) = parse_catalog_address(name) {
        if requested_source.is_some() || requested_manual_section.is_some() {
            return Err(QueryError::ConflictingSourceSelectors);
        }
        return query_catalog_address(name, &address, policy, host);
    }
    let section = requested_manual_section.map(str::trim);
    if section.is_some_and(|section| !crate::is_manual_section(section)) {
        return Err(QueryError::InvalidManualSection);
    }
    let section = section.map(ToOwned::to_owned);
    let source = requested_source.map(str::trim);
    if source.is_some_and(str::is_empty) {
        return Err(QueryError::InvalidSource);
    }
    let plan = policy.named_resolution_plan(section.is_some());
    if source.is_some() && (section.is_some() || plan.document == FullDocumentMode::NativeManual) {
        return Err(QueryError::ConflictingSourceSelectors);
    }
    let candidates = host.name_candidates(name);

    if plan.quick_reference == QuickReferenceMode::Only {
        if let Some(section) = section.as_deref()
            && !crate::is_command_manual_section(section)
        {
            return Err(QueryError::TldrManualSection {
                section: section.to_owned(),
            });
        }
        return query_tldr_only(name, &candidates, source, host);
    }

    // Personal documents and positive-priority sources form the preferred
    // registration phase. Explicit source selection always wins regardless of
    // its configured rank. Non-positive sources are consulted only after the
    // priority-zero native-manual phase fails.
    if plan.document == FullDocumentMode::Priority {
        let registered = host
            .locate_registered_document(&candidates, source, RegisteredLookupPhase::BeforeBuiltin)
            .map_err(|detail| QueryError::Registry { detail })?;
        if let Some(registered) = registered {
            return query_registered_document(name, &registered, host);
        }
        if source.is_some() {
            return Err(QueryError::NoReadableContent {
                name: name.to_owned(),
            });
        }
    }

    let mut manual = load_manual(name, &candidates, section.as_deref(), host);

    // A malformed page may omit its own section metadata. Preserve the
    // requested section so labels stay `name(N)`.
    if let (Ok(manual), Some(section)) = (&mut manual, section.as_deref())
        && manual.document.meta.manual_section.is_none()
    {
        manual.document.meta.manual_section = Some(section.to_owned());
    }

    let tldr = match plan.quick_reference {
        QuickReferenceMode::AttachToCommandManual => match &manual {
            Ok(manual) if manual_accepts_tldr(manual) => host.read_tldr(name).ok().flatten(),
            Err(_)
                if section
                    .as_deref()
                    .is_none_or(crate::is_command_manual_section) =>
            {
                host.read_tldr(name).ok().flatten()
            }
            Ok(_) | Err(_) => None,
        },
        QuickReferenceMode::Exclude => None,
        QuickReferenceMode::Only => unreachable!("tldr-only queries returned before manual I/O"),
    };

    match plan.document {
        FullDocumentMode::Priority => {
            finish_unqualified_manual(name, &candidates, manual, tldr, host)
        }
        FullDocumentMode::NativeManual => finish_selected_manual(name, manual, tldr),
        FullDocumentMode::None => unreachable!("tldr-only queries returned before manual I/O"),
    }
}

fn manual_accepts_tldr(manual: &LoadedManual) -> bool {
    let DocumentAddress::Manual { manual_section, .. } = &manual.address else {
        return false;
    };
    crate::is_command_manual_section(manual_section)
}

fn query_catalog_address(
    selector: &str,
    address: &DocumentAddress,
    policy: QueryPolicy,
    host: &dyn QueryHost,
) -> Result<ResolvedContent, QueryError> {
    match address {
        DocumentAddress::Markdown { .. } if policy == QueryPolicy::TldrOnly => {
            let registered = host
                .locate_registered_address(address)
                .map_err(|detail| QueryError::Registry { detail })?
                .ok_or_else(|| QueryError::TldrNotFound {
                    topic: selector.to_owned(),
                })?;
            query_registered_tldr(selector, &registered, host)?.ok_or_else(|| {
                QueryError::TldrNotFound {
                    topic: selector.to_owned(),
                }
            })
        }
        DocumentAddress::Markdown { .. } if policy == QueryPolicy::ManualOnly => {
            Err(QueryError::ConflictingSourceSelectors)
        }
        DocumentAddress::Markdown { .. } => {
            let registered = host
                .locate_registered_address(address)
                .map_err(|detail| QueryError::Registry { detail })?
                .ok_or_else(|| QueryError::NoReadableContent {
                    name: selector.to_owned(),
                })?;
            query_registered_document(selector, &registered, host)
        }
        DocumentAddress::Manual {
            name,
            manual_section,
        } => query_named_document(name, None, Some(manual_section), policy, host),
    }
}

fn query_tldr_only(
    name: &str,
    candidates: &[String],
    source: Option<&str>,
    host: &dyn QueryHost,
) -> Result<ResolvedContent, QueryError> {
    let before = host
        .locate_registered_document_groups(candidates, source, RegisteredLookupPhase::BeforeBuiltin)
        .map_err(|detail| QueryError::Registry { detail })?;
    if let Some(tldr) = first_registered_tldr(name, before, host)? {
        return Ok(tldr);
    }
    if source.is_some() {
        return Err(QueryError::TldrNotFound {
            topic: name.to_owned(),
        });
    }

    if let Some(tldr) = host.read_tldr(name).map_err(|detail| QueryError::Tldr {
        topic: name.to_owned(),
        detail,
    })? {
        return Ok(ResolvedContent {
            address: None,
            label: name.to_owned(),
            document: None,
            tldr: Some(tldr),
        });
    }

    let after = host
        .locate_registered_document_groups(candidates, None, RegisteredLookupPhase::AfterBuiltin)
        .map_err(|detail| QueryError::Registry { detail })?;
    first_registered_tldr(name, after, host)?.ok_or_else(|| QueryError::TldrNotFound {
        topic: name.to_owned(),
    })
}

fn first_registered_tldr(
    name: &str,
    groups: Vec<RegisteredSelectionGroup>,
    host: &dyn QueryHost,
) -> Result<Option<ResolvedContent>, QueryError> {
    for group in groups {
        let mut matches = Vec::new();
        for registered in group.documents {
            if let Some(tldr) = query_registered_tldr(name, &registered, host)? {
                matches.push(tldr);
            }
        }
        match matches.len() {
            0 => {}
            1 => return Ok(matches.pop()),
            _ => {
                let choices = matches
                    .iter()
                    .filter_map(|candidate| candidate.address.as_ref())
                    .map(DocumentAddress::catalog_path)
                    .collect::<Vec<_>>()
                    .join("', '");
                return Err(QueryError::Registry {
                    detail: format!(
                        "tldr selector '{name}' is ambiguous at one document priority: '{choices}'"
                    ),
                });
            }
        }
    }
    Ok(None)
}

fn query_registered_tldr(
    name: &str,
    registered: &RegisteredSelection,
    host: &dyn QueryHost,
) -> Result<Option<ResolvedContent>, QueryError> {
    let resolved = query_registered_document(name, registered, host)?;
    let Some(tldr) = resolved.tldr else {
        return Ok(None);
    };
    Ok(Some(ResolvedContent {
        address: resolved.address,
        label: resolved.label,
        document: None,
        tldr: Some(tldr),
    }))
}

fn finish_selected_manual(
    name: &str,
    manual: Result<LoadedManual, ManualLoadError>,
    tldr: Option<TldrDocument>,
) -> Result<ResolvedContent, QueryError> {
    match manual {
        Ok(manual) => Ok(ResolvedContent {
            address: Some(manual.address),
            label: name.to_owned(),
            document: Some(manual.document),
            tldr,
        }),
        Err(error) if tldr.is_some() => Err(QueryError::ManualWithTldr {
            error,
            topic: name.to_owned(),
        }),
        Err(error) => Err(QueryError::Manual(error)),
    }
}

fn finish_unqualified_manual(
    name: &str,
    candidates: &[String],
    manual: Result<LoadedManual, ManualLoadError>,
    tldr: Option<TldrDocument>,
    host: &dyn QueryHost,
) -> Result<ResolvedContent, QueryError> {
    match manual {
        Ok(manual) => Ok(ResolvedContent {
            address: Some(manual.address),
            label: name.to_owned(),
            document: Some(manual.document),
            tldr,
        }),
        Err(error) => {
            let registered = host
                .locate_registered_document(candidates, None, RegisteredLookupPhase::AfterBuiltin)
                .map_err(|detail| QueryError::Registry { detail })?;
            if let Some(registered) = registered {
                query_registered_document(name, &registered, host)
            } else if tldr.is_some() {
                Err(QueryError::ManualWithTldr {
                    error,
                    topic: name.to_owned(),
                })
            } else {
                Err(QueryError::Manual(error))
            }
        }
    }
}

fn parse_catalog_address(selector: &str) -> Option<DocumentAddress> {
    if let Some(path) = selector.strip_prefix("documents/")
        && !path.is_empty()
    {
        return Some(DocumentAddress::Markdown {
            path: path.to_owned(),
            origin: MarkdownOrigin::Documents,
        });
    }
    if let Some(rest) = selector.strip_prefix("sources/") {
        let (source, path) = rest.split_once('/')?;
        if !source.is_empty() && !path.is_empty() {
            return Some(DocumentAddress::Markdown {
                path: path.to_owned(),
                origin: MarkdownOrigin::Source {
                    name: source.to_owned(),
                },
            });
        }
    }
    if let Some(rest) = selector.strip_prefix("manual/") {
        let (manual_section, name) = rest.split_once('/')?;
        if !manual_section.is_empty() && !name.is_empty() && !name.contains('/') {
            return Some(DocumentAddress::Manual {
                name: name.to_owned(),
                manual_section: manual_section.to_owned(),
            });
        }
    }
    None
}

fn query_registered_document(
    name: &str,
    registered: &RegisteredSelection,
    host: &dyn QueryHost,
) -> Result<ResolvedContent, QueryError> {
    let path = &registered.path;
    let source_path = path.to_string_lossy().into_owned();
    let source = host
        .read_markdown(path)
        .map_err(|detail| QueryError::Markdown {
            path: source_path.clone(),
            detail,
        })?;
    let mut query = query_markdown_text(&source, Some(source_path))?;
    name.clone_into(&mut query.label);
    query.address = Some(registered.address.clone());
    Ok(query)
}

fn load_manual(
    requested_name: &str,
    candidates: &[String],
    section: Option<&str>,
    host: &dyn QueryHost,
) -> Result<LoadedManual, ManualLoadError> {
    let mut first_locate_error = None;
    let mut located = None;
    for candidate in candidates {
        let request = ManualRequest::new(candidate, section.map(ToOwned::to_owned));
        match host.locate_manual(&request) {
            Ok(page) => {
                located = Some(page);
                break;
            }
            Err(error) => {
                first_locate_error.get_or_insert(error);
            }
        }
    }
    let Some(page) = located else {
        let error =
            first_locate_error.unwrap_or_else(|| "no name candidates were available".to_owned());
        return Err(ManualLoadError::NotFound {
            name: requested_name.to_owned(),
            detail: error,
        });
    };

    let source_path = page.path.clone();
    let address = DocumentAddress::Manual {
        name: page.name.clone(),
        manual_section: page.section.clone(),
    };
    let document = host
        .parse_manual(&page)
        .map_err(|detail| ManualLoadError::Parse {
            name: requested_name.to_owned(),
            detail,
        })?;
    if document.sections.is_empty() {
        let diagnostics = document
            .diagnostics
            .iter()
            .map(|diagnostic| {
                let location = diagnostic.source.map_or_else(String::new, |source| {
                    format!(" at {}:{}", source.line, source.column)
                });
                format!("{:?}{location}: {}", diagnostic.level, diagnostic.message)
            })
            .collect::<Vec<_>>();
        return Err(ManualLoadError::Empty {
            name: requested_name.to_owned(),
            path: source_path,
            diagnostics,
        });
    }
    Ok(LoadedManual { document, address })
}
