use super::{
    StyledBoundaryRule::{NativeComponents, SingleTextOperand},
    is_complete_hanging_option_head, literal_declaration_ranges,
    literal_declaration_ranges_with_starts, literal_declaration_scan_with_starts,
    literal_option_names, scan_option_declarations, scan_option_declarations_with_numeric,
    scan_option_declarations_with_style, scan_option_declarations_with_style_ranges,
};
use std::ops::Range;
use std::time::{Duration, Instant};

fn native_form(operands: &[&str]) -> (String, Vec<Range<usize>>) {
    let mut form = String::new();
    let mut ranges = Vec::new();
    for operand in operands {
        let start = form.len();
        form.push_str(operand);
        ranges.push(start..form.len());
    }
    (form, ranges)
}

#[test]
fn complete_native_operand_ranges_prove_only_post_argument_declarations() {
    // Each exact TP/B, TP/BI, or TP/BR input first ran pinned CVS -Tutf8.
    // man_macro.c::in_line_eoln retains distinct text operands;
    // man_term.c::pre_alternate joins them, and term.c::term_word can
    // change fonts within one operand without creating a new boundary.
    let cases: &[(&[&str], &[usize], &[&str])] = &[
        (
            &["-L", "first, --fake,last,", "--all ", "FILE"],
            &[1, 3],
            &["-L", "--all"],
        ),
        (
            &["-o ", "FILE", ", --all ", "FILE"],
            &[1, 3],
            &["-o", "--all"],
        ),
        (
            &["--opt ", "arg,", "--all ", "FILE"],
            &[],
            &["--opt", "--all"],
        ),
        (&["--opt", " ARG, --all"], &[], &["--opt", "--all"]),
        (&["--opt", " ARG, --fake,last"], &[], &["--opt"]),
        (&["--opt ", "arg, --fake,--other"], &[], &["--opt"]),
        (
            &["-L", "dir, ", "--output=FILE, --all"],
            &[1],
            &["-L", "--output", "--all"],
        ),
        (&["-L", "arg,", "--all, text"], &[1], &["-L", "--all"]),
        (&["-a ARG, --all"], &[], &["-a", "--all"]),
        (&["-a ARG, -a"], &[], &["-a", "-a"]),
        (&["-a, text"], &[], &["-a"]),
        (
            &["-L", "arg,", " --all ", "FILE"],
            &[1, 3],
            &["-L", "--all"],
        ),
        (
            &["-L", "arg,", "\u{a0}--all ", "FILE"],
            &[1, 3],
            &["-L", "--all"],
        ),
        (&["-L", "arg|", "--all ", "FILE"], &[1, 3], &["-L", "--all"]),
        (
            &[
                "--pattern ",
                "\"first,",
                "--fake",
                ",last\",",
                "--all ",
                "FILE",
            ],
            &[1, 3, 5],
            &["--pattern", "--all"],
        ),
    ];
    for &(parts, argument_operands, expected) in cases {
        let (form, operands) = native_form(parts);
        let arguments = argument_operands
            .iter()
            .map(|&index| operands[index].start)
            .collect::<Vec<_>>();
        let scan = scan_option_declarations(&form, &operands, &arguments).unwrap();
        let (names, over_limit) = scan.names(&form);
        assert!(!over_limit, "{form}");
        assert_eq!(
            names
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            *expected,
            "{form}"
        );
        assert!(
            names
                .iter()
                .all(|(name, range)| &form[range.clone()] == name)
        );
    }
}

#[test]
fn native_operand_evidence_is_checked_and_long_blank_prefix_is_linear() {
    let (form, operands) = native_form(&[
        "-L",
        &format!("{}{}--all", " ".repeat(8192), ",".repeat(4096)),
    ]);
    assert!(scan_option_declarations(&form, &[0..2, 1..form.len()], &[2]).is_none());
    assert!(scan_option_declarations("-L\u{a0}x", std::slice::from_ref(&(0..3)), &[]).is_none());
    let started = Instant::now();
    let scan = scan_option_declarations(&form, &operands, &[2]).unwrap();
    assert_eq!(scan.names(&form).0, [("-L".into(), 0..2)]);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "native operand whitespace prefix was repeatedly rescanned"
    );
}

