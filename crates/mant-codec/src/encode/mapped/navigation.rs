//! Position zero-width navigation at the accepted syntax receiver.
use super::{BlockSyntax, MappedText, NavigationSite};

impl MappedText {
    pub(in crate::encode) fn syntax_site(mut self, syntax: BlockSyntax) -> Self {
        self.syntax = syntax;
        self.navigation = Some(NavigationSite {
            offset: 0,
            tail: 0,
            block: syntax != BlockSyntax::Phrasing,
            continuation_columns: 0,
            preamble: false,
        });
        self
    }

    pub(in crate::encode) fn tail_grammar(mut self, grammar: BlockSyntax, open_row: bool) -> Self {
        self.tail.grammar = grammar;
        self.tail.open_row = open_row;
        self
    }

    pub(in crate::encode) fn hard_rows(mut self, only_breaks: bool) -> Self {
        self.hard_rows = only_breaks;
        self.tail.hard_rows = only_breaks;
        self
    }

    /// Block navigation has its own syntax line before the actual receiver.
    /// It may join preceding phrasing without a soft-break separator cell.
    pub(in crate::encode) fn has_navigation_preamble(&self) -> bool {
        self.navigation
            .is_some_and(|site| site.preamble && site.offset == 0)
    }

    pub(in crate::encode) fn attach_navigation(&mut self, navigation: &str) {
        self.navigation_at(navigation, false);
    }

    pub(in crate::encode) fn append_navigation(&mut self, navigation: &str) {
        self.navigation_at(navigation, true);
    }

    pub(in crate::encode) fn attach_navigation_text(&mut self, navigation: Self, append: bool) {
        if navigation.text.is_empty() {
            return;
        }
        let offset = self.navigation_at(&navigation.text, append);
        self.owners.extend(
            navigation
                .owners
                .into_iter()
                .map(|(owner, range)| (owner, range.start + offset..range.end + offset)),
        );
    }

    fn navigation_at(&mut self, navigation: &str, append: bool) -> usize {
        if navigation.is_empty() {
            return 0;
        }
        let site = self.navigation.expect("a rendered block's syntax receiver");
        let offset = if append { site.tail } else { site.offset };
        let mut syntax = navigation.to_owned();
        if site.block {
            syntax.push('\n');
            syntax.push_str(&" ".repeat(site.continuation_columns));
        }
        self.insert(offset, &syntax);
        // The receiver precedes the first accepted child content. Keep a
        // parent's navigation outside nested entry ranges, including their
        // generated list prefixes; their own names and payload remain inside.
        for (_, range) in &mut self.owners {
            if range.start < offset && range.end > offset {
                range.start = offset + syntax.len();
            }
        }
        // Later annotations join this same zero-width phrasing, without
        // creating another syntax line or another paragraph boundary.
        self.navigation = Some(NavigationSite {
            tail: site.tail + navigation.len(),
            block: false,
            preamble: site.preamble || site.block,
            ..site
        });
        offset
    }
}
