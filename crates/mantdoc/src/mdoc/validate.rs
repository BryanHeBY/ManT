use super::{
    AuthorMode, BTreeMap, DocumentBuilder, NodeId, NodeKind, Recovery, SourceSpan,
    StructureOutcome, generated_system_name, is_mdoc_closing_delimiter, mark_no_print,
    mark_sentence_end, push_generated_text, push_generated_text_at, split_mdoc_inline_tokens,
};

pub(super) fn node_arguments(builder: &DocumentBuilder, node: NodeId) -> Vec<String> {
    builder
        .children(node)
        .into_iter()
        .flatten()
        .filter_map(|argument| builder.node_text(*argument))
        .map(str::to_owned)
        .collect()
}

/// Validate the standalone author macro's compact option surface.  The first
/// `-split`/`-nosplit` option selects the public layout mode; later options
/// are syntax-only and all remaining text is a single author phrase.
pub(super) fn validate_an(
    builder: &mut DocumentBuilder,
    node: NodeId,
    outcome: &mut StructureOutcome,
) {
    let Some(arguments) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return;
    };
    let mut option_count = 0;
    let mut author_mode = None;
    for argument in &arguments {
        let Some(option) = builder.node_text(*argument) else {
            break;
        };
        let mode = match option {
            "-split" => AuthorMode::Split,
            "-nosplit" => AuthorMode::NoSplit,
            _ => break,
        };
        if author_mode.is_some() {
            outcome.recoveries.push(Recovery::DuplicateArgument {
                macro_name: "An",
                argument: option.into(),
                location: builder.node_location(*argument),
            });
        } else {
            author_mode = Some(mode);
        }
        option_count += 1;
    }
    let retained = &arguments[option_count..];
    if option_count != 0 {
        let _ = builder.replace_children(node, retained);
    }
    let _ = builder.set_node_author_mode(node, author_mode);

    if author_mode.is_some() {
        if let Some(excess) = retained.first().copied() {
            outcome.recoveries.push(Recovery::InvalidArguments {
                message: format!(
                    "skipping excess arguments: An ... {}",
                    builder.node_text(excess).unwrap_or_default()
                )
                .into(),
                location: builder.node_location(excess),
            });
        }
        return;
    }

    let Some(last) = retained.last().copied() else {
        outcome.recoveries.push(Recovery::EmptyMacro {
            macro_name: "An",
            location: builder.node_location(node),
        });
        return;
    };
    let Some(text) = builder.node_text(last) else {
        return;
    };
    let Some((&delimiter, prefix)) = text.as_bytes().split_last() else {
        return;
    };
    if !matches!(
        delimiter,
        b',' | b'.' | b';' | b':' | b'!' | b'?' | b')' | b']'
    ) || prefix.last().is_none_or(u8::is_ascii_whitespace)
    {
        return;
    }
    let Some(location) = builder.node_location(last).and_then(|span| {
        span.end
            .checked_sub(1)
            .and_then(|start| SourceSpan::new(span.source, start, span.end).ok())
    }) else {
        return;
    };
    let display = if retained.len() == 1 {
        text.to_owned()
    } else {
        format!("... {text}")
    };
    outcome.recoveries.push(Recovery::TrailingDelimiterSpacing {
        macro_name: "An",
        display: display.into(),
        location: Some(location),
    });
}

/// Return the tag-style macros whose empty public elements are deleted by
/// legacy post-validation. `Cm` and `No` have additional context-sensitive
/// rules at their call sites and intentionally stay out of this table.
pub(super) fn empty_tag_macro_name(macro_name: Option<&str>) -> Option<&'static str> {
    match macro_name {
        Some("Dv") => Some("Dv"),
        Some("Em") => Some("Em"),
        Some("Er") => Some("Er"),
        Some("Ev") => Some("Ev"),
        Some("Ic") => Some("Ic"),
        Some("Li") => Some("Li"),
        Some("Ms") => Some("Ms"),
        Some("Sy") => Some("Sy"),
        Some("Va") => Some("Va"),
        _ => None,
    }
}

/// Return the ordinary `in_line()` tag-style macros whose leading delimiters
/// are published outside the element before an ordinary following word opens
/// a new element of the same kind. This excludes fixed-argument forms such as
/// `In` and `Xr`, plus `Fl`/`Fn`, which have their own argument rules.
pub(super) fn is_tag_style_delimiter_restart_macro(macro_name: Option<&str>) -> bool {
    matches!(
        macro_name,
        Some("Cd" | "Cm" | "Dv" | "Em" | "Er" | "Ev" | "Ic" | "Li" | "Ms" | "No" | "Sy" | "Va")
    )
}

/// Return the tag-style macros that use `post_delim_nb()` validation.
pub(super) fn tag_macro_name(macro_name: Option<&str>) -> Option<&'static str> {
    match macro_name {
        Some("Cm") => Some("Cm"),
        Some("Dv") => Some("Dv"),
        Some("Em") => Some("Em"),
        Some("Er") => Some("Er"),
        Some("Ev") => Some("Ev"),
        Some("Ic") => Some("Ic"),
        Some("Li") => Some("Li"),
        Some("Ms") => Some("Ms"),
        Some("Sy") => Some("Sy"),
        Some("Va") => Some("Va"),
        _ => None,
    }
}

/// Preserve the link macro's exceptional punctuation ownership. `Lk` keeps a
/// standalone closing delimiter inside its element, unlike ordinary
/// `in_line()` macros that release it to surrounding flow.
pub(super) fn mark_link_terminal_delimiter(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(last) = builder
        .children(node)
        .and_then(|children| children.last())
        .copied()
    else {
        return;
    };
    if !builder
        .node_text(last)
        .is_some_and(is_mdoc_closing_delimiter)
    {
        return;
    }
    if let Some(mut flags) = builder.node_flags(last) {
        flags.delimiter_close = true;
        let _ = builder.set_node_flags(last, flags);
    }
    mark_sentence_end(builder, last);
}

