use super::tbl::source_line;
use super::{
    DocumentBuilder, EquationTerminal, EquationTerminalToken, LEGACY_EQUATION_TREE_DEPTH_MESSAGE,
    LimitFinding, Limits, NodeId,
};

pub(super) struct ParsedEquation {
    pub(super) expression: String,
    pub(super) terminal: EquationTerminal,
    pub(super) limit: Option<LimitFinding>,
    pub(super) delimiter_changes: Vec<DelimiterChange>,
    pub(super) recursive_definition: bool,
    pub(super) empty_request: Option<Box<str>>,
    pub(super) missing_boxes: Vec<&'static str>,
}

#[derive(Clone, Copy)]
pub(super) enum DelimiterChange {
    Disable,
    Enable((char, char)),
    EnablePrevious,
}

#[allow(clippy::too_many_lines)] // Keeps definition, delimiter, and prefix-budget state in source order.
pub(super) fn parse_equation(
    builder: &DocumentBuilder,
    nodes: &[NodeId],
    limits: &Limits,
) -> ParsedEquation {
    let mut definitions = std::collections::BTreeMap::<String, Vec<EquationToken>>::new();
    let mut tokens = Vec::new();
    let mut source_token_count = 0_usize;
    let mut expansion_steps = 0_usize;
    let mut limit = None;
    let mut delimiter_changes = Vec::new();
    let mut recursive_definition = false;
    let mut empty_request = None;
    for line in nodes.iter().filter_map(|node| source_line(builder, *node)) {
        let mut line = line.text.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((change, remainder)) = parse_delimiter_directive(line) {
            delimiter_changes.push(change);
            line = remainder;
            if line.is_empty() {
                continue;
            }
        }
        if line.strip_prefix("tdefine").is_some() {
            empty_request.get_or_insert_with(|| trailing_empty_request(line).into());
            continue;
        }
        if let Some(name) = parse_undef(line) {
            definitions.remove(name);
            continue;
        }
        if let Some((name, replacement, remainder)) = parse_definition(line) {
            empty_request.get_or_insert_with(|| trailing_empty_request(line).into());
            if !definitions.contains_key(&name)
                && definitions.len() >= limits.max_equation_definitions
            {
                limit = Some(equation_limit(
                    crate::DiagnosticCode::LIMIT_EQUATION_DEFINITIONS,
                    "eqn preprocessing exceeds max_equation_definitions and retained a finite expression prefix",
                ));
                break;
            }
            let replacement = equation_tokens(&replacement);
            recursive_definition |= replacement
                .iter()
                .any(|token| !token.quoted && token.text == name);
            definitions.insert(name, replacement);
            line = remainder;
            if let Some((before, name, after)) = split_inline_undef(line) {
                let raw = equation_tokens(before);
                let expanded = expand_definitions(
                    &raw,
                    &definitions,
                    limits,
                    &mut expansion_steps,
                    &mut recursive_definition,
                )
                .unwrap_or_else(|failure| {
                    limit.get_or_insert(failure.limit);
                    failure.prefix
                });
                tokens.extend(expanded);
                definitions.remove(name);
                line = after;
            }
            if line.is_empty() {
                continue;
            }
        }
        let raw_tokens =
            consume_inline_definition_requests(&equation_tokens(line), &mut definitions);
        let remaining = limits
            .max_equation_tokens
            .saturating_sub(source_token_count);
        let accepted = raw_tokens.len().min(remaining);
        source_token_count = source_token_count.saturating_add(accepted);
        let expanded = match expand_definitions(
            &raw_tokens[..accepted],
            &definitions,
            limits,
            &mut expansion_steps,
            &mut recursive_definition,
        ) {
            Ok(expanded) => expanded,
            Err(failure) => {
                if limit.is_none() {
                    limit = Some(failure.limit);
                }
                failure.prefix
            }
        };
        let remaining = limits.max_equation_tokens.saturating_sub(tokens.len());
        if expanded.len() > remaining {
            tokens.extend(expanded.into_iter().take(remaining));
            if limit.is_none() {
                limit = Some(equation_limit(
                    crate::DiagnosticCode::LIMIT_EQUATION_TOKENS,
                    "eqn definition expansion exceeds max_equation_tokens and retained a finite expression prefix",
                ));
            }
            break;
        }
        tokens.extend(expanded);
        if accepted < raw_tokens.len() {
            limit = Some(equation_limit(
                crate::DiagnosticCode::LIMIT_EQUATION_TOKENS,
                "eqn preprocessing exceeds max_equation_tokens and retained a finite expression prefix",
            ));
            break;
        }
        if limit.is_some() {
            break;
        }
    }
    let (bounded_tokens, depth_truncated) =
        truncate_equation_tokens(&tokens, limits.max_equation_depth);
    if depth_truncated {
        if limit.is_none() {
            limit = Some(display_equation_depth_limit(limits));
        }
        tokens = bounded_tokens;
    }
    // mandoc aborts the complete display once recursive substitution is
    // observed: it does not retain tokens before or after the recursive
    // reference as a partial equation.  Preserve the null `eqn` AST value
    // while still reporting the recoverable input-stack diagnostic.
    let (expression, missing_boxes) = if recursive_definition {
        (String::new(), Vec::new())
    } else {
        normalize_equation_tokens_with_missing_boxes(&tokens)
    };
    ParsedEquation {
        expression,
        terminal: EquationTerminal {
            tokens: tokens
                .iter()
                .map(|token| EquationTerminalToken {
                    text: token.text.clone().into_boxed_str(),
                    quoted: token.quoted,
                })
                .collect(),
        },
        limit,
        delimiter_changes,
        recursive_definition,
        empty_request,
        missing_boxes,
    }
}

