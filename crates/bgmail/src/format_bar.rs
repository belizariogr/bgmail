//! The formatting toolbar of a rich editor, as in Gmail: undo, font, size, B/I/U,
//! color, alignment, lists, indent and images. The compose window and the signature
//! editor each put one next to their [`TextArea`].

use std::io::Cursor;
use std::path::Path;

use gpui::{
    deferred, AnyElement, App, ClickEvent, Context, Entity, EventEmitter, Focusable, FontWeight,
    MouseButton, PathPromptOptions, Render, Subscription, Task, Window,
};
use theme::{ActiveTheme, MAIL_TEXT_COLORS};
use ui::prelude::*;
use ui::{
    font_for_family, Align, Color, ContextMenu, FontFamily, Icon, IconButton, IconName, IconSize,
    InlineImage, Label, LabelSize, ListKind, TextArea, TextSize, Tooltip, MAX_INDENT,
};

use crate::attachments::human_size;
use crate::locale::{ActiveLanguage, Key, Language};

/// Widest an image goes in at, in CSS pixels (a mail's usual width); a wider file
/// shows scaled down, its proportions kept.
pub const MAX_IMAGE_WIDTH: u32 = 600;

/// Name of a font in the menu; the fonts' own names aren't translated.
pub(crate) fn family_label(family: FontFamily, language: Language) -> &'static str {
    match family {
        FontFamily::Sans => Key::FormatSansSerif.tr(language),
        FontFamily::Serif => Key::FormatSerif.tr(language),
        FontFamily::Mono => Key::FormatFixedWidth.tr(language),
        FontFamily::Wide => Key::FormatWide.tr(language),
        FontFamily::Narrow => Key::FormatNarrow.tr(language),
        FontFamily::ComicSans => "Comic Sans MS",
        FontFamily::Garamond => "Garamond",
        FontFamily::Georgia => "Georgia",
        FontFamily::Tahoma => "Tahoma",
        FontFamily::Trebuchet => "Trebuchet MS",
        FontFamily::Verdana => "Verdana",
    }
}

/// A dropdown of the formatting toolbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FormatMenu {
    Font,
    Size,
    Color,
    Align,
}

/// Why a picked image didn't go in the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageError {
    /// Over the bar's limit, in bytes.
    TooLarge(usize),
    /// Not PNG, JPEG or GIF.
    Unsupported,
    Unreadable(String),
}

impl ImageError {
    pub fn message(&self, language: Language) -> String {
        match self {
            ImageError::TooLarge(limit) => format!(
                "{} {}",
                Key::FormatImageTooLarge.tr(language),
                human_size(*limit as u64, language)
            ),
            ImageError::Unsupported => Key::FormatImageUnsupported.tr(language).to_string(),
            ImageError::Unreadable(detail) => {
                format!("{}: {detail}", Key::FormatImageUnreadable.tr(language))
            }
        }
    }
}

/// The file at `path` as an image of the text (see [`inline_image`]).
pub fn read_inline_image(path: &Path, max_bytes: usize) -> Result<InlineImage, ImageError> {
    let unreadable = |error: std::io::Error| ImageError::Unreadable(error.to_string());
    let size = std::fs::metadata(path).map_err(unreadable)?.len();
    if size > max_bytes as u64 {
        return Err(ImageError::TooLarge(max_bytes));
    }
    let data = std::fs::read(path).map_err(unreadable)?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    inline_image(name, data, max_bytes)
}

/// `data` as an image of the text: PNG, JPEG or GIF only, which every mail client
/// shows (Outlook doesn't show WebP), at most `max_bytes`, and at most
/// [`MAX_IMAGE_WIDTH`] wide.
pub fn inline_image(
    name: String,
    data: Vec<u8>,
    max_bytes: usize,
) -> Result<InlineImage, ImageError> {
    if data.len() > max_bytes {
        return Err(ImageError::TooLarge(max_bytes));
    }
    let format = image::guess_format(&data).map_err(|_| ImageError::Unsupported)?;
    let mime_type = match format {
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::Jpeg => "image/jpeg",
        image::ImageFormat::Gif => "image/gif",
        _ => return Err(ImageError::Unsupported),
    };
    let (width, height) = image::ImageReader::with_format(Cursor::new(&data), format)
        .into_dimensions()
        .map_err(|error| ImageError::Unreadable(error.to_string()))?;
    let (width, height) = shown_size(width, height);
    Ok(InlineImage::new(name, mime_type, data, width, height))
}

