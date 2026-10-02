use super::super::Inline;

/// Split a formatter word at the row boundaries its field passes decided
/// (term.c:220 with 205-207): each boundary is a cell offset inside the
/// word's content; the breakable blanks immediately before it were
/// consumed by the break, and the continuation starts at the boundary.
#[cfg(test)]
pub(super) fn split_word_at_row_boundaries(
    incoming: &[Inline],
    boundaries: &mut Vec<usize>,
) -> Vec<Inline> {
    boundaries.sort_unstable();
    boundaries.dedup();
    let mut output = Vec::with_capacity(incoming.len() + boundaries.len());
    let mut next_boundary = 0;
    split_nodes_at_boundaries(
        incoming,
        &mut 0,
        &mut next_boundary,
        boundaries,
        &mut output,
    );
    output
}

/// Returns true when the whole remainder was emitted (a boundary fell in a
/// non-cell node, so no further split can apply).
#[cfg(test)]
fn split_nodes_at_boundaries(
    nodes: &[Inline],
    cell: &mut usize,
    next_boundary: &mut usize,
    boundaries: &[usize],
    output: &mut Vec<Inline>,
) {
    for node in nodes {
        match node {
            Inline::Text { value } | Inline::Code { value } => {
                split_text_at_boundaries(value, node, cell, next_boundary, boundaries, output);
            }
            Inline::LineBreak { .. } => {
                output.push(node.clone());
                *cell += 1;
                advance_boundary(cell, next_boundary, boundaries);
            }
            Inline::Strong { children } => {
                output.push(Inline::Strong {
                    children: std::mem::take(&mut split_nodes_at_boundaries_owned(
                        children,
                        cell,
                        next_boundary,
                        boundaries,
                    )),
                });
            }
            Inline::Emphasis { children } => {
                output.push(Inline::Emphasis {
                    children: std::mem::take(&mut split_nodes_at_boundaries_owned(
                        children,
                        cell,
                        next_boundary,
                        boundaries,
                    )),
                });
            }
            Inline::PortableDisplay { display, children } => {
                output.push(Inline::PortableDisplay {
                    display: display.clone(),
                    children: split_nodes_at_boundaries_owned(
                        children,
                        cell,
                        next_boundary,
                        boundaries,
                    ),
                });
            }
            Inline::Link {
                target,
                title,
                children,
            } => {
                output.push(Inline::Link {
                    target: target.clone(),
                    title: title.clone(),
                    children: split_nodes_at_boundaries_owned(
                        children,
                        cell,
                        next_boundary,
                        boundaries,
                    ),
                });
            }
            other => output.push(other.clone()),
        }
    }
}

#[cfg(test)]
fn split_nodes_at_boundaries_owned(
    nodes: &[Inline],
    cell: &mut usize,
    next_boundary: &mut usize,
    boundaries: &[usize],
) -> Vec<Inline> {
    let mut output = Vec::with_capacity(nodes.len());
    split_nodes_at_boundaries(nodes, cell, next_boundary, boundaries, &mut output);
    output
}

pub(super) fn split_text_at_boundaries(
    value: &str,
    node: &Inline,
    cell: &mut usize,
    next_boundary: &mut usize,
    boundaries: &[usize],
    output: &mut Vec<Inline>,
) {
    split_text_with_blank_run(
        value,
        node,
        cell,
        next_boundary,
        boundaries,
        output,
        &mut BlankRun::default(),
    );
}

/// A blank run belongs to the current operand, not to each scalar visited
/// while projecting it. Keep its end until the cursor leaves that run.
#[derive(Default)]
struct BlankRun {
    end: usize,
    #[cfg(test)]
    inspected_cells: usize,
}

impl BlankRun {
    fn end_from(&mut self, chars: &[char], start: usize) -> usize {
        if start >= self.end {
            self.end = start;
            while self.end < chars.len()
                && super::super::super::is_formatter_word_blank(chars[self.end])
            {
                #[cfg(test)]
                {
                    self.inspected_cells += 1;
                }
                self.end += 1;
            }
        }
        self.end
    }
}