fn parse_delimiter_directive(value: &str) -> Option<(DelimiterChange, &str)> {
    let value = value.strip_prefix("delim")?.trim_start();
    if let Some(remainder) = value.strip_prefix("off") {
        return Some((DelimiterChange::Disable, remainder.trim_start()));
    }
    if let Some(remainder) = value.strip_prefix("on") {
        return Some((DelimiterChange::EnablePrevious, remainder.trim_start()));
    }
    let mut characters = value.char_indices();
    let (_, opening) = characters.next()?;
    let (closing_index, closing) = characters.next()?;
    let remainder = &value[closing_index + closing.len_utf8()..];
    Some((
        DelimiterChange::Enable((opening, closing)),
        remainder.trim_start(),
    ))
}

fn parse_definition(line: &str) -> Option<(String, String, &str)> {
    let remainder = line
        .strip_prefix("define")
        .or_else(|| line.strip_prefix("ndefine"))?
        .trim_start();
    let mut parts = remainder.splitn(2, char::is_whitespace);
    let name = parts.next()?.to_owned();
    let replacement = parts.next().unwrap_or_default().trim();
    if name.is_empty() {
        return None;
    }
    let mut characters = replacement.char_indices();
    let Some((_, delimiter)) = characters.next() else {
        return Some((name, String::new(), ""));
    };
    if matches!(delimiter, '\'' | '"' | '/' | '|' | ':' | '!')
        && let Some((closing, _)) = characters.find(|(_, character)| *character == delimiter)
    {
        return Some((
            name,
            replacement[delimiter.len_utf8()..closing].to_owned(),
            replacement[closing + delimiter.len_utf8()..].trim_start(),
        ));
    }
    Some((name, replacement.trim_matches(['\'', '"']).to_owned(), ""))
}

fn parse_undef(line: &str) -> Option<&str> {
    let remainder = line.strip_prefix("undef")?.trim_start();
    remainder.split_whitespace().next()
}

