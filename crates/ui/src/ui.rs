//! # UI
//!
//! Biblioteca de componentes visuais do rMail, inspirada no crate `ui` do Zed.
//!
//! Fornece um conjunto pequeno e reutilizável de componentes (`Label`, `Icon`,
//! `Button`, `IconButton`, `ListItem`) e helpers de layout (`h_flex`, `v_flex`),
//! todos integrados ao sistema de temas (`theme`).
//!
//! O objetivo é manter a API próxima à do Zed para facilitar a portabilidade de
//! padrões, evitando porém o "bloat" de recursos não usados por um cliente de
//! e-mail.

pub mod prelude;

mod button;
mod color;
mod icon;
mod label;
mod list_item;

pub use button::*;
pub use color::*;
pub use icon::*;
pub use label::*;
pub use list_item::*;

pub use prelude::{h_flex, v_flex};

// Reexporta os tipos de tema mais usados para conveniência dos consumidores.
pub use theme::{ActiveTheme, Appearance, Theme, ThemeColors};
