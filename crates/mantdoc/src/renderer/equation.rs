use super::{
    EquationTerminal, EquationTerminalToken, Limits, RenderFormat, TerminalFont,
    normalize_equation_symbol, render_terminal_font, render_visible_text,
};

/// One private node in the renderer's retained eqn box tree.
///
/// This deliberately parallels mandoc's `eqn_box`: public `Node::equation`
/// is a compatibility projection and cannot carry these font, decoration, or
/// grouping edges without changing the owned-AST contract.
#[derive(Clone, Debug)]
struct TerminalEquationBox {
    parent: Option<usize>,
    children: Vec<usize>,
    kind: TerminalEquationKind,
    position: TerminalEquationPosition,
    font: TerminalEquationFont,
    quoted: bool,
    text: Option<Box<str>>,
    left: Option<Box<str>>,
    right: Option<Box<str>>,
    top: Option<Box<str>>,
    bottom: Option<Box<str>>,
    expected_arguments: usize,
}

impl TerminalEquationBox {
    fn root() -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            kind: TerminalEquationKind::List,
            position: TerminalEquationPosition::None,
            font: TerminalEquationFont::None,
            quoted: false,
            text: None,
            left: None,
            right: None,
            top: None,
            bottom: None,
            expected_arguments: usize::MAX,
        }
    }

    fn child(font: TerminalEquationFont, parent: usize) -> Self {
        Self {
            parent: Some(parent),
            children: Vec::new(),
            kind: TerminalEquationKind::Text,
            position: TerminalEquationPosition::None,
            font,
            quoted: false,
            text: None,
            left: None,
            right: None,
            top: None,
            bottom: None,
            expected_arguments: usize::MAX,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminalEquationKind {
    Text,
    Subexpression,
    List,
    Pile,
    Matrix,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminalEquationPosition {
    None,
    Sup,
    Subsup,
    Sub,
    To,
    From,
    Fromto,
    Over,
    Sqrt,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminalEquationFont {
    None,
    Roman,
    Bold,
    Fat,
    Italic,
}

impl TerminalEquationFont {
    fn terminal(self) -> TerminalFont {
        match self {
            Self::None | Self::Roman => TerminalFont::Roman,
            Self::Bold | Self::Fat => TerminalFont::Bold,
            Self::Italic => TerminalFont::Italic,
        }
    }
}

/// An allocation arena makes the C parser's re-parenting operation explicit
/// without self-referential Rust pointers.  It is private, bounded by parser
/// token limits, and dropped immediately after one render call.
#[derive(Default)]
struct TerminalEquationTree {
    boxes: Vec<TerminalEquationBox>,
}

impl TerminalEquationTree {
    fn new() -> Self {
        Self {
            boxes: vec![TerminalEquationBox::root()],
        }
    }

    fn allocate(&mut self, parent: usize) -> usize {
        let font = self.boxes[parent].font;
        let index = self.boxes.len();
        self.boxes.push(TerminalEquationBox::child(font, parent));
        self.boxes[parent].children.push(index);
        index
    }

    fn parent(&self, index: usize) -> usize {
        self.boxes[index].parent.unwrap_or(0)
    }

    fn previous(&self, index: usize) -> Option<usize> {
        let parent = self.boxes[index].parent?;
        let siblings = &self.boxes[parent].children;
        let position = siblings.iter().position(|sibling| *sibling == index)?;
        position
            .checked_sub(1)
            .and_then(|position| siblings.get(position).copied())
    }

    fn next(&self, index: usize) -> Option<usize> {
        let parent = self.boxes[index].parent?;
        let siblings = &self.boxes[parent].children;
        let position = siblings.iter().position(|sibling| *sibling == index)?;
        siblings.get(position + 1).copied()
    }

    fn first(&self, index: usize) -> Option<usize> {
        self.boxes[index].children.first().copied()
    }

    fn move_to_available(&self, mut parent: usize) -> usize {
        while parent != 0
            && self.boxes[parent].children.len() >= self.boxes[parent].expected_arguments
        {
            parent = self.parent(parent);
        }
        parent
    }

    fn move_past_singletons(&self, mut parent: usize) -> usize {
        while parent != 0
            && self.boxes[parent].kind == TerminalEquationKind::List
            && self.boxes[parent].expected_arguments == 1
            && self.boxes[parent].children.len() == 1
        {
            parent = self.parent(parent);
        }
        parent
    }

    fn make_binary(&mut self, parent: usize) -> usize {
        let previous = self.boxes[parent]
            .children
            .pop()
            .expect("binary eqn operator has a left box");
        let binary = self.allocate(parent);
        self.boxes[binary].kind = TerminalEquationKind::Subexpression;
        self.boxes[binary].expected_arguments = 2;
        self.boxes[binary].children.push(previous);
        self.boxes[previous].parent = Some(binary);
        binary
    }

    fn add_text(
        &mut self,
        parent: usize,
        text: impl Into<Box<str>>,
        font: Option<TerminalEquationFont>,
    ) -> usize {
        let text = text.into();
        let node = self.allocate(parent);
        self.boxes[node].kind = TerminalEquationKind::Text;
        self.boxes[node].text = Some(text);
        if let Some(font) = font {
            self.boxes[node].font = font;
        }
        node
    }
}

/// Build the private eqn box tree using the same left/right association rules
/// that define mandoc's device behavior.  The parser has already applied the
/// public budgets and definition expansion, so this phase cannot widen its
/// resource envelope or affect AST/diagnostic compatibility.
fn parse_terminal_equation(tokens: &[EquationTerminalToken]) -> TerminalEquationTree {
    let tokens = coalesce_terminal_equation_escapes(tokens);
    let mut tree = TerminalEquationTree::new();
    let mut parent = 0_usize;
    let mut index = 0_usize;
    while let Some(token) = tokens.get(index) {
        index += 1;
        let text = token.text.as_ref();
        let keyword = (!token.quoted).then_some(text);
        match keyword {
            Some("mark" | "lineup" | "define" | "ndefine" | "tdefine" | "undef" | "delim") => {}
            Some("gfont" | "gsize" | "fwd" | "back" | "down" | "up") => {
                index = index.saturating_add(usize::from(tokens.get(index).is_some()));
            }
            Some("size") => {
                index = index.saturating_add(usize::from(tokens.get(index).is_some()));
                parent = tree.move_to_available(parent);
                let size = tree.allocate(parent);
                tree.boxes[size].kind = TerminalEquationKind::List;
                tree.boxes[size].expected_arguments = 1;
                parent = size;
            }
            Some("roman" | "bold" | "italic" | "fat") => {
                parent = tree.move_to_available(parent);
                let font = tree.allocate(parent);
                tree.boxes[font].kind = TerminalEquationKind::List;
                tree.boxes[font].expected_arguments = 1;
                tree.boxes[font].font = match text {
                    "roman" => TerminalEquationFont::Roman,
                    "bold" => TerminalEquationFont::Bold,
                    "italic" => TerminalEquationFont::Italic,
                    "fat" => TerminalEquationFont::Fat,
                    _ => unreachable!("matched eqn font keyword"),
                };
                parent = font;
            }
            Some("sqrt") => {
                parent = tree.move_to_available(parent);
                let sqrt = tree.allocate(parent);
                tree.boxes[sqrt].kind = TerminalEquationKind::Subexpression;
                tree.boxes[sqrt].position = TerminalEquationPosition::Sqrt;
                tree.boxes[sqrt].expected_arguments = 1;
                parent = sqrt;
            }
            Some("sub" | "sup" | "from" | "to") => {
                if tree.boxes[parent].children.is_empty() {
                    let _ = tree.add_text(parent, "", Some(TerminalEquationFont::Roman));
                }
                while parent != 0
                    && tree.boxes[parent].expected_arguments == 1
                    && tree.boxes[parent].children.len() == 1
                {
                    parent = tree.parent(parent);
                }
                if matches!(text, "from" | "to") {
                    let mut positioned = Some(parent);
                    while let Some(candidate) = positioned {
                        if matches!(
                            tree.boxes[candidate].position,
                            TerminalEquationPosition::Sub
                                | TerminalEquationPosition::Sup
                                | TerminalEquationPosition::Subsup
                                | TerminalEquationPosition::Sqrt
                                | TerminalEquationPosition::Over
                        ) {
                            parent = tree.parent(candidate);
                            break;
                        }
                        positioned = tree.boxes[candidate].parent;
                    }
                }
                if text == "sup" && tree.boxes[parent].position == TerminalEquationPosition::Sub {
                    tree.boxes[parent].position = TerminalEquationPosition::Subsup;
                    tree.boxes[parent].expected_arguments = 3;
                    continue;
                }
                if text == "to" && tree.boxes[parent].position == TerminalEquationPosition::From {
                    tree.boxes[parent].position = TerminalEquationPosition::Fromto;
                    tree.boxes[parent].expected_arguments = 3;
                    continue;
                }
                let positioned = tree.make_binary(parent);
                tree.boxes[positioned].position = match text {
                    "sub" => TerminalEquationPosition::Sub,
                    "sup" => TerminalEquationPosition::Sup,
                    "from" => TerminalEquationPosition::From,
                    "to" => TerminalEquationPosition::To,
                    _ => unreachable!("matched eqn position keyword"),
                };
                parent = positioned;
            }
            Some("over") => {
                if tree.boxes[parent].children.is_empty() {
                    let _ = tree.add_text(parent, "", Some(TerminalEquationFont::Roman));
                }
                parent = tree.move_to_available(parent);
                while parent != 0 && tree.boxes[parent].kind == TerminalEquationKind::Subexpression
                {
                    parent = tree.parent(parent);
                }
                let fraction = tree.make_binary(parent);
                tree.boxes[fraction].position = TerminalEquationPosition::Over;
                parent = fraction;
            }
            Some("left" | "{") => {
                parent = tree.move_to_available(parent);
                let list = tree.allocate(parent);
                tree.boxes[list].kind = TerminalEquationKind::List;
                if text == "left" {
                    let delimiter = tokens.get(index).map_or("", |token| token.text.as_ref());
                    index = index.saturating_add(usize::from(tokens.get(index).is_some()));
                    tree.boxes[list].left = Some(terminal_equation_delimiter(delimiter).into());
                }
                parent = list;
            }
            Some("right" | "}") => {
                let mut candidate = Some(parent);
                let mut closing = None;
                while let Some(current) = candidate {
                    let box_ = &tree.boxes[current];
                    if box_.kind == TerminalEquationKind::List
                        && box_.expected_arguments > 1
                        && (text == "}" || box_.left.is_some())
                    {
                        closing = Some(current);
                        break;
                    }
                    candidate = box_.parent;
                }
                if let Some(closing) = closing {
                    if text == "right" {
                        let delimiter = tokens.get(index).map_or("", |token| token.text.as_ref());
                        index = index.saturating_add(usize::from(tokens.get(index).is_some()));
                        tree.boxes[closing].right =
                            Some(terminal_equation_delimiter(delimiter).into());
                    }
                    parent = tree.parent(closing);
                    if text == "}"
                        && matches!(
                            tree.boxes[parent].kind,
                            TerminalEquationKind::Pile | TerminalEquationKind::Matrix
                        )
                    {
                        parent = tree.parent(parent);
                    }
                    parent = tree.move_past_singletons(parent);
                }
            }
            Some("pile" | "lpile" | "rpile" | "cpile" | "ccol" | "lcol" | "rcol") => {
                parent = tree.move_to_available(parent);
                let pile = tree.allocate(parent);
                tree.boxes[pile].kind = TerminalEquationKind::Pile;
                tree.boxes[pile].expected_arguments = 1;
                parent = pile;
            }
            Some("above") => {
                let mut pile = Some(parent);
                while let Some(current) = pile {
                    if tree.boxes[current].kind == TerminalEquationKind::Pile {
                        let row = tree.allocate(current);
                        tree.boxes[row].kind = TerminalEquationKind::List;
                        parent = row;
                        break;
                    }
                    pile = tree.boxes[current].parent;
                }
            }
            Some("matrix") => {
                parent = tree.move_to_available(parent);
                let matrix = tree.allocate(parent);
                tree.boxes[matrix].kind = TerminalEquationKind::Matrix;
                tree.boxes[matrix].expected_arguments = 1;
                parent = matrix;
            }
            Some("dyad" | "vec" | "under" | "bar" | "tilde" | "hat" | "dot" | "dotdot") => {
                if tree.boxes[parent].children.is_empty() {
                    let _ = tree.add_text(parent, "", Some(TerminalEquationFont::Roman));
                }
                let decorated = tree.make_binary(parent);
                tree.boxes[decorated].kind = TerminalEquationKind::List;
                tree.boxes[decorated].expected_arguments = 1;
                tree.boxes[decorated].font = TerminalEquationFont::Roman;
                match text {
                    "under" => tree.boxes[decorated].bottom = Some("\\[ul]".into()),
                    "bar" => tree.boxes[decorated].top = Some("\\[rn]".into()),
                    "vec" => tree.boxes[decorated].top = Some("\\[->]".into()),
                    "dyad" => tree.boxes[decorated].top = Some("\\[<>]".into()),
                    "tilde" => tree.boxes[decorated].top = Some("\\[a~]".into()),
                    "hat" => tree.boxes[decorated].top = Some("\\[ha]".into()),
                    "dot" => tree.boxes[decorated].top = Some("\\[a.]".into()),
                    "dotdot" => tree.boxes[decorated].top = Some("\\[ad]".into()),
                    _ => unreachable!("matched eqn decoration keyword"),
                }
            }
            _ => {
                parent = tree.move_to_available(parent);
                append_terminal_equation_text(&mut tree, parent, token);
            }
        }
    }
    tree
}

/// Scanner-stage escape normalization preserves a two-character roff escape
/// inside an eqn range as a bare backslash followed by its name. Rejoin that
/// bounded pair before device-box parsing so it remains one Roman symbol
/// rather than an empty text box plus italic prose.
fn coalesce_terminal_equation_escapes(
    tokens: &[EquationTerminalToken],
) -> Vec<EquationTerminalToken> {
    let mut output = Vec::with_capacity(tokens.len());
    let mut index = 0_usize;
    while let Some(token) = tokens.get(index) {
        if !token.quoted
            && token.text.as_ref() == "\\"
            && let Some(next) = tokens.get(index + 1).filter(|next| !next.quoted)
        {
            output.push(EquationTerminalToken {
                text: format!("\\({}", next.text).into(),
                quoted: false,
            });
            index += 2;
        } else {
            output.push(token.clone());
            index += 1;
        }
    }
    output
}

fn terminal_equation_delimiter(value: &str) -> &str {
    match value {
        "ceiling" => "\\[lc]",
        "floor" => "\\[lf]",
        other => other,
    }
}

fn append_terminal_equation_text(
    tree: &mut TerminalEquationTree,
    parent: usize,
    token: &EquationTerminalToken,
) {
    if token.quoted {
        let font = (tree.boxes[parent].font == TerminalEquationFont::None)
            .then_some(TerminalEquationFont::Italic);
        let node = tree.add_text(parent, token.text.clone(), font);
        tree.boxes[node].quoted = true;
        return;
    }
    let mapped = normalize_equation_symbol(&token.text);
    if mapped != token.text.as_ref() {
        let _ = tree.add_text(parent, mapped, None);
        return;
    }
    if token.text.starts_with("\\(") {
        let _ = tree.add_text(
            parent,
            token.text.clone(),
            Some(TerminalEquationFont::Roman),
        );
        return;
    }
    if equation_function(&token.text) {
        let _ = tree.add_text(
            parent,
            token.text.clone(),
            Some(TerminalEquationFont::Roman),
        );
        return;
    }
    if tree.boxes[parent].font != TerminalEquationFont::None || token.text.is_empty() {
        let _ = tree.add_text(parent, token.text.clone(), None);
        return;
    }
    let parts = split_terminal_equation_text(&token.text);
    let parent = if parts.len() > 1
        && tree.boxes[parent].children.len() + 1 >= tree.boxes[parent].expected_arguments
    {
        // Mandoc reparents a compound text box (for example `a+b`) into a
        // list before splitting it.  That keeps all pieces under a unary
        // operand such as `sqrt`, rather than letting only the first one be
        // consumed by the enclosing positional box.
        let list = tree.allocate(parent);
        tree.boxes[list].kind = TerminalEquationKind::List;
        list
    } else {
        parent
    };
    for (text, font) in parts {
        let _ = tree.add_text(parent, text, Some(font));
    }
}

fn equation_function(value: &str) -> bool {
    matches!(
        value,
        "acos"
            | "acsc"
            | "and"
            | "arc"
            | "asec"
            | "asin"
            | "atan"
            | "cos"
            | "cosh"
            | "coth"
            | "csc"
            | "det"
            | "exp"
            | "for"
            | "if"
            | "lim"
            | "ln"
            | "log"
            | "max"
            | "min"
            | "sec"
            | "sin"
            | "sinh"
            | "tan"
            | "tanh"
            | "Im"
            | "Re"
    )
}

fn split_terminal_equation_text(value: &str) -> Vec<(Box<str>, TerminalEquationFont)> {
    #[derive(Clone, Copy, Eq, PartialEq)]
    enum Class {
        Letter,
        Digit,
        Punctuation,
    }
    fn class(character: char, previous: Option<Class>, next: Option<char>) -> Class {
        if character.is_ascii_alphabetic() {
            Class::Letter
        } else if character.is_ascii_digit()
            || (character == '.'
                && (previous == Some(Class::Digit)
                    || next.is_some_and(|character| character.is_ascii_digit())))
        {
            Class::Digit
        } else {
            Class::Punctuation
        }
    }

    let characters = value.chars().collect::<Vec<_>>();
    let mut output = Vec::new();
    let mut start = 0_usize;
    let mut previous = None;
    for (index, character) in characters.iter().copied().enumerate() {
        let current = class(character, previous, characters.get(index + 1).copied());
        let boundary = index > 0
            && (current != previous.unwrap_or(current)
                || character == ','
                || characters[index - 1] == ',');
        if boundary {
            let text = characters[start..index].iter().collect::<String>();
            output.push((
                text.into_boxed_str(),
                if previous == Some(Class::Letter) {
                    TerminalEquationFont::Italic
                } else {
                    TerminalEquationFont::Roman
                },
            ));
            start = index;
        }
        previous = Some(current);
    }
    if start < characters.len() {
        let text = characters[start..].iter().collect::<String>();
        output.push((
            text.into_boxed_str(),
            if previous == Some(Class::Letter) {
                TerminalEquationFont::Italic
            } else {
                TerminalEquationFont::Roman
            },
        ));
    }
    output
}

#[derive(Default)]
struct TerminalEquationWriter {
    output: String,
    no_space: bool,
}

impl TerminalEquationWriter {
    fn attach(&mut self) {
        self.no_space = true;
    }

    fn word(&mut self, value: &str) {
        if value.is_empty() {
            return;
        }
        if !self.output.is_empty() && !self.no_space {
            self.output.push(' ');
        }
        self.output.push_str(value);
        self.no_space = false;
    }
}

/// Render retained eqn boxes using the terminal device's compact positional
/// syntax (`_`, `^`, `/`, and overstrikes).  The final prose wrapping pass
/// still owns line width and indentation, exactly as for ordinary text.
pub(super) fn render_terminal_equation(
    equation: &EquationTerminal,
    format: RenderFormat,
    limits: &Limits,
) -> String {
    let tree = parse_terminal_equation(&equation.tokens);
    let mut writer = TerminalEquationWriter::default();
    for child in tree.boxes[0].children.iter().copied() {
        render_terminal_equation_box(&tree, child, format, limits, &mut writer);
    }
    writer.output
}

fn render_terminal_equation_box(
    tree: &TerminalEquationTree,
    index: usize,
    format: RenderFormat,
    limits: &Limits,
    writer: &mut TerminalEquationWriter,
) {
    let box_ = &tree.boxes[index];
    let parent = box_.parent;
    let previous = tree.previous(index);
    let delimiter = (box_.kind == TerminalEquationKind::List && box_.expected_arguments > 1)
        || (box_.kind == TerminalEquationKind::Pile
            && (previous.is_some() || tree.next(index).is_some()))
        || parent.is_some_and(|parent| {
            let parent_box = &tree.boxes[parent];
            parent_box.position == TerminalEquationPosition::Sqrt
                || ((box_.top.is_some() || box_.bottom.is_some())
                    && parent_box.kind == TerminalEquationKind::Subexpression
                    && parent_box.position != TerminalEquationPosition::Over
                    && tree.next(index).is_some())
                || (box_.kind == TerminalEquationKind::Subexpression
                    && box_.position != TerminalEquationPosition::Sqrt
                    && ((parent_box.kind == TerminalEquationKind::List
                        && parent_box.expected_arguments == 1)
                        || (parent_box.kind == TerminalEquationKind::Subexpression
                            && box_.position != TerminalEquationPosition::Sqrt)))
        });
    if delimiter {
        let attach = parent.is_some_and(|parent| {
            (tree.boxes[parent].kind == TerminalEquationKind::Subexpression && previous.is_some())
                || (box_.kind == TerminalEquationKind::List
                    && tree.first(index).is_some_and(|first| {
                        !matches!(
                            tree.boxes[first].kind,
                            TerminalEquationKind::Pile | TerminalEquationKind::Matrix
                        )
                    })
                    && previous.is_some_and(|previous| {
                        tree.boxes[previous].kind == TerminalEquationKind::List
                            || (tree.boxes[previous].kind == TerminalEquationKind::Text
                                && tree.boxes[previous].text.as_deref().is_some_and(|text| {
                                    text.starts_with('\\') || text.starts_with(char::is_alphabetic)
                                }))
                    }))
        });
        if attach {
            writer.attach();
        }
        let parent_font = parent
            .map(|parent| tree.boxes[parent].font.terminal())
            .unwrap_or_default();
        writer.word(&render_terminal_font(
            &render_terminal_equation_text(box_.left.as_deref().unwrap_or("("), format, limits),
            parent_font,
        ));
        writer.attach();
    }

    if let Some(text) = box_.text.as_deref() {
        if text.starts_with(|character: char| {
            matches!(
                character,
                '!' | '\"' | '\'' | ')' | ',' | '.' | ':' | ';' | '?' | ']' | '}'
            )
        }) {
            writer.attach();
        }
        let rendered = render_terminal_equation_text(text, format, limits);
        writer.word(&render_terminal_font(&rendered, box_.font.terminal()));
        if text.ends_with(['"', '\'', '(', '[', '{'])
            || (previous.is_none() && (text.ends_with('-') || text.ends_with("\\[mi]")))
        {
            writer.attach();
        }
    }

    match box_.position {
        TerminalEquationPosition::Sqrt => {
            writer.word(&render_terminal_equation_text("\\(sr", format, limits));
            if let Some(child) = tree.first(index) {
                writer.attach();
                render_terminal_equation_box(tree, child, format, limits, writer);
            }
        }
        TerminalEquationPosition::Sup
        | TerminalEquationPosition::Sub
        | TerminalEquationPosition::Subsup
        | TerminalEquationPosition::To
        | TerminalEquationPosition::From
        | TerminalEquationPosition::Fromto
        | TerminalEquationPosition::Over => {
            let mut children = box_.children.iter().copied();
            if let Some(left) = children.next() {
                render_terminal_equation_box(tree, left, format, limits, writer);
            }
            writer.attach();
            writer.word(match box_.position {
                TerminalEquationPosition::Over => "/",
                TerminalEquationPosition::Sup | TerminalEquationPosition::To => "^",
                _ => "_",
            });
            if let Some(right) = children.next() {
                writer.attach();
                render_terminal_equation_box(tree, right, format, limits, writer);
            }
            if matches!(
                box_.position,
                TerminalEquationPosition::Subsup | TerminalEquationPosition::Fromto
            ) {
                writer.attach();
                writer.word("^");
                if let Some(upper) = children.next() {
                    writer.attach();
                    render_terminal_equation_box(tree, upper, format, limits, writer);
                }
            }
        }
        TerminalEquationPosition::None => {
            let mut children = box_.children.iter().copied();
            if box_.kind == TerminalEquationKind::Matrix
                && tree.first(index).is_some_and(|child| {
                    tree.boxes[child].kind == TerminalEquationKind::List
                        && tree.boxes[child].expected_arguments > 1
                })
            {
                children = tree.boxes[tree.first(index).expect("matrix has first child")]
                    .children
                    .iter()
                    .copied();
            }
            for child in children {
                let child = if box_.kind == TerminalEquationKind::Pile
                    && tree.boxes[child].kind == TerminalEquationKind::List
                    && tree.boxes[child].expected_arguments > 1
                    && tree.boxes[child].children.len() == 1
                {
                    tree.boxes[child].children[0]
                } else {
                    child
                };
                render_terminal_equation_box(tree, child, format, limits, writer);
            }
        }
    }

    if let Some(top) = box_.top.as_deref() {
        writer.attach();
        let parent_font = parent
            .map(|parent| tree.boxes[parent].font.terminal())
            .unwrap_or_default();
        writer.word(&render_terminal_font(
            &render_terminal_equation_text(top, format, limits),
            parent_font,
        ));
    }
    if box_.bottom.is_some() {
        writer.attach();
        writer.word("_");
    }
    if delimiter {
        writer.attach();
        let parent_font = parent
            .map(|parent| tree.boxes[parent].font.terminal())
            .unwrap_or_default();
        writer.word(&render_terminal_font(
            &render_terminal_equation_text(box_.right.as_deref().unwrap_or(")"), format, limits),
            parent_font,
        ));
        if let Some(parent) = parent
            && tree.boxes[parent].kind == TerminalEquationKind::Subexpression
            && tree.boxes[parent]
                .children
                .last()
                .is_some_and(|last| *last != index)
        {
            writer.attach();
        }
    }
}

/// Render the retained device eqn tree as the mathematical-markup fragment emitted by
/// mandoc's HTML backend.  It intentionally feeds the existing regression
/// extractor through the native eqn math element; surrounding HTML structure
/// remains the responsibility of the general native HTML renderer.
pub(super) fn render_html_equation(equation: &EquationTerminal, limits: &Limits) -> String {
    let tree = parse_terminal_equation(&equation.tokens);
    if tree.boxes[0].children.is_empty() {
        return String::new();
    }
    let mut output = String::new();
    render_html_equation_box(&tree, 0, limits, &mut output);
    output
}

fn render_html_equation_box(
    tree: &TerminalEquationTree,
    index: usize,
    limits: &Limits,
    output: &mut String,
) {
    let box_ = &tree.boxes[index];
    let post = match box_.position {
        TerminalEquationPosition::To => Some("mover"),
        TerminalEquationPosition::Sup => Some("msup"),
        TerminalEquationPosition::From => Some("munder"),
        TerminalEquationPosition::Sub => Some("msub"),
        TerminalEquationPosition::Over => Some("mfrac"),
        TerminalEquationPosition::Fromto => Some("munderover"),
        TerminalEquationPosition::Subsup => Some("msubsup"),
        TerminalEquationPosition::Sqrt => Some("msqrt"),
        TerminalEquationPosition::None if box_.top.is_some() && box_.bottom.is_some() => {
            Some("munderover")
        }
        TerminalEquationPosition::None if box_.top.is_some() => Some("mover"),
        TerminalEquationPosition::None if box_.bottom.is_some() => Some("munder"),
        TerminalEquationPosition::None
            if box_.kind == TerminalEquationKind::Pile
                && tree.first(index).is_some_and(|child| {
                    tree.boxes[child].kind == TerminalEquationKind::List
                        && tree.boxes[child].expected_arguments > 1
                }) =>
        {
            Some("mtable")
        }
        TerminalEquationPosition::None
            if box_.kind == TerminalEquationKind::List
                && box_.expected_arguments > 1
                && box_.parent.is_some_and(|parent| {
                    tree.boxes[parent].kind == TerminalEquationKind::Pile
                }) =>
        {
            Some("mtd")
        }
        TerminalEquationPosition::None => None,
    };

    if let Some(text) = box_.text.as_deref() {
        render_html_equation_text(text, box_.font, box_.quoted, limits, output);
        return;
    }
    if box_.kind == TerminalEquationKind::Matrix {
        render_html_equation_matrix(tree, index, limits, output);
        return;
    }

    if post == Some("mtd") {
        output.push_str("<mtr><mtd>");
    } else if let Some(post) = post {
        output.push('<');
        output.push_str(post);
        output.push('>');
    } else if box_.left.is_some() || box_.right.is_some() {
        output.push_str("<mfenced");
        if let Some(left) = box_.left.as_deref() {
            output.push_str(" open=\"");
            append_html_math_attribute(left, limits, output);
            output.push('"');
        }
        if let Some(right) = box_.right.as_deref() {
            output.push_str(" close=\"");
            append_html_math_attribute(right, limits, output);
            output.push('"');
        }
        output.push_str("><mrow>");
    } else {
        output.push_str("<mrow>");
    }

    for child in box_.children.iter().copied() {
        render_html_equation_box(tree, child, limits, output);
    }
    if let Some(bottom) = box_.bottom.as_deref() {
        render_html_equation_operator(bottom, limits, output);
    }
    if let Some(top) = box_.top.as_deref() {
        render_html_equation_operator(top, limits, output);
    }

    if post == Some("mtd") {
        output.push_str("</mtd></mtr>");
    } else if let Some(post) = post {
        output.push_str("</");
        output.push_str(post);
        output.push('>');
    } else if box_.left.is_some() || box_.right.is_some() {
        output.push_str("</mrow></mfenced>");
    } else {
        output.push_str("</mrow>");
    }
}

/// Matrix columns arrive in eqn source order, but mathematical markup requires rows. Each
/// ccol/lcol/rcol is represented by a private pile whose direct children are
/// the rows; transpose those bounded child lists without touching public AST
/// equation text.
fn render_html_equation_matrix(
    tree: &TerminalEquationTree,
    index: usize,
    limits: &Limits,
    output: &mut String,
) {
    let Some(scope) = tree.first(index) else {
        return;
    };
    let scope = &tree.boxes[scope];
    if scope.kind != TerminalEquationKind::List || scope.expected_arguments <= 1 {
        render_html_equation_box(
            tree,
            tree.first(index).expect("matrix child exists"),
            limits,
            output,
        );
        return;
    }
    let columns = &scope.children;
    let rows = columns
        .iter()
        .map(|column| tree.boxes[*column].children.len())
        .max()
        .unwrap_or(0);
    if rows == 0 {
        return;
    }
    output.push_str("<mtable>");
    for row in 0..rows {
        output.push_str("<mtr>");
        for column in columns {
            output.push_str("<mtd>");
            if let Some(cell) = tree.boxes[*column].children.get(row).copied() {
                let cell_box = &tree.boxes[cell];
                if cell_box.kind == TerminalEquationKind::List
                    && cell_box
                        .parent
                        .is_some_and(|parent| tree.boxes[parent].kind == TerminalEquationKind::Pile)
                {
                    for child in cell_box.children.iter().copied() {
                        render_html_equation_box(tree, child, limits, output);
                    }
                } else {
                    render_html_equation_box(tree, cell, limits, output);
                }
            }
            output.push_str("</mtd>");
        }
        output.push_str("</mtr>");
    }
    output.push_str("</mtable>");
}

fn render_html_equation_text(
    text: &str,
    font: TerminalEquationFont,
    quoted: bool,
    limits: &Limits,
    output: &mut String,
) {
    let mut visible = render_visible_text(text, RenderFormat::Utf8, limits);
    if quoted {
        visible = visible.replace(' ', "\n");
    }
    let mut characters = visible.chars();
    let first = characters.next();
    let tag = if text.starts_with("\\[") {
        "mo"
    } else if first.is_some_and(|character| character.is_ascii_digit())
        || (first == Some('.')
            && characters
                .next()
                .is_some_and(|character| character.is_ascii_digit()))
    {
        "mn"
    } else if first.is_some_and(|character| !character.is_alphabetic()) {
        if visible.chars().any(char::is_alphanumeric) {
            "mi"
        } else {
            "mo"
        }
    } else {
        "mi"
    };
    let default_font = if tag == "mi" && visible.chars().count() == 1 {
        TerminalEquationFont::Italic
    } else {
        TerminalEquationFont::Roman
    };
    output.push('<');
    output.push_str(tag);
    if font != TerminalEquationFont::None && font != default_font {
        match font {
            TerminalEquationFont::Roman => output.push_str(" fontstyle=\"normal\""),
            TerminalEquationFont::Bold | TerminalEquationFont::Fat => {
                output.push_str(" fontweight=\"bold\"");
            }
            TerminalEquationFont::Italic => output.push_str(" fontstyle=\"italic\""),
            TerminalEquationFont::None => {}
        }
    }
    output.push('>');
    append_html_math_text(&visible, output);
    output.push_str("</");
    output.push_str(tag);
    output.push('>');
}

fn render_html_equation_operator(text: &str, limits: &Limits, output: &mut String) {
    output.push_str("<mo>");
    append_html_math_text(
        &render_visible_text(text, RenderFormat::Utf8, limits),
        output,
    );
    output.push_str("</mo>");
}

fn append_html_math_attribute(text: &str, limits: &Limits, output: &mut String) {
    let visible = render_visible_text(text, RenderFormat::Utf8, limits);
    for character in visible.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '"' => output.push_str("&quot;"),
            _ => output.push(character),
        }
    }
}

fn append_html_math_text(text: &str, output: &mut String) {
    for character in text.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            character if character.is_ascii() => output.push(character),
            character => {
                use std::fmt::Write as _;
                let _ = write!(output, "&#x{:04X};", u32::from(character));
            }
        }
    }
}