pub(super) fn trailing_empty_request(line: &str) -> String {
    let words = line.split_whitespace().collect::<Vec<_>>();
    let Some(index) = words
        .iter()
        .rposition(|word| matches!(*word, "define" | "undef" | "tdefine"))
    else {
        return String::new();
    };
    let request = words[index];
    match request {
        "define" | "undef" if index + 1 == words.len() => request.to_owned(),
        "define" if index + 2 == words.len() => format!("{request} {}", words[index + 1]),
        "tdefine" if index + 1 >= words.len().saturating_sub(1) => request.to_owned(),
        _ => String::new(),
    }
}

fn consume_inline_definition_requests(
    tokens: &[EquationToken],
    definitions: &mut std::collections::BTreeMap<String, Vec<EquationToken>>,
) -> Vec<EquationToken> {
    let mut output = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if !token.quoted && token.text == "undef" {
            if let Some(name) = tokens.get(index + 1).filter(|name| !name.quoted) {
                definitions.remove(&name.text);
                index += 2;
                continue;
            }
            break;
        }
        if !token.quoted && matches!(token.text.as_str(), "define" | "tdefine") {
            break;
        }
        output.push(token.clone());
        index += 1;
    }
    output
}

fn split_inline_undef(line: &str) -> Option<(&str, &str, &str)> {
    let words = line
        .split_whitespace()
        .map(|word| (word.as_ptr() as usize - line.as_ptr() as usize, word));
    for (offset, word) in words {
        if word != "undef" {
            continue;
        }
        let after_request = &line[offset + word.len()..];
        let name = after_request.split_whitespace().next()?;
        let name_offset = name.as_ptr() as usize - line.as_ptr() as usize;
        return Some((&line[..offset], name, &line[name_offset + name.len()..]));
    }
    None
}

#[derive(Clone, Debug)]
pub(super) struct EquationToken {
    pub(super) text: String,
    pub(super) quoted: bool,
}

impl EquationToken {
    fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            quoted: false,
        }
    }
}

