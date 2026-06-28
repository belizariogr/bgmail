use gpui::{AnyElement, ClickEvent};

use crate::prelude::*;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// Uma linha clicável de lista, com estados de hover e seleção.
///
/// Aceita um *slot* inicial (geralmente um [`Icon`]), conteúdo principal e um
/// *slot* final (badge/contador). É a base das listas da barra lateral e da
/// lista de mensagens.
#[derive(IntoElement)]
pub struct ListItem {
    id: ElementId,
    selected: bool,
    start_slot: Option<AnyElement>,
    end_slot: Option<AnyElement>,
    children: Vec<AnyElement>,
    on_click: Option<ClickHandler>,
}

impl ListItem {
    /// Cria uma nova linha de lista.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            selected: false,
            start_slot: None,
            end_slot: None,
            children: Vec::new(),
            on_click: None,
        }
    }

    /// Marca a linha como selecionada.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Define o slot inicial (ícone à esquerda).
    pub fn start_slot(mut self, element: impl IntoElement) -> Self {
        self.start_slot = Some(element.into_any_element());
        self
    }

    /// Define o slot final (badge/contador à direita).
    pub fn end_slot(mut self, element: impl IntoElement) -> Self {
        self.end_slot = Some(element.into_any_element());
        self
    }

    /// Adiciona um elemento ao conteúdo principal.
    pub fn child(mut self, element: impl IntoElement) -> Self {
        self.children.push(element.into_any_element());
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

impl RenderOnce for ListItem {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.theme().colors();
        let selected_bg = colors.element_selected;
        let hover_bg = colors.element_hover;

        h_flex()
            .id(self.id)
            .w_full()
            .gap_2()
            .px_2()
            .py_1p5()
            .rounded_md()
            .when(self.selected, |el| el.bg(selected_bg))
            .when(!self.selected, |el| el.hover(move |el| el.bg(hover_bg)))
            .when_some(self.start_slot, |el, slot| el.child(slot))
            .child(
                // Conteúdo principal ocupa o espaço restante.
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .children(self.children),
            )
            .when_some(self.end_slot, |el, slot| el.child(slot))
            .when_some(self.on_click, |el, handler| {
                el.cursor_pointer()
                    .on_click(move |event, window, cx| handler(event, window, cx))
            })
    }
}
