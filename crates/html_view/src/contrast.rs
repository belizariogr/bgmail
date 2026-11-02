//! Keeps text readable on dark pages. Mail often fixes its text color inline for a
//! white page (Outlook writes `color: rgb(0, 0, 0)` on every paragraph), which on a
//! dark page is invisible and beats any stylesheet. Rewriting the HTML can't tell
//! that apart from dark text on a white block of the message, so this runs on what
//! is painted, where each text's real background is known.

use kurbo::Rect;
use peniko::Color;

use crate::display_list::{Brush, Command, DisplayList};

/// Colors of the dark page text is adapted for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DarkPage {
    pub background: [u8; 4],
    /// Text color of the page: what neutral text becomes.
    pub text: [u8; 4],
}

/// Below this contrast ratio text is considered unreadable (WCAG for large text).
const MIN_CONTRAST: f32 = 3.0;
/// Contrast adapted colors are brought to (WCAG for normal text).
const TARGET_CONTRAST: f32 = 4.5;
/// Colors less saturated than this count as neutral (black, greys).
const NEUTRAL_SATURATION: f32 = 0.2;
/// Fills at least this opaque hide what is behind them.
const OPAQUE_ALPHA: f32 = 0.5;
/// A fill this thin is a line (an underline), drawn like text, not a background.
const LINE_THICKNESS: f64 = 3.0;

/// What is behind a point of the page.
#[derive(Clone, Copy)]
enum Behind {
    Color(Color),
    /// An image or gradient: nothing can be said about it.
    Unknown,
}

/// Adapts the text of `list` that can't be read on what is behind it.
pub(crate) fn adapt_text_contrast(list: &mut DisplayList, page: DarkPage) {
    let [r, g, b, a] = page.background;
    let background = Color::from_rgba8(r, g, b, a);
    let [r, g, b, a] = page.text;
    let text = Color::from_rgba8(r, g, b, a);

    let mut clips: Vec<Rect> = Vec::new();
    let mut painted: Vec<(Rect, Behind)> = Vec::new();
    for command in &mut list.commands {
        match command {
            Command::PushLayer { clip, .. } => clips.push(*clip),
            Command::Pop => {
                clips.pop();
            }
            Command::Fill { geometry, brush } => {
                let bounds = geometry.bounds();
                let visible = clips
                    .iter()
                    .fold(bounds, |rect, clip| rect.intersect(*clip));
                match brush {
                    Brush::Solid(color) if is_line(bounds) => {
                        adapt(color, behind(&painted, bounds.center(), background), text);
                    }
                    Brush::Solid(color) if color.components[3] >= OPAQUE_ALPHA => {
                        painted.push((visible, Behind::Color(*color)));
                    }
                    Brush::Solid(_) => {}
                    Brush::Gradient { .. } | Brush::Image { .. } => {
                        painted.push((visible, Behind::Unknown));
                    }
                }
            }
            Command::Text(run) => {
                let left = run.words.first().map_or(0.0, |word| word.x);
                let center = kurbo::Point::new(
                    f64::from(left),
                    f64::from(run.baseline - run.font_size / 3.0),
                );
                adapt(&mut run.color, behind(&painted, center, background), text);
            }
            _ => {}
        }
    }
}

fn is_line(bounds: Rect) -> bool {
    bounds.height() <= LINE_THICKNESS && bounds.width() > bounds.height() * 2.0
}

/// The last thing painted at `point`.
fn behind(painted: &[(Rect, Behind)], point: kurbo::Point, page: Color) -> Behind {
    painted
        .iter()
        .rev()
        .find(|(rect, _)| rect.contains(point))
        .map_or(Behind::Color(page), |(_, behind)| *behind)
}

fn adapt(color: &mut Color, behind: Behind, page_text: Color) {
    if let Behind::Color(background) = behind {
        if contrast(*color, background) < MIN_CONTRAST {
            *color = readable_color(*color, background, page_text);
        }
    }
}

/// `color` made readable on `background`: neutral text takes the page's text color
/// when that reads, and colored text keeps its hue, only lighter or darker.
fn readable_color(color: Color, background: Color, page_text: Color) -> Color {
    let [r, g, b, alpha] = color.components;
    let (hue, saturation, _) = rgb_to_hsl(r, g, b);
    if saturation < NEUTRAL_SATURATION && contrast(page_text, background) >= TARGET_CONTRAST {
        return page_text;
    }
    let lighten = luminance(background) < 0.18;
    let mut lightness = if lighten { 0.0 } else { 1.0 };
    let step = if lighten { 0.02 } else { -0.02 };
    loop {
        let [r, g, b] = hsl_to_rgb(hue, saturation, lightness);
        let candidate = Color::new([r, g, b, alpha]);
        let done = if lighten {
            lightness >= 1.0
        } else {
            lightness <= 0.0
        };
        if done || contrast(candidate, background) >= TARGET_CONTRAST {
            return candidate;
        }
        lightness += step;
    }
}

/// WCAG relative luminance.
fn luminance(color: Color) -> f32 {
    let channel = |value: f32| {
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let [r, g, b, _] = color.components;
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

/// WCAG contrast ratio, from 1 to 21.
fn contrast(a: Color, b: Color) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let lightness = (max + min) / 2.0;
    let delta = max - min;
    if delta <= f32::EPSILON {
        return (0.0, 0.0, lightness);
    }
    let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs()).max(f32::EPSILON);
    let hue = if max == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    (hue / 6.0, saturation.min(1.0), lightness)
}

fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> [f32; 3] {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = hue * 6.0;
    let x = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match sector as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let base = lightness - chroma / 2.0;
    [r + base, g + base, b + base]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocumentOptions, HtmlDocument};

    const PAGE: DarkPage = DarkPage {
        background: [30, 30, 30, 255],
        text: [230, 230, 230, 255],
    };

    /// Color of each text run of `html` on the dark page.
    fn text_colors(html: &str) -> Vec<(String, [u8; 3])> {
        let mut document = HtmlDocument::parse(
            html,
            DocumentOptions {
                dark_page: Some(PAGE),
                ..Default::default()
            },
        );
        document.prepare(600.0);
        document
            .display_list()
            .commands
            .iter()
            .filter_map(|command| match command {
                Command::Text(run) => Some((
                    run.words
                        .iter()
                        .map(|w| w.text.as_str())
                        .collect::<String>(),
                    {
                        let [r, g, b, _] = run.color.to_rgba8().to_u8_array();
                        [r, g, b]
                    },
                )),
                _ => None,
            })
            .collect()
    }

    fn color_of(colors: &[(String, [u8; 3])], text: &str) -> [u8; 3] {
        colors
            .iter()
            .find(|(run, _)| run.contains(text))
            .unwrap_or_else(|| panic!("{text} not in {colors:?}"))
            .1
    }

    const PAGE_STYLE: &str = "<body style=\"margin:0;background:#1e1e1e\">";

    #[test]
    fn inline_black_on_the_dark_page_takes_the_page_text_color() {
        let colors = text_colors(&format!(
            "{PAGE_STYLE}<p style=\"color:rgb(0,0,0)\">Ola</p></body>"
        ));
        assert_eq!(color_of(&colors, "Ola"), [230, 230, 230]);
    }

    #[test]
    fn dark_text_on_its_own_white_block_stays() {
        let colors = text_colors(&format!(
            "{PAGE_STYLE}<div style=\"background:white\"><p style=\"color:#000\">Cell</p></div>\
             <p style=\"color:#000\">Out</p></body>"
        ));
        assert_eq!(color_of(&colors, "Cell"), [0, 0, 0]);
        assert_eq!(color_of(&colors, "Out"), [230, 230, 230]);
    }

    #[test]
    fn colored_text_keeps_its_hue_and_becomes_readable() {
        let colors = text_colors(&format!(
            "{PAGE_STYLE}<p style=\"color:rgb(21,61,99)\">Azul</p></body>"
        ));
        let [r, g, b] = color_of(&colors, "Azul");
        assert!(b > g && g > r, "{:?}", [r, g, b]);
        let adapted = Color::from_rgb8(r, g, b);
        assert!(contrast(adapted, Color::from_rgb8(30, 30, 30)) >= TARGET_CONTRAST);
    }

    #[test]
    fn readable_colors_are_left_alone() {
        let colors = text_colors(&format!(
            "{PAGE_STYLE}<p style=\"color:rgb(255,200,0)\">Amarelo</p></body>"
        ));
        assert_eq!(color_of(&colors, "Amarelo"), [255, 200, 0]);
    }

    #[test]
    fn text_on_an_image_is_left_alone() {
        let page = Color::from_rgb8(30, 30, 30);
        let point = kurbo::Point::new(5.0, 5.0);
        let painted = [(Rect::new(0.0, 0.0, 10.0, 10.0), Behind::Unknown)];
        let mut color = Color::BLACK;
        adapt(&mut color, behind(&painted, point, page), Color::WHITE);
        assert_eq!(color, Color::BLACK);
    }

    #[test]
    fn without_the_option_nothing_changes() {
        let mut document = HtmlDocument::parse(
            &format!("{PAGE_STYLE}<p style=\"color:rgb(0,0,0)\">Ola</p></body>"),
            DocumentOptions::default(),
        );
        document.prepare(600.0);
        let list = document.display_list();
        let color = list.commands.iter().find_map(|command| match command {
            Command::Text(run) => Some(run.color),
            _ => None,
        });
        assert_eq!(color.unwrap().to_rgba8().to_u8_array(), [0, 0, 0, 255]);
    }

    #[test]
    fn hsl_round_trips() {
        for [r, g, b] in [
            [0.1, 0.5, 0.9],
            [0.9, 0.2, 0.1],
            [0.3, 0.8, 0.4],
            [0.5, 0.5, 0.5],
        ] {
            let (h, s, l) = rgb_to_hsl(r, g, b);
            let [x, y, z] = hsl_to_rgb(h, s, l);
            assert!((x - r).abs() < 0.001 && (y - g).abs() < 0.001 && (z - b).abs() < 0.001);
        }
    }

    #[test]
    fn contrast_goes_from_one_to_twenty_one() {
        assert!((contrast(Color::WHITE, Color::WHITE) - 1.0).abs() < 0.01);
        assert!((contrast(Color::BLACK, Color::WHITE) - 21.0).abs() < 0.01);
    }
}