fn split_text_with_blank_run(
    value: &str,
    node: &Inline,
    cell: &mut usize,
    next_boundary: &mut usize,
    boundaries: &[usize],
    output: &mut Vec<Inline>,
    blank_run: &mut BlankRun,
) {
    let chars: Vec<char> = value.chars().collect();
    let mut index = 0;
    let mut run = String::with_capacity(value.len());
    while index < chars.len() {
        // A row boundary before this graph — the zero-width breakpoint's
        // cell (term.c:287-300; the pass truncated at it, 362-366) or a
        // word-tail candidate an overrun cut at (353-354): the row ends
        // here and the graph starts the next one. The breakpoint itself
        // never prints (term.c:396-398).
        while boundaries.get(*next_boundary) == Some(cell) {
            push_split_text(&mut run, node, output);
            output.push(Inline::line_break());
            advance_boundary(cell, next_boundary, boundaries);
        }
        let character = chars[index];
        if character == '\n' {
            run.push(character);
            push_split_text(&mut run, node, output);
            *cell += 1;
            index += 1;
            advance_boundary(cell, next_boundary, boundaries);
            continue;
        }
        if super::super::super::is_formatter_word_blank(character) {
            // Measure this blank run once. term.c:205-207 consumes a run
            // only at the accepted pass boundary; a later boundary must
            // preserve the same blanks in source order, without rescanning
            // their suffix at every scalar. When its end lands on the next
            // row boundary, the run is the break's consumed separator
            // (term.c:205-207): the accepted row ends here and the word
            // continues on the next device row.
            let end = blank_run.end_from(&chars, index);
            if boundaries.get(*next_boundary) == Some(&(*cell + (end - index))) {
                while run
                    .chars()
                    .next_back()
                    .is_some_and(super::super::super::is_formatter_word_blank)
                {
                    run.pop();
                }
                push_split_text(&mut run, node, output);
                output.push(Inline::line_break());
                *cell += end - index;
                index = end;
                advance_boundary(cell, next_boundary, boundaries);
                continue;
            }
        }
        run.push(character);
        *cell += 1;
        index += 1;
    }
    push_split_text(&mut run, node, output);
}

pub(super) fn advance_boundary(cell: &mut usize, next_boundary: &mut usize, boundaries: &[usize]) {
    if boundaries.get(*next_boundary) == Some(cell) {
        *next_boundary += 1;
    }
}

/// Split delayed glyphs out of a semantic wrapper's projected annotation.
///
/// A `\z` glyph armed before a wrapper macro stays pending until the
/// wrapper's first real formatter word settles it (its own boundary, its
/// marker blank, or a later graph's retreat, term.c:573-589 with 901-927).
/// Wrappers that compact their whole operand stream into one Code, Link,
/// or `PortableDisplay` node capture that projection, so this ejects the
/// first `remaining` visible characters back out of the annotation. The
/// extracted glyphs keep their own node identity: an arm-time style run
/// stays wrapped in its Strong/Emphasis container (mirroring the pre-wrap
/// ledger split in `inline/links.rs`), while an annotation container
/// (Link, `PortableDisplay`) is rebuilt from its unclaimed remainder only,
/// because the delayed glyphs were never part of that annotation. A
/// wrapper emptied by the split still survives when its typed identity is
/// meaningful on its own.
pub(in crate::mandoc) fn split_owned_glyph_prefix(
    nodes: Vec<Inline>,
    remaining: &mut usize,
    original: &Inline,
) -> (Vec<Inline>, Vec<Inline>) {
    let mut prefix = Vec::new();
    let mut suffix = Vec::new();
    for node in nodes {
        if *remaining == 0 {
            suffix.push(node);
            continue;
        }
        if node == *original {
            // Only the glyph's cached annotation proves this is its own
            // node. A new empty-label link can also contain exactly one
            // glyph, so its visible size is never ownership evidence.
            let owned = visible_character_count(&node);
            *remaining -= owned;
            prefix.push(node);
            continue;
        }
        match node {
            Inline::Text { value } => {
                split_value_prefix(&value, false, remaining, &mut prefix, &mut suffix);
            }
            Inline::Code { value } => {
                split_value_prefix(&value, true, remaining, &mut prefix, &mut suffix);
            }
            Inline::Strong { children } => {
                let (before, after) = split_owned_glyph_prefix(children, remaining, original);
                if !before.is_empty() {
                    prefix.push(Inline::Strong { children: before });
                }
                if !after.is_empty() {
                    suffix.push(Inline::Strong { children: after });
                }
            }
            Inline::Emphasis { children } => {
                let (before, after) = split_owned_glyph_prefix(children, remaining, original);
                if !before.is_empty() {
                    prefix.push(Inline::Emphasis { children: before });
                }
                if !after.is_empty() {
                    suffix.push(Inline::Emphasis { children: after });
                }
            }
            Inline::Link {
                target,
                title,
                children,
            } => {
                let (owned, after) = split_owned_glyph_prefix(children, remaining, original);
                prefix.extend(owned);
                // A typed destination is identity data: the node survives
                // even when every visible cell returned to its real owner.
                suffix.push(Inline::Link {
                    target,
                    title,
                    children: after,
                });
            }
            Inline::PortableDisplay { display, children } => {
                let (owned, after) = split_owned_glyph_prefix(children, remaining, original);
                prefix.extend(owned);
                suffix.push(Inline::PortableDisplay {
                    display,
                    children: after,
                });
            }
            Inline::Equation { .. } => {
                // A delayed zero-advance glyph is never equation payload.
                // Preserve the node intact to keep the visit order.
                prefix.push(node);
            }
            Inline::Anchor { .. } | Inline::LineBreak { .. } => prefix.push(node),
        }
    }
    (prefix, suffix)
}

