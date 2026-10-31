//! The UI's text scale: every UI text size derives from one base size, so
//! the interface grows or shrinks as a whole.

use gpui::{px, Pixels};

/// Base UI text size (px): body text such as list rows and labels.
pub const UI_FONT_SIZE: f32 = 15.0;

/// A UI text size `offset` px from [`UI_FONT_SIZE`] (e.g. `-2.0` for
/// secondary text).
pub fn ui_text_size(offset: f32) -> Pixels {
    px(UI_FONT_SIZE + offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_follow_the_base() {
        assert_eq!(ui_text_size(0.0), px(UI_FONT_SIZE));
        assert_eq!(ui_text_size(-2.0), px(UI_FONT_SIZE - 2.0));
    }
}
