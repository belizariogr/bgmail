use gpui::FontWeight;

use crate::prelude::*;

/// Text sizes available for [`Label`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum LabelSize {
    /// 11px — very subtle metadata.
    XSmall,
    /// 12px — message preview, timestamp.
    Small,
    /// 14px — default text.
    #[default]
    Default,
    /// 16px — section titles.
    Large,
}

impl LabelSize {
    fn px(self) -> Pixels {
        match self {
            LabelSize::XSmall => px(11.0),
            LabelSize::Small => px(12.0),
            LabelSize::Default => px(14.0),
            LabelSize::Large => px(16.0),
        }
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
        div()
            .text_size(self.size.px())
            .text_color(color)
            .font_weight(self.weight)
            .when(self.single_line, |el| {
                el.overflow_hidden().whitespace_nowrap().text_ellipsis()
            })
            .child(self.text)
    }
}
