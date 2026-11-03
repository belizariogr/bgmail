use blitz_traits::node_id::NodeId;
use cssparser::ToCss;
use selectors::context::QuirksMode;
use std::sync::atomic::Ordering as Ao;
use std::{
    io::Cursor,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize},
        mpsc::Sender,
    },
};
use style::{
    font_face::{FontFaceSourceFormat, FontFaceSourceFormatKeyword, FontStyleRange, Source},
    media_queries::MediaList,
    properties::PropertyDeclarationBlock,
    servo_arc::Arc as ServoArc,
    shared_lock::SharedRwLock,
    shared_lock::{Locked, SharedRwLockReadGuard},
    stylesheets::{
        AllowImportRules, CssRule, DocumentStyleSheet, ImportRule, Origin, Stylesheet,
        StylesheetInDocument, StylesheetLoader as ServoStylesheetLoader, UrlExtraData,
        import_rule::{ImportLayer, ImportSheet, ImportSupportsCondition},
    },
    values::{CssUrl, SourceLocation},
};

use blitz_traits::net::{AbortSignal, Bytes, NetHandler, NetProvider, Request};
use blitz_traits::shell::ShellProvider;

use url::Url;

use crate::{document::DocumentEvent, util::ImageType};

pub(crate) fn stamped_request(url: Url, signal: Option<&AbortSignal>) -> Request {
    let mut req = Request::get(url);
    if let Some(sig) = signal {
        req = req.signal(sig.clone());
    }
    req
}

/// Carries `@font-face` descriptors from CSS parsing through to font
/// registration so `parley::Collection::register_fonts` can alias the bytes
/// under the `font-family` declared in CSS rather than whatever family name
/// the TTF's own `name` table reports.
///
/// All fields are `Option` because each descriptor is independently optional
/// at the CSS level. Missing fields fall back to the values parley reads
/// from the font's own metadata.
#[derive(Clone, Debug, Default)]
pub struct FontFaceOverrides {
    /// `font-family` descriptor (the alias the rest of the stylesheet uses).
    pub family_name: Option<String>,
    /// `font-weight` descriptor as a single CSS weight (100–900). Stylo
    /// parses this as a range; we record the lower bound, which equals the
    /// upper bound in the common single-value case.
    pub weight: Option<f32>,
    /// `font-style` descriptor mapped to fontique's `FontStyle`.
    pub style: Option<parley::fontique::FontStyle>,
}

#[derive(Clone, Debug)]
pub enum Resource {
    /// BGMail: plus the frames of an animated image.
    Image(
        ImageType,
        u32,
        u32,
        Arc<Vec<u8>>,
        Option<Arc<crate::node::AnimationFrames>>,
    ),
    #[cfg(feature = "svg")]
    Svg(ImageType, crate::node::SvgImageData),
    Css(DocumentStyleSheet),
    /// Stylesheet loaded for an `@import` rule, to be attached to the rule on the document thread
    ImportedCss(ServoArc<Locked<ImportRule>>, ServoArc<Stylesheet>),
    Font(Bytes, FontFaceOverrides),
    /// HTML fetched for an `<iframe>` element's `src`
    DocumentSrc(String),
    None,
}

pub(crate) struct ResourceHandler<T: Send + Sync + 'static> {
    doc_id: usize,
    request_id: usize,
    node_id: Option<NodeId>,
    tx: Sender<DocumentEvent>,
    shell_provider: Arc<dyn ShellProvider>,
    data: T,
}

impl<T: Send + Sync + 'static> ResourceHandler<T> {
    pub(crate) fn new(
        tx: Sender<DocumentEvent>,
        doc_id: usize,
        node_id: Option<NodeId>,
        shell_provider: Arc<dyn ShellProvider>,
        data: T,
    ) -> Self {
        static REQUEST_ID_COUNTER: AtomicUsize = AtomicUsize::new(0);
        Self {
            request_id: REQUEST_ID_COUNTER.fetch_add(1, Ao::Relaxed),
            doc_id,
            node_id,
            tx,
            shell_provider,
            data,
        }
    }

    pub(crate) fn boxed(
        tx: Sender<DocumentEvent>,
        doc_id: usize,
        node_id: Option<NodeId>,
        shell_provider: Arc<dyn ShellProvider>,
        data: T,
    ) -> Box<dyn NetHandler>
    where
        ResourceHandler<T>: NetHandler,
    {
        Box::new(Self::new(tx, doc_id, node_id, shell_provider, data)) as _
    }

    pub(crate) fn request_id(&self) -> usize {
        self.request_id
    }

    fn respond(&self, resolved_url: String, result: Result<Resource, String>) {
        let response = ResourceLoadResponse {
            request_id: self.request_id,
            node_id: self.node_id,
            resolved_url: Some(resolved_url),
            result,
        };
        let _ = self.tx.send(DocumentEvent::ResourceLoad(response));
        self.shell_provider.request_redraw();
    }
}

