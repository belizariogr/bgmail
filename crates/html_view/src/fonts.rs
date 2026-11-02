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

/// Lightest face accepted as "black" (ExtraBold is 800, Black 900).
const BLACK_FACE_MIN_WEIGHT: f32 = 850.0;

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
            blitz_dom::set_family_aliases(weight_aliases(&mut collection));
            set_fallback_order(&mut collection, &platform::fallback_order());
            FontContext {
                collection,
                source_cache: SourceCache::new_shared(),
            }
        })
        .clone()
}

/// Installed family names "<Family> Black" whose prefix is exactly an
/// installed family holding a black face ("Arial Black" → "Arial"), aliased
/// to that family at weight 900. Platforms that file the face under its base
/// family (Windows fonts on Linux: "Arial Black" is family "Arial", style
/// Black) would otherwise give the regular face; at 900 the family picks the
/// black one. Where it is a family of its own (macOS) the base family stops
/// at Bold, so an alias would make the text lighter: no alias there.
/// Only names and scanned attributes are used (no font files are read).
pub(crate) fn weight_aliases(collection: &mut Collection) -> Vec<(String, String, f32)> {
    const SUFFIX: &str = " black";
    let names: Vec<String> = collection
        .family_names()
        .filter(|name| {
            name.len() > SUFFIX.len()
                && name
                    .get(name.len() - SUFFIX.len()..)
                    .is_some_and(|end| end.eq_ignore_ascii_case(SUFFIX))
        })
        .map(str::to_string)
        .collect();
    names
        .into_iter()
        .filter_map(|name| {
            let prefix = &name[..name.len() - SUFFIX.len()];
            let id = collection.family_id(prefix)?;
            let info = collection.family(id)?;
            let heavy = has_black_face(info.fonts().iter().map(|font| font.weight().value()));
            let family = info.name().to_string();
            // The exact family, not a fuzzy match.
            (heavy && family.eq_ignore_ascii_case(prefix)).then(|| (name.clone(), family, 900.0))
        })
        .collect()
}

