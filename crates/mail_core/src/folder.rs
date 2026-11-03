//! Remote folders (IMAP mailboxes) and how BGMail maps them to system roles.

use serde::{Deserialize, Serialize};

/// What a folder is *for*, independent of its server-side name, so the UI can
/// show "Sent" for `[Gmail]/Sent Mail`, `INBOX.Enviados` or `Gesendet`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FolderRole {
    Inbox,
    Sent,
    Drafts,
    Trash,
    Junk,
    Archive,
    All,
    Flagged,
    Important,
    Custom,
}

impl FolderRole {
    /// Stable lowercase key (same as the serde form) for storage columns.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Sent => "sent",
            Self::Drafts => "drafts",
            Self::Trash => "trash",
            Self::Junk => "junk",
            Self::Archive => "archive",
            Self::All => "all",
            Self::Flagged => "flagged",
            Self::Important => "important",
            Self::Custom => "custom",
        }
    }

    /// Inverse of [`FolderRole::as_str`]; `None` for unknown keys.
    pub fn from_str_opt(value: &str) -> Option<Self> {
        Some(match value {
            "inbox" => Self::Inbox,
            "sent" => Self::Sent,
            "drafts" => Self::Drafts,
            "trash" => Self::Trash,
            "junk" => Self::Junk,
            "archive" => Self::Archive,
            "all" => Self::All,
            "flagged" => Self::Flagged,
            "important" => Self::Important,
            "custom" => Self::Custom,
            _ => return None,
        })
    }
}

/// RFC 6154 SPECIAL-USE attributes (plus XLIST's `\Inbox`), case-insensitive.
/// This is authoritative when present; name heuristics are only a fallback.
pub fn role_from_special_use<S: AsRef<str>>(attributes: &[S]) -> Option<FolderRole> {
    attributes
        .iter()
        .find_map(|attr| match attr.as_ref().to_ascii_lowercase().as_str() {
            "\\inbox" => Some(FolderRole::Inbox),
            "\\sent" => Some(FolderRole::Sent),
            "\\drafts" => Some(FolderRole::Drafts),
            "\\trash" => Some(FolderRole::Trash),
            "\\junk" => Some(FolderRole::Junk),
            "\\archive" => Some(FolderRole::Archive),
            "\\all" => Some(FolderRole::All),
            "\\flagged" => Some(FolderRole::Flagged),
            "\\important" => Some(FolderRole::Important),
            _ => None,
        })
}

/// Folded (lowercase, accent-free) well-known names per role. Covers servers
/// without SPECIAL-USE (many cPanel/Dovecot/Exchange setups) in the languages
/// BGMail users are most likely to meet.
const NAME_ROLES: &[(FolderRole, &[&str])] = &[
    (
        FolderRole::Sent,
        &[
            "sent",
            "sent items",
            "sent mail",
            "sent messages",
            "enviados",
            "itens enviados",
            "mensagens enviadas",
            "elementos enviados",
            "gesendet",
            "gesendete elemente",
            "envoyes",
            "elements envoyes",
        ],
    ),
    (
        FolderRole::Drafts,
        &[
            "drafts",
            "draft",
            "rascunhos",
            "borradores",
            "entwurfe",
            "brouillons",
        ],
    ),
    (
        FolderRole::Trash,
        &[
            "trash",
            "deleted items",
            "deleted messages",
            "bin",
            "lixeira",
            "itens excluidos",
            "papelera",
            "elementos eliminados",
            "papierkorb",
            "geloschte elemente",
            "corbeille",
        ],
    ),
    (
        FolderRole::Junk,
        &[
            "junk",
            "spam",
            "junk e-mail",
            "junk email",
            "lixo eletronico",
            "bulk mail",
            "correo no deseado",
            "courrier indesirable",
        ],
    ),
    (
        FolderRole::Archive,
        &[
            "archive", "archives", "arquivo", "arquivos", "archivo", "archiv",
        ],
    ),
];

