use gpui::ClickEvent;

use crate::prelude::*;

/// Estilo visual de um [`Button`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ButtonStyle {
    /// Fundo sutil que se destaca da superfície (ação secundária).
    #[default]
    Subtle,
    /// Fundo de acento sólido (ação primária).
    Filled,
    /// Sem fundo até receber hover (ação terciária / toolbar).
    Ghost,
}

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// Um botão com rótulo de texto.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    style: ButtonStyle,
    full_width: bool,
    on_click: Option<ClickHandler>,
}

impl Button {
    /// Cria um novo botão.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            style: ButtonStyle::default(),
            full_width: false,
            on_click: None,
        }
    }

    /// Define o estilo do botão.
    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    /// Faz o botão ocupar toda a largura disponível.
    pub fn full_width(mut self) -> Self {
        self.full_width = true;
        self
    }

    /// Registra o handler de clique.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.theme().colors();
        let (bg, text_color, hover_bg) = match self.style {
            ButtonStyle::Subtle => (colors.element_background, colors.text, colors.element_hover),
            ButtonStyle::Filled => (colors.accent, colors.text_on_accent, colors.accent),
            ButtonStyle::Ghost => (gpui::transparent_black(), colors.text, colors.element_hover),
        };

        h_flex()
            .id(self.id)
            .justify_center()
            .gap_1()
            .px_3()
            .py_1p5()
            .rounded_md()
            .bg(bg)
            .text_size(px(13.0))
            .text_color(text_color)
            .when(self.full_width, |el| el.w_full())
            .hover(move |el| el.bg(hover_bg))
            .child(self.label)
            .when_some(self.on_click, |el, handler| {
                el.cursor_pointer()
                    .on_click(move |event, window, cx| handler(event, window, cx))
            })
    }
}

/// Um botão composto apenas por um ícone (usado em toolbars).
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: IconName,
    size: IconSize,
    color: Color,
    selected: bool,
    on_click: Option<ClickHandler>,
}

impl IconButton {
    /// Cria um novo botão de ícone.
    pub fn new(id: impl Into<ElementId>, icon: IconName) -> Self {
        Self {
            id: id.into(),
            icon,
            size: IconSize::default(),
            color: Color::Default,
            selected: false,
            on_click: None,
        }
    }

    /// Define o tamanho do ícone.
    pub fn size(mut self, size: IconSize) -> Self {
        self.size = size;
        self
    }

    /// Define a cor do ícone.
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Marca o botão como selecionado/ativo.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Registra o handler de clique.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.theme().colors();
        let hover_bg = colors.element_hover;
        let selected_bg = colors.element_active;
        let color = if self.selected {
            Color::Accent
        } else {
            self.color
        };

        h_flex()
            .id(self.id)
            .justify_center()
            .size(px(28.0))
            .rounded_md()
            .when(self.selected, |el| el.bg(selected_bg))
            .hover(move |el| el.bg(hover_bg))
            .child(Icon::new(self.icon).size(self.size).color(color))
            .when_some(self.on_click, |el, handler| {
                el.cursor_pointer()
                    .on_click(move |event, window, cx| handler(event, window, cx))
            })
    }
}
