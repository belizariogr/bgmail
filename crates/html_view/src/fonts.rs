//! Font selection that matches what browsers do.
//!
//! Fontique expands every CSS generic family into the platform's whole sorted
//! font list, so text missing from the first font falls into whatever font
//! comes next (Windows symbol fonts, CJK fonts with half-width punctuation…).
//! Browsers resolve a generic to **one** font and pick a font for each
//! uncovered character from the platform's character fallback instead. This
//! module gives Blitz a font collection with that policy.

use parley::fontique::{
    Blob, Collection, CollectionOptions, FallbackKey, FamilyId, GenericFamily, Script, ScriptExt,
    SourceCache,
};
use parley::FontContext;
use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

/// Generic families resolved to a single font. `Emoji` is left expanded:
/// Parley appends it only for emoji clusters, where any colour font will do.
const SINGLE_FONT_GENERICS: &[GenericFamily] = &[
    GenericFamily::Serif,
    GenericFamily::SansSerif,
    GenericFamily::Monospace,
    GenericFamily::Cursive,
    GenericFamily::Fantasy,
    GenericFamily::SystemUi,
    GenericFamily::UiSerif,
    GenericFamily::UiSansSerif,
    GenericFamily::UiMonospace,
    GenericFamily::UiRounded,
    GenericFamily::Math,
    GenericFamily::FangSong,
];

/// Scripts without a sample in fontique's table that still key fallbacks.
const EXTRA_FALLBACK_SCRIPTS: &[[u8; 4]] = &[*b"Zyyy", *b"Zinh", *b"Zzzz"];

/// Font context shared by every document: the system font scan runs once per
/// process (it is the slowest part of creating a document). Also installs the
/// generic family names in Blitz.
pub(crate) fn font_context() -> FontContext {
    static SHARED: OnceLock<FontContext> = OnceLock::new();
    SHARED
        .get_or_init(|| {
            let mut collection = Collection::new(CollectionOptions {
                shared: true,
                system_fonts: true,
            });
            // Blitz registers this for list markers when it builds its own context.
            collection.register_fonts(Blob::new(Arc::new(blitz_dom::BULLET_FONT) as _), None);
            let names = generic_family_names(&mut collection, &platform::generic_families());
            blitz_dom::set_generic_family_names(names);
            set_fallback_order(&mut collection, &platform::fallback_order());
            FontContext {
                collection,
                source_cache: SourceCache::new_shared(),
            }
        })
        .clone()
}

/// One family name per generic family: the platform's choice (`preferred`)
/// when the collection has it, else the first of fontique's expanded list.
pub(crate) fn generic_family_names(
    collection: &mut Collection,
    preferred: &[(GenericFamily, String)],
) -> Vec<(GenericFamily, String)> {
    SINGLE_FONT_GENERICS
        .iter()
        .filter_map(|&generic| {
            let id = preferred
                .iter()
                .find(|(g, _)| *g == generic)
                .and_then(|(_, name)| collection.family_id(name))
                .or_else(|| collection.generic_families(generic).next())?;
            Some((generic, collection.family_name(id)?.to_string()))
        })
        .collect()
}

/// Makes `fallback` (family names in the platform's character-fallback order)
/// the fallback list of every script, so each character missing from the
/// author's fonts gets the first of them that has it. An empty order keeps
/// fontique's per-script fallback.
pub(crate) fn set_fallback_order(collection: &mut Collection, fallback: &[String]) {
    let mut seen = HashSet::new();
    let ids: Vec<FamilyId> = fallback
        .iter()
        .filter_map(|name| collection.family_id(name))
        .filter(|id| seen.insert(*id))
        .collect();
    if ids.is_empty() {
        return;
    }
    let scripts = Script::all_samples()
        .iter()
        .map(|(script, _)| *script)
        .chain(
            EXTRA_FALLBACK_SCRIPTS
                .iter()
                .map(|s| Script::from_bytes(*s)),
        );
    for script in scripts {
        collection.set_fallbacks(FallbackKey::new(script, None), ids.iter().copied());
    }
}