#[allow(unused)]
pub struct ResourceLoadResponse {
    pub request_id: usize,
    pub node_id: Option<NodeId>,
    pub resolved_url: Option<String>,
    pub result: Result<Resource, String>,
}

pub struct StylesheetHandler {
    pub source_url: Url,
    pub guard: SharedRwLock,
    pub net_provider: Arc<dyn NetProvider>,
    pub abort_signal: Option<AbortSignal>,
}

impl NetHandler for ResourceHandler<StylesheetHandler> {
    fn error(self: Box<Self>, url: String, error: String) {
        self.respond(url, Err(error));
    }

    fn bytes(self: Box<Self>, resolved_url: String, bytes: Bytes) {
        let Ok(css) = std::str::from_utf8(&bytes) else {
            return self.respond(resolved_url, Err(String::from("Invalid UTF8")));
        };

        // NOTE(Nico): I don't *think* external stylesheets should have HTML entities escaped
        // let escaped_css = html_escape::decode_html_entities(css);

        let sheet = Stylesheet::from_str(
            css,
            self.data.source_url.clone().into(),
            Origin::Author,
            ServoArc::new(self.data.guard.wrap(MediaList::empty())),
            self.data.guard.clone(),
            Some(&StylesheetLoader {
                tx: self.tx.clone(),
                doc_id: self.doc_id,
                net_provider: self.data.net_provider.clone(),
                shell_provider: self.shell_provider.clone(),
                abort_signal: self.data.abort_signal.clone(),
                import_depth: 0,
                import_budget: new_import_budget(),
            }),
            None, // error_reporter
            QuirksMode::NoQuirks,
            AllowImportRules::Yes,
        );
        keep_only_paint_in_state_rules(&sheet, &self.data.guard);

        self.respond(
            resolved_url,
            Ok(Resource::Css(DocumentStyleSheet(ServoArc::new(sheet)))),
        );
    }
}

/// Maximum depth of nested `@import` rules. Imports nested deeper than this
/// are refused, preventing unbounded recursion (e.g. a stylesheet whose
/// resolved `@import` URL yields another stylesheet importing in turn, with
/// the URL growing geometrically at each level).
const MAX_IMPORT_DEPTH: usize = 16;

/// BGMail: maximum `@import`s of a stylesheet and all it imports. The depth alone lets a
/// `data:` sheet importing itself through its fragment cost `k^16` parses in a message.
const MAX_IMPORTS: usize = 64;

pub(crate) fn new_import_budget() -> Arc<AtomicUsize> {
    Arc::new(AtomicUsize::new(MAX_IMPORTS))
}

#[derive(Clone)]
pub(crate) struct StylesheetLoader {
    pub(crate) tx: Sender<DocumentEvent>,
    pub(crate) doc_id: usize,
    pub(crate) net_provider: Arc<dyn NetProvider>,
    pub(crate) shell_provider: Arc<dyn ShellProvider>,
    pub(crate) abort_signal: Option<AbortSignal>,
    /// Depth of `@import` nesting for the stylesheet this loader is parsing
    /// (0 for a top-level stylesheet)
    pub(crate) import_depth: usize,
    /// BGMail: imports left; see [`MAX_IMPORTS`].
    pub(crate) import_budget: Arc<AtomicUsize>,
}
impl ServoStylesheetLoader for StylesheetLoader {
    fn request_stylesheet(
        &self,
        url: CssUrl,
        location: SourceLocation,
        lock: &SharedRwLock,
        media: ServoArc<Locked<MediaList>>,
        supports: Option<ImportSupportsCondition>,
        layer: ImportLayer,
    ) -> ServoArc<Locked<ImportRule>> {
        // BGMail: a URL that doesn't resolve (`@import url()`) is refused too, not a panic:
        // messages are full of them, once stripped of what they'd load.
        let spent = || {
            self.import_budget
                .fetch_update(Ao::AcqRel, Ao::Acquire, |left| left.checked_sub(1))
                .is_err()
        };
        if self.import_depth >= MAX_IMPORT_DEPTH
            || !supports.as_ref().is_none_or(|s| s.enabled)
            || url.url().is_none()
            || spent()
        {
            return ServoArc::new(lock.wrap(ImportRule {
                url,
                stylesheet: ImportSheet::new_refused(),
                supports,
                layer,
                source_location: location,
            }));
        }

        let import = ImportRule {
            url,
            stylesheet: ImportSheet::new_pending(),
            supports,
            layer,
            source_location: location,
        };

        let url = import.url.url().expect("checked above").clone();
        let import = ServoArc::new(lock.wrap(import));
        self.net_provider.fetch(
            self.doc_id,
            stamped_request(url.as_ref().clone(), self.abort_signal.as_ref()),
            ResourceHandler::boxed(
                self.tx.clone(),
                self.doc_id,
                None, // node_id
                self.shell_provider.clone(),
                NestedStylesheetHandler {
                    url: url.clone(),
                    loader: StylesheetLoader {
                        import_depth: self.import_depth + 1,
                        ..self.clone()
                    },
                    lock: lock.clone(),
                    media,
                    import_rule: import.clone(),
                    net_provider: self.net_provider.clone(),
                },
            ),
        );

        import
    }
}