/// `width`×`height` narrowed to [`MAX_IMAGE_WIDTH`], keeping the proportions.
fn shown_size(width: u32, height: u32) -> (u32, u32) {
    if width <= MAX_IMAGE_WIDTH {
        return (width.max(1), height.max(1));
    }
    let height = (u64::from(height) * u64::from(MAX_IMAGE_WIDTH) / u64::from(width)).max(1);
    (MAX_IMAGE_WIDTH, u32::try_from(height).unwrap_or(u32::MAX))
}

/// What the bar tells its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormatBarEvent {
    /// A picked image couldn't go in; the message is localized.
    ImageFailed(String),
}

/// The toolbar of one editor.
pub struct FormatBar {
    editor: Entity<TextArea>,
    /// Dropdown open.
    menu: Option<FormatMenu>,
    max_image_bytes: usize,
    _image_task: Option<Task<()>>,
    _subscription: Subscription,
}

impl EventEmitter<FormatBarEvent> for FormatBar {}

impl FormatBar {
    /// A bar for `editor`; it follows the editor's caret to show its formatting.
    pub fn new(editor: Entity<TextArea>, max_image_bytes: usize, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&editor, |_, _, cx| cx.notify());
        Self {
            editor,
            menu: None,
            max_image_bytes,
            _image_task: None,
            _subscription: subscription,
        }
    }

    /// Whether a dropdown is open (its owner catches the clicks around it).
    pub fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    /// Picks an image file and puts it at the caret, read off the UI thread.
    fn pick_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        let max_bytes = self.max_image_bytes;
        self._image_task = Some(cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let read = cx
                .background_executor()
                .spawn(async move { read_inline_image(&path, max_bytes) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| this.finish_image(read, window, cx));
        }));
    }

    fn finish_image(
        &mut self,
        read: Result<InlineImage, ImageError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match read {
            Ok(image) => {
                self.editor
                    .update(cx, |area, cx| area.insert_image(image, cx));
                window.focus(&self.editor.focus_handle(cx));
            }
            Err(error) => cx.emit(FormatBarEvent::ImageFailed(error.message(cx.language()))),
        }
    }

    /// Runs `apply` on the editor and keeps typing there: the toolbar's clicks must
    /// not take the caret away.
    fn format_body(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        apply: impl FnOnce(&mut TextArea, &mut Context<TextArea>),
    ) {
        self.menu = None;
        self.editor.update(cx, apply);
        window.focus(&self.editor.focus_handle(cx));
        cx.notify();
    }

    fn toggle_format_menu(&mut self, menu: FormatMenu, cx: &mut Context<Self>) {
        self.menu = (self.menu != Some(menu)).then_some(menu);
        cx.notify();
    }

    /// The tools, on one row that wraps in a narrow window.
    fn render_bar(
        &self,
        language: Language,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.theme().colors();
        let border = colors.border;
        let area = self.editor.read(cx);
        let format = area.format();
        let (can_undo, can_redo) = (area.can_undo(), area.can_redo());
        let icon = |id: &'static str,
                    label: Key,
                    shortcut: Option<(&str, &str)>,
                    icon: IconName,
                    active: bool,
                    enabled: bool,
                    cx: &mut Context<Self>,
                    apply: fn(&mut TextArea, &mut Context<TextArea>)| {
            let holder = div().id(id);
            let holder = match shortcut {
                Some((mac, other)) => holder.tooltip(Tooltip::with_shortcut(
                    label.tr(language),
                    crate::shortcuts::format_binding(crate::shortcuts::primary_binding(mac, other)),
                )),
                None => holder.tooltip(Tooltip::text(label.tr(language))),
            };
            holder.child(
                IconButton::new(id, icon)
                    .size(IconSize::Small)
                    .selected(active)
                    .disabled(!enabled)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.format_body(window, cx, apply);
                    })),
            )
        };
        let separator = || div().w(px(1.0)).h(px(18.0)).mx_1().bg(border);
        let chars = format.chars;
        let para = format.para;
        // Takes the room left of the send button, and wraps in a narrow window.
        h_flex()
            .flex_1()
            .min_w_0()
            .flex_wrap()
            .child(icon(
                "format-undo",
                Key::FormatUndo,
                Some(("cmd-z", "ctrl-z")),
                IconName::FormatUndo,
                false,
                can_undo,
                cx,
                TextArea::undo_step,
            ))
            .child(icon(
                "format-redo",
                Key::FormatRedo,
                Some(("cmd-shift-z", "ctrl-y")),
                IconName::FormatRedo,
                false,
                can_redo,
                cx,
                TextArea::redo_step,
            ))
            .child(separator())
            .child(self.render_font_picker(chars.family, language, window, cx))
            .child(separator())
            .child(self.render_size_picker(para.size, language, cx))
            .child(separator())
            .child(self.render_style_button(
                "format-bold",
                Key::FormatBold,
                ("cmd-b", "ctrl-b"),
                chars.bold,
                language,
                cx,
                TextArea::toggle_bold,
            ))
            .child(self.render_style_button(
                "format-italic",
                Key::FormatItalic,
                ("cmd-i", "ctrl-i"),
                chars.italic,
                language,
                cx,
                TextArea::toggle_italic,
            ))
            .child(self.render_style_button(
                "format-underline",
                Key::FormatUnderline,
                ("cmd-u", "ctrl-u"),
                chars.underline,
                language,
                cx,
                TextArea::toggle_underline,
            ))
            .child(self.render_color_picker(chars.color, language, cx))
            .child(separator())
            .child(self.render_align_picker(para.align, language, cx))
            .child(icon(
                "format-numbered",
                Key::FormatNumbered,
                None,
                IconName::FormatListNumbered,
                para.list == Some(ListKind::Numbered),
                true,
                cx,
                |area, cx| area.toggle_list(ListKind::Numbered, cx),
            ))
            .child(icon(
                "format-bullets",
                Key::FormatBullets,
                None,
                IconName::FormatListBullet,
                para.list == Some(ListKind::Bullet),
                true,
                cx,
                |area, cx| area.toggle_list(ListKind::Bullet, cx),
            ))
            .child(icon(
                "format-indent-less",
                Key::FormatIndentLess,
                None,
                IconName::FormatIndentLess,
                false,
                para.indent > 0,
                cx,
                |area, cx| area.indent(-1, cx),
            ))
            .child(icon(
                "format-indent-more",
                Key::FormatIndentMore,
                None,
                IconName::FormatIndentMore,
                false,
                para.indent < MAX_INDENT,
                cx,
                |area, cx| area.indent(1, cx),
            ))
            .child(separator())
            .child(
                div()
                    .id("format-image")
                    .tooltip(Tooltip::text(Key::FormatImage.tr(language)))
                    .child(
                        IconButton::new("format-image", IconName::Image)
                            .size(IconSize::Small)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.pick_image(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    /// B, I or U: a letter showing the style it applies.
    #[allow(clippy::too_many_arguments)]
    fn render_style_button(
        &self,
        id: &'static str,
        label: Key,
        shortcut: (&str, &str),
        active: bool,
        language: Language,
        cx: &mut Context<Self>,
        apply: fn(&mut TextArea, &mut Context<TextArea>),
    ) -> impl IntoElement {
        let colors = cx.theme().colors();
        let (hover, selected, accent, text) = (
            colors.element_hover,
            colors.element_active,
            colors.text_accent,
            colors.text,
        );
        let (letter, weight, italic, underline) = match id {
            "format-bold" => ("B", FontWeight::BOLD, false, false),
            "format-italic" => ("I", FontWeight::NORMAL, true, false),
            _ => ("U", FontWeight::NORMAL, false, true),
        };
        let binding = crate::shortcuts::format_binding(crate::shortcuts::primary_binding(
            shortcut.0, shortcut.1,
        ));
        h_flex()
            .id(id)
            .size(px(28.0))
            .justify_center()
            .rounded_md()
            .cursor_pointer()
            .when(active, |el| el.bg(selected))
            .hover(move |el| el.bg(hover))
            .tooltip(Tooltip::with_shortcut(label.tr(language), binding))
            .text_color(if active { accent } else { text })
            .font_weight(weight)
            .when(italic, |el| el.italic())
            .when(underline, |el| el.underline())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| {
                this.format_body(window, cx, apply);
            }))
            .child(letter)
    }

    /// A button with text that opens one of the formatting dropdowns above it.
    #[allow(clippy::too_many_arguments)]
    fn render_menu_button(
        &self,
        id: &'static str,
        menu: FormatMenu,
        tooltip: Key,
        content: impl IntoElement,
        dropdown: Option<AnyElement>,
        language: Language,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let hover = cx.theme().colors().element_hover;
        h_flex()
            .id(id)
            .relative()
            .h(px(28.0))
            .px_1p5()
            .gap_1()
            .rounded_md()
            .cursor_pointer()
            .hover(move |el| el.bg(hover))
            .tooltip(Tooltip::text(tooltip.tr(language)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_format_menu(menu, cx)))
            .child(content)
            .child(
                Icon::new(IconName::CaretDown)
                    .size(IconSize::XXSmall)
                    .color(Color::Muted),
            )
            .children(dropdown.filter(|_| self.menu == Some(menu)))
    }

    /// Right above its button, which is its positioning parent: the toolbar is at
    /// the bottom of the window.
    fn format_dropdown(content: impl IntoElement) -> AnyElement {
        deferred(div().absolute().left_0().bottom(px(30.0)).child(content))
            .with_priority(1)
            .into_any_element()
    }

    /// A menu whose entries run `apply` on the body with their value, the
    /// current one checked.
    fn format_menu_of<T: Copy + PartialEq + 'static>(
        &self,
        options: &[(T, Key)],
        current: T,
        language: Language,
        cx: &mut Context<Self>,
        apply: fn(&mut TextArea, T, &mut Context<TextArea>),
    ) -> AnyElement {
        let view = cx.weak_entity();
        let mut menu = ContextMenu::new();
        for (value, key) in options.iter().copied() {
            let view = view.clone();
            let handler = move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                let _ = view.update(cx, |this, cx| {
                    this.format_body(window, cx, |area, cx| apply(area, value, cx));
                });
            };
            menu = if value == current {
                menu.entry_with_key_hint(key.tr(language), "✓", handler)
            } else {
                menu.entry(key.tr(language), handler)
            };
        }
        Self::format_dropdown(menu)
    }

    fn render_font_picker(
        &self,
        current: FontFamily,
        language: Language,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let dropdown = (self.menu == Some(FormatMenu::Font)).then(|| {
            let inherited = window.text_style().font().family;
            let view = cx.weak_entity();
            let mut menu = ContextMenu::new();
            for family in FontFamily::ALL {
                let view = view.clone();
                let handler = move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    let _ = view.update(cx, |this, cx| {
                        this.format_body(window, cx, |area, cx| area.set_font_family(family, cx));
                    });
                };
                let label = family_label(family, language);
                menu = if family == current {
                    menu.entry_with_key_hint(label, "✓", handler)
                } else {
                    menu.entry(label, handler)
                }
                .in_font(font_for_family(family, &inherited, window));
            }
            Self::format_dropdown(menu)
        });
        self.render_menu_button(
            "format-font",
            FormatMenu::Font,
            Key::FormatFont,
            Label::new(family_label(current, language)).size(LabelSize::Small),
            dropdown,
            language,
            cx,
        )
    }

    fn render_size_picker(
        &self,
        current: TextSize,
        language: Language,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        const OPTIONS: [(TextSize, Key); 4] = [
            (TextSize::Small, Key::FormatSmall),
            (TextSize::Normal, Key::FormatNormal),
            (TextSize::Large, Key::FormatLarge),
            (TextSize::Huge, Key::FormatHuge),
        ];
        let dropdown =
            self.format_menu_of(&OPTIONS, current, language, cx, TextArea::set_text_size);
        self.render_menu_button(
            "format-size",
            FormatMenu::Size,
            Key::FormatSize,
            div().font_weight(FontWeight::BOLD).child("TT"),
            Some(dropdown),
            language,
            cx,
        )
    }

    fn render_align_picker(
        &self,
        current: Align,
        language: Language,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        const OPTIONS: [(Align, Key); 3] = [
            (Align::Left, Key::FormatAlignLeft),
            (Align::Center, Key::FormatAlignCenter),
            (Align::Right, Key::FormatAlignRight),
        ];
        let dropdown = self.format_menu_of(&OPTIONS, current, language, cx, TextArea::set_align);
        let icon = match current {
            Align::Left => IconName::FormatAlignLeft,
            Align::Center => IconName::FormatAlignCenter,
            Align::Right => IconName::FormatAlignRight,
        };
        self.render_menu_button(
            "format-align",
            FormatMenu::Align,
            Key::FormatAlign,
            Icon::new(icon).size(IconSize::Small),
            Some(dropdown),
            language,
            cx,
        )
    }

    /// "A" underlined with the current color, and a grid of colors.
    fn render_color_picker(
        &self,
        current: Option<u32>,
        language: Language,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors();
        let (surface, border, accent, text) = (
            colors.elevated_surface_background,
            colors.border,
            colors.accent,
            colors.text,
        );
        let swatch = |id: usize, color: Option<u32>, cx: &mut Context<Self>| {
            h_flex()
                .id(("format-color", id))
                .size(px(22.0))
                .justify_center()
                .rounded_sm()
                .cursor_pointer()
                .border_1()
                .border_color(if color == current { accent } else { border })
                .when_some(color, |el, rgb_value| el.bg(gpui::rgb(rgb_value)))
                .when(color.is_none(), |el| {
                    el.child(Icon::new(IconName::Clear).size(IconSize::XSmall))
                })
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.format_body(window, cx, |area, cx| area.set_text_color(color, cx));
                }))
        };
        let mut grid = div()
            .occlude()
            .w(px(166.0))
            .p_1()
            .flex()
            .flex_wrap()
            .gap_1()
            .bg(surface)
            .border_1()
            .border_color(border)
            .rounded_md()
            .shadow_lg()
            .child(swatch(0, None, cx));
        for (ix, color) in MAIL_TEXT_COLORS.into_iter().enumerate() {
            grid = grid.child(swatch(ix + 1, Some(color), cx));
        }
        self.render_menu_button(
            "format-color-menu",
            FormatMenu::Color,
            Key::FormatColor,
            v_flex()
                .items_center()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .line_height(px(16.0))
                        .child("A"),
                )
                .child(
                    div()
                        .w(px(12.0))
                        .h(px(2.0))
                        .bg(current.map_or(text, |rgb_value| gpui::rgb(rgb_value).into())),
                ),
            Some(Self::format_dropdown(grid)),
            language,
            cx,
        )
    }
}

