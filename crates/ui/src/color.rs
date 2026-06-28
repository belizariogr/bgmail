use gpui::{App, Hsla};
use theme::ActiveTheme;

/// Semantic colors resolved from the active theme.
///
/// Instead of scattering literal `Hsla` values across components, we use
/// semantic roles (`Default`, `Muted`, `Accent`, ...) that are resolved at
/// render time against the active [`theme::Theme`]. This way, switching between
/// light and dark automatically reflects in every component.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Color {
    /// Default text/icon color.
    #[default]
    Default,
    /// Muted/secondary content.
    Muted,
    /// Disabled content.
    Disabled,
    /// Accent color (links, counters, active states).
    Accent,
    /// Text over accent/selection surfaces.
    OnAccent,
    /// Success state.
    Success,
    /// Warning state.
    Warning,
    /// Error state.
    Error,
    /// Arbitrary color.
    Custom(Hsla),
}

impl Color {
    /// Resolves the semantic color to a concrete [`Hsla`] using the active theme.
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