struct NestedStylesheetHandler {
    loader: StylesheetLoader,
    lock: SharedRwLock,
    url: ServoArc<Url>,
    media: ServoArc<Locked<MediaList>>,
    import_rule: ServoArc<Locked<ImportRule>>,
    net_provider: Arc<dyn NetProvider>,
}

impl NetHandler for ResourceHandler<NestedStylesheetHandler> {
    fn bytes(self: Box<Self>, resolved_url: String, bytes: Bytes) {
        let Ok(css) = std::str::from_utf8(&bytes) else {
            return self.respond(resolved_url, Err(String::from("Invalid UTF8")));
        };

        // NOTE(Nico): I don't *think* external stylesheets should have HTML entities escaped
        // let escaped_css = html_escape::decode_html_entities(css);

        let sheet = ServoArc::new(Stylesheet::from_str(
            css,
            UrlExtraData(self.data.url.clone()),
            Origin::Author,
            self.data.media.clone(),
            self.data.lock.clone(),
            Some(&self.data.loader),
            None, // error_reporter
            QuirksMode::NoQuirks,
            AllowImportRules::Yes,
        ));
        keep_only_paint_in_state_rules(&sheet, &self.data.lock);

        // Fetch @font-face fonts
        fetch_font_face(
            self.tx.clone(),
            self.doc_id,
            self.node_id,
            &sheet,
            &self.data.net_provider,
            &self.shell_provider,
            &self.data.lock.read(),
            self.data.loader.abort_signal.as_ref(),
        );

        let import_rule = self.data.import_rule.clone();
        self.respond(resolved_url, Ok(Resource::ImportedCss(import_rule, sheet)))
    }
}

struct FontFaceHandler {
    format: FontFaceSourceFormatKeyword,
    overrides: FontFaceOverrides,
}
impl NetHandler for ResourceHandler<FontFaceHandler> {
    fn bytes(mut self: Box<Self>, resolved_url: String, bytes: Bytes) {
        let result = self.data.parse(bytes);
        self.respond(resolved_url, result)
    }
}
/// BGMail: most bytes a compressed web font may expand to (WOFF2's own limit), and most a
/// document's web fonts may take.
const MAX_FONT_BYTES: u64 = 128 << 20;
pub(crate) const MAX_DOCUMENT_FONT_BYTES: u64 = 512 << 20;

/// BGMail: the total original length of a WOFF1 file's tables, from its directory.
#[cfg(feature = "woff")]
fn woff1_table_bytes(bytes: &[u8]) -> Option<u64> {
    const HEADER: usize = 44;
    const ENTRY: usize = 20;
    let tables = u16::from_be_bytes(bytes.get(12..14)?.try_into().ok()?) as usize;
    (0..tables).try_fold(0u64, |total, table| {
        // tag, offset, compLength, origLength, origChecksum
        let at = HEADER + table * ENTRY + 12;
        let length = u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?);
        Some(total + length as u64)
    })
}

