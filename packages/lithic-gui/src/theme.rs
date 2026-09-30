//! Lithic's own light and dark themes: stone surfaces with an ochre accent.

use std::sync::LazyLock;

use iced::{Color, Theme, theme::Palette};

static DARK: LazyLock<Theme> = LazyLock::new(|| {
  Theme::custom("Lithic Dark", Palette {
    background: Color::from_rgb8(0x21, 0x20, 0x1E),
    text:       Color::from_rgb8(0xE6, 0xE1, 0xD8),
    primary:    Color::from_rgb8(0xC4, 0x86, 0x4A),
    success:    Color::from_rgb8(0x86, 0xA8, 0x6E),
    warning:    Color::from_rgb8(0xD9, 0xAD, 0x52),
    danger:     Color::from_rgb8(0xC9, 0x5B, 0x4F),
  })
});

static LIGHT: LazyLock<Theme> = LazyLock::new(|| {
  Theme::custom("Lithic Light", Palette {
    background: Color::from_rgb8(0xF5, 0xF2, 0xEC),
    text:       Color::from_rgb8(0x2A, 0x27, 0x23),
    primary:    Color::from_rgb8(0x9E, 0x5A, 0x22),
    success:    Color::from_rgb8(0x4C, 0x76, 0x3B),
    warning:    Color::from_rgb8(0x9A, 0x6B, 0x12),
    danger:     Color::from_rgb8(0xA8, 0x3E, 0x33),
  })
});

pub fn lithic(dark: bool) -> Theme {
  if dark { DARK.clone() } else { LIGHT.clone() }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn variants_match_their_brightness() {
    assert!(lithic(true).extended_palette().is_dark);
    assert!(!lithic(false).extended_palette().is_dark);
  }
}
