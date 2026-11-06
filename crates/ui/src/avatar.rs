use std::sync::{Arc, Mutex};

use gpui::{canvas, img, point, Bounds, FontWeight, ImageSource, Point, RenderImage};

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
    logo: Option<SvgLogo>,
}

/// An SVG logo, drawn at exactly the size it shows at in device pixels and kept
/// until that size changes. Scaling a bigger picture down on the GPU blurs a
/// small logo; this keeps it as sharp as the screen allows.
#[derive(Clone)]
pub struct SvgLogo {
    svg: Arc<[u8]>,
    drawn: Arc<Mutex<Option<Drawn>>>,
}

/// A drawing of a logo and the side, in device pixels, it was drawn for.
type Drawn = (u32, Arc<RenderImage>);

impl SvgLogo {
    pub fn new(svg: Vec<u8>) -> Self {
        Self {
            svg: svg.into(),
            drawn: Arc::default(),
        }
    }

    /// The logo as a `side`×`side` device-pixel image, drawn on first need.
    /// `None` when the SVG doesn't parse.
    pub fn image(&self, side: u32) -> Option<Arc<RenderImage>> {
        let mut drawn = self
            .drawn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((drawn_side, image)) = drawn.as_ref() {
            if *drawn_side == side {
                return Some(image.clone());
            }
        }
        let image = Arc::new(rasterize_svg(&self.svg, side)?);
        *drawn = Some((side, image.clone()));
        Some(image)
    }
}

/// Draws `svg` fitted and centered in a `side`×`side` square, as the BGRA with
/// straight alpha GPUI's render images hold (resvg gives premultiplied RGBA).
pub fn rasterize_svg(svg: &[u8], side: u32) -> Option<RenderImage> {
    use resvg::{tiny_skia, usvg};
    let side = side.max(1);
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).ok()?;
    let size = tree.size();
    let scale = side as f32 / size.width().max(size.height());
    let offset_x = (side as f32 - size.width() * scale) / 2.0;
    let offset_y = (side as f32 - size.height() * scale) / 2.0;
    let mut pixmap = tiny_skia::Pixmap::new(side, side)?;
    let transform =
        tiny_skia::Transform::from_translate(offset_x, offset_y).pre_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let mut pixels = pixmap.take();
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        let alpha = pixel[3] as u32;
        if alpha > 0 && alpha < 255 {
            for channel in &mut pixel[..3] {
                *channel = ((*channel as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    let buffer = image::RgbaImage::from_raw(side, side, pixels)?;
    Some(RenderImage::new(smallvec::smallvec![image::Frame::new(
        buffer
    )]))
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
    pub fn logo(mut self, logo: Option<SvgLogo>) -> Self {
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
        let side = (f32::from(self.size) * window.scale_factor()).round() as u32;
        if let Some(image) = self.logo.and_then(|logo| logo.image(side)) {
            return img(ImageSource::Render(image))
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

    const PURPLE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><rect width="100" height="100" fill="#820ad1"/></svg>"##;

    /// Colors come out in the order GPUI reads (BGRA): red and blue once traded
    /// places, and Nubank's purple showed magenta.
    #[test]
    fn a_logo_keeps_its_colors() {
        let image = rasterize_svg(PURPLE, 8).unwrap();
        assert_eq!(&image.as_bytes(0).unwrap()[..4], [0xd1, 0x0a, 0x82, 0xff]);
    }

    /// Drawn at the exact device size, whatever the SVG's own, so the GPU never
    /// scales it.
    #[test]
    fn a_logo_is_drawn_at_the_size_it_shows_at() {
        let image = rasterize_svg(PURPLE, 60).unwrap();
        assert_eq!(image.size(0).width.0, 60);
        assert_eq!(image.size(0).height.0, 60);
        let logo = SvgLogo::new(PURPLE.to_vec());
        let first = logo.image(60).unwrap();
        assert!(Arc::ptr_eq(&first, &logo.image(60).unwrap()), "kept");
        assert_eq!(logo.image(90).unwrap().size(0).width.0, 90, "redrawn");
        assert!(SvgLogo::new(b"not svg".to_vec()).image(30).is_none());
    }

    /// Semi-transparent edges keep their color once alpha is no longer premultiplied.
    #[test]
    fn soft_edges_keep_their_color() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" fill="#820ad1" fill-opacity="0.5"/></svg>"##;
        let image = rasterize_svg(svg, 4).unwrap();
        let pixel = &image.as_bytes(0).unwrap()[..4];
        assert!(
            pixel[0].abs_diff(0xd1) <= 2 && pixel[2].abs_diff(0x82) <= 2,
            "{pixel:?}"
        );
        assert!(pixel[3].abs_diff(128) <= 1);
    }

    #[test]
    fn the_initial_is_upper_cased() {
        assert_eq!(Avatar::new('l').initial, 'L');
        assert_eq!(Avatar::new('ç').initial, 'Ç');
        assert_eq!(Avatar::new('?').initial, '?');
    }
}