/// Apply the `post_tag()` delimiter validation that is otherwise hidden when
/// a tag-style macro is parsed as a callable macro inside another request.
pub(super) fn validate_tag(
    builder: &DocumentBuilder,
    node: NodeId,
    macro_name: &'static str,
    recoveries: &mut Vec<Recovery>,
) {
    let Some(children) = builder.children(node) else {
        return;
    };
    let Some(last) = children.last().copied() else {
        return;
    };
    let Some(text) = builder.node_text(last) else {
        return;
    };
    let Some((&delimiter, prefix)) = text.as_bytes().split_last() else {
        return;
    };
    if !matches!(
        delimiter,
        b',' | b'.' | b';' | b':' | b'!' | b'?' | b')' | b']'
    ) || prefix.last().is_none_or(u8::is_ascii_whitespace)
    {
        return;
    }
    let Some(location) = builder.node_location(last).and_then(|span| {
        // Parsed source words retain a logical start but may share a physical
        // control-line end after the inline splitter has separated them.
        // The attached ASCII delimiter is therefore relative to that logical
        // word start, never to the shared physical end.
        let base = builder.node_source_position(last)?;
        let offset = u32::try_from(text.len().checked_sub(1)?).ok()?;
        let column = base.column.checked_add(offset)?;
        Some(
            SourceSpan::new(span.source, span.start, span.end)
                .ok()?
                .with_logical_start(crate::SourcePosition {
                    line: base.line,
                    column,
                }),
        )
    }) else {
        return;
    };
    let display = if children.first().copied() == Some(last) {
        text.to_owned()
    } else {
        format!("... {text}")
    };
    recoveries.push(Recovery::TrailingDelimiterSpacing {
        macro_name,
        display: display.into(),
        location: Some(location),
    });
}

/// Apply mdoc's library-catalogue expansion.  A known library hides its
/// selector and prepends its generated description; an unknown library is
/// rendered as `library \(lq<selector>\(rq` while preserving later authored
/// arguments.  This is AST semantics, not a renderer substitution.
pub(super) fn validate_library(
    builder: &mut DocumentBuilder,
    node: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
    deferred_recoveries: &mut Vec<Recovery>,
    outer_delimiters: &mut Vec<NodeId>,
) -> bool {
    let mut children = builder
        .children(node)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    if children.len() > 1
        && children
            .last()
            .and_then(|child| builder.node_text(*child))
            .is_some_and(is_mdoc_closing_delimiter)
    {
        let delimiter = children.pop().expect("length was checked");
        let Some(mut flags) = builder.node_flags(delimiter) else {
            return false;
        };
        flags.delimiter_close = true;
        flags.sentence_end = builder
            .node_text(delimiter)
            .is_some_and(|text| matches!(text, "." | "!" | "?"));
        if !builder.set_node_flags(delimiter, flags) || !builder.replace_children(node, &children) {
            return false;
        }
        outer_delimiters.push(delimiter);
    }
    let Some(first) = children.first().copied() else {
        outcome.recoveries.push(Recovery::EmptyMacro {
            macro_name: "Lb",
            location: builder.node_location(node),
        });
        return false;
    };
    let Some(library) = builder.node_text(first).map(str::to_owned) else {
        return true;
    };

    validate_no_break_trailing_delimiter(builder, node, "Lb", deferred_recoveries);

    if let Some(description) = mdoc_library_description(&library) {
        if builder.node_count() >= max_nodes {
            if outcome.node_limit_location.is_none() {
                outcome.node_limit_location = builder.node_location(node);
            }
            return false;
        }
        // Catalogue rows are Rust-owned generated text rather than physical
        // roff input.  Their historical table intentionally stores doubled
        // escapes for source readability; expose the one-escape spelling the
        // normal document escape pass expects so `\\-` remains a hyphen and
        // not a visible reverse solidus in the engine projection.
        let description = description.replace(r"\\-", r"\-").replace(r"\\~", r"\~");
        let Some(description_node) = push_generated_text(builder, node, &description, false) else {
            if outcome.node_limit_location.is_none() {
                outcome.node_limit_location = builder.node_location(node);
            }
            return false;
        };
        let Some(mut flags) = builder.node_flags(first) else {
            return false;
        };
        flags.no_print = true;
        if !builder.set_node_flags(first, flags) {
            return false;
        }
        let mut reordered = Vec::with_capacity(children.len().saturating_add(1));
        reordered.push(description_node);
        reordered.extend(children);
        return builder.replace_children(node, &reordered);
    }

    if builder.node_count().saturating_add(3) > max_nodes {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = builder.node_location(node);
        }
        return false;
    }
    deferred_recoveries.push(Recovery::UnknownLibrary {
        library: library.into(),
        location: builder.node_location(first),
    });
    let Some(generic) = push_generated_text(builder, node, "library", false) else {
        return false;
    };
    let Some(opening) = push_generated_text(builder, node, r"\(lq", false) else {
        return false;
    };
    let Some(closing) = push_generated_text(builder, node, r"\(rq", false) else {
        return false;
    };
    let Some(mut flags) = builder.node_flags(opening) else {
        return false;
    };
    flags.delimiter_open = true;
    if !builder.set_node_flags(opening, flags) {
        return false;
    }
    let Some(mut flags) = builder.node_flags(closing) else {
        return false;
    };
    flags.delimiter_close = true;
    if !builder.set_node_flags(closing, flags) {
        return false;
    }

    let mut reordered = Vec::with_capacity(children.len().saturating_add(3));
    reordered.extend([generic, opening, first, closing]);
    reordered.extend(children.into_iter().skip(1));
    builder.replace_children(node, &reordered)
}