impl FontFaceHandler {
    fn parse(&mut self, bytes: Bytes) -> Result<Resource, String> {
        if self.format == FontFaceSourceFormatKeyword::None && bytes.len() >= 4 {
            self.format = match &bytes.as_ref()[0..4] {
                // WOFF (v1) files begin with 0x774F4646 ('wOFF' in ascii)
                // See: <https://w3c.github.io/woff/woff1/spec/Overview.html#WOFFHeader>
                #[cfg(feature = "woff")]
                b"wOFF" => FontFaceSourceFormatKeyword::Woff,
                // WOFF2 files begin with 0x774F4632 ('wOF2' in ascii)
                // See: <https://w3c.github.io/woff/woff2/#woff20Header>
                #[cfg(feature = "woff")]
                b"wOF2" => FontFaceSourceFormatKeyword::Woff2,
                // Opentype fonts with CFF data begin with 0x4F54544F ('OTTO' in ascii)
                // See: <https://learn.microsoft.com/en-us/typography/opentype/spec/otff#organization-of-an-opentype-font>
                b"OTTO" => FontFaceSourceFormatKeyword::Opentype,
                // Opentype fonts truetype outlines begin with 0x00010000
                // See: <https://learn.microsoft.com/en-us/typography/opentype/spec/otff#organization-of-an-opentype-font>
                &[0x00, 0x01, 0x00, 0x00] => FontFaceSourceFormatKeyword::Truetype,
                // Truetype fonts begin with 0x74727565 ('true' in ascii)
                // See: <https://developer.apple.com/fonts/TrueType-Reference-Manual/RM06/Chap6.html#ScalerTypeNote>
                b"true" => FontFaceSourceFormatKeyword::Truetype,
                _ => FontFaceSourceFormatKeyword::None,
            }
        }

        // Satisfy rustc's mutability linting with woff feature both enabled/disabled
        #[cfg(feature = "woff")]
        let mut bytes = bytes;

        match self.format {
            #[cfg(feature = "woff")]
            FontFaceSourceFormatKeyword::Woff => {
                #[cfg(feature = "tracing")]
                tracing::info!("Decompressing woff1 font");

                // BGMail: table entries may overlap, so a small file can claim gigabytes.
                if woff1_table_bytes(&bytes).is_none_or(|total| total > MAX_FONT_BYTES) {
                    return Ok(Resource::None);
                }

                // Use wuff crate to decompress font
                let decompressed = wuff::decompress_woff1(&bytes).ok();

                if let Some(decompressed) = decompressed {
                    bytes = Bytes::from(decompressed);
                } else {
                    #[cfg(feature = "tracing")]
                    tracing::warn!("Failed to decompress woff1 font");
                }
            }
            #[cfg(feature = "woff")]
            FontFaceSourceFormatKeyword::Woff2 => {
                #[cfg(feature = "tracing")]
                tracing::info!("Decompressing woff2 font");

                // Use wuff crate to decompress font
                let decompressed = wuff::decompress_woff2(&bytes).ok();

                if let Some(decompressed) = decompressed {
                    bytes = Bytes::from(decompressed);
                } else {
                    #[cfg(feature = "tracing")]
                    tracing::warn!("Failed to decompress woff2 font");
                }
            }
            FontFaceSourceFormatKeyword::None => {
                // Should this be an error?
                return Ok(Resource::None);
            }
            _ => {}
        }

        Ok(Resource::Font(bytes, std::mem::take(&mut self.overrides)))
    }
}

/// Fetch the fonts of all `@font-face` rules in `sheet`
#[allow(clippy::too_many_arguments)]
pub(crate) fn fetch_font_face(
    tx: Sender<DocumentEvent>,
    doc_id: usize,
    node_id: Option<NodeId>,
    sheet: &Stylesheet,
    network_provider: &Arc<dyn NetProvider>,
    shell_provider: &Arc<dyn ShellProvider>,
    read_guard: &SharedRwLockReadGuard,
    abort_signal: Option<&AbortSignal>,
) {
    fetch_font_face_rules(
        sheet.contents(read_guard).rules(read_guard).iter(),
        tx,
        doc_id,
        node_id,
        network_provider,
        shell_provider,
        read_guard,
        abort_signal,
    )
}