pub(super) fn equation_depth(tokens: &[EquationToken]) -> usize {
    let mut depth = 0_usize;
    let mut maximum = 0_usize;
    for token in tokens {
        match token.text.as_str() {
            "{" => {
                depth = depth.saturating_add(1);
                maximum = maximum.max(depth);
            }
            "}" => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    maximum
}

/// Drop the innermost brace-contained equation content beyond a configured
/// depth while retaining a balanced token sequence that the normalizer can
/// process without recursive stack growth.
fn truncate_equation_tokens(
    tokens: &[EquationToken],
    maximum_depth: usize,
) -> (Vec<EquationToken>, bool) {
    let mut output = Vec::with_capacity(tokens.len().min(maximum_depth.saturating_mul(2)));
    let mut depth = 0_usize;
    let mut discarded_depth = None;
    let mut truncated = false;

    for token in tokens {
        if let Some(start) = discarded_depth {
            match token.text.as_str() {
                "{" => depth = depth.saturating_add(1),
                "}" => {
                    depth = depth.saturating_sub(1);
                    if depth < start {
                        discarded_depth = None;
                    }
                }
                _ => {}
            }
            continue;
        }

        match token.text.as_str() {
            "{" if depth >= maximum_depth => {
                depth = depth.saturating_add(1);
                discarded_depth = Some(depth);
                truncated = true;
            }
            "{" => {
                depth = depth.saturating_add(1);
                output.push(token.clone());
            }
            "}" => {
                depth = depth.saturating_sub(1);
                output.push(token.clone());
            }
            _ => output.push(token.clone()),
        }
    }

    (output, truncated)
}

fn display_equation_depth_limit(limits: &Limits) -> LimitFinding {
    if limits.max_equation_depth == 256 {
        equation_limit(
            crate::DiagnosticCode::LEGACY_EQUATION_TREE_DEPTH_LIMIT,
            LEGACY_EQUATION_TREE_DEPTH_MESSAGE,
        )
    } else {
        equation_limit(
            crate::DiagnosticCode::LIMIT_EQUATION_DEPTH,
            "eqn preprocessing exceeds max_equation_depth and retained a finite expression prefix",
        )
    }
}

enum EquationWork {
    Token(EquationToken),
    CloseDefinition(String),
}

pub(super) struct ExpansionFailure {
    pub(super) limit: LimitFinding,
    pub(super) prefix: Vec<EquationToken>,
}

pub(super) fn expand_definitions(
    tokens: &[EquationToken],
    definitions: &std::collections::BTreeMap<String, Vec<EquationToken>>,
    limits: &Limits,
    expansion_steps: &mut usize,
    recursive_definition: &mut bool,
) -> Result<Vec<EquationToken>, ExpansionFailure> {
    let mut work = tokens
        .iter()
        .rev()
        .cloned()
        .map(EquationWork::Token)
        .collect::<Vec<_>>();
    let mut active = std::collections::BTreeSet::new();
    let mut output = Vec::new();
    while let Some(item) = work.pop() {
        if *expansion_steps >= limits.max_equation_expansion_steps {
            return Err(ExpansionFailure {
                limit: equation_limit(
                    crate::DiagnosticCode::LIMIT_EQUATION_EXPANSION_STEPS,
                    "eqn preprocessing exceeds max_equation_expansion_steps and retained a finite expression prefix",
                ),
                prefix: output,
            });
        }
        *expansion_steps = expansion_steps.saturating_add(1);
        match item {
            EquationWork::CloseDefinition(name) => {
                active.remove(&name);
            }
            EquationWork::Token(token) => {
                if !token.quoted
                    && let Some(replacement) = definitions.get(&token.text)
                {
                    if active.insert(token.text.clone()) {
                        work.push(EquationWork::CloseDefinition(token.text));
                        work.extend(replacement.iter().rev().cloned().map(EquationWork::Token));
                        continue;
                    }
                    *recursive_definition = true;
                    continue;
                }
                if output.len() >= limits.max_equation_tokens {
                    return Err(ExpansionFailure {
                        limit: equation_limit(
                            crate::DiagnosticCode::LIMIT_EQUATION_TOKENS,
                            "eqn definition expansion exceeds max_equation_tokens and retained a finite expression prefix",
                        ),
                        prefix: output,
                    });
                }
                output.push(token);
            }
        }
    }
    Ok(output)
}

pub(super) fn equation_limit(code: &'static str, message: &'static str) -> LimitFinding {
    LimitFinding {
        code,
        message,
        location: None,
    }
}

pub(super) fn equation_tokens(value: &str) -> Vec<EquationToken> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = None;
    for character in value.chars() {
        if let Some(quote) = quoted {
            if character == quote {
                if !current.is_empty() {
                    tokens.push(EquationToken {
                        text: std::mem::take(&mut current),
                        quoted: true,
                    });
                }
                quoted = None;
            } else {
                current.push(character);
            }
            continue;
        }
        match character {
            '\'' | '"' => {
                if !current.is_empty() {
                    tokens.push(EquationToken::plain(std::mem::take(&mut current)));
                }
                quoted = Some(character);
            }
            // eqn(7) treats braces as grammar tokens and ignores `^`/`~` as
            // whitespace. Other punctuation remains in its source text box;
            // the legacy owned-AST projection later renders its unquoted
            // visible infix pieces separately.
            '{' | '}' => {
                if !current.is_empty() {
                    tokens.push(EquationToken::plain(std::mem::take(&mut current)));
                }
                tokens.push(EquationToken::plain(character.to_string()));
            }
            '^' | '~' => {
                if !current.is_empty() {
                    tokens.push(EquationToken::plain(std::mem::take(&mut current)));
                }
            }
            character if character.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(EquationToken::plain(std::mem::take(&mut current)));
                }
            }
            _ => current.push(character),
        }
    }
    if !current.is_empty() {
        tokens.push(EquationToken {
            text: current,
            quoted: quoted.is_some(),
        });
    }
    tokens
}