/// Match `post_delim_nb()` for the family of macros that keeps punctuation in
/// its own presentation flow.  The current callers are intentionally narrow;
/// retaining its complete historical false-positive filtering here prevents
/// `.Lb` validation from over-reporting compared with libmandoc.
pub(super) fn validate_no_break_trailing_delimiter(
    builder: &DocumentBuilder,
    node: NodeId,
    macro_name: &'static str,
    recoveries: &mut Vec<Recovery>,
) {
    let Some(children) = builder.children(node) else {
        return;
    };
    let Some(last) = children.last().copied() else {
        return;
    };
    let Some(text) = builder.node_text(last) else {
        return;
    };
    let bytes = text.as_bytes();
    let Some((&delimiter, prefix)) = bytes.split_last() else {
        return;
    };
    if prefix.is_empty()
        || !matches!(
            delimiter,
            b',' | b'.' | b';' | b':' | b'!' | b'?' | b')' | b']' | b'|'
        )
    {
        return;
    }
    let delimiter_index = bytes.len().saturating_sub(1);
    if delimiter_index >= 2
        && matches!(
            bytes.get(delimiter_index - 2..delimiter_index),
            Some(b"\\&" | b"\\e")
        )
    {
        return;
    }
    match delimiter {
        b')' if text.contains('(') => return,
        b'.' if bytes.len() >= 3 && bytes[bytes.len() - 3..] == *b"..." => return,
        // `post_delim_nb()` suppresses the false positive for C-style
        // variable declarations ending in a semicolon.
        b';' if macro_name == "Vt" => return,
        b'?' if prefix.last() == Some(&b'?') => return,
        b']' if text.contains('[') => return,
        b'|' if bytes.len() == 2 && prefix == b"|" => return,
        _ => {}
    }
    if bytes.len() == 2 && !prefix[0].is_ascii_alphanumeric() {
        return;
    }
    let Some(location) = text_offset_location(builder, last, delimiter_index) else {
        return;
    };
    let display = if generated_system_name(macro_name).is_some() {
        // The synthetic operating-system word is public AST structure but
        // not part of `post_delim_nb()`'s authored diagnostic phrase.
        text.to_owned()
    } else if children.len() == 1 {
        text.to_owned()
    } else {
        format!("... {text}")
    };
    recoveries.push(Recovery::TrailingDelimiterSpacing {
        macro_name,
        display: display.into(),
        location: Some(location),
    });
}

/// Match `post_pf()`'s source-line requirement: `.Pf` owns one literal prefix
/// argument, but another visible token must follow it on that same line.
/// Delimiters alone do not satisfy that requirement; a closing `Pc` does,
/// because the punctuation remains owned by its enclosing partial block.
pub(super) fn validate_prefix_following(
    builder: &DocumentBuilder,
    node: NodeId,
    following: &[NodeId],
    recoveries: &mut Vec<Recovery>,
) {
    let Some(position) = builder.node_source_position(node) else {
        return;
    };
    let same_line = following
        .iter()
        .copied()
        .take_while(|candidate| {
            builder
                .node_source_position(*candidate)
                .is_some_and(|candidate_position| candidate_position.line == position.line)
        })
        .collect::<Vec<_>>();
    if same_line.iter().any(|candidate| {
        builder.node_macro_name(*candidate) == Some("Pc")
            || builder
                .node_macro_name(*candidate)
                .is_some_and(|macro_name| macro_name != "Pc")
            || builder
                .node_text(*candidate)
                .is_some_and(|text| !is_mdoc_closing_delimiter(text))
    }) {
        return;
    }

    let prefix = node_arguments(builder, node).join(" ");
    let display = if prefix.is_empty() {
        same_line
            .iter()
            .find_map(|candidate| builder.node_text(*candidate))
            .map_or_else(|| "Pf at eol".to_owned(), |text| format!("Pf {text}"))
    } else {
        format!("Pf {prefix}")
    };
    recoveries.push(Recovery::PrefixWithoutFollowing {
        display: display.into_boxed_str(),
        location: builder.node_location(node),
    });
}