/// Split one text or code value at the `remaining`-th visible character.
/// The delayed prefix keeps its authored node type; only a code suffix
/// stays code, because that styling belongs to the wrapper's own run.
fn split_value_prefix(
    value: &str,
    code: bool,
    remaining: &mut usize,
    prefix: &mut Vec<Inline>,
    suffix: &mut Vec<Inline>,
) {
    let split = value
        .char_indices()
        .find_map(|(index, character)| {
            if !character.is_whitespace() {
                *remaining -= 1;
            }
            (*remaining == 0).then_some(index + character.len_utf8())
        })
        .unwrap_or(value.len());
    if split > 0 {
        prefix.push(Inline::Text {
            value: value[..split].to_owned(),
        });
    }
    if split < value.len() {
        let tail = value[split..].to_owned();
        suffix.push(if code {
            Inline::Code { value: tail }
        } else {
            Inline::Text { value: tail }
        });
    }
}

/// Non-whitespace scalar count of one node tree, matching the count the
/// zero-advance ledger reports for a pending glyph.
fn visible_character_count(node: &Inline) -> usize {
    match node {
        Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => value
            .chars()
            .filter(|character| !character.is_whitespace())
            .count(),
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::PortableDisplay { children, .. }
        | Inline::Link { children, .. } => children.iter().map(visible_character_count).sum(),
        Inline::Anchor { .. } | Inline::LineBreak { .. } => 0,
    }
}

fn push_split_text(run: &mut String, node: &Inline, output: &mut Vec<Inline>) {
    if run.is_empty() {
        return;
    }
    let text = std::mem::take(run);
    match node {
        Inline::Code { .. } => output.push(Inline::Code { value: text }),
        _ => output.push(Inline::Text { value: text }),
    }
}