#[test]
fn native_bold_numeric_short_name_is_not_an_ordinary_negative_argument() {
    // The exact TP/BR `\\-4 ", " \\-\\-ipv4` and `\\-6 ", " \\-\\-ipv6`
    // inputs, and TP/B `\\-4`, ran pinned CVS -Tutf8 before these
    // assertions. man_term.c::pre_alternate prints each BR child with
    // its own initial font; term.c::term_word executes the dash escape.
    for (digit, long) in [("-4", "--ipv4"), ("-6", "--ipv6")] {
        let (form, operands) = native_form(&[digit, ", ", long]);
        let scan = scan_option_declarations_with_numeric(&form, &operands, &[], &[0]).unwrap();
        assert_eq!(
            scan.names(&form),
            (
                vec![(digit.to_owned(), 0..2), (long.to_owned(), 4..form.len())],
                false
            )
        );
        assert_eq!(
            scan_option_declarations(&form, &operands, &[])
                .unwrap()
                .names(&form)
                .0,
            [(long.to_owned(), 4..form.len())]
        );
    }
    assert_eq!(
        scan_option_declarations_with_numeric("-4", &[], &[], &[0])
            .unwrap()
            .names("-4")
            .0,
        [("-4".into(), 0..2)]
    );

    // The exact TP/B `--number -4,--fake,20` input also ran pinned CVS
    // -Tutf8. Its following negative number is an ordinary argument,
    // not another native bold operand start; punctuation inside it must
    // not manufacture a `--fake` declaration.
    let argument = "--number -4,--fake,20";
    assert_eq!(
        scan_option_declarations_with_numeric(argument, &[], &[], &[])
            .unwrap()
            .names(argument)
            .0,
        [("--number".into(), 0..8)]
    );
    // The exact TP/BR `"--number " "\\fB-4,--fake,20"` also ran CVS:
    // term.c::term_word makes the second operand bold, but there is no
    // declaration separator. Its number stays in the argument state.
    let (styled_argument, operands) = native_form(&["--number ", "-4,--fake,20"]);
    assert_eq!(
        scan_option_declarations_with_numeric(&styled_argument, &operands, &[], &[9])
            .unwrap()
            .names(&styled_argument)
            .0,
        [("--number".into(), 0..8)]
    );
    assert!(
        scan_option_declarations_with_numeric(argument, &[], &[], &[9]).is_none(),
        "an argument byte is not the native head's first visible glyph"
    );
    assert!(
        scan_option_declarations_with_numeric("-4, --ipv4", &[0..2, 2..4, 4..10], &[], &[1])
            .is_none(),
        "numeric evidence must identify the exact token start"
    );
}