/// Resolve the stable mdoc library-name catalogue.  It mirrors mandoc 1.14.6
/// plus the wrapper's pinned `libbsd` addition, but is native data rather than
/// a runtime dependency on the former C vendor tree.
#[allow(clippy::too_many_lines)] // The pinned upstream library-name catalogue is intentionally data-local.
pub(super) fn mdoc_library_description(name: &str) -> Option<&'static str> {
    match name {
        "lib80211" => Some(r"802.11 Wireless Network Management Library (lib80211, \\-l80211)"),
        "libalias" => Some(r"Packet Aliasing Library (libalias, \\-lalias)"),
        "libarchive" => Some(r"Streaming Archive Library (libarchive, \\-larchive)"),
        "libarm" => Some(r"ARM Architecture Library (libarm, \\-larm)"),
        "libarm32" => Some(r"ARM32 Architecture Library (libarm32, \\-larm32)"),
        "libbe" => Some(r"Boot Environment Library (libbe, \\-lbe)"),
        "libbluetooth" => Some(r"Bluetooth Library (libbluetooth, \\-lbluetooth)"),
        "libbsd" => Some(r"Utility functions from BSD systems (libbsd, \\-lbsd)"),
        "libbsdxml" => Some(r"eXpat XML parser library (libbsdxml, \\-lbsdxml)"),
        "libbsm" => Some(r"Basic Security Module Library (libbsm, \\-lbsm)"),
        "libc" => Some(r"Standard C\\~Library (libc, \\-lc)"),
        "libc_r" => Some(r"Reentrant C\\~Library (libc_r, \\-lc_r)"),
        "libcalendar" => Some(r"Calendar Arithmetic Library (libcalendar, \\-lcalendar)"),
        "libcam" => Some(r"Common Access Method User Library (libcam, \\-lcam)"),
        "libcasper" => Some(r"Casper Library (libcasper, \\-lcasper)"),
        "libcdk" => Some(r"Curses Development Kit Library (libcdk, \\-lcdk)"),
        "libcipher" => Some(r"FreeSec Crypt Library (libcipher, \\-lcipher)"),
        "libcompat" => Some(r"Compatibility Library (libcompat, \\-lcompat)"),
        "libcrypt" => Some(r"Crypt Library (libcrypt, \\-lcrypt)"),
        "libcurses" => Some(r"Curses Library (libcurses, \\-lcurses)"),
        "libcuse" => Some(r"Userland Character Device Library (libcuse, \\-lcuse)"),
        "libdevattr" => Some(r"Device attribute and event library (libdevattr, \\-ldevattr)"),
        "libdevctl" => Some(r"Device Control Library (libdevctl, \\-ldevctl)"),
        "libdevinfo" => {
            Some(r"Device and Resource Information Utility Library (libdevinfo, \\-ldevinfo)")
        }
        "libdevstat" => Some(r"Device Statistics Library (libdevstat, \\-ldevstat)"),
        "libdisk" => Some(r"Interface to Slice and Partition Labels Library (libdisk, \\-ldisk)"),
        "libdl" => Some(r"Dynamic Linker Services Filter (libdl, \\-ldl)"),
        "libdm" => Some(r"Device Mapper Library (libdm, \\-ldm)"),
        "libdwarf" => Some(r"DWARF Access Library (libdwarf, \\-ldwarf)"),
        "libedit" => Some(r"Command Line Editor Library (libedit, \\-ledit)"),
        "libefi" => Some(r"EFI Runtime Services Library (libefi, \\-lefi)"),
        "libelf" => Some(r"ELF Access Library (libelf, \\-lelf)"),
        "libevent" => Some(r"Event Notification Library (libevent, \\-levent)"),
        "libexecinfo" => Some(r"Backtrace Information Library (libexecinfo, \\-lexecinfo)"),
        "libfetch" => Some(r"File Transfer Library (libfetch, \\-lfetch)"),
        "libfsid" => Some(r"Filesystem Identification Library (libfsid, \\-lfsid)"),
        "libftpio" => Some(r"FTP Connection Management Library (libftpio, \\-lftpio)"),
        "libform" => Some(r"Curses Form Library (libform, \\-lform)"),
        "libgeom" => Some(r"Userland API Library for Kernel GEOM subsystem (libgeom, \\-lgeom)"),
        "libgpio" => Some(r"General-Purpose Input Output (GPIO) library (libgpio, \\-lgpio)"),
        "libhammer" => Some(r"HAMMER Filesystem Userland Library (libhammer, \\-lhammer)"),
        "libi386" => Some(r"i386 Architecture Library (libi386, \\-li386)"),
        "libintl" => Some(r"Internationalized Message Handling Library (libintl, \\-lintl)"),
        "libipsec" => Some(r"IPsec Policy Control Library (libipsec, \\-lipsec)"),
        "libiscsi" => Some(r"iSCSI protocol library (libiscsi, \\-liscsi)"),
        "libisns" => Some(r"iSNS protocol library (libisns, \\-lisns)"),
        "libjail" => Some(r"Jail Library (libjail, \\-ljail)"),
        "libkcore" => Some(r"Kernel Memory Core Access Library (libkcore, \\-lkcore)"),
        "libkiconv" => Some(r"Kernel-side iconv Library (libkiconv, \\-lkiconv)"),
        "libkse" => Some(r"N:M Threading Library (libkse, \\-lkse)"),
        "libkvm" => Some(r"Kernel Data Access Library (libkvm, \\-lkvm)"),
        "libm" => Some(r"Math Library (libm, \\-lm)"),
        "libm68k" => Some(r"m68k Architecture Library (libm68k, \\-lm68k)"),
        "libmagic" => Some(r"Magic Number Recognition Library (libmagic, \\-lmagic)"),
        "libmandoc" => Some(r"Mandoc Macro Compiler Library (libmandoc, \\-lmandoc)"),
        "libmd" => Some(r"Message Digest (MD4, MD5, etc.) Support Library (libmd, \\-lmd)"),
        "libmemstat" => {
            Some(r"Kernel Memory Allocator Statistics Library (libmemstat, \\-lmemstat)")
        }
        "libmenu" => Some(r"Curses Menu Library (libmenu, \\-lmenu)"),
        "libmj" => Some(r"Minimalist JSON library (libmj, \\-lmj)"),
        "libnetgraph" => Some(r"Netgraph User Library (libnetgraph, \\-lnetgraph)"),
        "libnetpgp" => {
            Some(r"Netpgp Signing, Verification, Encryption and Decryption (libnetpgp, \\-lnetpgp)")
        }
        "libnetpgpverify" => Some(r"Netpgp Verification (libnetpgpverify, \\-lnetpgpverify)"),
        "libnpf" => Some(r"NPF Packet Filter Library (libnpf, \\-lnpf)"),
        "libnv" => Some(r"Name/value pairs library (libnv, \\-lnv)"),
        "libossaudio" => Some(r"OSS Audio Emulation Library (libossaudio, \\-lossaudio)"),
        "libpam" => Some(r"Pluggable Authentication Module Library (libpam, \\-lpam)"),
        "libpanel" => Some(r"Z-order for curses windows (libpanel, \\-lpanel)"),
        "libpcap" => Some(r"Packet capture Library (libpcap, \\-lpcap)"),
        "libpci" => Some(r"PCI Bus Access Library (libpci, \\-lpci)"),
        "libpmc" => Some(r"Performance Counters Library (libpmc, \\-lpmc)"),
        "libppath" => Some(r"Property-List Paths Library (libppath, \\-lppath)"),
        "libposix" => Some(r"POSIX Compatibility Library (libposix, \\-lposix)"),
        "libposix1e" => Some(r"POSIX.1e Security API Library (libposix1e, \\-lposix1e)"),
        "libproc" => Some(r"Processor Monitoring and Analysis Library (libproc, \\-lproc)"),
        "libprocstat" => {
            Some(r"Process and Files Information Retrieval (libprocstat, \\-lprocstat)")
        }
        "libprop" => Some(r"Property Container Object Library (libprop, \\-lprop)"),
        "libpthread" => Some(r"POSIX Threads Library (libpthread, \\-lpthread)"),
        "libpthread_dbg" => Some(r"POSIX Threads Library (libpthread_dbg, \\-lpthread_dbg)"),
        "libpuffs" => Some(r"puffs Convenience Library (libpuffs, \\-lpuffs)"),
        "libquota" => Some(r"Disk Quota Access Library (libquota, \\-lquota)"),
        "libradius" => Some(r"RADIUS Client Library (libradius, \\-lradius)"),
        "librefuse" => {
            Some(r"File System in Userspace Convenience Library (librefuse, \\-lrefuse)")
        }
        "libresolv" => Some(r"DNS Resolver Library (libresolv, \\-lresolv)"),
        "librpcsec_gss" => {
            Some(r"RPC GSS-API Authentication Library (librpcsec_gss, \\-lrpcsec_gss)")
        }
        "librpcsvc" => Some(r"RPC Service Library (librpcsvc, \\-lrpcsvc)"),
        "librt" => Some(r"POSIX Real\\-time Library (librt, \\-lrt)"),
        "librtld_db" => {
            Some(r"Debugging interface to the runtime linker Library (librtld_db, \\-lrtld_db)")
        }
        "librumpclient" => Some(
            r"Clientside Stubs for rump Kernel Remote Protocols (librumpclient, \\-lrumpclient)",
        ),
        "libsaslc" => {
            Some(r"Simple Authentication and Security Layer client library (libsaslc, \\-lsaslc)")
        }
        "libsbuf" => Some(r"Safe String Composition Library (libsbuf, \\-lsbuf)"),
        "libsdp" => Some(r"Bluetooth Service Discovery Protocol User Library (libsdp, \\-lsdp)"),
        "libssp" => Some(r"Buffer Overflow Protection Library (libssp, \\-lssp)"),
        "libstand" => Some(r"Standalone Applications Library (libstand, \\-lstand)"),
        "libstdthreads" => Some(r"C11 Threads Library (libstdthreads, \\-lstdthreads)"),
        "libSystem" => Some(r"System Library (libSystem, \\-lSystem)"),
        "libsysdecode" => Some(r"System Argument Decoding Library (libsysdecode, \\-lsysdecode)"),
        "libtacplus" => Some(r"TACACS+ Client Library (libtacplus, \\-ltacplus)"),
        "libtcplay" => Some(r"TrueCrypt-compatible API library (libtcplay, \\-ltcplay)"),
        "libtermcap" => Some(r"Termcap Access Library (libtermcap, \\-ltermcap)"),
        "libterminfo" => Some(r"Terminfo Access Library (libterminfo, \\-lterminfo)"),
        "libthr" => Some(r"1:1 Threading Library (libthr, \\-lthr)"),
        "libufs" => Some(r"UFS File System Access Library (libufs, \\-lufs)"),
        "libugidfw" => Some(r"Userland Firewall Library (libugidfw, \\-lugidfw)"),
        "libulog" => Some(r"User Login Record Library (libulog, \\-lulog)"),
        "libusbhid" => Some(r"USB Human Interface Devices Library (libusbhid, \\-lusbhid)"),
        "libutil" => Some(r"System Utilities Library (libutil, \\-lutil)"),
        "libvgl" => Some(r"Video Graphics Library (libvgl, \\-lvgl)"),
        "libx86_64" => Some(r"x86_64 Architecture Library (libx86_64, \\-lx86_64)"),
        "libxo" => Some(r"Text, XML, JSON, and HTML Output Emission Library (libxo, \\-lxo)"),
        "libz" => Some(r"Compression Library (libz, \\-lz)"),
        _ => None,
    }
}

