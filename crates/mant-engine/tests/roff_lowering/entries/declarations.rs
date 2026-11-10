//! Source-proven declaration names, forms, and semantic owner relationships.

use super::super::*;

#[test]
fn composite_environment_options_do_not_promote_shell_labels() {
    let path = temporary_source(
        "environment-options",
        ".TH DEMO 1\n\
         .SH \"ENVIRONMENT OPTIONS\"\n\
         .TP\n\
         Unix Bourne shell:\n\
         UNZIP=-qq; export UNZIP\n\
         .TP\n\
         \\-q\n\
         Be quiet.\n",
    );

    let document = parse_manual_source(&path).expect("lower environment option fixture");
    fs::remove_file(path).expect("remove fixture");
    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("definitions");
    };
    assert_eq!(
        items[0].entry.as_ref().expect("term").kind,
        mant_ir::EntryKind::Term
    );
    assert_eq!(items[1].entry.as_ref().expect("option").names, ["-q"]);
    assert!(document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("manual.semantic-entry.unclassified-definition")
            && diagnostic.message.contains("Unix Bourne shell:")
    }));
}

#[test]
fn tq_terms_share_one_semantic_option_identity() {
    let path = temporary_source(
        "tq-aliases",
        ".TH TQ-ALIASES 7\n\
         .SH OPTIONS\n\
         .TP\n\
         .B \\-\\-alpha\n\
         .TQ\n\
         .B \\-a\n\
         .TQ\n\
         .B \\-\\-ALPHA\n\
         Enable alpha mode.\n",
    );

    let document = parse_manual_source(&path).expect("lower TQ aliases");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0]
            .terms
            .iter()
            .map(|term| inline_text(term))
            .collect::<Vec<_>>(),
        ["--alpha", "-a", "--ALPHA"]
    );
    assert_eq!(
        items[0].entry.as_ref().expect("option identity").names,
        ["--alpha", "-a", "--ALPHA"]
    );
}

#[test]
fn exports_the_accepted_bsd_lifecycle_spelling() {
    // Exact source rerun with pristine CVS before changing this assertion.
    // mdoc_validate.c::post_bx() adds BSD after the authored operand; native
    // reading and Markdown both retain that accepted spelling. NODE_EOS
    // arms two ordinary cells at the next actual term_word() (term.c:573-589).
    let source = b".Dd August 19, 2026\n.Dt BSD-LIFECYCLE 7\n.Os\n.Sh DESCRIPTION\n.Bx\n.Bx -alpha\n.Bx -beta\n.Bx -devel .\n.Bx 4.3 .\n.Bx 4.3 Net/2 .\n.Bx 386 0.1 .\n";
    let document = parse_manual_bytes(std::path::Path::new("bsd-lifecycle.7"), source)
        .expect("lower mdoc BSD lifecycle forms");

    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one BSD lifecycle paragraph");
    };
    assert_eq!(
        inline_text(children),
        "BSD -alphaBSD -betaBSD -develBSD.  4.3BSD.  4.3BSD-Net/2.  386BSD-0.1."
    );
    let query = mant_ir::ResolvedContent {
        address: None,
        label: "bsd-lifecycle.7".into(),
        document: Some(document),
        tldr: None,
    };
    let portable = mant_codec::encode::render_markdown(&query);
    for operand in ["-alphaBSD", "-betaBSD", "-develBSD"] {
        assert!(portable.contains(operand), "{portable}");
    }
    assert!(!portable.contains("currently"), "{portable}");
    let readback = mant_loader::load_markdown_text(&portable, None).unwrap();
    let [Block::Paragraph { children, .. }] = readback.document.as_ref().unwrap().sections[0]
        .blocks
        .as_slice()
    else {
        panic!("accepted BSD spelling must keep its paragraph owner");
    };
    assert_eq!(
        inline_text(children),
        "BSD -alphaBSD -betaBSD -develBSD.  4.3BSD.  4.3BSD-Net/2.  386BSD-0.1."
    );
}

#[test]
fn preserves_mdoc_command_names_in_each_synopsis_form() {
    let document = parse_manual_bytes(
        std::path::Path::new("fido2-cred.1"),
        b".Dd August 19, 2026\n.Dt FIDO2-CRED 1\n.Os\n.Sh NAME\n.Nm fido2-cred\n.Nd make a credential\n.Sh SYNOPSIS\n.Nm\n.Fl M\n.Op Fl i Ar input_file\n.Nm fido2-cred\n.Fl V\n.Nm helper\n.Op Fl q\n",
    )
    .expect("lower mdoc synopsis names");
    let synopsis = &document.sections[1];
    let rendered = synopsis
        .blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph { children, .. } => inline_text(children),
            block => panic!("expected synopsis paragraph, got {block:?}"),
        })
        .collect::<Vec<_>>();

    assert_eq!(
        rendered,
        [
            "fido2-cred -M [-i input_file]",
            "fido2-cred -V",
            "helper [-q]",
        ]
    );
}