/// Guesses a role from a folder's name using only its last path segment, so
/// `INBOX.Sent` and `[Gmail]/Sent Mail` both resolve. `INBOX` is matched on the
/// whole name only (a nested `Archive/Inbox` is a user folder). Only at the top, under
/// `INBOX` or under Gmail's own folder: anywhere else it is a folder of the user's that
/// happens to be called that, like `Projects/Trash` (user decision).
pub fn role_from_name(name: &str, delimiter: Option<&str>) -> Option<FolderRole> {
    if name.trim().eq_ignore_ascii_case("inbox") {
        return Some(FolderRole::Inbox);
    }
    if !is_where_system_folders_live(name, delimiter) {
        return None;
    }
    let decoded = decode_imap_utf7(last_segment(name, delimiter));
    let folded = fold(&decoded);
    NAME_ROLES
        .iter()
        .find(|(_, names)| names.contains(&folded.as_str()))
        .map(|(role, _)| *role)
}

/// A mailbox as returned by `LIST`, with the derived role and selectability.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteFolder {
    /// Full server-side name (still modified-UTF-7 encoded), used in commands.
    pub name: String,
    pub delimiter: Option<String>,
    pub attributes: Vec<String>,
    pub role: Option<FolderRole>,
    pub selectable: bool,
}

impl RemoteFolder {
    /// Derives `role` (SPECIAL-USE first, then name) and `selectable`
    /// (`\Noselect` / `\NonExistent` cannot be opened).
    pub fn new(name: impl Into<String>, delimiter: Option<&str>, attributes: Vec<String>) -> Self {
        let name = name.into();
        let role = role_from_special_use(&attributes).or_else(|| role_from_name(&name, delimiter));
        let selectable = !attributes.iter().any(|a| {
            a.eq_ignore_ascii_case("\\Noselect") || a.eq_ignore_ascii_case("\\NonExistent")
        });
        Self {
            name,
            delimiter: delimiter.map(str::to_string),
            attributes,
            role,
            selectable,
        }
    }

    /// Human name: last path segment, modified UTF-7 decoded.
    pub fn display_name(&self) -> String {
        decode_imap_utf7(last_segment(&self.name, self.delimiter.as_deref()))
    }
}

fn is_where_system_folders_live(name: &str, delimiter: Option<&str>) -> bool {
    let Some(delimiter) = delimiter.filter(|d| !d.is_empty()) else {
        return true;
    };
    let mut parents: Vec<&str> = name.split(delimiter).collect();
    parents.pop();
    match parents[..] {
        [] => true,
        [parent] => ["inbox", "[gmail]", "[google mail]"]
            .iter()
            .any(|known| parent.eq_ignore_ascii_case(known)),
        _ => false,
    }
}

fn last_segment<'a>(name: &'a str, delimiter: Option<&str>) -> &'a str {
    match delimiter.filter(|d| !d.is_empty()) {
        Some(d) => name.rsplit(d).next().unwrap_or(name),
        None => name,
    }
}

/// Lowercase + strip Latin accents (enough for folder names; full Unicode
/// normalization would pull in a dependency for no practical gain here).
fn fold(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ñ' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ý' | 'ÿ' => 'y',
            other => other,
        })
        .collect()
}

/// Modified BASE64 alphabet of RFC 3501 §5.1.3 (`,` instead of `/`).
const UTF7_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,";

/// Decodes an IMAP mailbox name (RFC 3501 §5.1.3). Malformed shift sequences
/// are kept verbatim rather than failing: a slightly odd label beats a hidden
/// folder.
pub fn decode_imap_utf7(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let Some(dash) = after.find('-') else {
            out.push_str(&rest[amp..]);
            return out;
        };
        let encoded = &after[..dash];
        if encoded.is_empty() {
            out.push('&');
        } else {
            match decode_utf7_run(encoded) {
                Some(text) => out.push_str(&text),
                None => out.push_str(&rest[amp..amp + dash + 2]),
            }
        }
        rest = &after[dash + 1..];
    }
    out.push_str(rest);
    out
}