/// Keep exactly each native owner's accepted scalar interval. A cached
/// glyph can resume an earlier owner after a later owner's marker: acceptance
/// follows that identity, never the append location. Typed links and authored
/// anchors survive even when all their visible cells were rejected.
pub(in crate::mandoc::inline::flow) fn retain_native_field_owners(
    nodes: &mut Vec<Inline>,
    limits: &std::collections::BTreeMap<String, super::native_passes::OwnerAcceptance>,
) -> bool {
    fn retain(
        nodes: &mut Vec<Inline>,
        limits: &std::collections::BTreeMap<String, super::native_passes::OwnerAcceptance>,
        owner: &mut Option<String>,
        prefix_printed: &mut Option<bool>,
        positions: &mut std::collections::BTreeMap<String, usize>,
        found: &mut bool,
    ) {
        nodes.retain_mut(|node| {
            if let Inline::Anchor { id, .. } = node {
                if let Some(serial) = id.as_str().strip_prefix(super::INTERNAL_FIELD_PREFIX) {
                    // Generated minbl precedes the source word marker. It is
                    // not a scalar of the preceding word and cannot consume
                    // that word's accepted interval. term_field()389-427 prints
                    // it only with this next word's first accepted graph.
                    let word = format!("{}{serial}", super::INTERNAL_FIELD_WORD);
                    *prefix_printed =
                        Some(limits.get(&word).is_some_and(|range| range.prefix_printed));
                } else if limits.contains_key(id.as_str()) {
                    *owner = Some(id.as_str().to_owned());
                    *prefix_printed = None;
                    *found = true;
                }
                return true;
            }
            match node {
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::PortableDisplay { children, .. }
                | Inline::Link { children, .. } => {
                    retain(children, limits, owner, prefix_printed, positions, found);
                    !children.is_empty()
                        || matches!(node, Inline::Link { .. } | Inline::PortableDisplay { .. })
                }
                _ if prefix_printed.is_some() => prefix_printed == &Some(true),
                _ if owner.is_none() => true,
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => {
                    let owner = owner.as_ref().expect("native owner");
                    if value.is_empty() {
                        // An invisible native graph can own an empty physical
                        // row witness. A glyphless Rust fragment alone cannot
                        // establish it (term.c:340-349,475-481).
                        return limits.get(owner).is_some_and(|range| range.native_cells);
                    }
                    let position = positions.entry(owner.clone()).or_default();
                    let count = value.chars().count();
                    let accepted = limits
                        .get(owner)
                        .map_or(0, |range| range.scalars)
                        .saturating_sub(*position);
                    if count > accepted {
                        let end = value
                            .char_indices()
                            .nth(accepted)
                            .map_or(value.len(), |(byte, _)| byte);
                        value.truncate(end);
                    }
                    *position = position.saturating_add(count);
                    !value.is_empty()
                }
                Inline::LineBreak { .. } => {
                    let owner = owner.as_ref().expect("native owner");
                    limits.get(owner).is_some_and(|range| range.native_cells)
                }
                Inline::Anchor { .. } => unreachable!(),
            }
        });
    }
    let mut found = false;
    retain(
        nodes,
        limits,
        &mut None,
        &mut None,
        &mut std::collections::BTreeMap::new(),
        &mut found,
    );
    found
}

#[cfg(test)]
mod blank_run_tests {
    use crate::mandoc::inline::plain_text;

    use super::{BlankRun, Inline, split_text_with_blank_run, split_word_at_row_boundaries};

    #[test]
    fn later_boundary_inspects_each_blank_once() {
        // Exact pristine CVS -Tascii/-Tutf8/-Tlint was run before this
        // assertion for HANG `.No "X                Y\p Z"`: the first
        // blanks stay before Y; only the accepted boundary before Z consumes
        // its separator. term.c:205-207,263-367 decides those boundaries.
        // This tests projection work, independently of device soft wrapping.
        for count in [1_024, 2_048, 4_096, 8_192] {
            let value = format!("X{}Y Z", " ".repeat(count));
            let node = Inline::Text {
                value: value.clone(),
            };
            let mut blank_run = BlankRun::default();
            let mut output = Vec::new();
            let mut cell = 0;
            let mut boundary = 0;
            split_text_with_blank_run(
                &value,
                &node,
                &mut cell,
                &mut boundary,
                &[count + 3],
                &mut output,
                &mut blank_run,
            );
            assert_eq!(plain_text(&output), format!("X{}Y\nZ", " ".repeat(count)));
            assert_eq!(blank_run.inspected_cells, count + 1);
            assert_eq!(cell, count + 4);
            assert_eq!(boundary, 1);
        }
    }

    #[test]
    fn accepted_run_end_consumes_only_that_separator() {
        // Exact CVS profiles for HANG `.br` then `.No "X                Y Z"`
        // preserve X / Y Z on separate rows. term.c:205-207 consumes only
        // the blank run at the accepted boundary, retaining the later blank.
        let node = Inline::Text {
            value: "X                Y Z".to_owned(),
        };
        let output = split_word_at_row_boundaries(&[node], &mut vec![17]);
        assert_eq!(plain_text(&output), "X\nY Z");
    }

    #[test]
    fn multiple_run_boundaries_preserve_style_and_source_order() {
        // Exact CVS profiles for `.Sy "X\p    Y\p        Z"` preserve
        // three styled rows. term.c:205-207 consumes each breaking run;
        // wrapping accepted projection in Strong cannot change its order.
        let source = Inline::Strong {
            children: vec![Inline::Text {
                value: "X    Y        Z".to_owned(),
            }],
        };
        let output = split_word_at_row_boundaries(&[source], &mut vec![14, 5]);
        assert_eq!(plain_text(&output), "X\nY\nZ");
        assert!(matches!(&output[..], [Inline::Strong { .. }]));
    }
}