/// Whether a family with faces of these weights can render weight 900.
fn has_black_face(mut weights: impl Iterator<Item = f32>) -> bool {
    weights.any(|weight| weight >= BLACK_FACE_MIN_WEIGHT)
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

/// `font` with its family renamed to `family`, so GPUI (which caches a family's
/// faces when first asked) sees each embedded web font as a family of its own:
/// otherwise the faces of "Open Sans" that load after the first one (often the
/// italic, as Google Fonts lists it first) are never found. `None` when it is
/// not a single-face sfnt.
pub(crate) fn with_family_name(font: &[u8], family: &str) -> Option<Vec<u8>> {
    const HEADER: usize = 12;
    const RECORD: usize = 16;
    let be16 = |at: usize| Some(u16::from_be_bytes(font.get(at..at + 2)?.try_into().ok()?));
    let be32 = |at: usize| Some(u32::from_be_bytes(font.get(at..at + 4)?.try_into().ok()?));
    if !matches!(font.get(..4)?, [0, 1, 0, 0] | b"OTTO" | b"true") {
        return None;
    }
    let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    for ix in 0..usize::from(be16(4)?) {
        let record = HEADER + ix * RECORD;
        let tag: [u8; 4] = font.get(record..record + 4)?.try_into().ok()?;
        let offset = be32(record + 8)? as usize;
        let length = be32(record + 12)? as usize;
        if &tag != b"name" {
            tables.push((tag, font.get(offset..offset.checked_add(length)?)?.to_vec()));
        }
    }
    tables.push((*b"name", name_table(family)));
    tables.sort_by_key(|(tag, _)| *tag);

    let count = u16::try_from(tables.len()).ok()?;
    let log2 = count.ilog2();
    let mut out = font[..4].to_vec();
    out.extend_from_slice(&count.to_be_bytes());
    out.extend_from_slice(&((1u16 << log2) * 16).to_be_bytes());
    out.extend_from_slice(&(log2 as u16).to_be_bytes());
    out.extend_from_slice(&((count - (1u16 << log2)) * 16).to_be_bytes());
    let mut offset = HEADER + tables.len() * RECORD;
    let mut head_offset = None;
    for (tag, data) in &mut tables {
        if tag == b"head" {
            // The adjustment is summed as zero, then patched below.
            data.get_mut(8..12)?.fill(0);
            head_offset = Some(offset);
        }
        out.extend_from_slice(tag.as_slice());
        out.extend_from_slice(&table_checksum(data).to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len().next_multiple_of(4);
    }
    for (_, data) in &tables {
        out.extend_from_slice(data);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    if let Some(head) = head_offset {
        let adjustment = 0xB1B0_AFBAu32.wrapping_sub(table_checksum(&out));
        out[head + 8..head + 12].copy_from_slice(&adjustment.to_be_bytes());
    }
    Some(out)
}

/// A `name` table naming only family, style, full and PostScript names.
fn name_table(family: &str) -> Vec<u8> {
    const WINDOWS: [u16; 3] = [3, 1, 0x409]; // platform, encoding, US English
    let utf16 =
        |text: &str| -> Vec<u8> { text.encode_utf16().flat_map(u16::to_be_bytes).collect() };
    let family_bytes = utf16(family);
    let regular = utf16("Regular");
    let records: [(u16, &[u8]); 4] = [
        (1, &family_bytes),
        (2, &regular),
        (4, &family_bytes),
        (6, &family_bytes),
    ];
    let mut table = Vec::new();
    table.extend_from_slice(&0u16.to_be_bytes());
    table.extend_from_slice(&(records.len() as u16).to_be_bytes());
    table.extend_from_slice(&(6 + records.len() as u16 * 12).to_be_bytes());
    let mut strings = Vec::new();
    for (name_id, text) in records {
        for field in WINDOWS {
            table.extend_from_slice(&field.to_be_bytes());
        }
        table.extend_from_slice(&name_id.to_be_bytes());
        table.extend_from_slice(&(text.len() as u16).to_be_bytes());
        table.extend_from_slice(&(strings.len() as u16).to_be_bytes());
        strings.extend_from_slice(text);
    }
    table.extend_from_slice(&strings);
    table
}

/// OpenType table checksum: big-endian u32 sum, zero padded.
fn table_checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn with_family_name_renames_without_losing_tables() {
        use skrifa::{string::StringId, MetadataProvider, Tag};
        let source = blitz_dom::BULLET_FONT.to_vec();
        let renamed = super::with_family_name(&source, "BGMail Web 1").expect("sfnt");
        let font = skrifa::FontRef::new(&renamed).expect("still a font");
        let family = font
            .localized_strings(StringId::FAMILY_NAME)
            .english_or_first()
            .expect("family")
            .to_string();
        assert_eq!(family, "BGMail Web 1");
        let original = skrifa::FontRef::new(&source).unwrap();
        assert_eq!(font.charmap().map('A'), original.charmap().map('A'));
        for tag in [b"head", b"hhea", b"maxp", b"cmap"] {
            assert!(font.table_data(Tag::new(tag)).is_some());
        }
    }

    #[test]
    fn with_family_name_refuses_what_is_not_a_single_font() {
        assert_eq!(super::with_family_name(b"ttcf....", "x"), None);
        assert_eq!(super::with_family_name(b"", "x"), None);
    }

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

    #[test]
    fn black_faces_are_at_least_850() {
        assert!(has_black_face([400.0, 700.0, 900.0].into_iter()));
        assert!(has_black_face([850.0].into_iter()));
        assert!(!has_black_face([400.0, 700.0, 800.0].into_iter()));
        assert!(!has_black_face(std::iter::empty()));
    }

    #[test]
    fn weight_aliases_need_the_base_family() {
        // No system fonts: no base family, so nothing is aliased.
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        assert!(weight_aliases(&mut collection).is_empty());
    }
}