/// Mirror `post_delim()` for mdoc constructs whose final phrase must leave
/// punctuation in outer flow.  `Nd` owns one joined Body phrase, so the
/// diagnostic prints that complete phrase rather than the abbreviated
/// `... tail` form used by macros with separately owned arguments.  A closing
/// parenthesis is explicitly accepted by upstream for description text.
pub(super) fn validate_attached_trailing_delimiter(
    builder: &DocumentBuilder,
    node: NodeId,
    macro_name: &'static str,
    recoveries: &mut Vec<Recovery>,
) {
    let Some(children) = builder.children(node) else {
        return;
    };
    let Some(last) = children.last().copied() else {
        return;
    };
    let Some(text) = builder.node_text(last) else {
        return;
    };
    let Some((delimiter_index, delimiter)) = text.char_indices().last() else {
        return;
    };
    if delimiter == ')' || !is_mdoc_closing_delimiter(&text[delimiter_index..]) {
        return;
    }
    let Some(location) = builder.node_location(last).and_then(|span| {
        span.end
            .checked_sub(u32::try_from(delimiter.len_utf8()).ok()?)
            .and_then(|start| SourceSpan::new(span.source, start, span.end).ok())
    }) else {
        return;
    };
    let display = children
        .iter()
        .filter_map(|child| builder.node_text(*child))
        .collect::<Vec<_>>()
        .join(" ");
    if display.is_empty() {
        return;
    }
    recoveries.push(Recovery::TrailingDelimiter {
        macro_name,
        display: display.into(),
        location: Some(location),
    });
}

/// Complete the delayed `post_nd()` delimiter validation for all descriptions
/// ended by the current structural boundary.  An `.Nd` owns both its control
/// line and following physical prose, so checking it at declaration time
/// misses punctuation attached to the final following text line.
pub(super) fn flush_pending_nd_delimiters(
    builder: &DocumentBuilder,
    bodies: &mut Vec<NodeId>,
    recoveries: &mut Vec<Recovery>,
) {
    for body in bodies.drain(..) {
        // `post_nd()` treats an empty Body as a recoverable missing
        // description, even though the Block/Head/Body shape remains part of
        // the public tree.  Delay this until the next boundary because
        // following physical prose belongs to the same Body.
        if builder.children(body).is_some_and(<[NodeId]>::is_empty) {
            recoveries.push(Recovery::MissingDescription {
                location: builder.node_location(body),
            });
            continue;
        }
        // A following paragraph stays in an `Nd` Body.  Once it contains a
        // later direct text phrase, `post_delim()` prints only that final
        // phrase with the legacy ellipsis marker instead of the control-line
        // argument preceding it.
        let trailing_text = builder.children(body).and_then(|children| {
            let first_text = children
                .iter()
                .copied()
                .find(|child| builder.node_kind(*child) == Some(NodeKind::Text))?;
            children
                .iter()
                .rev()
                .copied()
                .find(|child| builder.node_kind(*child) == Some(NodeKind::Text))
                .filter(|last_text| *last_text != first_text)
        });
        if let Some(text) = trailing_text {
            validate_nd_following_text_delimiter(builder, text, recoveries);
        } else {
            validate_attached_trailing_delimiter(builder, body, "Nd", recoveries);
        }
    }
}

