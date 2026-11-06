use gpui::FontWeight;

use crate::prelude::*;

/// Text sizes available for [`Label`], relative to [`crate::UI_FONT_SIZE`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum LabelSize {
    /// Base − 3px — very subtle metadata.
    XSmall,
    /// Base − 2px — message preview, timestamp.
    Small,
    /// The base size — default text.
    #[default]
    Default,
    /// Base + 2px — section titles.
    Large,
}

impl LabelSize {
    fn px(self) -> Pixels {
        crate::ui_text_size(match self {
            LabelSize::XSmall => -3.0,
            LabelSize::Small => -2.0,
            LabelSize::Default => 0.0,
            LabelSize::Large => 2.0,
        })
    }
}

/// A themed text label.
///
/// ```ignore
/// Label::new("Inbox").size(LabelSize::Small).color(Color::Muted)
/// ```
#[derive(IntoElement)]
pub struct Label {
    text: SharedString,
    size: LabelSize,
    color: Color,
    hover_color: Option<Color>,
    weight: FontWeight,
    single_line: bool,
}

impl Label {
    /// Creates a new label.
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            size: LabelSize::default(),
            color: Color::default(),
            hover_color: None,
            weight: FontWeight::NORMAL,
            single_line: false,
        }
    }

    /// Sets the text size.
    pub fn size(mut self, size: LabelSize) -> Self {
        self.size = size;
        self
    }

    /// Sets the semantic text color.
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Sets the text color while the pointer is over the label.
    pub fn hover_color(mut self, color: Color) -> Self {
        self.hover_color = Some(color);
        self
    }

    /// Sets the font weight.
    pub fn weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

    /// Shortcut for bold text.
    pub fn bold(mut self) -> Self {
        self.weight = FontWeight::BOLD;
        self
    }

    /// Shortcut for semibold text.
    pub fn semibold(mut self) -> Self {
        self.weight = FontWeight::SEMIBOLD;
        self
    }

    /// Truncates the text to a single line with an ellipsis.
    pub fn single_line(mut self) -> Self {
        self.single_line = true;
        self
    }
}

impl RenderOnce for Label {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self.color.hsla(cx);
        let hover = self.hover_color.map(|color| color.hsla(cx));
        div()
            .text_size(self.size.px())
            .text_color(color)
            .when_some(hover, |el, hover| el.hover(move |el| el.text_color(hover)))
            .font_weight(self.weight)
            .when(self.single_line, |el| {
                el.overflow_hidden().whitespace_nowrap().text_ellipsis()
            })
            .child(self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_color_is_kept_apart_from_the_color() {
        let label = Label::new("a").color(Color::Muted);
        assert_eq!(label.hover_color, None);
        let label = label.hover_color(Color::Default);
        assert_eq!(label.color, Color::Muted);
        assert_eq!(label.hover_color, Some(Color::Default));
    }
}