pub(super) fn normalize_equation_tokens(tokens: &[EquationToken]) -> String {
    EquationNormalizer::new(tokens).normalize()
}

fn normalize_equation_tokens_with_missing_boxes(
    tokens: &[EquationToken],
) -> (String, Vec<&'static str>) {
    EquationNormalizer::new(tokens).normalize_with_missing_boxes()
}

/// Flatten the subset of the eqn box tree that the legacy C shim exposes in
/// `Node::equation`.  This is intentionally not a renderer: font and accent
/// boxes are retained by mandoc's renderers but omitted by its owned AST
/// projection, whereas positions and delimiters remain observable text.
struct EquationNormalizer<'a> {
    tokens: &'a [EquationToken],
    index: usize,
    missing_boxes: Vec<&'static str>,
}

impl<'a> EquationNormalizer<'a> {
    fn new(tokens: &'a [EquationToken]) -> Self {
        Self {
            tokens,
            index: 0,
            missing_boxes: Vec::new(),
        }
    }

    fn normalize(&mut self) -> String {
        self.sequence(None)
    }

    fn normalize_with_missing_boxes(mut self) -> (String, Vec<&'static str>) {
        let expression = self.sequence(None);
        (expression, self.missing_boxes)
    }

    fn sequence(&mut self, terminator: Option<Terminator>) -> String {
        let mut values = Vec::new();
        while let Some(token) = self.peek().cloned() {
            if !token.quoted && terminator.is_some_and(|expected| expected.matches(&token.text)) {
                self.index += 1;
                break;
            }
            if token.quoted {
                values.push(self.atom());
                continue;
            }
            match token.text.as_str() {
                "}" | "right" => {
                    // An unmatched close is diagnosed by the full grammar;
                    // the compatibility projection simply does not render it.
                    self.index += 1;
                    if token.text == "right" {
                        self.index = self
                            .index
                            .saturating_add(usize::from(self.peek().is_some()));
                    }
                }
                "{" => {
                    self.index += 1;
                    let value = self.sequence(Some(Terminator::Brace));
                    if !value.is_empty() {
                        values.push(value);
                    }
                }
                "left" => {
                    self.index += 1;
                    let opening = self.next_text().unwrap_or_default();
                    let content = self.sequence(Some(Terminator::Right));
                    let closing = self.take_closing_delimiter();
                    values.push(format!(
                        "{}{}{}",
                        normalize_left_equation_delimiter(&opening),
                        content,
                        normalize_right_equation_delimiter(&closing)
                    ));
                }
                "sqrt" => {
                    self.index += 1;
                    let value = self.atom();
                    values.push(format!("sqrt({value})"));
                }
                "sub" | "from" => {
                    let paired = if token.text == "sub" { "sup" } else { "to" };
                    self.index += 1;
                    let lower = self.position_atom();
                    let left = values.pop().unwrap_or_default();
                    if self
                        .peek()
                        .is_some_and(|token| !token.quoted && token.text == paired)
                    {
                        self.index += 1;
                        let upper = self.position_atom();
                        values.push(format!("{left} _ {lower} ^ {upper}"));
                    } else {
                        values.push(format!("{left} _ {lower}"));
                    }
                }
                "sup" | "to" | "over" => {
                    let operator = match token.text.as_str() {
                        "sup" | "to" => "^",
                        "over" => "/",
                        _ => unreachable!(),
                    };
                    self.index += 1;
                    let right = if token.text == "over" {
                        self.atom()
                    } else {
                        self.position_atom()
                    };
                    let left = values.pop();
                    if left.is_none() && token.text == "over" {
                        self.missing_boxes.push("over");
                    }
                    let left = left.unwrap_or_default();
                    values.push(format!("{left} {operator} {right}"));
                }
                // These grammar tokens affect layout, font, or decoration
                // only. `above` is a pile separator rather than a fraction;
                // `copy_equation()` intentionally omits accent boxes.
                "above" | "mark" | "lineup" | "dyad" | "vec" | "under" | "bar" | "tilde"
                | "hat" | "dot" | "dotdot" | "roman" | "bold" | "italic" | "fat" | "pile"
                | "lpile" | "rpile" | "cpile" | "ccol" | "lcol" | "rcol" | "matrix" | "define"
                | "ndefine" | "tdefine" | "undef" | "delim" => self.index += 1,
                // Size, global size, global font, and horizontal/vertical
                // movements consume one argument but do not affect the AST's
                // renderer-neutral text.
                "size" | "gsize" | "gfont" | "fwd" | "back" | "down" | "up" => {
                    self.index += 1;
                    self.index = self
                        .index
                        .saturating_add(usize::from(self.peek().is_some()));
                }
                _ => values.push(self.atom()),
            }
        }
        join_equation_terms(&values)
    }