/// Complete `post_sh_name()` after a `NAME` section has received every direct
/// child.  libmandoc deliberately does not descend through a partial block:
/// an `.Nm` or `.Nd` nested in `.Oo`, for example, remains invalid NAME
/// content rather than satisfying this section-level contract.
pub(super) fn flush_pending_name_section(
    builder: &DocumentBuilder,
    section_body: &mut Option<NodeId>,
    recoveries: &mut Vec<Recovery>,
) {
    let Some(body) = section_body.take() else {
        return;
    };
    let Some(children) = builder.children(body) else {
        return;
    };
    let mut has_name = false;
    let mut has_description = false;
    let mut index = 0;
    while index < children.len() {
        let child = children[index];
        match builder.node_macro_name(child) {
            Some("Nm") => {
                if has_name {
                    let name = node_arguments(builder, child).join(" ");
                    recoveries.push(Recovery::NameSectionMissingComma {
                        name: name.into_boxed_str(),
                        location: builder.node_location(child),
                    });
                }
                has_name = true;
            }
            Some("Nd") => {
                has_description = true;
                if index + 1 < children.len() {
                    recoveries.push(Recovery::DescriptionNotAtEndOfName {
                        location: builder.node_location(child),
                    });
                }
                break;
            }
            _ if builder.node_kind(child) == Some(NodeKind::Text)
                && builder.node_text(child) == Some(",")
                && children
                    .get(index + 1)
                    .is_some_and(|next| builder.node_macro_name(*next) == Some("Nm")) =>
            {
                // `post_sh_name()` accepts the separating comma itself and
                // then proceeds to the name macro.
                index += 1;
                has_name = true;
            }
            _ => {
                let content = builder
                    .node_macro_name(child)
                    .map_or_else(|| "text".to_owned(), str::to_owned);
                recoveries.push(Recovery::BadNameSectionContent {
                    content: content.into_boxed_str(),
                    location: builder.node_location(child),
                });
            }
        }
        index += 1;
    }
    let location = builder.node_location(body);
    if !has_name {
        recoveries.push(Recovery::NameSectionMissingName {
            location: location.clone(),
        });
    }
    if !has_description {
        recoveries.push(Recovery::NameSectionMissingDescription { location });
    }
}

/// Complete `post_sh_authors()` after an `AUTHORS` section has received all
/// of its descendant blocks.  A nested author entry is sufficient, but an
/// option-only or empty `.An` is not.
pub(super) fn flush_pending_authors_section(
    builder: &DocumentBuilder,
    authors_body: &mut Option<NodeId>,
    recoveries: &mut Vec<Recovery>,
) {
    let Some(body) = authors_body.take() else {
        return;
    };
    if contains_populated_author(builder, body) {
        return;
    }
    recoveries.push(Recovery::AuthorsSectionWithoutAuthor {
        location: builder.node_location(body),
    });
}

/// Iteratively mirror libmandoc's recursive `child_an()` predicate without
/// exposing parser input depth to the host call stack.
pub(super) fn contains_populated_author(builder: &DocumentBuilder, root: NodeId) -> bool {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if builder.node_macro_name(node) == Some("An")
            && builder
                .children(node)
                .is_some_and(|children| !children.is_empty())
        {
            return true;
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().copied());
        }
    }
    false
}

/// Mirror `post_fname()` for `Fn` Elements and validated `Fo` Heads.  A name
/// wrapped as one complete parenthesized phrase is accepted; every other
/// parenthesis is a source-precise upstream warning.
pub(super) fn validate_function_name(
    builder: &DocumentBuilder,
    node: NodeId,
    recoveries: &mut Vec<Recovery>,
) {
    let Some(name_node) = builder
        .children(node)
        .and_then(|children| children.first())
        .copied()
    else {
        return;
    };
    let Some(name) = builder.node_text(name_node) else {
        return;
    };
    let offset = if name.starts_with('(') {
        if name.ends_with(')') {
            return;
        }
        0
    } else {
        let Some(offset) = name.bytes().position(|byte| matches!(byte, b'(' | b')')) else {
            return;
        };
        offset
    };
    recoveries.push(Recovery::FunctionNameParenthesis {
        name: name.into(),
        location: text_offset_location(builder, name_node, offset),
    });
}

/// Mirror `post_fa()` for standalone arguments and the arguments carried by a
/// function declaration.  A comma after a callback or array opener is part of
/// that type expression; only the first earlier comma in each source phrase
/// is diagnosed.
pub(super) fn validate_function_argument_commas(
    builder: &DocumentBuilder,
    node: NodeId,
    recoveries: &mut Vec<Recovery>,
) {
    let Some(children) = builder.children(node) else {
        return;
    };
    for child in children {
        let Some(argument) = builder.node_text(*child) else {
            continue;
        };
        let Some(offset) = argument
            .bytes()
            .position(|byte| matches!(byte, b',' | b'(' | b'{'))
        else {
            continue;
        };
        if argument.as_bytes().get(offset) != Some(&b',') {
            continue;
        }
        recoveries.push(Recovery::FunctionArgumentComma {
            argument: argument.into(),
            location: text_offset_location(builder, *child, offset),
        });
    }
}

/// Return a one-byte logical location inside a text node.  Scanner words may
/// share one physical control-line end, so callers must derive positions from
/// their retained logical start rather than from `SourceSpan::end`.
pub(super) fn text_offset_location(
    builder: &DocumentBuilder,
    node: NodeId,
    offset: usize,
) -> Option<SourceSpan> {
    let span = builder.node_location(node)?;
    let base = builder.node_source_position(node)?;
    let offset = u32::try_from(offset).ok()?;
    let column = base.column.checked_add(offset)?;
    SourceSpan::new(span.source, span.start, span.end)
        .ok()
        .map(|span| {
            span.with_logical_start(crate::SourcePosition {
                line: base.line,
                column,
            })
        })
}

/// Validate the final physical prose phrase owned by a preceding `.Nd`.
pub(super) fn validate_nd_following_text_delimiter(
    builder: &DocumentBuilder,
    node: NodeId,
    recoveries: &mut Vec<Recovery>,
) {
    let Some(text) = builder.node_text(node) else {
        return;
    };
    let Some((delimiter_index, delimiter)) = text.char_indices().last() else {
        return;
    };
    if delimiter == ')' || !is_mdoc_closing_delimiter(&text[delimiter_index..]) {
        return;
    }
    let Some(location) = builder.node_location(node).and_then(|span| {
        span.end
            .checked_sub(u32::try_from(delimiter.len_utf8()).ok()?)
            .and_then(|start| SourceSpan::new(span.source, start, span.end).ok())
    }) else {
        return;
    };
    recoveries.push(Recovery::TrailingDelimiter {
        macro_name: "Nd",
        display: format!("... {text}").into(),
        location: Some(location),
    });
}