/// Fontconfig: Chromium's Linux defaults for the generics, and its character
/// fallback (`FcFontSort` of a family-less pattern, first font covering the
/// character).
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod platform {
    use fontconfig::{Fontconfig, Pattern, UnicodeCoverage, FC_FAMILY};
    use parley::fontique::GenericFamily;
    use std::ffi::CString;

    /// Chromium's default font settings on Linux; fontconfig substitutes
    /// installed equivalents (e.g. Liberation Sans for Arial).
    const BROWSER_GENERICS: &[(GenericFamily, &str)] = &[
        (GenericFamily::Serif, "Times New Roman"),
        (GenericFamily::SansSerif, "Arial"),
        (GenericFamily::Monospace, "monospace"),
        (GenericFamily::Cursive, "Comic Sans MS"),
        (GenericFamily::Fantasy, "Impact"),
        (GenericFamily::SystemUi, "system-ui"),
    ];

    pub fn generic_families() -> Vec<(GenericFamily, String)> {
        let Some(fc) = Fontconfig::new() else {
            return Vec::new();
        };
        BROWSER_GENERICS
            .iter()
            .filter_map(|&(generic, name)| {
                let mut pattern = Pattern::new(&fc).ok()?;
                pattern
                    .add_string(FC_FAMILY, &CString::new(name).ok()?)
                    .ok()?;
                let font = pattern.font_match().ok()?;
                Some((generic, font.get_string(FC_FAMILY).ok()?.to_string()))
            })
            .collect()
    }

    pub fn fallback_order() -> Vec<String> {
        let Some(fc) = Fontconfig::new() else {
            return Vec::new();
        };
        let Ok(mut pattern) = Pattern::new(&fc) else {
            return Vec::new();
        };
        // Trimming drops fonts that add no coverage, which never changes the
        // first font covering a given character.
        let Ok(fonts) = pattern.sort_fonts(UnicodeCoverage::Trim) else {
            return Vec::new();
        };
        fonts
            .iter()
            .filter_map(|font| font.get_string(FC_FAMILY).ok().map(str::to_string))
            .collect()
    }
}

/// Other platforms keep fontique's native generic and per-script fallback.
#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
mod platform {
    use parley::fontique::GenericFamily;

    pub fn generic_families() -> Vec<(GenericFamily, String)> {
        Vec::new()
    }

    pub fn fallback_order() -> Vec<String> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A collection of three bundled fonts (distinct families), no system fonts.
    fn test_collection() -> (Collection, [FamilyId; 3]) {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/");
        let fonts: [Vec<u8>; 3] = [
            blitz_dom::BULLET_FONT.to_vec(),
            std::fs::read(format!("{root}fa-brands-400.ttf")).unwrap(),
            std::fs::read(format!("{root}fa-solid-900.ttf")).unwrap(),
        ];
        let ids =
            fonts.map(|font| collection.register_fonts(Blob::new(Arc::new(font) as _), None)[0].0);
        assert_eq!(ids.iter().collect::<HashSet<_>>().len(), 3);
        (collection, ids)
    }

    fn name(collection: &mut Collection, id: FamilyId) -> String {
        collection.family_name(id).unwrap().to_string()
    }

    #[test]
    fn generic_names_prefer_the_platform_choice_then_the_first_family() {
        let (mut c, [a, b, d]) = test_collection();
        c.set_generic_families(GenericFamily::SansSerif, [a, b].into_iter());
        c.set_generic_families(GenericFamily::Serif, [b, d].into_iter());
        let (a_name, d_name) = (name(&mut c, a), name(&mut c, d));
        let preferred = [
            (GenericFamily::Serif, d_name.clone()),
            (GenericFamily::Monospace, "Not Installed".to_string()),
        ];
        let names = generic_family_names(&mut c, &preferred);
        assert!(names.contains(&(GenericFamily::Serif, d_name)));
        assert!(names.contains(&(GenericFamily::SansSerif, a_name)));
        // No list and no installed preference: no name (keeps Parley's lookup).
        assert!(!names.iter().any(|(g, _)| *g == GenericFamily::Monospace));
        assert!(!names.iter().any(|(g, _)| *g == GenericFamily::Emoji));
    }

    #[test]
    fn fallback_order_applies_to_every_script_deduplicated() {
        let (mut c, [a, b, _]) = test_collection();
        let (a_name, b_name) = (name(&mut c, a), name(&mut c, b));
        let order = [b_name.clone(), "Missing".into(), a_name, b_name];
        set_fallback_order(&mut c, &order);
        for script in [*b"Latn", *b"Hani", *b"Zyyy"] {
            let key = FallbackKey::new(Script::from_bytes(script), None);
            let ids: Vec<_> = c.fallback_families(key).collect();
            assert_eq!(ids, [b, a]);
        }
    }

    #[test]
    fn empty_fallback_order_keeps_platform_fallback() {
        let (mut c, [a, _, _]) = test_collection();
        let key = FallbackKey::new(Script::from_bytes(*b"Latn"), None);
        c.set_fallbacks(key, std::iter::once(a));
        set_fallback_order(&mut c, &[]);
        assert_eq!(c.fallback_families(key).collect::<Vec<_>>(), [a]);
    }
}
