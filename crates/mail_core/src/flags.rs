//! IMAP system flags (RFC 3501 §2.3.2) as plain booleans.

use serde::{Deserialize, Serialize};

/// The system flags BGMail tracks. Keywords (`$Forwarded`, labels, …) are not
/// modelled yet; they are ignored on input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageFlags {
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
    pub draft: bool,
    pub deleted: bool,
}

impl MessageFlags {
    /// Reads IMAP flag atoms case-insensitively (servers differ in casing).
    pub fn from_imap<I, S>(flags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut out = Self::default();
        for flag in flags {
            match flag.as_ref().trim().to_ascii_lowercase().as_str() {
                "\\seen" => out.seen = true,
                "\\flagged" => out.flagged = true,
                "\\answered" => out.answered = true,
                "\\draft" => out.draft = true,
                "\\deleted" => out.deleted = true,
                _ => {}
            }
        }
        out
    }

    /// Canonical IMAP atoms for the set flags, in a stable order.
    pub fn to_imap(&self) -> Vec<&'static str> {
        [
            (self.seen, "\\Seen"),
            (self.flagged, "\\Flagged"),
            (self.answered, "\\Answered"),
            (self.draft, "\\Draft"),
            (self.deleted, "\\Deleted"),
        ]
        .into_iter()
        .filter_map(|(set, atom)| set.then_some(atom))
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_imap_is_case_insensitive_and_ignores_keywords() {
        let flags = MessageFlags::from_imap(["\\SEEN", "\\flagged", "$Forwarded", "\\Draft"]);
        assert_eq!(
            flags,
            MessageFlags {
                seen: true,
                flagged: true,
                draft: true,
                ..Default::default()
            }
        );
        let owned: Vec<String> = vec!["\\Answered".into(), "\\Deleted".into()];
        let flags = MessageFlags::from_imap(&owned);
        assert!(flags.answered && flags.deleted && !flags.seen);
    }

    #[test]
    fn to_imap_round_trips() {
        let all = MessageFlags {
            seen: true,
            flagged: true,
            answered: true,
            draft: true,
            deleted: true,
        };
        assert_eq!(
            all.to_imap(),
            vec!["\\Seen", "\\Flagged", "\\Answered", "\\Draft", "\\Deleted"]
        );
        assert_eq!(MessageFlags::from_imap(all.to_imap()), all);
        assert!(MessageFlags::default().to_imap().is_empty());
    }
}
