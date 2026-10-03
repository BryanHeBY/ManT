//! Shared parameter characters and complete-token evidence.

#[derive(Clone, Copy, Default)]
enum WordState {
    #[default]
    Empty,
    Word,
    TrailingSpace,
    Invalid,
}

#[derive(Clone, Copy, Default)]
enum Capitalization {
    #[default]
    Uncased,
    Uppercase,
    Other,
}

#[derive(Clone, Copy, Default)]
pub(super) struct ParameterToken {
    word: WordState,
    starts_with_dash: bool,
    capitalization: Capitalization,
    uppercase_metavariable: bool,
}

pub(super) fn parameter_character(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '_' | '-')
}

impl ParameterToken {
    pub(super) fn from_text(value: &str) -> Self {
        value.chars().fold(Self::default(), Self::observe)
    }

    pub(super) fn is_empty(self) -> bool {
        matches!(self.word, WordState::Empty)
    }

    pub(super) fn observe(mut self, character: char) -> Self {
        // Uppercase is additional evidence, never an exclusive token state.
        // Keep the existing all-uppercase argument-sequence evidence across
        // whitespace; ordinary/styled completion still requires one word.
        self.capitalization = match self.capitalization {
            Capitalization::Other => Capitalization::Other,
            _ if character.is_uppercase() => Capitalization::Uppercase,
            current
                if character.is_ascii_digit()
                    || matches!(character, '_' | '-')
                    || character.is_whitespace() =>
            {
                current
            }
            _ => Capitalization::Other,
        };
        // Restart evidence retains its initial ASCII uppercase requirement.
        // A valid word such as 1A or _SCRIPT still needs independent evidence
        // on its right; validity alone cannot promote an internal --fake.
        self.uppercase_metavariable = if self.is_empty() {
            character.is_ascii_uppercase()
        } else {
            self.uppercase_metavariable
                && (character.is_ascii_uppercase()
                    || character.is_ascii_digit()
                    || matches!(character, '_' | '-')
                    || character.is_whitespace())
        };
        self.word = match self.word {
            WordState::Empty if parameter_character(character) => {
                self.starts_with_dash = character == '-';
                WordState::Word
            }
            WordState::Word if parameter_character(character) => WordState::Word,
            WordState::Word | WordState::TrailingSpace if character.is_whitespace() => {
                WordState::TrailingSpace
            }
            _ => WordState::Invalid,
        };
        self
    }

    pub(super) fn is_word(self) -> bool {
        matches!(self.word, WordState::Word | WordState::TrailingSpace)
    }

    pub(super) fn is_bare(self) -> bool {
        self.is_word() && !self.starts_with_dash
    }

    pub(super) fn is_uppercase(self) -> bool {
        matches!(self.capitalization, Capitalization::Uppercase)
    }

    pub(super) fn is_uppercase_metavariable(self) -> bool {
        self.uppercase_metavariable
    }
}
