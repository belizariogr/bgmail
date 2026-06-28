//! # Theme
//!
//! Sistema de temas do rMail, modelado a partir do crate `theme` do Zed, porém
//! enxuto e focado nos elementos necessários para um cliente de e-mail.
//!
//! Um [`Theme`] é uma coleção de cores ([`ThemeColors`]) usada para construir
//! uma aparência consistente em todos os componentes da UI. Há dois temas
//! embutidos: escuro (baseado no *VSCode Dark Modern*) e claro (baseado no
//! *VSCode Light Modern*).
//!
//! O tema ativo é armazenado como um [`gpui::Global`] e acessado através da
//! trait [`ActiveTheme`], implementada para [`App`]. Isso permite escrever
//! `cx.theme().colors().background` em qualquer componente.

use std::sync::Arc;

use gpui::{rgb, App, Global, Hsla};

/// Converte uma cor hexadecimal (`0xRRGGBB`) em [`Hsla`].
///
/// Centraliza a conversão para que as paletas sejam declaradas de forma legível
/// usando valores hexadecimais, como nos arquivos de tema do VSCode.
#[inline]
fn hex(value: u32) -> Hsla {
    rgb(value).into()
}

/// Aparência do tema: claro ou escuro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Appearance {
    /// Tema claro.
    Light,
    /// Tema escuro.
    Dark,
}

impl Appearance {
    /// Retorna `true` se a aparência for clara.
    pub fn is_light(self) -> bool {
        matches!(self, Appearance::Light)
    }

    /// Retorna a aparência oposta (usado pelo botão de alternância de tema).
    pub fn toggled(self) -> Self {
        match self {
            Appearance::Light => Appearance::Dark,
            Appearance::Dark => Appearance::Light,
        }
    }
}

/// Conjunto de cores que define a aparência da UI.
///
/// Os nomes seguem a convenção do Zed (`background`, `surface_background`,
/// `element_hover`, etc.) para facilitar a portabilidade de componentes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeColors {
    /// Fundo principal da aplicação e do painel de leitura.
    pub background: Hsla,
    /// Fundo de superfícies "fixas" como a barra lateral e listas.
    pub surface_background: Hsla,
    /// Fundo de superfícies elevadas (menus de contexto, popovers, diálogos).
    pub elevated_surface_background: Hsla,

    /// Cor de borda padrão (divisores entre painéis).
    pub border: Hsla,
    /// Borda de menor contraste, para divisões sutis.
    pub border_variant: Hsla,
    /// Borda de elemento em foco (foco de teclado).
    pub border_focused: Hsla,

    /// Fundo de um elemento interativo (botão, input).
    pub element_background: Hsla,
    /// Fundo de elemento sob hover do mouse.
    pub element_hover: Hsla,
    /// Fundo de elemento pressionado/ativo.
    pub element_active: Hsla,
    /// Fundo de elemento selecionado (ex.: item de lista ativo).
    pub element_selected: Hsla,

    /// Cor de texto padrão.
    pub text: Hsla,
    /// Texto atenuado/secundário (prévia de mensagem, horário).
    pub text_muted: Hsla,
    /// Texto desabilitado.
    pub text_disabled: Hsla,
    /// Texto de destaque/acento (links, contadores).
    pub text_accent: Hsla,
    /// Texto sobre superfícies selecionadas/acento.
    pub text_on_accent: Hsla,

    /// Cor de preenchimento padrão de ícones.
    pub icon: Hsla,
    /// Cor de ícones atenuados.
    pub icon_muted: Hsla,
    /// Cor de ícones de acento (estado ativo).
    pub icon_accent: Hsla,

    /// Cor de acento principal (azul de seleção/realce).
    pub accent: Hsla,

    /// Fundo da barra de título / toolbar superior.
    pub title_bar_background: Hsla,
    /// Fundo da barra de status inferior.
    pub status_bar_background: Hsla,
    /// Fundo do painel lateral (lista de contas/caixas).
    pub panel_background: Hsla,

    /// Cor de sucesso (ex.: conexão estabelecida).
    pub success: Hsla,
    /// Cor de aviso.
    pub warning: Hsla,
    /// Cor de erro.
    pub error: Hsla,
}

/// Um tema completo: identidade + aparência + cores.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// Nome legível do tema.
    pub name: &'static str,
    /// Se é claro ou escuro.
    pub appearance: Appearance,
    /// As cores do tema.
    pub colors: ThemeColors,
}

impl Theme {
    /// Atalho para acessar as cores do tema.
    #[inline]
    pub fn colors(&self) -> &ThemeColors {
        &self.colors
    }

    /// Atalho para a aparência do tema.
    #[inline]
    pub fn appearance(&self) -> Appearance {
        self.appearance
    }