#[test]
fn bold_underlined_run_is_a_name_only_at_a_proved_native_boundary() {
    // The exact TP/BI `"-L" "\\f[BI]dir"` input ran pinned CVS
    // -Tutf8 first. pre_alternate() joins the children without a space;
    // term_word() changes the second child's font, not its argument role.
    let (glued, operands) = native_form(&["-L", "dir"]);
    let scan = scan_option_declarations_with_style(&glued, &operands, &[], &[2], &[]).unwrap();
    assert_eq!(scan.names(&glued).0, [("-L".into(), 0..2)]);

    // These exact TP/BI operands ran pinned CVS -Tutf8 as well. Their
    // independent comma-delimited third operand is a new declaration;
    // a BI run beginning there is not the preceding italic argument.
    let (separated, operands) = native_form(&["-L", "arg,", "--all ", "FILE"]);
    let scan = scan_option_declarations_with_style(
        &separated,
        &operands,
        &[operands[1].start, operands[3].start],
        &[operands[2].start],
        &[],
    )
    .unwrap();
    assert_eq!(
        scan.names(&separated).0,
        [("-L".into(), 0..2), ("--all".into(), 6..11)]
    );

    // An initial native B operand whose executed font is BI still has a
    // complete name at its first glyph. The exact `B "\\f[BI]--all"`
    // input ran pinned CVS -Tutf8 before this assertion.
    let initial = "--all";
    assert_eq!(
        scan_option_declarations_with_style(initial, &[0..initial.len()], &[], &[0], &[])
            .unwrap()
            .names(initial)
            .0,
        [("--all".into(), 0..5)]
    );

    // Pinned CVS also executed this exact TP/BI quoted-argument head.
    // term_word() prints the quote across font operands; its internal
    // bold-underlined `--fake` is not an authored delimiter restart.
    let (quoted, operands) = native_form(&[
        "--pattern ",
        "\"first,",
        "--fake",
        ",last\",",
        "--all ",
        "FILE",
    ]);
    let scan = scan_option_declarations_with_style(
        &quoted,
        &operands,
        &[operands[1].start, operands[3].start, operands[5].start],
        &[operands[2].start],
        &[],
    )
    .unwrap();
    let all = quoted.find("--all").unwrap();
    assert_eq!(
        scan.names(&quoted).0,
        [("--pattern".into(), 0..9), ("--all".into(), all..all + 5)]
    );
    assert!(
        scan_option_declarations_with_style("--all", &[0..5], &[], &[0; 65], &[]).is_none(),
        "unbounded style evidence must not grow the semantic work set"
    );
}

#[test]
fn visible_quoted_argument_does_not_restart_a_declaration() {
    // Both exact .IP inputs ran pinned CVS -Tutf8 first. man_term.c::
    // pre_IP prints the sole label operand, and term.c::term_word emits
    // \(dq as visible quotes around one parameter containing commas.
    let argument = "--pattern \"one,--fake,two\"";
    let ranges = literal_declaration_ranges(argument);
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0], 0..argument.len());
    assert_eq!(literal_option_names(argument), [("--pattern".into(), 0..9)]);

    let followed = "--pattern \"one,--fake,two\", --all";
    let start = followed.find("--all").unwrap();
    assert_eq!(literal_declaration_ranges(followed).len(), 2);
    assert_eq!(
        literal_option_names(followed),
        [
            ("--pattern".into(), 0..9),
            ("--all".into(), start..start + 5)
        ]
    );

    // term.c::term_word prints plain apostrophes literally. Only an
    // apostrophe beginning an argument opens a quoted interval; an
    // apostrophe inside a word does not consume later declarations.
    let single = "--pattern 'one,--fake,two', --all";
    let start = single.find("--all").unwrap();
    assert_eq!(literal_declaration_ranges(single).len(), 2);
    assert_eq!(
        literal_option_names(single),
        [
            ("--pattern".into(), 0..9),
            ("--all".into(), start..start + 5)
        ]
    );

    let adjacent = "--pattern,'one,--fake,two'";
    assert_eq!(literal_option_names(adjacent), [("--pattern".into(), 0..9)]);
}

#[test]
fn visible_display_quotes_can_wrap_a_name_without_opening_an_argument() {
    // Exact `.TP` / `.B “--foo”` ran pinned CVS -Tutf8 first.  pre_B
    // selects bold; term_word emits the quotation marks and spelling.
    let form = "“--foo”";
    assert_eq!(literal_option_names(form), [("--foo".into(), 3..8)]);
}

