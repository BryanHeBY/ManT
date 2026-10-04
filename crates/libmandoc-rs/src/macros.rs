//! Typed macro identities at the owned syntax boundary.
//!
//! The pinned `roff.h::roff_tok` and `roff.c::roff_name` define this inventory.
//! Parser sentinels and dispatch placeholders have no named variants. The
//! numeric C token values never enter this public API.

use std::{fmt, ops::Deref};

macro_rules! define_family {
    ($(#[$meta:meta])* pub enum $family:ident { $($variant:ident => $name:literal,)* }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub enum $family {
            $(#[doc = concat!("The `", $name, "` macro or request.")]
              $variant,)*
        }

        impl $family {
            /// Every named token in this family, in pinned registry order.
            pub const ALL: &'static [Self] = &[$(Self::$variant,)*];

            /// Recognize an exact, case-sensitive spelling in this family.
            #[must_use]
            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)*
                    _ => None,
                }
            }

            /// Source spelling without the leading request control character.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)*
                }
            }
        }

        impl AsRef<str> for $family {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl std::fmt::Display for $family {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        #[cfg(feature = "serde")]
        impl serde::Serialize for $family {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where S: serde::Serializer {
                serializer.serialize_str(self.as_str())
            }
        }

        #[cfg(feature = "serde")]
        impl<'de> serde::Deserialize<'de> for $family {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where D: serde::Deserializer<'de> {
                struct NameVisitor;
                impl serde::de::Visitor<'_> for NameVisitor {
                    type Value = $family;
                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        formatter.write_str(concat!("a pinned ", stringify!($family), " name"))
                    }
                    fn visit_str<E>(self, name: &str) -> Result<Self::Value, E>
                    where E: serde::de::Error {
                        $family::from_name(name).ok_or_else(|| E::unknown_variant(name, &[$($name,)*]))
                    }
                }
                deserializer.deserialize_str(NameVisitor)
            }
        }
    };
}

mod man;
mod mdoc;
mod roff;

pub use man::ManMacro;
pub use mdoc::MdocMacro;
pub use roff::RoffMacro;

/// One owned source macro identity, with allocation only for unknown names.
///
/// Known variants preserve the macro family and exact case-sensitive source
/// spelling. [`Self::from_name`] recognizes the canonical identity exposed by
/// the completed AST: `Dd` belongs to mdoc and `TH` to man. The same names also
/// have roff preprocessing entries; use [`RoffMacro::from_name`] and convert
/// that value explicitly when inspecting the preprocessing registry.
///
/// With `serde`, values use the existing source-name string representation.
/// Deserialization follows that canonical AST policy. Consequently an explicit
/// `Roff(Dd)` or `Roff(Th)` serializes to the same spelling as its semantic macro
/// counterpart and deserializes to `Mdoc(Dd)` or `Man(Th)` respectively. Each
/// family enum can be serialized separately to preserve its family.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MacroToken {
    /// A roff(7) formatter or preprocessing request.
    Roff(RoffMacro),
    /// A traditional man(7) macro.
    Man(ManMacro),
    /// A semantic mdoc(7) macro.
    Mdoc(MdocMacro),
    /// An unrecognized macro, preserving its original spelling.
    Unknown(String),
}

impl MacroToken {
    /// Recognize a canonical owned-AST token, preserving unknown names.
    ///
    /// Known names use static enum data and do not allocate. Macro families
    /// are checked in mdoc, man, then roff order to resolve `Dd` and `TH`.
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        Self::known_name(name).unwrap_or_else(|| Self::Unknown(name.to_owned()))
    }

    fn known_name(name: &str) -> Option<Self> {
        // The registry's spelling distinguishes semantic macro families.
        // Select the applicable table before recognizing the full name;
        // known preprocessing-only uppercase names fall back to roff.
        match name.as_bytes() {
            [b'%', ..] | [b'A'..=b'Z', b'a'..=b'z' | b'0'..=b'9', ..] => {
                MdocMacro::from_name(name).map(Self::Mdoc)
            }
            b"in" => Some(Self::Man(ManMacro::In)),
            [b'A'..=b'Z', ..] => ManMacro::from_name(name)
                .map(Self::Man)
                .or_else(|| RoffMacro::from_name(name).map(Self::Roff)),
            _ => RoffMacro::from_name(name).map(Self::Roff),
        }
    }

    /// Source spelling without the leading request control character.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Roff(token) => token.as_str(),
            Self::Man(token) => token.as_str(),
            Self::Mdoc(token) => token.as_str(),
            Self::Unknown(name) => name,
        }
    }

    /// Whether this identity belongs to the pinned named-token inventory.
    #[must_use]
    pub const fn is_known(&self) -> bool {
        !matches!(self, Self::Unknown(_))
    }
}

impl From<RoffMacro> for MacroToken {
    fn from(token: RoffMacro) -> Self {
        Self::Roff(token)
    }
}

impl From<ManMacro> for MacroToken {
    fn from(token: ManMacro) -> Self {
        Self::Man(token)
    }
}

impl From<MdocMacro> for MacroToken {
    fn from(token: MdocMacro) -> Self {
        Self::Mdoc(token)
    }
}

impl From<&str> for MacroToken {
    fn from(name: &str) -> Self {
        Self::from_name(name)
    }
}

impl From<String> for MacroToken {
    fn from(name: String) -> Self {
        Self::known_name(&name).unwrap_or(Self::Unknown(name))
    }
}

impl AsRef<str> for MacroToken {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for MacroToken {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for MacroToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(feature = "serde")]
mod serialization;

#[cfg(test)]
mod tests;