#[test]
fn man_optional_arguments_keep_brackets_and_argument_styles() {
    for (requests, expected) in [
        (".OP --verbose", "[--verbose]"),
        (".OP --file FILE", "[--file FILE]"),
        (
            ".OP --file FILE\n.OP --verbose",
            "[--file FILE] [--verbose]",
        ),
    ] {
        for wrapper in [false, true] {
            let source = if wrapper {
                format!(".TH PROBE 1\n.SH SYNOPSIS\n.SY probe\n{requests}\n.YS\n")
            } else {
                format!(".TH PROBE 1\n.SH DESCRIPTION\n{requests}\n")
            };
            let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let text = mant_render::render_query_text(&query);
            assert!(text.contains(expected), "{source}: {text}");
            let markdown = mant_codec::encode::render_markdown(&query);
            assert!(markdown.contains("**--"), "{markdown}");
            if requests.contains("FILE") {
                assert!(markdown.contains("*FILE*"), "{markdown}");
            }
        }
    }
}

#[test]
fn keeps_command_names_in_extended_mdoc_synopsis_terms() {
    let document = parse_manual_bytes(
        std::path::Path::new("extended-synopsis.8"),
        b".Dd August 19, 2026\n.Dt EXTENDED-SYNOPSIS 8\n.Os\n.Sh NAME\n\
.Nm zinject\n.Nd inject faults\n.Sh SYNOPSIS\n.Bl -tag -width Ds\n\
.It Xo\n.Nm zinject\n.Xc\nList injections.\n\
.It Xo\n.Nm zinject\n.Fl b Ar bookmark\n.Xc\nInject a bookmark.\n.El\n",
    )
    .expect("lower extended mdoc synopsis terms");

    let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
        panic!("expected synopsis definition list");
    };
    assert_eq!(inline_text(&items[0].terms[0]), "zinject");
    assert_eq!(inline_text(&items[1].terms[0]), "zinject -b bookmark");
    assert!(matches!(
        items[0].terms[0].as_slice(),
        [Inline::Anchor { id, .. }, Inline::Strong { .. }]
            if items[0].entry.as_ref().is_some_and(|identity| &identity.id == id)
    ));
    assert!(
        items[1].terms[0]
            .iter()
            .any(|inline| matches!(inline, Inline::Strong { .. }))
    );
}

#[test]
fn independent_mdoc_option_forms_do_not_borrow_the_following_description() {
    let document = parse_manual_bytes(
        std::path::Path::new("shared-option-forms.1"),
        b".Dd August 27, 2026\n.Dt SHARED-OPTION-FORMS 1\n.Os\n.Sh OPTIONS\n\
.Bl -tag -width Ds\n\
.It Fl L Xo\n.Sm off\n.Oo Ar bind_address : Oc\n.Ar port : host : hostport\n.Sm on\n.Xc\n\
.It Fl L Xo\n.Sm off\n.Oo Ar bind_address : Oc\n.Ar port : remote_socket\n.Sm on\n.Xc\n\
.It Fl L Xo\n.Sm off\n.Ar local_socket : host : hostport\n.Sm on\n.Xc\n\
.It Fl L Xo\n.Sm off\n.Ar local_socket : remote_socket\n.Sm on\n.Xc\n\
Forward a local socket.\n.El\n",
    )
    .expect("lower option forms with a shared description");

    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("expected an option definition list");
    };
    assert_eq!(items.len(), 4);
    assert_eq!(
        items
            .iter()
            .map(|item| inline_text(&item.terms[0]))
            .collect::<Vec<_>>(),
        [
            "-L [bind_address:]port:host:hostport",
            "-L [bind_address:]port:remote_socket",
            "-L local_socket:host:hostport",
            "-L local_socket:remote_socket"
        ]
    );
    assert!(items.iter().all(|item| item.terms.len() == 1));
    assert!(items[..3].iter().all(|item| item.description.is_empty()));
    assert_ne!(items[3].description.len(), 0);
}

#[test]
fn independent_mdoc_options_keep_their_own_descriptions() {
    let document = parse_manual_bytes(
        std::path::Path::new("shared-option-description.1"),
        b".Dd August 29, 2026\n.Dt SHARED-OPTION-DESCRIPTION 1\n.Os\n.Sh OPTIONS\n\
.Bl -tag -width Ds\n\
.It Fl I Ar encoding\n\
.It Fl O Ar encoding\n\
Convert filenames from the specified encoding.\n\
.El\n",
    )
    .expect("lower distinct options with a shared description");

    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("expected an option definition list");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(
        items
            .iter()
            .map(|item| inline_text(&item.terms[0]))
            .collect::<Vec<_>>(),
        ["-I encoding", "-O encoding"]
    );
    assert!(items.iter().all(|item| item.terms.len() == 1));
    assert!(items[..1].iter().all(|item| item.description.is_empty()));
    assert_ne!(items[1].description.len(), 0);
}