#[test]
fn ordinary_argument_punctuation_does_not_restart_a_declaration() {
    // These exact .IP labels first ran pinned CVS -Tutf8. man_term.c::
    // pre_IP emits one label operand; term.c::term_word keeps the plain
    // parameter and its punctuation after the bold --list spelling.
    for form in [
        "--list first,--fake,last",
        "--list, first,--fake,last",
        "--list first|--fake|last",
    ] {
        assert_eq!(
            literal_option_names(form),
            [("--list".into(), 0..6)],
            "{form}"
        );
    }
    let followed = "--list first,--fake,last, --all";
    let start = followed.find("--all").unwrap();
    assert_eq!(
        literal_option_names(followed),
        [("--list".into(), 0..6), ("--all".into(), start..start + 5)]
    );
}

#[test]
fn native_parameter_interval_blocks_internal_font_changed_option_spelling() {
    // This exact TP/BI head with `first,\fB--fake\fI,last,` ran pinned
    // CVS -Thtml first. man_term.c::pre_alternate keeps one italic operand
    // even when term.c::term_word changes fonts inside it; only the later
    // bold operand is an independent declaration candidate.
    let form = "-Lfirst,--fake,last,--all FILE";
    let all = form.find("--all").unwrap();
    assert_eq!(
        literal_declaration_ranges_with_starts(form, &[all], &[2]),
        [0..all - 1, all..form.len()]
    );
    assert_eq!(
        literal_declaration_scan_with_starts(form, &[all], &[2], NativeComponents)
            .names(form)
            .0,
        [("-L".into(), 0..2), ("--all".into(), all..all + 5)]
    );
    // Whitespace inside the same underlined native operand is not a
    // fresh declaration either; an independent later bold operand is.
    let spaced = "-Lfirst, --fake, last,--all FILE";
    let all = spaced.find("--all").unwrap();
    assert_eq!(
        literal_declaration_ranges_with_starts(spaced, &[all], &[2]),
        [0..all - 1, all..spaced.len()]
    );
    // A BI operand switches font, not authored quote/bracket scope. Each
    // exact TP/BI input ran pinned CVS -Tutf8 before this assertion.
    for argument in ["(first,--fake,", "\"first,--fake,"] {
        let form = format!("-L{argument}--all FILE");
        let all = form.find("--all").unwrap();
        assert_eq!(
            literal_declaration_ranges_with_starts(&form, &[all], &[2]),
            std::iter::once(0..form.len()).collect::<Vec<_>>()
        );
        assert_eq!(
            literal_declaration_ranges_with_starts(&form, &[], &[2]),
            std::iter::once(0..form.len()).collect::<Vec<_>>(),
            "no operand may reset {argument}"
        );
        assert_eq!(
            literal_declaration_scan_with_starts(&form, &[all], &[2], NativeComponents)
                .names(&form)
                .0,
            [("-L".into(), 0..2)]
        );
    }
    // Quote/parenthesis closure in a later operand, however, permits
    // the next proved declaration after its terminal comma.
    for argument in ["(first,--fake,last),", "\"first,--fake,last\","] {
        let form = format!("-L{argument}--all FILE");
        let all = form.find("--all").unwrap();
        assert_eq!(
            literal_declaration_scan_with_starts(&form, &[all], &[2], NativeComponents)
                .names(&form)
                .0,
            [("-L".into(), 0..2), ("--all".into(), all..all + 5)]
        );
    }
    // The real cross-operand quote case includes a bold --fake operand
    // inside the same quoted argument; only the post-quote --all is a
    // declaration. Pinned CVS man_term.c::pre_alternate retains the
    // operand font switch without ending the quote.
    let form = "--pattern \"first,--fake,last\",--all FILE";
    let fake = form.find("--fake").unwrap();
    let all = form.find("--all").unwrap();
    assert_eq!(
        literal_declaration_scan_with_starts(form, &[fake, all], &[10], NativeComponents)
            .names(form)
            .0,
        [("--pattern".into(), 0..9), ("--all".into(), all..all + 5)]
    );
    for argument in ["first|--fake|last,", "-10,--fake,20,", "first/--fake/last,"] {
        let form = format!("-L{argument}--all FILE");
        let all = form.find("--all").unwrap();
        assert_eq!(
            literal_declaration_scan_with_starts(&form, &[all], &[2], NativeComponents)
                .names(&form)
                .0,
            [("-L".into(), 0..2), ("--all".into(), all..all + 5)],
            "{argument}"
        );
    }
}