impl Render for FormatBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_bar(cx.language(), window, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::new(width, height))
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn every_font_has_a_distinct_label() {
        let labels: std::collections::HashSet<_> = FontFamily::ALL
            .into_iter()
            .map(|family| family_label(family, Language::Portuguese))
            .collect();
        assert_eq!(labels.len(), FontFamily::ALL.len());
    }

    #[test]
    fn an_image_goes_in_at_its_size_up_to_a_mail_width() {
        let image = inline_image("logo.png".into(), png(120, 40), 1 << 20).unwrap();
        assert_eq!((image.width, image.height), (120, 40));
        assert_eq!(image.mime_type, "image/png");
        let wide = inline_image("wide.png".into(), png(1200, 300), 1 << 20).unwrap();
        assert_eq!((wide.width, wide.height), (600, 150));
        assert_eq!(shown_size(5000, 1), (600, 1));
    }

    #[test]
    fn an_image_mail_clients_cant_show_or_too_big_is_refused() {
        let bytes = png(10, 10);
        assert_eq!(
            inline_image("a.png".into(), bytes.clone(), bytes.len() - 1).unwrap_err(),
            ImageError::TooLarge(bytes.len() - 1)
        );
        assert_eq!(
            inline_image("a.txt".into(), b"hello".to_vec(), 1 << 20).unwrap_err(),
            ImageError::Unsupported
        );
        let mut webp = b"RIFF\0\0\0\0WEBPVP8 ".to_vec();
        webp.extend([0; 16]);
        assert_eq!(
            inline_image("a.webp".into(), webp, 1 << 20).unwrap_err(),
            ImageError::Unsupported
        );
        let truncated = bytes[..20].to_vec();
        assert!(matches!(
            inline_image("a.png".into(), truncated, 1 << 20),
            Err(ImageError::Unreadable(_))
        ));
    }

    #[test]
    fn a_picked_file_is_read_with_its_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logo.png");
        std::fs::write(&path, png(8, 4)).unwrap();
        let image = read_inline_image(&path, 1 << 20).unwrap();
        assert_eq!(image.name, "logo.png");
        assert_eq!(
            read_inline_image(&path, 10).unwrap_err(),
            ImageError::TooLarge(10)
        );
        assert!(matches!(
            read_inline_image(&dir.path().join("missing.png"), 1 << 20),
            Err(ImageError::Unreadable(_))
        ));
    }

    #[test]
    fn image_errors_read_as_sentences() {
        let language = Language::English;
        assert!(ImageError::TooLarge(1 << 20)
            .message(language)
            .contains("1.0 MB"));
        assert!(!ImageError::Unsupported.message(language).is_empty());
        assert!(ImageError::Unreadable("boom".into())
            .message(language)
            .ends_with("boom"));
    }
}