    fn atom(&mut self) -> String {
        let Some(token) = self.peek().cloned() else {
            return String::new();
        };
        if token.quoted {
            self.index += 1;
            return token.text;
        }
        match token.text.as_str() {
            "{" => {
                self.index += 1;
                self.sequence(Some(Terminator::Brace))
            }
            "left" => {
                self.index += 1;
                let opening = self.next_text().unwrap_or_default();
                let content = self.sequence(Some(Terminator::Right));
                let closing = self.take_closing_delimiter();
                format!(
                    "{}{}{}",
                    normalize_left_equation_delimiter(&opening),
                    content,
                    normalize_right_equation_delimiter(&closing)
                )
            }
            "sqrt" => {
                self.index += 1;
                format!("sqrt({})", self.atom())
            }
            // A binary `over` where an operand is required constructs the
            // same empty fraction box that mandoc exposes through its owned
            // AST. The enclosing operator emits the single recovery finding
            // for its missing left box; this nested malformed box is visible
            // only through the stable ` / ` projection.
            "over" => {
                self.index += 1;
                " / ".to_owned()
            }
            // Font boxes are transparent in the legacy owned-AST equation
            // projection: preserve their governed atom but not the layout
            // instruction itself. This matters when a font prefix appears
            // directly as a fraction, subscript, or superscript operand.
            "roman" | "bold" | "italic" | "fat" => {
                self.index += 1;
                self.atom()
            }
            _ => {
                self.index += 1;
                split_compound_equation_text(&token.text)
                    .unwrap_or_else(|| normalize_equation_symbol(&token.text).to_owned())
            }
        }
    }

    /// Position operators require an operand, but a second grammar keyword
    /// occupies that slot while contributing no text (`x sub 1 sup sup`).
    /// Consume that malformed keyword so it cannot re-enter the outer stream
    /// as literal equation content.
    fn position_atom(&mut self) -> String {
        if self.peek().is_some_and(|token| {
            !token.quoted && matches!(token.text.as_str(), "sub" | "from" | "sup" | "to" | "over")
        }) {
            self.index += 1;
            return String::new();
        }
        self.atom()
    }

    fn take_closing_delimiter(&mut self) -> String {
        self.next_text().unwrap_or_default()
    }

    fn next_text(&mut self) -> Option<String> {
        let token = self.peek()?.text.clone();
        self.index += 1;
        Some(token)
    }

    fn peek(&self) -> Option<&EquationToken> {
        self.tokens.get(self.index)
    }
}

#[derive(Clone, Copy)]
enum Terminator {
    Brace,
    Right,
}

impl Terminator {
    fn matches(self, token: &str) -> bool {
        matches!((self, token), (Self::Brace, "}") | (Self::Right, "right"))
    }
}

fn normalize_left_equation_delimiter(value: &str) -> &str {
    match value {
        "ceiling" => "\\[lc]",
        "floor" => "\\[lf]",
        other => other,
    }
}

fn normalize_right_equation_delimiter(value: &str) -> &str {
    match value {
        "ceiling" => "\\[rc]",
        "floor" => "\\[rf]",
        other => other,
    }
}

