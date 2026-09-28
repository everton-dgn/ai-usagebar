//! Display identity obtained from the authenticated account, never a credential.

use std::fmt;

/// Validated display email. Debug output deliberately omits personal data.
#[derive(Clone, PartialEq, Eq)]
pub struct AccountEmail(String);

impl AccountEmail {
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.len() > 254
            || crate::display::sanitize_untrusted_field(value) != value
            || value.chars().any(|c| c.is_control() || c.is_whitespace())
            || value.chars().any(|c| {
                matches!(
                    c,
                    '\u{200b}'..='\u{200d}' | '\u{2060}' | '\u{feff}' | '\u{00ad}'
                )
            })
            || value.contains(['<', '>', '"', '\\'])
        {
            return None;
        }
        let (local, domain) = value.split_once('@')?;
        if local.is_empty() || domain.is_empty() || domain.contains('@') {
            return None;
        }
        Some(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AccountEmail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountEmail([redacted])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_identity_without_leaking_it_to_debug() {
        let email = AccountEmail::parse(" person@example.test ").unwrap();
        assert_eq!(email.as_str(), "person@example.test");
        assert!(!format!("{email:?}").contains("person"));
        for invalid in [
            "",
            "token",
            "@host",
            "user@",
            "a@b@c",
            "a\n@b",
            "<a@b>",
            "a\u{202e}@b",
            "a\u{200b}@b",
            "a\u{2060}@b",
            "a\u{feff}@b",
        ] {
            assert!(AccountEmail::parse(invalid).is_none(), "{invalid:?}");
        }
        assert!(AccountEmail::parse(&format!("{}@b", "a".repeat(254))).is_none());
    }
}
