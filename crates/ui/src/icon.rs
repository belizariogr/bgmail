use crate::prelude::*;

/// Tamanhos disponíveis para [`Icon`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum IconSize {
    /// 12px.
    XSmall,
    /// 14px.
    #[default]
    Small,
    /// 16px.
    Medium,
}

impl IconSize {
    /// Tamanho do glifo em si.
    fn glyph(self) -> Pixels {
        match self {
            IconSize::XSmall => px(12.0),
            IconSize::Small => px(14.0),
            IconSize::Medium => px(16.0),
        }
    }
}

/// Conjunto de ícones usados pelo rMail.
///
/// No mock visual os ícones são renderizados como glifos Unicode monocromáticos.
/// A intenção é trocar a implementação interna por SVGs (como no Zed) sem alterar
/// os locais de chamada — por isso o enum abstrai o glifo concreto.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IconName {
    Inbox,
    Sent,
    Drafts,
    Trash,
    Junk,
    Archive,
    Star,
    StarFilled,
    Flag,
    Reply,
    ReplyAll,
    Forward,
    Compose,
    Search,
    Settings,
    Refresh,
    ChevronRight,
    ChevronDown,
    Account,
    Attachment,
    Mail,
}

impl IconName {
    /// Glifo Unicode que representa o ícone no mock.
    fn glyph(self) -> &'static str {
        match self {
            IconName::Inbox => "\u{2709}",        // ✉
            IconName::Sent => "\u{2197}",         // ↗
            IconName::Drafts => "\u{270E}",       // ✎
            IconName::Trash => "\u{2326}",        // ⌦
            IconName::Junk => "\u{26A0}",         // ⚠
            IconName::Archive => "\u{1F5C4}",     // 🗄
            IconName::Star => "\u{2606}",         // ☆
            IconName::StarFilled => "\u{2605}",   // ★
            IconName::Flag => "\u{2691}",         // ⚑
            IconName::Reply => "\u{21A9}",        // ↩
            IconName::ReplyAll => "\u{21BA}",     // ↺
            IconName::Forward => "\u{21AA}",      // ↪
            IconName::Compose => "\u{270D}",      // ✍
            IconName::Search => "\u{2315}",       // ⌕
            IconName::Settings => "\u{2699}",     // ⚙
            IconName::Refresh => "\u{21BB}",      // ↻
            IconName::ChevronRight => "\u{203A}", // ›
            IconName::ChevronDown => "\u{2304}",  // ⌄
            IconName::Account => "\u{25C9}",      // ◉
            IconName::Attachment => "\u{1F4CE}",  // 📎
            IconName::Mail => "\u{2709}",         // ✉
        }
    }
}

/// Um ícone temático.
#[derive(IntoElement)]
pub struct Icon {
    name: IconName,
    size: IconSize,
    color: Color,
}

impl Icon {
    /// Cria um novo ícone.
    pub fn new(name: IconName) -> Self {
        Self {
            name,
            size: IconSize::default(),
            color: Color::default(),
        }
    }

    /// Define o tamanho do ícone.
    pub fn size(mut self, size: IconSize) -> Self {
        self.size = size;
        self
    }

    /// Define a cor semântica do ícone.
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }
}

impl RenderOnce for Icon {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self.color.hsla(cx);
        div()
            .flex()
            .items_center()
            .justify_center()
            .text_size(self.size.glyph())
            .text_color(color)
            .child(self.name.glyph())
    }
}
