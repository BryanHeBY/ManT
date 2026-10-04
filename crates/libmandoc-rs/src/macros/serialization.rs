//! The owned token keeps the existing source-name string wire format.

use super::MacroToken;
use std::fmt;

impl serde::Serialize for MacroToken {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for MacroToken {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct NameVisitor;
        impl serde::de::Visitor<'_> for NameVisitor {
            type Value = MacroToken;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a source macro name")
            }

            fn visit_str<E>(self, name: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(MacroToken::from_name(name))
            }

            fn visit_string<E>(self, name: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(MacroToken::from(name))
            }
        }
        deserializer.deserialize_string(NameVisitor)
    }
}