/// Apply the standard AT&T UNIX spelling expansion and resume mdoc's inline
/// grammar after the selector.  `mandoc` retains the authored selector for a
/// known version (but hides it from rendering), while an unknown selector is
/// displayed after the generic generated prefix and reported as a warning.
pub(super) fn validate_at(
    builder: &mut DocumentBuilder,
    node: NodeId,
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Vec<NodeId> {
    let Some(arguments) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return Vec::new();
    };
    let Some((&first, tail)) = arguments.split_first() else {
        // The no-argument `At` spelling has a public generated default,
        // rather than being an empty formatting request.  Insert it during
        // validation so an earlier `.de At` cannot replace the package result.
        if builder.node_count() < max_nodes {
            let _ = push_generated_text(builder, node, "AT&T UNIX", false);
        }
        return Vec::new();
    };
    let Some(argument) = builder.node_text(first).map(str::to_owned) else {
        return Vec::new();
    };

    let expanded = at_version(&argument);
    let generated = expanded.unwrap_or("AT&T UNIX");
    if expanded.is_none() {
        outcome.recoveries.push(Recovery::UnknownAtVersion {
            argument: argument.into(),
            location: builder.node_location(first),
        });
    }
    if builder.node_count() >= max_nodes {
        return Vec::new();
    }
    let Some(prefix) = push_generated_text_at(
        builder,
        node,
        generated,
        false,
        expanded.and(builder.node_location(first)),
    ) else {
        return Vec::new();
    };
    if expanded.is_some() {
        mark_no_print(builder, first);
    }
    let _ = builder.replace_children(node, &[prefix, first]);
    split_mdoc_inline_tokens(builder, node, tail, spacing_enabled, max_nodes, outcome)
}

/// Standard `.St` selectors and their public expansion text.
///
/// The table is part of mdoc's semantic contract, not a renderer alias: the
/// selector stays in the tree as a hidden authored child and this description
/// is a generated sibling. Keep the spellings pinned to the stable mandoc
/// baseline used by the compatibility corpus.
pub(super) fn standard_description(selector: &str) -> Option<&'static str> {
    Some(match selector {
        "-p1003.1-88" => "IEEE Std 1003.1-1988 (\\(lqPOSIX.1\\(rq)",
        "-p1003.1-90" => "IEEE Std 1003.1-1990 (\\(lqPOSIX.1\\(rq)",
        "-p1003.1-96" | "-iso9945-1-96" => "ISO/IEC 9945-1:1996 (\\(lqPOSIX.1\\(rq)",
        "-p1003.1-2001" => "IEEE Std 1003.1-2001 (\\(lqPOSIX.1\\(rq)",
        "-p1003.1-2004" => "IEEE Std 1003.1-2004 (\\(lqPOSIX.1\\(rq)",
        "-p1003.1-2008" => "IEEE Std 1003.1-2008 (\\(lqPOSIX.1\\(rq)",
        "-p1003.1-2024" => "IEEE Std 1003.1-2024 (\\(lqPOSIX.1\\(rq)",
        "-p1003.1" => "IEEE Std 1003.1 (\\(lqPOSIX.1\\(rq)",
        "-p1003.1b" => "IEEE Std 1003.1b (\\(lqPOSIX.1b\\(rq)",
        "-p1003.1b-93" => "IEEE Std 1003.1b-1993 (\\(lqPOSIX.1b\\(rq)",
        "-p1003.1c-95" => "IEEE Std 1003.1c-1995 (\\(lqPOSIX.1c\\(rq)",
        "-p1003.1g-2000" => "IEEE Std 1003.1g-2000 (\\(lqPOSIX.1g\\(rq)",
        "-p1003.1i-95" => "IEEE Std 1003.1i-1995 (\\(lqPOSIX.1i\\(rq)",
        "-p1003.2" => "IEEE Std 1003.2 (\\(lqPOSIX.2\\(rq)",
        "-p1003.2-92" => "IEEE Std 1003.2-1992 (\\(lqPOSIX.2\\(rq)",
        "-p1003.2a-92" => "IEEE Std 1003.2a-1992 (\\(lqPOSIX.2a\\(rq)",
        "-isoC" | "-isoC-90" => "ISO/IEC 9899:1990 (\\(lqISO\\~C90\\(rq)",
        "-isoC-amd1" => "ISO/IEC 9899/AMD1:1995 (\\(lqISO\\~C90, Amendment 1\\(rq)",
        "-isoC-tcor1" => "ISO/IEC 9899/TCOR1:1994 (\\(lqISO\\~C90, Technical Corrigendum 1\\(rq)",
        "-isoC-tcor2" => "ISO/IEC 9899/TCOR2:1995 (\\(lqISO\\~C90, Technical Corrigendum 2\\(rq)",
        "-isoC-99" => "ISO/IEC 9899:1999 (\\(lqISO\\~C99\\(rq)",
        "-isoC-2011" => "ISO/IEC 9899:2011 (\\(lqISO\\~C11\\(rq)",
        "-isoC-2023" => "ISO/IEC 9899:2024 (\\(lqISO\\~C23\\(rq)",
        "-iso9945-1-90" => "ISO/IEC 9945-1:1990 (\\(lqPOSIX.1\\(rq)",
        "-iso9945-2-93" => "ISO/IEC 9945-2:1993 (\\(lqPOSIX.2\\(rq)",
        "-ansiC" | "-ansiC-89" => "ANSI X3.159-1989 (\\(lqANSI\\~C89\\(rq)",
        "-ieee754" => "IEEE Std 754-1985",
        "-iso8802-3" => "ISO 8802-3: 1989",
        "-iso8601" => "ISO 8601",
        "-ieee1275-94" => "IEEE Std 1275-1994 (\\(lqOpen Firmware\\(rq)",
        "-xpg3" => "X/Open Portability Guide Issue\\~3 (\\(lqXPG3\\(rq)",
        "-xpg4" => "X/Open Portability Guide Issue\\~4 (\\(lqXPG4\\(rq)",
        "-xpg4.2" => "X/Open Portability Guide Issue\\~4, Version\\~2 (\\(lqXPG4.2\\(rq)",
        "-xbd5" => "X/Open Base Definitions Issue\\~5 (\\(lqXBD5\\(rq)",
        "-xcu5" => "X/Open Commands and Utilities Issue\\~5 (\\(lqXCU5\\(rq)",
        "-xsh4.2" => {
            "X/Open System Interfaces and Headers Issue\\~4, Version\\~2 (\\(lqXSH4.2\\(rq)"
        }
        "-xsh5" => "X/Open System Interfaces and Headers Issue\\~5 (\\(lqXSH5\\(rq)",
        "-xns5" => "X/Open Networking Services Issue\\~5 (\\(lqXNS5\\(rq)",
        "-xns5.2" => "X/Open Networking Services Issue\\~5.2 (\\(lqXNS5.2\\(rq)",
        "-xcurses4.2" => "X/Open Curses Issue\\~4, Version\\~2 (\\(lqXCURSES4.2\\(rq)",
        "-susv1" => "Version\\~1 of the Single UNIX Specification (\\(lqSUSv1\\(rq)",
        "-susv2" => "Version\\~2 of the Single UNIX Specification (\\(lqSUSv2\\(rq)",
        "-susv3" => "Version\\~3 of the Single UNIX Specification (\\(lqSUSv3\\(rq)",
        "-susv4" => "Version\\~4 of the Single UNIX Specification (\\(lqSUSv4\\(rq)",
        "-svid4" => "System\\~V Interface Definition, Fourth Edition (\\(lqSVID4\\(rq)",
        _ => return None,
    })
}