/// Match the shim's visible projection of an unquoted text box containing
/// simple infix punctuation. The eqn parser retains the source box, but the
/// legacy owned-AST walk publishes its visible operands with separators. Keep
/// multi-character relation operators intact and never apply this to quoted
/// text boxes.
fn split_compound_equation_text(value: &str) -> Option<String> {
    let characters = value.chars().collect::<Vec<_>>();
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut split = false;
    for (index, character) in characters.iter().copied().enumerate() {
        let previous = index.checked_sub(1).and_then(|index| characters.get(index));
        let next = characters.get(index + 1);
        let operator = matches!(character, '+' | '-' | '/')
            && previous
                .is_some_and(|character| !matches!(character, '+' | '-' | '/' | '<' | '>' | '='))
            && next
                .is_some_and(|character| !matches!(character, '+' | '-' | '/' | '<' | '>' | '='));
        if operator {
            if current.is_empty() {
                return None;
            }
            parts.push(std::mem::take(&mut current));
            parts.push(character.to_string());
            split = true;
        } else {
            current.push(character);
        }
    }
    if !split || current.is_empty() {
        return None;
    }
    parts.push(current);
    Some(parts.join(" "))
}

pub(crate) fn normalize_equation_symbol(value: &str) -> &str {
    match value {
        "ldots" => "...",
        "alpha" => "\\[*a]",
        "beta" => "\\[*b]",
        "chi" => "\\[*x]",
        "delta" => "\\[*d]",
        "epsilon" => "\\[*e]",
        "eta" => "\\[*y]",
        "gamma" => "\\[*g]",
        "iota" => "\\[*i]",
        "kappa" => "\\[*k]",
        "lambda" => "\\[*l]",
        "mu" => "\\[*m]",
        "nu" => "\\[*n]",
        "omega" => "\\[*w]",
        "omicron" => "\\[*o]",
        "phi" => "\\[*f]",
        "pi" => "\\[*p]",
        "psi" => "\\[*q]",
        "rho" => "\\[*r]",
        "sigma" => "\\[*s]",
        "tau" => "\\[*t]",
        "theta" => "\\[*h]",
        "upsilon" => "\\[*u]",
        "xi" => "\\[*c]",
        "zeta" => "\\[*z]",
        "DELTA" => "\\[*D]",
        "GAMMA" => "\\[*G]",
        "LAMBDA" => "\\[*L]",
        "OMEGA" => "\\[*W]",
        "PHI" => "\\[*F]",
        "PI" => "\\[*P]",
        "PSI" => "\\[*Q]",
        "SIGMA" => "\\[*S]",
        "THETA" => "\\[*H]",
        "UPSILON" => "\\[*U]",
        "XI" => "\\[*C]",
        "inter" => "\\[ca]",
        "union" => "\\[cu]",
        "prod" => "\\[product]",
        "int" => "\\[integral]",
        "sum" => "\\[sum]",
        "grad" | "del" => "\\[gr]",
        "times" => "\\[mu]",
        "cdot" => "\\[pc]",
        "nothing" => "\\[&]",
        "approx" => "\\[~~]",
        "prime" => "\\[fm]",
        "half" => "\\[12]",
        "partial" => "\\[pd]",
        "inf" => "\\[if]",
        ">>" => "\\[>>]",
        "<<" => "\\[<<]",
        "<-" => "\\[<-]",
        "->" => "\\[->]",
        "+-" => "\\[+-]",
        "!=" => "\\[!=]",
        "==" => "\\[==]",
        "<=" => "\\[<=]",
        ">=" => "\\[>=]",
        "-" => "\\[-]",
        other => other,
    }
}

fn join_equation_terms(tokens: &[String]) -> String {
    let mut output = String::new();
    for token in tokens {
        let close = matches!(token.as_str(), ")" | "]");
        if !output.is_empty() && !close && !output.ends_with(['(', '[']) {
            output.push(' ');
        }
        output.push_str(token);
    }
    output
}