/// Render an eqn expression with the terminal device's legacy ASCII names.
///
/// The ordinary text path intentionally turns unknown non-ASCII glyphs into
/// `?`.  Equation boxes carry the authored `\\[*…]` spelling, however, and
/// mandoc's ASCII device preserves the conventional Greek names instead.  Do
/// this before generic escape normalization so UTF-8 remains the catalog
/// glyph while ASCII retains the device's more useful textual form.
pub(super) fn render_terminal_equation_text(
    text: &str,
    format: RenderFormat,
    limits: &Limits,
) -> String {
    if format != RenderFormat::Ascii {
        return render_visible_text(text, format, limits);
    }
    let bytes = text.as_bytes();
    let mut escaped = String::with_capacity(text.len());
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        if bytes.get(cursor..cursor.saturating_add(2)) == Some(b"\\[")
            && let Some(close) = bytes[cursor + 2..].iter().position(|byte| *byte == b']')
        {
            let end = cursor + 2 + close;
            let name = &text[cursor + 2..end];
            if let Some(replacement) = ascii_equation_special_character(name) {
                escaped.push_str(replacement);
            } else {
                escaped.push_str(&text[cursor..=end]);
            }
            cursor = end + 1;
            continue;
        }
        let character = text[cursor..]
            .chars()
            .next()
            .expect("cursor remains within a valid UTF-8 string");
        escaped.push(character);
        cursor += character.len_utf8();
    }
    render_visible_text(&escaped, format, limits)
}