fn decode_utf7_run(encoded: &str) -> Option<String> {
    let mut bits: u32 = 0;
    let mut bit_count = 0;
    let mut bytes = Vec::new();
    for b in encoded.bytes() {
        let value = UTF7_ALPHABET.iter().position(|&a| a == b)? as u32;
        bits = (bits << 6) | value;
        bit_count += 6;
        if bit_count >= 8 {
            bit_count -= 8;
            bytes.push((bits >> bit_count) as u8);
            bits &= (1 << bit_count) - 1;
        }
    }
    if bytes.len() % 2 != 0 {
        return None;
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16(&units).ok()
}

/// Encodes a mailbox name for IMAP commands (RFC 3501 §5.1.3).
pub fn encode_imap_utf7(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pending: Vec<u16> = Vec::new();
    for c in input.chars() {
        if (' '..='~').contains(&c) {
            flush_utf7_run(&mut pending, &mut out);
            if c == '&' {
                out.push_str("&-");
            } else {
                out.push(c);
            }
        } else {
            let mut buf = [0u16; 2];
            pending.extend_from_slice(c.encode_utf16(&mut buf));
        }
    }
    flush_utf7_run(&mut pending, &mut out);
    out
}

fn flush_utf7_run(units: &mut Vec<u16>, out: &mut String) {
    if units.is_empty() {
        return;
    }
    let bytes: Vec<u8> = units.iter().flat_map(|u| u.to_be_bytes()).collect();
    out.push('&');
    for chunk in bytes.chunks(3) {
        let n = chunk.len();
        let mut buf = [0u8; 3];
        buf[..n].copy_from_slice(chunk);
        let triple = (u32::from(buf[0]) << 16) | (u32::from(buf[1]) << 8) | u32::from(buf[2]);
        // n bytes need n + 1 sextets; no '=' padding in the modified form.
        for i in 0..=n {
            let sextet = (triple >> (18 - 6 * i)) & 0x3f;
            out.push(UTF7_ALPHABET[sextet as usize] as char);
        }
    }
    out.push('-');
    units.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_ROLES: [FolderRole; 10] = [
        FolderRole::Inbox,
        FolderRole::Sent,
        FolderRole::Drafts,
        FolderRole::Trash,
        FolderRole::Junk,
        FolderRole::Archive,
        FolderRole::All,
        FolderRole::Flagged,
        FolderRole::Important,
        FolderRole::Custom,
    ];

    #[test]
    fn role_str_round_trips_and_matches_serde() {
        for role in ALL_ROLES {
            assert_eq!(FolderRole::from_str_opt(role.as_str()), Some(role));
            assert_eq!(
                serde_json::to_string(&role).unwrap(),
                format!("\"{}\"", role.as_str())
            );
        }
        assert_eq!(FolderRole::from_str_opt("Inbox"), None);
        assert_eq!(FolderRole::from_str_opt("bogus"), None);
    }

    #[test]
    fn special_use_attributes() {
        assert_eq!(
            role_from_special_use(&["\\HasNoChildren", "\\SENT"]),
            Some(FolderRole::Sent)
        );
        for (attr, role) in [
            ("\\Drafts", FolderRole::Drafts),
            ("\\trash", FolderRole::Trash),
            ("\\Junk", FolderRole::Junk),
            ("\\Archive", FolderRole::Archive),
            ("\\All", FolderRole::All),
            ("\\Flagged", FolderRole::Flagged),
            ("\\Important", FolderRole::Important),
            ("\\Inbox", FolderRole::Inbox),
        ] {
            assert_eq!(role_from_special_use(&[attr]), Some(role), "{attr}");
        }
        assert_eq!(role_from_special_use(&["\\HasChildren"]), None);
        assert_eq!(role_from_special_use::<&str>(&[]), None);
    }

    #[test]
    fn name_heuristics_use_last_segment_and_fold_accents() {
        assert_eq!(role_from_name("inbox", None), Some(FolderRole::Inbox));
        assert_eq!(role_from_name("INBOX", Some("/")), Some(FolderRole::Inbox));
        assert_eq!(role_from_name("Work/Inbox", Some("/")), None);
        assert_eq!(role_from_name("Projetos/Lixeira", Some("/")), None);
        assert_eq!(role_from_name("Clientes/Sent", Some("/")), None);
        assert_eq!(role_from_name("INBOX/Trash/Sent", Some("/")), None);
        assert_eq!(
            role_from_name("INBOX/Trash", Some("/")),
            Some(FolderRole::Trash)
        );
        assert_eq!(
            role_from_name("[Google Mail]/Trash", Some("/")),
            Some(FolderRole::Trash)
        );
        assert_eq!(role_from_name("Trash", Some("/")), Some(FolderRole::Trash));
        assert_eq!(
            role_from_name("INBOX.Sent", Some(".")),
            Some(FolderRole::Sent)
        );
        assert_eq!(
            role_from_name("[Gmail]/Sent Mail", Some("/")),
            Some(FolderRole::Sent)
        );
        assert_eq!(
            role_from_name("Itens Excluídos", None),
            Some(FolderRole::Trash)
        );
        assert_eq!(
            role_from_name("Itens Exclu&AO0-dos", None),
            Some(FolderRole::Trash)
        );
        assert_eq!(role_from_name("ENTWÜRFE", None), Some(FolderRole::Drafts));
        assert_eq!(role_from_name("Envoyés", None), Some(FolderRole::Sent));
        assert_eq!(
            role_from_name("Lixo Eletrônico", None),
            Some(FolderRole::Junk)
        );
        assert_eq!(role_from_name("Junk E-mail", None), Some(FolderRole::Junk));
        assert_eq!(role_from_name("Arquivos", None), Some(FolderRole::Archive));
        assert_eq!(role_from_name("Papierkorb", None), Some(FolderRole::Trash));
        assert_eq!(role_from_name("Rascunhos", None), Some(FolderRole::Drafts));
        assert_eq!(role_from_name("Projects", None), None);
    }

    #[test]
    fn remote_folder_derives_role_and_selectable() {
        let sent = RemoteFolder::new("Enviados", Some("/"), vec!["\\Drafts".into()]);
        assert_eq!(sent.role, Some(FolderRole::Drafts), "special-use wins");
        let by_name = RemoteFolder::new("INBOX/Trash", Some("/"), vec![]);
        assert_eq!(by_name.role, Some(FolderRole::Trash));
        assert!(by_name.selectable);
        let parent = RemoteFolder::new("[Gmail]", Some("/"), vec!["\\NoSelect".into()]);
        assert!(!parent.selectable);
        assert_eq!(parent.role, None);
        let gone = RemoteFolder::new("Old", None, vec!["\\NonExistent".into()]);
        assert!(!gone.selectable);
    }

    #[test]
    fn display_name_is_decoded_last_segment() {
        let folder = RemoteFolder::new("Clientes/Jos&AOk-", Some("/"), vec![]);
        assert_eq!(folder.display_name(), "José");
        let flat = RemoteFolder::new("A&-B", None, vec![]);
        assert_eq!(flat.display_name(), "A&B");
    }

    #[test]
    fn utf7_known_vectors() {
        assert_eq!(decode_imap_utf7("&AOk-"), "é");
        assert_eq!(encode_imap_utf7("é"), "&AOk-");
        assert_eq!(decode_imap_utf7("&-"), "&");
        assert_eq!(encode_imap_utf7("&"), "&-");
        // RFC 3501 example.
        assert_eq!(
            decode_imap_utf7("~peter/mail/&U,BTFw-/&ZeVnLIqe-"),
            "~peter/mail/台北/日本語"
        );
        assert_eq!(
            encode_imap_utf7("~peter/mail/台北/日本語"),
            "~peter/mail/&U,BTFw-/&ZeVnLIqe-"
        );
    }

    #[test]
    fn utf7_round_trips_and_tolerates_garbage() {
        for text in ["Itens Excluídos", "R&D 😀 ok", "plain", "", "ção&ã"] {
            assert_eq!(decode_imap_utf7(&encode_imap_utf7(text)), text, "{text}");
        }
        assert_eq!(decode_imap_utf7("broken&AOk"), "broken&AOk");
        assert_eq!(decode_imap_utf7("bad&!!-x"), "bad&!!-x");
    }
}