/// Fetch the fonts of the `@font-face` rules among `rules`
#[allow(clippy::too_many_arguments)]
pub(crate) fn fetch_font_face_rules<'a>(
    rules: impl Iterator<Item = &'a CssRule>,
    tx: Sender<DocumentEvent>,
    doc_id: usize,
    node_id: Option<NodeId>,
    network_provider: &Arc<dyn NetProvider>,
    shell_provider: &Arc<dyn ShellProvider>,
    read_guard: &SharedRwLockReadGuard,
    abort_signal: Option<&AbortSignal>,
) {
    rules
        .filter_map(|rule| match rule {
            CssRule::FontFace(font_face) => {
                let descriptor = &font_face.read_with(read_guard).descriptors;
                let family = descriptor.font_family.as_ref()?;
                let src = descriptor.src.as_ref()?;
                // Capture the @font-face descriptors so parley can register
                // the font under the CSS-declared family name (and weight /
                // style) rather than whatever metadata the TTF reports.
                let overrides = FontFaceOverrides {
                    family_name: Some(family.name.to_string()),
                    weight: descriptor
                        .font_weight
                        .as_ref()
                        .and_then(|range| range.0.compute().map(|w| w.value())),
                    style: descriptor.font_style.as_ref().map(stylo_to_fontique_style),
                };
                Some((src, overrides))
            }
            _ => None,
        })
        .for_each(|(source_list, overrides)| {
            // Find the first font source in the source list that specifies a font of a type
            // that we support.
            let preferred_source = source_list
                .0
                .iter()
                .filter_map(|source| match source {
                    Source::Url(url_source) => Some(url_source),
                    // TODO: support local fonts in @font-face
                    Source::Local(_) => None,
                })
                .find_map(|url_source| {
                    let mut format = match &url_source.format_hint {
                        Some(FontFaceSourceFormat::Keyword(fmt)) => *fmt,
                        Some(FontFaceSourceFormat::String(str)) => match str.as_str() {
                            "woff2" => FontFaceSourceFormatKeyword::Woff2,
                            "ttf" => FontFaceSourceFormatKeyword::Truetype,
                            "otf" => FontFaceSourceFormatKeyword::Opentype,
                            _ => FontFaceSourceFormatKeyword::None,
                        },
                        _ => FontFaceSourceFormatKeyword::None,
                    };
                    // A url with no extension, such as a `data:` url, is
                    // sniffed from its bytes once fetched.
                    if format == FontFaceSourceFormatKeyword::None
                        && let Some((_, end)) = url_source.url.as_str().rsplit_once('.')
                    {
                        format = match end {
                            "woff2" => FontFaceSourceFormatKeyword::Woff2,
                            "woff" => FontFaceSourceFormatKeyword::Woff,
                            "ttf" => FontFaceSourceFormatKeyword::Truetype,
                            "otf" => FontFaceSourceFormatKeyword::Opentype,
                            "svg" => FontFaceSourceFormatKeyword::Svg,
                            "eot" => FontFaceSourceFormatKeyword::EmbeddedOpentype,
                            _ => FontFaceSourceFormatKeyword::None,
                        }
                    }

                    if matches!(
                        format,
                        FontFaceSourceFormatKeyword::Svg
                            | FontFaceSourceFormatKeyword::EmbeddedOpentype
                    ) {
                        #[cfg(feature = "tracing")]
                        tracing::warn!("Skipping unsupported font of type {:?}", format);
                        return None;
                    }

                    #[cfg(not(feature = "woff"))]
                    if matches!(
                        format,
                        FontFaceSourceFormatKeyword::Woff | FontFaceSourceFormatKeyword::Woff2
                    ) {
                        #[cfg(feature = "tracing")]
                        tracing::warn!("Skipping unsupported font of type {:?}", format);
                        return None;
                    }

                    // A relative url with no base url to resolve against
                    // yields None; skip the source instead of panicking
                    let Some(url) = url_source.url.url() else {
                        #[cfg(feature = "tracing")]
                        tracing::warn!("Skipping @font-face source with unresolvable url");
                        return None;
                    };
                    let url = url.as_ref().clone();
                    Some((url, format))
                });

            if let Some((url, format)) = preferred_source {
                network_provider.fetch(
                    doc_id,
                    stamped_request(url, abort_signal),
                    ResourceHandler::boxed(
                        tx.clone(),
                        doc_id,
                        node_id,
                        shell_provider.clone(),
                        FontFaceHandler { format, overrides },
                    ),
                );
            }
        })
}

/// Translate stylo's `@font-face` `font-style` descriptor into the fontique
/// `FontStyle` enum used by parley. Stylo encodes Italic and Oblique-with-
/// angle distinctly; CSS's bare `normal` is parsed as `Oblique(0deg, 0deg)`
/// by stylo (see the `FontStyle::parse` impl in stylo's `font_face.rs`), so
/// that pattern is treated as `Normal` here.
fn stylo_to_fontique_style(style: &FontStyleRange) -> parley::fontique::FontStyle {
    use parley::fontique::FontStyle as Fq;
    match style {
        FontStyleRange::Italic => Fq::Italic,
        FontStyleRange::Oblique(min, max) => {
            let angle = min.degrees();
            // Stylo emits `Oblique(0deg, 0deg)` for the literal CSS `normal`
            // keyword. Map that back to `Normal` so parley's font matching
            // doesn't misclassify upright fonts.
            if angle.is_none_or(|a| a == 0.0) && max.degrees().is_none_or(|a| a == 0.0) {
                Fq::Normal
            } else {
                Fq::Oblique(angle)
            }
        }
    }
}

/// BGMail: most frames kept of an animated GIF, and most decoded bytes for
/// all of them; past either the image stays still (its first frame).
const MAX_ANIMATION_FRAMES: usize = 400;
const MAX_ANIMATION_BYTES: usize = 64 << 20;

/// BGMail: most decoded bytes of one still image (64 megapixels), and of all the images
/// of a document. A few hundred bytes of GIF or PNG can declare a huge canvas, and the
/// decoder allocates what the header declares.
const MAX_IMAGE_BYTES: u64 = 256 << 20;
const MAX_DOCUMENT_IMAGE_BYTES: u64 = 2 << 30;

/// BGMail: what an image takes as RGBA, if within [`MAX_IMAGE_BYTES`].
fn rgba_bytes(width: u32, height: u32) -> Result<u64, String> {
    let bytes = width as u64 * height as u64 * 4;
    if bytes > MAX_IMAGE_BYTES {
        return Err("the image takes too much memory".to_string());
    }
    Ok(bytes)
}

pub(crate) fn new_image_budget() -> Arc<AtomicU64> {
    Arc::new(AtomicU64::new(MAX_DOCUMENT_IMAGE_BYTES))
}

