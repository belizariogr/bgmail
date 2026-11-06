use std::sync::Arc;

use gpui::{canvas, img, point, Bounds, FontWeight, Image, Point};

use crate::prelude::*;

/// A round avatar showing a person's initial, or a logo when one is given.
///
/// The letter is centered by its ink, not its line box: a line box puts the
/// glyph where the font's ascent and descent say, which leaves a capital a
/// little high or low and a narrow letter off to one side.
///
/// ```ignore
/// Avatar::new('L').size(px(30.0))
/// ```
#[derive(IntoElement)]
pub struct Avatar {
    initial: char,
    size: Pixels,
    background: Color,
    color: Color,
    logo: Option<Arc<Image>>,
}

impl Avatar {
    /// An avatar for `initial` (upper-cased), on the accent color.
    pub fn new(initial: char) -> Self {
        Self {
            initial: initial.to_uppercase().next().unwrap_or(initial),
            size: px(30.0),
            background: Color::Accent,
            color: Color::OnAccent,
            logo: None,
        }
    }

    /// Shows `logo` (cropped to the circle) in place of the initial.
    pub fn logo(mut self, logo: Option<Arc<Image>>) -> Self {
        self.logo = logo;
        self
    }

    /// Diameter of the circle.
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }
}

/// Where to put the pen so `ink` (glyph bounds from the pen position) is
/// centered in `area`.
pub fn ink_centered_origin(area: Bounds<Pixels>, ink: Bounds<Pixels>) -> Point<Pixels> {
    let center = area.center();
    let ink_center = ink.center();
    point(center.x - ink_center.x, center.y - ink_center.y)
}

impl RenderOnce for Avatar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        if let Some(logo) = self.logo {
            return img(logo)
                .size(self.size)
                .flex_shrink_0()
                .rounded_full()
                .into_any_element();
        }
        let font_size = crate::ui_text_size(1.0);
        let font = gpui::Font {
            weight: FontWeight::SEMIBOLD,
            ..window.text_style().font()
        };
        let font_id = window.text_system().resolve_font(&font);
        let ink = window.text_system().glyph_ink_bounds(
            font_id,
            self.initial,
            font_size,
            window.scale_factor(),
        );
        let color = self.color.hsla(cx);
        let circle = div()
            .size(self.size)
            .flex_shrink_0()
            .rounded_full()
            .bg(self.background.hsla(cx));
        match ink {
            Some((glyph, ink)) => circle.child(
                canvas(
                    |_, _, _| {},
                    move |area, _, window, _| {
                        let origin = ink_centered_origin(area, ink);
                        window
                            .paint_glyph(origin, font_id, glyph, font_size, color)
                            .ok();
                    },
                )
                .size_full(),
            ),
            // Not in the UI font (another script falls back to another font):
            // the line box is the best there is.
            None => circle
                .flex()
                .items_center()
                .justify_center()
                .text_color(color)
                .text_size(font_size)
                .line_height(font_size)
                .font_weight(FontWeight::SEMIBOLD)
                .child(self.initial.to_string()),
        }
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use gpui::size;

    use super::*;

    #[test]
    fn the_ink_lands_in_the_middle_of_the_circle() {
        let area = Bounds::new(point(px(100.0), px(50.0)), size(px(30.0), px(30.0)));
        // A capital from the baseline: 10 wide, 11 tall, one pixel right of the pen.
        let ink = Bounds::new(point(px(1.0), px(-11.0)), size(px(10.0), px(11.0)));
        let origin = ink_centered_origin(area, ink);
        let painted = Bounds::new(
            point(origin.x + ink.origin.x, origin.y + ink.origin.y),
            ink.size,
        );
        assert_eq!(painted.center(), area.center());
    }

    #[test]
    fn the_initial_is_upper_cased() {
        assert_eq!(Avatar::new('l').initial, 'L');
        assert_eq!(Avatar::new('ç').initial, 'Ç');
        assert_eq!(Avatar::new('?').initial, '?');
    }
}