    /// Tema escuro embutido, baseado no *VSCode Dark Modern*.
    pub fn dark() -> Self {
        Theme {
            name: "rMail Dark",
            appearance: Appearance::Dark,
            colors: ThemeColors {
                background: hex(0x1f1f1f),
                surface_background: hex(0x181818),
                elevated_surface_background: hex(0x252526),

                border: hex(0x2b2b2b),
                border_variant: hex(0x313131),
                border_focused: hex(0x0078d4),

                element_background: hex(0x313131),
                element_hover: hex(0x2a2d2e),
                element_active: hex(0x37373d),
                element_selected: hex(0x04395e),

                text: hex(0xcccccc),
                text_muted: hex(0x9d9d9d),
                text_disabled: hex(0x5a5a5a),
                text_accent: hex(0x4daafc),
                text_on_accent: hex(0xffffff),

                icon: hex(0xcccccc),
                icon_muted: hex(0x858585),
                icon_accent: hex(0x4daafc),

                accent: hex(0x0078d4),

                title_bar_background: hex(0x181818),
                status_bar_background: hex(0x181818),
                panel_background: hex(0x181818),

                success: hex(0x89d185),
                warning: hex(0xcca700),
                error: hex(0xf14c4c),
            },
        }
    }

    /// Tema claro embutido, baseado no *VSCode Light Modern*.
    pub fn light() -> Self {
        Theme {
            name: "rMail Light",
            appearance: Appearance::Light,
            colors: ThemeColors {
                background: hex(0xffffff),
                surface_background: hex(0xf8f8f8),
                elevated_surface_background: hex(0xffffff),

                border: hex(0xe5e5e5),
                border_variant: hex(0xeaeaea),
                border_focused: hex(0x005fb8),

                element_background: hex(0xf3f3f3),
                element_hover: hex(0xf0f0f0),
                element_active: hex(0xe4e6f1),
                element_selected: hex(0xcfe3fa),

                text: hex(0x3b3b3b),
                text_muted: hex(0x767676),
                text_disabled: hex(0xa0a0a0),
                text_accent: hex(0x005fb8),
                text_on_accent: hex(0xffffff),

                icon: hex(0x3b3b3b),
                icon_muted: hex(0x616161),
                icon_accent: hex(0x005fb8),

                accent: hex(0x005fb8),

                title_bar_background: hex(0xf8f8f8),
                status_bar_background: hex(0xf8f8f8),
                panel_background: hex(0xf8f8f8),

                success: hex(0x1a7f37),
                warning: hex(0xbf8803),
                error: hex(0xcc2936),
            },
        }
    }

    /// Retorna o tema embutido correspondente à [`Appearance`] informada.
    pub fn for_appearance(appearance: Appearance) -> Self {
        match appearance {
            Appearance::Light => Theme::light(),
            Appearance::Dark => Theme::dark(),
        }
    }
}

/// Estado global que guarda o tema ativo da aplicação.
pub struct GlobalTheme(pub Arc<Theme>);

impl Global for GlobalTheme {}

/// Inicializa o sistema de temas no [`App`] com a aparência informada.
pub fn init(appearance: Appearance, cx: &mut App) {
    cx.set_global(GlobalTheme(Arc::new(Theme::for_appearance(appearance))));
}

/// Substitui o tema ativo.
pub fn set_theme(theme: Theme, cx: &mut App) {
    cx.set_global(GlobalTheme(Arc::new(theme)));
}

/// Alterna entre o tema claro e o escuro, retornando a nova aparência.
pub fn toggle_appearance(cx: &mut App) -> Appearance {
    let next = cx.global::<GlobalTheme>().0.appearance.toggled();
    set_theme(Theme::for_appearance(next), cx);
    next
}

/// Trait para acessar o tema ativo a partir de um contexto.
pub trait ActiveTheme {
    /// Retorna o tema ativo.
    fn theme(&self) -> &Arc<Theme>;
}

impl ActiveTheme for App {
    fn theme(&self) -> &Arc<Theme> {
        &self.global::<GlobalTheme>().0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_toggles() {
        assert_eq!(Appearance::Light.toggled(), Appearance::Dark);
        assert_eq!(Appearance::Dark.toggled(), Appearance::Light);
        assert!(Appearance::Light.is_light());
        assert!(!Appearance::Dark.is_light());
    }

    #[test]
    fn builtin_themes_have_matching_appearance() {
        assert_eq!(Theme::dark().appearance(), Appearance::Dark);
        assert_eq!(Theme::light().appearance(), Appearance::Light);
        assert_eq!(Theme::dark().name, "rMail Dark");
        assert_eq!(Theme::light().name, "rMail Light");
    }

    #[test]
    fn for_appearance_returns_correct_theme() {
        assert_eq!(Theme::for_appearance(Appearance::Dark), Theme::dark());
        assert_eq!(Theme::for_appearance(Appearance::Light), Theme::light());
    }

    #[test]
    fn dark_and_light_differ() {
        assert_ne!(
            Theme::dark().colors().background,
            Theme::light().colors().background
        );
    }

    #[test]
    fn hex_conversion_is_stable() {
        // Branco e preto puros devem permanecer nos extremos de luminosidade.
        assert!(hex(0xffffff).l > 0.99);
        assert!(hex(0x000000).l < 0.01);
    }
}