/// BGMail: the frames of an animated GIF (more than one frame), composited
/// to full canvases by the decoder, as (width, height, frames). Delays under
/// 20ms play at 100ms, like browsers.
pub(crate) fn decode_gif_animation(
    bytes: &[u8],
) -> Option<(u32, u32, crate::node::AnimationFrames)> {
    use image::{AnimationDecoder, ImageDecoder};
    if !bytes.starts_with(b"GIF8") {
        return None;
    }
    let mut decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).ok()?;
    // BGMail: `GifDecoder::new` sets no limits, and the canvas is allocated before the
    // first frame reaches the check below. Twice the budget: the decoder keeps a canvas
    // and a frame buffer besides the frame it returns.
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(2 * MAX_ANIMATION_BYTES as u64);
    decoder.set_limits(limits).ok()?;
    let mut frames = Vec::new();
    let mut size = None;
    let mut total = 0usize;
    for frame in decoder.into_frames() {
        let frame = frame.ok()?;
        let (numer, denom) = frame.delay().numer_denom_ms();
        let delay = if denom == 0 { 0 } else { numer / denom };
        let delay = if delay < 20 { 100 } else { delay };
        let buffer = frame.into_buffer();
        size.get_or_insert((buffer.width(), buffer.height()));
        total += buffer.as_raw().len();
        if frames.len() >= MAX_ANIMATION_FRAMES || total > MAX_ANIMATION_BYTES {
            return None;
        }
        frames.push((
            linebender_resource_handle::Blob::new(Arc::new(buffer.into_raw())),
            delay,
        ));
    }
    let (width, height) = size?;
    (frames.len() > 1).then_some((width, height, crate::node::AnimationFrames { frames }))
}

/// BGMail: selector pieces whose match changes with what the user does.
const STATE_PSEUDO_CLASSES: &[&str] = &[":hover", ":active", ":focus", ":checked", ":target"];

/// BGMail: whether a declaration in a rule matching on [`STATE_PSEUDO_CLASSES`] only changes
/// how boxes look. As Gmail keeps `:hover` for colors but strips what generates content.
fn is_paint_property(name: &str) -> bool {
    matches!(
        name,
        "color" | "opacity" | "cursor" | "box-shadow" | "text-shadow"
    ) || name.starts_with("background-")
        || name.starts_with("text-decoration-")
        || name.ends_with("-color")
}

/// BGMail: reduces rules that apply on hover, focus and the like to paint properties, so no
/// box is created or freed by moving the pointer. That path holds most of the renderer's
/// crashes on hostile CSS, and past the first frame nothing protects the user from it.
/// Mail keeps working: hover effects are nearly always colors, and a message must also
/// read without them, as Outlook has none.
pub(crate) fn keep_only_paint_in_state_rules(sheet: &Stylesheet, lock: &SharedRwLock) {
    let mut blocks = Vec::new();
    {
        let guard = lock.read();
        collect_state_blocks(
            sheet.contents(&guard).rules(&guard).iter(),
            false,
            &guard,
            &mut blocks,
        );
    }
    let mut guard = lock.write();
    for block in blocks {
        let block = block.write_with(&mut guard);
        let mut kept = PropertyDeclarationBlock::new();
        for (declaration, importance) in block.declaration_importance_iter() {
            if is_paint_property(&declaration.id().name()) {
                kept.push(declaration.clone(), importance);
            }
        }
        *block = kept;
    }
}

fn collect_state_blocks<'a>(
    rules: impl Iterator<Item = &'a CssRule>,
    in_state_rule: bool,
    guard: &'a SharedRwLockReadGuard,
    blocks: &mut Vec<ServoArc<Locked<PropertyDeclarationBlock>>>,
) {
    for rule in rules {
        let (nested, in_state_rule) = match rule {
            CssRule::Style(rule) => {
                let rule = rule.read_with(guard);
                let selectors = rule.selectors.to_css_string();
                let in_state_rule = in_state_rule
                    || STATE_PSEUDO_CLASSES
                        .iter()
                        .any(|state| selectors.contains(state));
                if in_state_rule {
                    blocks.push(rule.block.clone());
                }
                (rule.rules.as_ref(), in_state_rule)
            }
            CssRule::NestedDeclarations(rule) => {
                if in_state_rule {
                    blocks.push(rule.read_with(guard).block.clone());
                }
                (None, in_state_rule)
            }
            CssRule::Media(rule) => (Some(&rule.rules), in_state_rule),
            CssRule::Supports(rule) => (Some(&rule.rules), in_state_rule),
            CssRule::Container(rule) => (Some(&rule.rules), in_state_rule),
            CssRule::LayerBlock(rule) => (Some(&rule.rules), in_state_rule),
            CssRule::Document(rule) => (Some(&rule.rules), in_state_rule),
            CssRule::StartingStyle(rule) => (Some(&rule.rules), in_state_rule),
            // Its bounds can match on state too, and nothing in mail needs it.
            CssRule::Scope(rule) => (Some(&rule.rules), true),
            _ => (None, in_state_rule),
        };
        if let Some(nested) = nested {
            collect_state_blocks(
                nested.read_with(guard).0.iter(),
                in_state_rule,
                guard,
                blocks,
            );
        }
    }
}