/// The ASCII fallback spellings from mandoc 1.14.6's `chars.c` Greek table.
///
/// These are the entries emitted by eqn's canonical `\\[*…]` lowering.  The
/// rest of the character catalog continues through the normal terminal
/// fallback, which intentionally reports unsupported glyphs as `?`.
fn ascii_equation_special_character(name: &str) -> Option<&'static str> {
    let spelling = match name {
        "*A" => "A",
        "*B" => "B",
        "*G" => "<Gamma>",
        "*D" => "<Delta>",
        "*E" => "E",
        "*Z" => "Z",
        "*Y" => "H",
        "*H" => "<Theta>",
        "*I" => "I",
        "*K" => "K",
        "*L" => "<Lambda>",
        "*M" => "M",
        "*N" => "N",
        "*C" => "<Xi>",
        "*O" => "O",
        "*P" => "<Pi>",
        "*R" => "P",
        "*S" => "<Sigma>",
        "*T" => "T",
        "*U" => "Y",
        "*F" => "<Phi>",
        "*X" => "X",
        "*Q" => "<Psi>",
        "*W" => "<Omega>",
        "*a" => "<alpha>",
        "*b" => "<beta>",
        "*g" => "<gamma>",
        "*d" => "<delta>",
        "*e" | "+e" => "<epsilon>",
        "*z" => "<zeta>",
        "*y" => "<eta>",
        "*h" | "+h" => "<theta>",
        "*i" => "<iota>",
        "*k" => "<kappa>",
        "*l" => "<lambda>",
        "*m" => "<mu>",
        "*n" => "<nu>",
        "*c" => "<xi>",
        "*o" => "o",
        "*p" | "+p" => "<pi>",
        "*r" => "<rho>",
        "*s" | "ts" => "<sigma>",
        "*t" => "<tau>",
        "*u" => "<upsilon>",
        "*f" | "+f" => "<phi>",
        "*x" => "<chi>",
        "*q" => "<psi>",
        "*w" => "<omega>",
        _ => return None,
    };
    Some(spelling)
}

pub(super) fn ascii_terminal_character(character: char) -> char {
    if character.is_ascii() {
        character
    } else if matches!(character, '\u{2010}' | '\u{2011}' | '\u{2212}') {
        '-'
    } else if character == '\u{a0}' {
        ' '
    } else {
        '?'
    }
}
