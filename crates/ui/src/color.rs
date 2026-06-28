use gpui::{App, Hsla};
use theme::ActiveTheme;

/// Cores semânticas resolvidas a partir do tema ativo.
///
/// Em vez de espalhar valores `Hsla` literais pelos componentes, usamos papéis
/// semânticos (`Default`, `Muted`, `Accent`, ...) que são resolvidos em tempo de
/// renderização contra o [`theme::Theme`] ativo. Assim, alternar entre claro e
/// escuro reflete automaticamente em todos os componentes.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Color {
    /// Cor de texto/ícone padrão.
    #[default]
    Default,
    /// Conteúdo atenuado/secundário.
    Muted,
    /// Conteúdo desabilitado.
    Disabled,
    /// Cor de acento (links, contadores, estados ativos).
    Accent,
    /// Texto sobre superfícies de acento/seleção.
    OnAccent,
    /// Estado de sucesso.
    Success,
    /// Estado de aviso.
    Warning,
    /// Estado de erro.
    Error,
    /// Cor arbitrária.
    Custom(Hsla),
}

impl Color {
    /// Resolve a cor semântica para um [`Hsla`] concreto usando o tema ativo.
    pub fn hsla(self, cx: &App) -> Hsla {
        let colors = cx.theme().colors();
        match self {
            Color::Default => colors.text,
            Color::Muted => colors.text_muted,
            Color::Disabled => colors.text_disabled,
            Color::Accent => colors.text_accent,
            Color::OnAccent => colors.text_on_accent,
            Color::Success => colors.success,
            Color::Warning => colors.warning,
            Color::Error => colors.error,
            Color::Custom(color) => color,
        }
    }
}
