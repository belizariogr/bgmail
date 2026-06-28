use gpui::FontWeight;

use crate::prelude::*;

/// Tamanhos de texto disponíveis para [`Label`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum LabelSize {
    /// 11px — metadados muito discretos.
    XSmall,
    /// 12px — prévia de mensagem, horário.
    Small,
    /// 14px — texto padrão.
    #[default]
    Default,
    /// 16px — títulos de seção.
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

/// Um rótulo de texto temático.
///
/// ```ignore
/// Label::new("Caixa de entrada").size(LabelSize::Small).color(Color::Muted)
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
    /// Cria um novo rótulo.
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            size: LabelSize::default(),
            color: Color::default(),
            weight: FontWeight::NORMAL,
            single_line: false,
        }
    }

    /// Define o tamanho do texto.
    pub fn size(mut self, size: LabelSize) -> Self {
        self.size = size;
        self
    }

    /// Define a cor semântica do texto.
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Define o peso da fonte.
    pub fn weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

    /// Atalho para texto em negrito.
    pub fn bold(mut self) -> Self {
        self.weight = FontWeight::BOLD;
        self
    }

    /// Atalho para texto semi-negrito.
    pub fn semibold(mut self) -> Self {
        self.weight = FontWeight::SEMIBOLD;
        self
    }

    /// Trunca o texto em uma única linha com reticências.
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