#[test]
fn checked_scan_keeps_the_short_or_long_connector() {
    // Both exact `.B -q or --quiet` and styled `.IP` counterparts ran
    // pinned CVS -Tutf8 first. man_term.c::pre_B/pre_IP emit the complete
    // visible head; `or` connects two names rather than beginning an
    // ordinary argument in the shared declaration grammar.
    for form in ["-q or --quiet", "-a or --all"] {
        let scan = literal_declaration_scan_with_starts(form, &[], &[], SingleTextOperand);
        assert_eq!(scan.names(form).0, literal_option_names(form), "{form}");
        assert_eq!(scan.names(form).0.len(), 2, "{form}");
    }
}

#[test]
fn source_neutral_name_limit_rejects_the_whole_head() {
    // The exact 64- and 65-name TP/B heads ran pinned CVS -Tutf8 first.
    // man_term.c::pre_B and term.c::term_word render both complete heads;
    // the 64-name ceiling belongs to semantic extraction, not mandoc.
    let form = (0..64)
        .map(|index| format!("--n{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    assert_eq!(literal_option_names(&form).len(), 64);
    let over_limit = format!("{form}, --n64");
    assert!(literal_option_names(&over_limit).is_empty());
}

#[test]
fn completed_italic_metavariables_allow_only_outside_styled_declarations() {
    // The exact PP/RS inputs for the positive and negative forms ran the
    // pinned CVS reference -Tutf8 first. man_term.c::pre_PP/pre_RS retain
    // the paragraph/indent structure; term.c::term_word executes each
    // authored font escape before emitting the visible punctuation.
    let form = "-<number>, -n <number>, --max-count=<number>";
    let second = form.find("-n <number>").unwrap() + 3;
    let last = form.rfind("<number>").unwrap();
    let spans = [0..9, second..second + 8, last..last + 8];
    let scan = scan_option_declarations_with_style_ranges(form, &[], &spans, &[], &[])
        .expect("valid final-style ranges");
    assert_eq!(
        scan.names(form).0,
        [
            (
                "-n".into(),
                form.find("-n").unwrap()..form.find("-n").unwrap() + 2
            ),
            (
                "--max-count".into(),
                form.find("--max-count").unwrap()..form.find("--max-count").unwrap() + 11,
            ),
        ]
    );
    assert!(super::is_complete_hanging_option_head(form));

    let ordinary = "-n <number>, --all";
    let all = ordinary.find("--all").unwrap();
    let scan =
        scan_option_declarations_with_style_ranges(ordinary, &[], &[3..11], &[], &[]).unwrap();
    assert_eq!(
        scan.names(ordinary).0,
        [("-n".into(), 0..2), ("--all".into(), all..all + 5)]
    );
    let single = "-<number>";
    assert!(
        scan_option_declarations_with_style_ranges(single, &[], &[0..9], &[], &[])
            .unwrap()
            .names(single)
            .0
            .is_empty()
    );
    assert!(!super::is_complete_hanging_option_head(single));

    for (form, spans, complete_syntax) in [
        ("-<number>, --fake", vec![0..17], true),
        ("-<number>, --fake", vec![0..9, 11..17], true),
        ("-<number, --fake", vec![0..8], false),
    ] {
        let scan = scan_option_declarations_with_style_ranges(form, &[], &spans, &[], &[])
            .expect("valid range bounds");
        assert!(scan.names(form).0.is_empty(), "{form}: {spans:?}");
        // Hanging admission checks only complete visible syntax. Final
        // style evidence is a separate gate; it must reject both fully
        // italic spellings even when their plain text looks complete.
        assert_eq!(
            super::is_complete_hanging_option_head(form),
            complete_syntax,
            "{form}: {spans:?}"
        );
    }
    let fake = "-L first, --fake,last";
    let scan = scan_option_declarations_with_style_ranges(fake, &[], &[3..8], &[], &[]).unwrap();
    assert_eq!(scan.names(fake).0, [("-L".into(), 0..2)]);

    // rg(1)'s exact `-g GLOB, --glob=GLOB` head ran pinned CVS
    // -Tutf8 first. term.c::term_word() ends the italic GLOB run before
    // printing the comma; man_term.c::pre_RS() only indents its body.
    let all_caps = "-g GLOB, --glob=GLOB";
    let last = all_caps.rfind("GLOB").unwrap();
    let scan = scan_option_declarations_with_style_ranges(
        all_caps,
        &[],
        &[3..7, last..last + 4],
        &[],
        &[],
    )
    .unwrap();
    assert_eq!(
        scan.names(all_caps).0,
        [("-g".into(), 0..2), ("--glob".into(), 9..15),]
    );

    // A comma still inside one executed italic parameter, or a
    // nonterminal fake candidate after that parameter, is not a new
    // declaration. Both exact TP/B font variants ran pinned CVS -Tutf8.
    let internal = "-g GLOB,--fake,last";
    let scan =
        scan_option_declarations_with_style_ranges(internal, &[], &[3..19], &[], &[]).unwrap();
    assert_eq!(scan.names(internal).0, [("-g".into(), 0..2)]);
    let nonterminal = "-g GLOB, --fake,last";
    let scan =
        scan_option_declarations_with_style_ranges(nonterminal, &[], &[3..7], &[], &[]).unwrap();
    assert_eq!(scan.names(nonterminal).0, [("-g".into(), 0..2)]);
}

#[test]
fn italic_native_operand_inside_open_brace_does_not_restart_on_slash() {
    // The exact `{-n/-NUM` operand split ran pinned CVS -Tutf8 first.
    // A font/operand boundary does not close the authored brace scope.
    let form = "{-n/-NUM";
    let scan =
        scan_option_declarations_with_style_ranges(form, &[0..4, 4..8], &[4..8], &[], &[]).unwrap();
    assert_eq!(scan.names(form).0, [("-n".into(), 1..3)]);
}

#[test]
fn whitespace_alias_limit_never_falls_back_to_a_partial_first_name() {
    // The four exact TP/B inputs (64/65 unique/repeated names) ran pinned
    // CVS -Tutf8 first. man_macro.c::blk_imp keeps one complete HEAD;
    // man_term.c::pre_B and term.c::term_word print every spelling. The
    // 64-name ceiling governs semantic extraction, not native output.
    for repeated in [false, true] {
        let names = (1..64)
            .map(|index| {
                if repeated {
                    "--same".to_owned()
                } else {
                    format!("--n{index}")
                }
            })
            .collect::<Vec<_>>();
        let bounded = format!("-a {}", names.join(" "));
        assert_eq!(crate::literal_option_aliases(&bounded).unwrap().len(), 64);
        assert_eq!(literal_option_names(&bounded).len(), 64);
        let scan = scan_option_declarations(&bounded, &[], &[]).unwrap();
        let (found, over_limit) = scan.names(&bounded);
        assert!(!over_limit, "repeated={repeated}");
        assert_eq!(found.len(), 64);

        let suffix = if repeated { "--same" } else { "--n64" };
        let exceeded = format!("{bounded} {suffix}");
        assert!(crate::literal_option_aliases(&exceeded).is_none());
        assert!(literal_option_names(&exceeded).is_empty());
        let scan = scan_option_declarations(&exceeded, &[], &[]).unwrap();
        let (_, over_limit) = scan.names(&exceeded);
        assert!(over_limit, "repeated={repeated}");
    }
}

#[test]
fn provisional_pattern_cannot_publish_a_truncated_name_group() {
    // The exact TP/B heads with 64 and 65 long names after -### ran
    // pinned CVS -Tutf8 first. man_term.c::pre_B and term.c::term_word
    // preserve both complete forms; the semantic limit is all-or-nothing.
    let names = (0..64)
        .map(|index| format!("--n{index}"))
        .collect::<Vec<_>>()
        .join(" ");
    let bounded = format!("-### {names}");
    assert_eq!(literal_option_names(&bounded).len(), 64);
    let over_limit = format!("{bounded} --n64");
    assert!(literal_option_names(&over_limit).is_empty());
    assert!(
        scan_option_declarations(&over_limit, &[], &[])
            .expect("valid source-neutral evidence")
            .names(&over_limit)
            .1
    );
}

#[test]
fn single_ip_operand_font_switch_is_not_an_independent_boundary() {
    // Both exact `.IP` inputs ran pinned CVS -Tutf8 first. Its HEAD has
    // one text operand (man_term.c::pre_IP); term.c::term_word applies
    // inline font escapes without making another native component.
    let inline_font = "-L first,--fake,last,";
    assert_eq!(
        literal_declaration_scan_with_starts(inline_font, &[], &[3], SingleTextOperand)
            .names(inline_font)
            .0,
        [("-L".into(), 0..2)]
    );
    // Even comma+space plus a later bold run cannot establish an
    // independent declaration after the styled middle parameter. The
    // same first operand can contain `first, \fB--fake`, so omit `--all`
    // conservatively rather than promoting a false name.
    let spaced = "-a, --operand, --all";
    assert_eq!(
        literal_declaration_scan_with_starts(spaced, &[], &[4], SingleTextOperand)
            .names(spaced)
            .0,
        [("-a".into(), 0..2)]
    );
    let plain = "-a, --all";
    assert_eq!(
        literal_declaration_scan_with_starts(plain, &[], &[], SingleTextOperand)
            .names(plain)
            .0,
        [("-a".into(), 0..2), ("--all".into(), 4..9)]
    );
}

#[test]
fn provisional_pattern_still_bounds_ordinary_and_styled_arguments() {
    // Each exact TP/B or TP/BI input first ran pinned CVS -Tutf8.
    // man_macro.c::blk_imp retains one HEAD, man_term.c::pre_alternate
    // joins BI operands, and term.c::term_word prints punctuation inside
    // a following parameter without manufacturing declaration nodes.
    for form in [
        "-### --long first,--fake,last",
        "-### --long first|--fake|last",
        "-### --long -10,--fake,20",
        "-### --long=FILE",
    ] {
        assert_eq!(
            literal_option_names(form),
            [("--long".into(), 5..11)],
            "{form}"
        );
    }
    let followed = "-### --long first,--fake,last, --all";
    let start = followed.find("--all").unwrap();
    assert_eq!(
        literal_option_names(followed),
        [("--long".into(), 5..11), ("--all".into(), start..start + 5)]
    );
}

#[test]
fn negative_number_parameter_does_not_start_a_declaration() {
    // All three exact .IP labels ran pinned CVS -Tutf8 first. Under
    // man_term.c::pre_IP the visible -10 is still part of one HEAD; it
    // does not license a name inside its comma-separated parameter.
    for form in ["--number -10,--fake,20", "--number, -10,--fake,20"] {
        assert_eq!(literal_option_names(form), [("--number".into(), 0..8)]);
    }
    let followed = "--number -10,--fake,20, --all";
    let start = followed.find("--all").unwrap();
    assert_eq!(
        literal_option_names(followed),
        [
            ("--number".into(), 0..8),
            ("--all".into(), start..start + 5)
        ]
    );
}

#[test]
fn long_plain_argument_gap_does_not_rescan_suffix_per_scalar() {
    // The exact .IP head with 8192 spaces ran pinned CVS -Tutf8 first.
    // man_term.c::pre_IP prints that one HEAD; the syntax scan must not
    // repeatedly inspect its remaining whitespace between separators.
    let form = format!("--list {}first,--fake,last", " ".repeat(8192));
    let started = Instant::now();
    assert_eq!(literal_option_names(&form), [("--list".into(), 0..6)]);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "long literal head was rescanned per scalar"
    );
}

#[test]
fn hanging_heads_require_complete_declaration_syntax() {
    // Each spelling was first executed in a minimal PP/B/RS input with
    // pinned CVS -Tutf8. The formatter keeps the whole PP presentation
    // head; this source-neutral rule alone decides whether it is a name.
    for accepted in [
        "--git-dir",
        "--output FILE",
        "--git-dir=path",
        "-a, --all",
        "--foo [=FILE]",
    ] {
        assert!(is_complete_hanging_option_head(accepted), "{accepted}");
    }
    for rejected in [
        "--git-dir intervening text",
        "--foo --bar",
        "-a / --all",
        "GLOB, --fake",
        "ordinary prose",
    ] {
        assert!(!is_complete_hanging_option_head(rejected), "{rejected}");
    }
}

#[test]
fn hanging_short_prefix_only_licenses_a_checked_long_name() {
    // Each minimal spelling ran pinned CVS -Tutf8 in a man SH/RS page
    // first. man_term.c::print_man_node() preserves the full head while
    // term.c::term_word() executes its final font and glyph boundaries.
    for (form, name) in [("-., --hidden", "--hidden"), ("-0, --null", "--null")] {
        assert_eq!(super::literal_option_names(form)[0].0, name);
        assert!(!super::is_complete_hanging_option_head(form));
        assert!(super::is_complete_hanging_option_head_with_provisional(
            form,
            |prefix, names| prefix == (0..2) && names.len() == 1
        ));
        assert!(!super::is_complete_hanging_option_head_with_provisional(
            form,
            |_, _| false
        ));
    }
    for form in ["-10, --fake", "-0,--fake", "-0, --fake intervening text"] {
        assert!(
            !super::is_complete_hanging_option_head_with_provisional(form, |_, _| true),
            "{form}"
        );
    }
}

#[test]
fn names_remain_separate_from_attached_values_and_unproved_aliases() {
    // Exact TP heads first ran pinned CVS -Tutf8. man_macro.c::blk_imp
    // retains one HEAD; man_term.c::pre_TP prints its label before BODY.
    assert_eq!(
        literal_option_names("--width=NUMBER"),
        [("--width".into(), 0..7)]
    );
    assert_eq!(
        literal_option_names("--output=FILE"),
        [("--output".into(), 0..8)]
    );
    assert_eq!(
        literal_option_names("--set=KEY,VALUE"),
        [("--set".into(), 0..5)]
    );
    assert_eq!(
        literal_option_names("-f, --file=ARCHIVE"),
        [("-f".into(), 0..2), ("--file".into(), 4..10)]
    );
    assert_eq!(literal_option_names("-a, text"), [("-a".into(), 0..2)]);
    assert_eq!(
        literal_option_names("--foo --bar"),
        [("--foo".into(), 0..5)]
    );
    assert_eq!(
        literal_option_names("-o/path/--help"),
        [("-o".into(), 0..2)]
    );
    assert_eq!(literal_option_names("-o /-NUM"), [("-o".into(), 0..2)]);
    assert_eq!(literal_option_names("-a / --all"), [("-a".into(), 0..2)]);
    assert_eq!(
        literal_option_names("[-n/--number]"),
        [("-n".into(), 1..3), ("--number".into(), 4..12)]
    );
    assert_eq!(
        literal_option_names("-a, -a"),
        [("-a".into(), 0..2), ("-a".into(), 4..6)]
    );
    assert!(literal_option_names("-1").is_empty());
    assert!(literal_option_names("FILE").is_empty());
}