#[test]
fn generic_callable_terms_are_addressable_without_borrowing_a_later_taxonomy_body() {
    // Reduced from the Pod-generated perltoc(1) layout. Its `.IP` sequence is
    // an index of independent subjects: only ENVIRONMENT owns the following
    // prose. A callable term with sigilled parameters is still a useful exact
    // selector, but generic terms must not form a declaration group and gain
    // the environment description as shared context.
    let document = parse_manual_bytes(
        std::path::Path::new("pod-taxonomy.1"),
        b".TH POD-TAXONOMY 1\n.SH CONTENTS\n.IP run_filter($cmd,$src) 4\n.IP pm_to_blib 4\n.IP _autosplit 4\n.IP ENVIRONMENT 4\nPERL_INSTALL_ROOT controls the install root.\n",
    )
    .expect("lower Pod-style taxonomy");

    let Block::DefinitionList {
        items,
        declaration_groups,
        ..
    } = &document.sections[0].blocks[0]
    else {
        panic!("expected definition list");
    };
    assert_eq!(
        items[0].entry.as_ref().expect("callable entry").names,
        ["run_filter"]
    );
    assert!(items[..3].iter().all(|item| item.description.is_empty()));
    let Block::Paragraph { children, .. } = &items[3].description[0] else {
        panic!("expected environment paragraph");
    };
    assert!(inline_text(children).contains("PERL_INSTALL_ROOT"));
    assert!(
        declaration_groups.is_empty(),
        "generic index labels must not inherit ENVIRONMENT's body: {declaration_groups:?}"
    );
}

#[test]
fn unclassified_native_heads_do_not_split_an_example_into_a_declaration_group() {
    // Reduced from fluidsynth(1).  The source uses TP layout for a multi-line
    // command transcript.  `router_begin note` is deliberately not a
    // selectable declaration, so semantic preparation must not discard that
    // physical head and then treat the following two lines as a fresh group
    // with the final line's prose as shared context.
    let document = parse_manual_bytes(
        std::path::Path::new("command-transcript.1"),
        b".TH COMMAND-TRANSCRIPT 1\n.SH COMMANDS\n\
.TP\n.B Router examples\n\
.TP\nrouter_clear\n\
.TP\nrouter_begin note\n\
.TP\nrouter_chan 0 7 0 15\n\
.TP\nrouter_end\n\
Will accept only note events.\n",
    )
    .expect("lower command transcript");

    let Block::DefinitionList {
        items,
        declaration_groups,
        ..
    } = &document.sections[0].blocks[0]
    else {
        panic!("expected one definition list");
    };
    assert!(
        items
            .iter()
            .any(|item| inline_text(&item.terms[0]) == "router_chan 0 7 0 15")
    );
    assert!(declaration_groups.is_empty(), "{declaration_groups:?}");
}

#[test]
fn preserves_a_single_mdoc_option_argument_and_its_description() {
    let document = parse_manual_bytes(
        std::path::Path::new("option-with-argument.1"),
        b".Dd August 29, 2026\n.Dt OPTION-WITH-ARGUMENT 1\n.Os\n.Sh OPTIONS\n\
.Bl -tag -width Ds\n\
.It Fl Z Ar mode\n\
Select the archive mode without losing this description.\n\
.El\n",
    )
    .expect("lower an mdoc option with one argument");

    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("expected an option definition list");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(inline_text(&items[0].terms[0]), "-Z mode");
    assert_eq!(items[0].entry.as_ref().unwrap().names, ["-Z"]);
    assert!(items[0].description.iter().any(|description| {
        matches!(description, Block::Paragraph { children, .. }
            if inline_text(children)
                == "Select the archive mode without losing this description.")
    }));
}

#[test]
fn separates_alternative_terms_in_an_extended_mdoc_definition_head() {
    let document = parse_manual_bytes(
        std::path::Path::new("extended-term-alternatives.8"),
        b".Dd August 19, 2026\n.Dt EXTENDED-TERM-ALTERNATIVES 8\n.Os\n.Sh OPTIONS\n\
.Bl -tag -width Ds\n.It Xo\n.Sm off\n.Ar ipaddr\n.Op / Ar masklen\n.Pp\n\
.Ar ipaddr\n.Op / Ar prefixlen\n.Sm on\n.Xc\nAccept this peer.\n.El\n",
    )
    .expect("lower alternative extended definition terms");

    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("expected a definition list");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].terms.len(), 2);
    // This exact source was rerun against pristine CVS in all five profiles.
    // termp_pp_pre() calls term_vspace() even inside HEAD: the first term
    // carries that completed empty row, independently of the term split.
    assert_eq!(inline_text(&items[0].terms[0]), "ipaddr[/masklen]\n");
    assert_eq!(inline_text(&items[0].terms[1]), "ipaddr[/prefixlen]");
    let content = ResolvedContent {
        address: None,
        label: "extended-term-alternatives.8".to_owned(),
        document: Some(document),
        tldr: None,
    };
    let text = mant_render::render_query_text(&content);
    assert!(text.contains("ipaddr[/masklen]\n\n"), "{text:?}");
}