/// Handles HTML fetched for an `<iframe>` element's `src`
pub(crate) struct DocumentSrcHandler;

impl NetHandler for ResourceHandler<DocumentSrcHandler> {
    fn bytes(self: Box<Self>, resolved_url: String, bytes: Bytes) {
        let html = String::from_utf8_lossy(&bytes).into_owned();
        self.respond(resolved_url, Ok(Resource::DocumentSrc(html)));
    }
}

pub struct ImageHandler {
    kind: ImageType,
    /// BGMail: decoded bytes the document may still take; see [`MAX_DOCUMENT_IMAGE_BYTES`].
    budget: Arc<AtomicU64>,
}
impl ImageHandler {
    pub(crate) fn new(kind: ImageType, budget: Arc<AtomicU64>) -> Self {
        Self { kind, budget }
    }

    /// BGMail: takes `bytes` from the document's budget, if it still has them.
    fn charge(&self, bytes: u64) -> Result<(), String> {
        let charged = self
            .budget
            .fetch_update(Ao::Relaxed, Ao::Relaxed, |left| left.checked_sub(bytes));
        if charged.is_err() {
            // Later images aren't worth decoding either.
            self.budget.store(0, Ao::Relaxed);
            return Err("the document's images take too much memory".to_string());
        }
        Ok(())
    }
}

impl NetHandler for ResourceHandler<ImageHandler> {
    fn bytes(self: Box<Self>, resolved_url: String, bytes: Bytes) {
        let result = self.data.parse(bytes);
        self.respond(resolved_url, result)
    }
}

