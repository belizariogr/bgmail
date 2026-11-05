//! Searching one folder: `inbox: some text`. A folder goes by its slug, its name in
//! lowercase, without accents or special characters, and with dashes for spaces, so
//! that it is easy to type: "Minha Pasta!" is `minha-pasta`. A subfolder keeps its levels
//! apart: "ThinkSoft/Pendências" is `thinksoft/pendencias`, or `thinksoft.pendencias`,
//! as servers write it.

use storage::{fold_for_search, FOLDER_LEVEL_SEPARATOR};

/// The slug of one level of a folder's name.
fn level_slug(level: &str) -> String {
    let mut slug = String::new();
    for ch in fold_for_search(level).chars() {
        if ch.is_alphanumeric() {
            slug.push(ch);
        } else if (ch.is_whitespace() || ch == '-') && !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.truncate(slug.trim_end_matches('-').len());
    slug
}

/// The slug of a folder's name; empty when nothing in the name survives.
pub fn folder_slug(name: &str) -> String {
    name.split(FOLDER_LEVEL_SEPARATOR)
        .map(level_slug)
        .filter(|level| !level.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// The slugs a typed scope can stand for: a `.` separates levels, as on some servers, or
/// is one that went from a name like "v1.2".
pub fn typed_slugs(typed: &str) -> [String; 2] {
    [
        folder_slug(&typed.replace('.', &FOLDER_LEVEL_SEPARATOR.to_string())),
        folder_slug(&typed.replace('.', "")),
    ]
}

/// Splits `slug: text`, with `text` trimmed, possibly empty: the colon is followed by a
/// space or by nothing. So `http://a` is not a scope, and whether `re: hello` is depends
/// on there being a folder called that, which the caller knows.
pub fn split_scope(query: &str) -> Option<(&str, &str)> {
    let (slug, text) = query.trim_start().split_once(':')?;
    if slug.is_empty() || slug.contains(char::is_whitespace) {
        return None;
    }
    if !text.is_empty() && !text.starts_with(char::is_whitespace) {
        return None;
    }
    Some((slug, text.trim()))
}

/// What to type in the search field to search the folder called `name`, ready for the
/// text; `None` for a name with no slug.
pub fn scope_prefix(name: &str) -> Option<String> {
    let slug = folder_slug(name);
    (!slug.is_empty()).then(|| format!("{slug}: "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slug_is_easy_to_type() {
        for (name, slug) in [
            ("Inbox", "inbox"),
            ("Minha Pasta", "minha-pasta"),
            ("  Minha   Pasta!  ", "minha-pasta"),
            ("Clientes São Paulo", "clientes-sao-paulo"),
            ("Caixa de Entrada", "caixa-de-entrada"),
            ("Think-Soft 2026", "think-soft-2026"),
            ("A - B", "a-b"),
            ("Salvos/Outros", "salvos/outros"),
            ("ThinkSoft/Pendências", "thinksoft/pendencias"),
            ("A B/C D/E!", "a-b/c-d/e"),
            ("[Gmail]/Saved", "gmail/saved"),
            ("100% (ok)", "100-ok"),
            ("!!!", ""),
        ] {
            assert_eq!(folder_slug(name), slug, "{name}");
        }
    }

    #[test]
    fn a_typed_scope_takes_either_delimiter() {
        let slash = typed_slugs("ThinkSoft/Pendencias");
        assert_eq!(slash[0], "thinksoft/pendencias");
        let dot = typed_slugs("thinksoft.pendencias");
        assert_eq!(dot[0], "thinksoft/pendencias");
        assert_eq!(typed_slugs("v1.2")[1], folder_slug("v1.2"));
        assert_eq!(typed_slugs("Pendências")[0], folder_slug("Pendências"));
        assert_eq!(typed_slugs("my_folder")[0], folder_slug("my_folder"));
        assert_eq!(typed_slugs("Minha-Pasta!")[0], "minha-pasta");
    }

    #[test]
    fn a_scope_is_a_word_a_colon_and_a_space() {
        assert_eq!(split_scope("inbox: boleto"), Some(("inbox", "boleto")));
        assert_eq!(
            split_scope("  minha-pasta:   a b  "),
            Some(("minha-pasta", "a b"))
        );
        assert_eq!(split_scope("inbox: "), Some(("inbox", "")));
        assert_eq!(
            split_scope("thinksoft/pendencias: a"),
            Some(("thinksoft/pendencias", "a"))
        );
        assert_eq!(
            split_scope("thinksoft.pendencias: a"),
            Some(("thinksoft.pendencias", "a"))
        );
        assert_eq!(split_scope("inbox:"), Some(("inbox", "")));
        for query in [
            "boleto",
            "http://a.test",
            "12:30",
            "minha pasta: a",
            ": a",
            "",
        ] {
            assert_eq!(split_scope(query), None, "{query}");
        }
    }

    #[test]
    fn the_prefix_is_ready_for_the_text() {
        let prefix = scope_prefix("Minha Pasta").unwrap();
        assert_eq!(prefix, "minha-pasta: ");
        assert_eq!(split_scope(&prefix), Some(("minha-pasta", "")));
        assert_eq!(scope_prefix("!!!"), None);
    }
}
