//! Widget styles derived from the active theme's palette, so they work in
//! light, dark and preset themes alike.

use iced::widget::{button, container, text};
use iced::{Background, Border, Color, Shadow, Theme, Vector};

pub const RADIUS: f32 = 8.0;

/// Raised surface for grouped content.
pub fn card(theme: &Theme) -> container::Style {
   let p = theme.extended_palette();
   container::Style {
      background: Some(p.background.weakest.color.into()),
      border: Border {
         color: p.background.weak.color,
         width: 1.0,
         radius: RADIUS.into(),
      },
      ..container::Style::default()
   }
}

pub fn list_row(theme: &Theme) -> container::Style {
   let p = theme.extended_palette();
   container::Style {
      background: Some(p.background.weakest.color.into()),
      border: Border {
         color: p.background.weak.color,
         width: 1.0,
         radius: 0.0.into(),
      },
      ..container::Style::default()
   }
}

pub fn sidebar(theme: &Theme) -> container::Style {
   let p = theme.extended_palette();
   container::Style {
      background: Some(p.background.weaker.color.into()),
      border: Border {
         color: p.background.weak.color,
         width: 1.0,
         radius: 0.0.into(),
      },
      ..container::Style::default()
   }
}

pub fn nav(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
   move |theme, status| {
      let p = theme.extended_palette();
      let (background, text_color) = match (active, status) {
         (true, _) => (Some(p.primary.weak.color), p.primary.weak.text),
         (false, button::Status::Hovered | button::Status::Pressed) => {
            (Some(p.background.weak.color), p.background.weak.text)
         }
         (false, _) => (None, p.background.base.text),
      };
      button::Style {
         background: background.map(Background::Color),
         text_color,
         border: Border {
            radius: 6.0.into(),
            ..Border::default()
         },
         ..button::Style::default()
      }
   }
}

/// A card that reacts to the pointer, for clickable list rows.
pub fn row_card(theme: &Theme, status: button::Status) -> button::Style {
   let p = theme.extended_palette();
   let background = match status {
      button::Status::Hovered | button::Status::Pressed => p.background.weak.color,
      _ => p.background.weakest.color,
   };
   button::Style {
      background: Some(background.into()),
      text_color: p.background.base.text,
      border: Border {
         color: p.background.weak.color,
         width: 1.0,
         radius: RADIUS.into(),
      },
      ..button::Style::default()
   }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
   Neutral,
   Accent,
   Good,
   Warn,
   Bad,
}

pub fn badge(tone: Tone) -> impl Fn(&Theme) -> container::Style {
   move |theme| {
      let p = theme.extended_palette();
      let pair = match tone {
         Tone::Neutral => p.background.weak,
         Tone::Accent => p.primary.weak,
         Tone::Good => p.success.weak,
         Tone::Warn => p.warning.weak,
         Tone::Bad => p.danger.weak,
      };
      container::Style {
         background: Some(pair.color.into()),
         text_color: Some(pair.text),
         border: Border {
            radius: 10.0.into(),
            ..Border::default()
         },
         ..container::Style::default()
      }
   }
}

/// A panel tinted by tone, for inline notices.
pub fn notice(tone: Tone) -> impl Fn(&Theme) -> container::Style {
   move |theme| {
      let p = theme.extended_palette();
      let (pair, edge) = match tone {
         Tone::Neutral | Tone::Accent => (p.primary.weak, p.primary.base.color),
         Tone::Good => (p.success.weak, p.success.base.color),
         Tone::Warn => (p.warning.weak, p.warning.base.color),
         Tone::Bad => (p.danger.weak, p.danger.base.color),
      };
      container::Style {
         background: Some(pair.color.into()),
         text_color: Some(pair.text),
         border: Border {
            color: edge,
            width: 1.0,
            radius: RADIUS.into(),
         },
         ..container::Style::default()
      }
   }
}

pub fn backdrop(_theme: &Theme) -> container::Style {
   container::Style {
      background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
      ..container::Style::default()
   }
}

pub fn dialog(theme: &Theme) -> container::Style {
   let p = theme.extended_palette();
   container::Style {
      background: Some(p.background.base.color.into()),
      text_color: Some(p.background.base.text),
      border: Border {
         color: p.background.strong.color,
         width: 1.0,
         radius: 12.0.into(),
      },
      shadow: Shadow {
         color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
         offset: Vector::new(0.0, 8.0),
         blur_radius: 24.0,
      },
      ..container::Style::default()
   }
}

pub fn toast(tone: Tone) -> impl Fn(&Theme) -> container::Style {
   move |theme| {
      let p = theme.extended_palette();
      let edge = match tone {
         Tone::Neutral | Tone::Accent => p.primary.base.color,
         Tone::Good => p.success.base.color,
         Tone::Warn => p.warning.base.color,
         Tone::Bad => p.danger.base.color,
      };
      container::Style {
         background: Some(p.background.base.color.into()),
         text_color: Some(p.background.base.text),
         border: Border {
            color: edge,
            width: 2.0,
            radius: RADIUS.into(),
         },
         shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.3),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 12.0,
         },
         ..container::Style::default()
      }
   }
}

/// Muted text for secondary details.
pub fn muted(theme: &Theme) -> text::Style {
   let p = theme.extended_palette();
   text::Style {
      color: Some(Color {
         a: 0.62,
         ..p.background.base.text
      }),
   }
}

pub fn tab(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
   move |theme, status| {
      let p = theme.extended_palette();
      let text_color = if active {
         p.primary.base.color
      } else {
         p.background.base.text
      };
      let background = match status {
         button::Status::Hovered | button::Status::Pressed if !active => Some(p.background.weak.color),
         _ => None,
      };
      button::Style {
         background: background.map(Background::Color),
         text_color,
         border: Border {
            color: if active {
               p.primary.base.color
            } else {
               Color::TRANSPARENT
            },
            width: if active { 2.0 } else { 0.0 },
            radius: 6.0.into(),
         },
         ..button::Style::default()
      }
   }
}