/// Standard `.At` selectors and their public expansion text.
pub(super) fn at_version(argument: &str) -> Option<&'static str> {
    Some(match argument {
        "v1" => "Version\\~1 AT&T UNIX",
        "v2" => "Version\\~2 AT&T UNIX",
        "v3" => "Version\\~3 AT&T UNIX",
        "v4" => "Version\\~4 AT&T UNIX",
        "v5" => "Version\\~5 AT&T UNIX",
        "v6" => "Version\\~6 AT&T UNIX",
        "v7" => "Version\\~7 AT&T UNIX",
        "32v" => "Version\\~7 AT&T UNIX/32V",
        "III" => "AT&T System\\~III UNIX",
        "V" => "AT&T System\\~V UNIX",
        "V.1" => "AT&T System\\~V Release\\~1 UNIX",
        "V.2" => "AT&T System\\~V Release\\~2 UNIX",
        "V.3" => "AT&T System\\~V Release\\~3 UNIX",
        "V.4" => "AT&T System\\~V Release\\~4 UNIX",
        _ => return None,
    })
}

/// mdoc 的 `.Fd` 及文本型 `.Fl`/`.Sy`/`.Ar`/`.Em` 按展开后的前序参数宽度定位后续参数。
/// 扫描器保留原始跨度，故只在已证实的宏语义中重定位公开 AST。
pub(super) fn rebase_expanded_argument_locations(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(arguments) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return;
    };
    let mut prior_delta = 0_i32;
    for argument in arguments {
        if prior_delta != 0 && builder.node_location(argument).is_some() {
            rebase_expanded_subtree_locations(builder, argument, prior_delta);
        }
        prior_delta =
            prior_delta.saturating_add(builder.node_argument_expansion_width_delta(argument));
    }
}

/// An expanded control-line argument can contain a nested inline macro.  The
/// nested node and all of its public descendants inherit the preceding
/// expansion width even when only its direct parent owns the escape spelling.
pub(super) fn rebase_expanded_subtree_locations(
    builder: &mut DocumentBuilder,
    root: NodeId,
    delta: i32,
) {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if let Some(mut location) = builder.node_location(node) {
            let start = location.start.saturating_add_signed(delta);
            // Public spans remain within authored source bytes: only rebase
            // the logical start when it stays before the lexical end.
            if start <= location.end {
                location.start = start;
                let _ = builder.set_node_location(node, Some(location));
            }
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().copied());
        }
    }
}

/// Apply roff expansion width to the completed `Op` subtree. Option bodies
/// can acquire nested callable macros only during mdoc restructuring, so the
/// scanner-stage argument rebase cannot make their descendants inherit a
/// preceding string expansion.
pub(super) fn rebase_option_expansion_locations(builder: &mut DocumentBuilder, root: NodeId) {
    let mut option_roots = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node != root
            && builder.node_kind(node) == Some(NodeKind::Block)
            && builder.node_macro_name(node) == Some("Op")
        {
            option_roots.push(node);
            continue;
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().copied());
        }
    }

    for option in option_roots {
        rebase_completed_option_locations(builder, option);
    }
}

pub(super) fn rebase_completed_option_locations(builder: &mut DocumentBuilder, option: NodeId) {
    let mut entries = Vec::new();
    let mut pending = vec![option];
    while let Some(node) = pending.pop() {
        if let Some(location) = builder.node_location(node)
            && let Ok(physical) = SourceSpan::new(location.source, location.start, location.end)
            && let Some(position) = builder.source_position(&physical)
        {
            entries.push((node, physical, position));
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().copied());
        }
    }
    entries.sort_by_key(|(_, location, _)| (location.source, location.start, location.end));

    let mut deltas = BTreeMap::<(crate::SourceId, u32), i32>::new();
    for (node, location, position) in entries {
        let delta = *deltas.entry((location.source, position.line)).or_default();
        if delta != 0 {
            let column = position.column.saturating_add_signed(delta);
            let _ = builder.set_node_logical_start(
                node,
                crate::SourcePosition {
                    line: position.line,
                    column,
                },
            );
        }
        if builder.node_kind(node) == Some(NodeKind::Text) {
            let entry = deltas.entry((location.source, position.line)).or_default();
            *entry = entry.saturating_add(builder.node_argument_expansion_width_delta(node));
        }
    }
}