impl ImageHandler {
    fn parse(&self, bytes: Bytes) -> Result<Resource, String> {
        // BGMail: don't decode what couldn't be kept anyway.
        if self.budget.load(Ao::Relaxed) == 0 {
            return Err("the document's images take too much memory".to_string());
        }
        if let Some((width, height, animation)) = decode_gif_animation(&bytes) {
            let total: usize = animation.frames.iter().map(|frame| frame.0.len()).sum();
            self.charge(total as u64)?;
            let first = animation.frames[0].0.clone();
            let data = Arc::new(first.data().to_vec());
            return Ok(Resource::Image(
                self.kind,
                width,
                height,
                data,
                Some(Arc::new(animation)),
            ));
        }
        let mut reader = image::ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .expect("IO errors impossible with Cursor");
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(MAX_IMAGE_BYTES);
        reader.limits(limits);
        let image_err = match reader.decode() {
            Ok(image) => {
                let width = image.width();
                let height = image.height();
                // BGMail: charged before converting: the limit above only saw the decoded
                // format, and a grayscale image takes four times that as RGBA.
                self.charge(rgba_bytes(width, height)?)?;
                let raw_rgba8_data = image.into_rgba8().into_raw();
                return Ok(Resource::Image(
                    self.kind,
                    width,
                    height,
                    Arc::new(raw_rgba8_data),
                    None,
                ));
            }
            Err(e) => e.to_string(),
        };

        #[cfg(feature = "svg")]
        let svg_err = {
            use crate::util::parse_svg_image;
            match parse_svg_image(&bytes) {
                Ok(svg) => return Ok(Resource::Svg(self.kind, svg)),
                Err(e) => e.to_string(),
            }
        };
        #[cfg(not(feature = "svg"))]
        let svg_err = "svg feature disabled";

        Err(format!(
            "Could not parse image ({} bytes): image-crate error: {image_err}; svg fallback error: {svg_err}",
            bytes.len()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A GIF of `frames` 2×1 frames (solid colors) shown `delay_ms` each.
    pub(crate) fn gif(frames: &[[u8; 4]], delay_ms: u32) -> Vec<u8> {
        use image::codecs::gif::{GifEncoder, Repeat};
        let mut bytes = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut bytes);
            encoder.set_repeat(Repeat::Infinite).unwrap();
            for color in frames {
                let buffer = image::RgbaImage::from_pixel(2, 1, image::Rgba(*color));
                let delay = image::Delay::from_numer_denom_ms(delay_ms, 1);
                encoder
                    .encode_frame(image::Frame::from_parts(buffer, 0, 0, delay))
                    .unwrap();
            }
        }
        bytes
    }

    /// A GIF with one 1×1 frame on a canvas of 65535×65535, in a few bytes.
    fn huge_canvas_gif() -> Vec<u8> {
        let mut bytes = b"GIF89a\xff\xff\xff\xff\x80\0\0\0\0\0\xff\xff\xff".to_vec();
        bytes.extend_from_slice(b"!\xf9\x04\0\n\0\0\0,\0\0\0\0\x01\0\x01\0\0\x02\x02D\x01\0;");
        bytes
    }

    #[test]
    fn a_small_gif_cant_allocate_the_canvas_it_declares() {
        // Without limits this asks for 16 GiB before the first frame.
        assert!(decode_gif_animation(&huge_canvas_gif()).is_none());
        let handler = ImageHandler::new(ImageType::Image, new_image_budget());
        assert!(handler.parse(Bytes::from(huge_canvas_gif())).is_err());
    }

    #[cfg(feature = "woff")]
    #[test]
    fn woff1_tables_are_summed_from_the_directory() {
        let mut file = vec![0u8; 44];
        file[12..14].copy_from_slice(&2u16.to_be_bytes());
        for length in [7u32, 8] {
            let mut entry = [0u8; 20];
            entry[12..16].copy_from_slice(&length.to_be_bytes());
            file.extend_from_slice(&entry);
        }
        assert_eq!(woff1_table_bytes(&file), Some(15));
        // A directory past the end of the file isn't a font.
        file.truncate(60);
        assert_eq!(woff1_table_bytes(&file), None);
    }

    #[test]
    fn images_are_measured_as_the_rgba_they_become() {
        // A grayscale image passes the decoder's limit at a quarter of this.
        assert_eq!(rgba_bytes(16, 2), Ok(128));
        assert!(rgba_bytes(16000, 16000).is_err());
    }

    #[test]
    fn images_share_a_budget_per_document() {
        // Two pixels of RGBA: 8 bytes.
        let still = gif(&[[1; 4]], 50);
        let budget = Arc::new(AtomicU64::new(12));
        let handler = ImageHandler::new(ImageType::Image, budget.clone());
        assert!(handler.parse(Bytes::from(still.clone())).is_ok());
        assert_eq!(budget.load(Ao::Relaxed), 4);
        // Past the budget nothing is kept, and nothing later is decoded.
        assert!(handler.parse(Bytes::from(still.clone())).is_err());
        assert_eq!(budget.load(Ao::Relaxed), 0);
        assert!(handler.parse(Bytes::from(still)).is_err());
    }

    #[test]
    fn animated_gifs_keep_every_frame() {
        let bytes = gif(&[[255, 0, 0, 255], [0, 0, 255, 255]], 50);
        let (width, height, animation) = decode_gif_animation(&bytes).unwrap();
        assert_eq!((width, height), (2, 1));
        assert_eq!(animation.frames.len(), 2);
        assert_eq!(animation.frames[1].0.data()[..4], [0, 0, 255, 255]);
        assert_eq!(animation.frames[0].1, 50);
        // Too-short delays play at 100ms, like browsers; still GIFs aren't animations.
        let fast = decode_gif_animation(&gif(&[[0; 4], [255; 4]], 0))
            .unwrap()
            .2;
        assert_eq!(fast.frames[0].1, 100);
        assert!(decode_gif_animation(&gif(&[[0; 4]], 50)).is_none());
        assert!(decode_gif_animation(b"\x89PNG").is_none());
    }

    #[test]
    fn animation_frames_follow_the_clock() {
        let bytes = gif(&[[1; 4], [2; 4], [3; 4]], 100);
        let animation = decode_gif_animation(&bytes).unwrap().2;
        assert_eq!(animation.duration_ms(), 300);
        let frame = |ms| {
            let (frame, next) = animation.frame_at(ms).unwrap();
            (frame.data()[0], next)
        };
        assert_eq!(frame(0), (1, 100));
        assert_eq!(frame(150), (2, 50));
        assert_eq!(frame(299), (3, 1));
        // Loops forever.
        assert_eq!(frame(300), (1, 100));
    }
    use parley::fontique::FontStyle as Fq;
    use style::values::specified::Angle;

    fn oblique(min_deg: f32, max_deg: f32) -> FontStyleRange {
        FontStyleRange::Oblique(Angle::from_degrees(min_deg), Angle::from_degrees(max_deg))
    }

    #[test]
    fn italic_maps_to_italic() {
        assert_eq!(stylo_to_fontique_style(&FontStyleRange::Italic), Fq::Italic,);
    }

    #[test]
    fn oblique_zero_zero_maps_to_normal() {
        // Stylo parses bare CSS `normal` as `Oblique(0deg, 0deg)`; the
        // helper must round-trip that back to `FontStyle::Normal` so
        // parley's matching doesn't misclassify upright fonts.
        assert_eq!(stylo_to_fontique_style(&oblique(0.0, 0.0)), Fq::Normal);
    }

    #[test]
    fn oblique_single_angle_maps_to_oblique_with_min() {
        assert_eq!(
            stylo_to_fontique_style(&oblique(14.0, 14.0)),
            Fq::Oblique(Some(14.0)),
        );
    }

    #[test]
    fn oblique_range_uses_min_angle() {
        // For a range, fontique's single-angle representation takes the
        // lower bound — confirm `min` (not `max`) is what gets through.
        assert_eq!(
            stylo_to_fontique_style(&oblique(10.0, 20.0)),
            Fq::Oblique(Some(10.0)),
        );
    }
}
