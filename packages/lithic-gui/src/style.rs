//! Widget styles derived from the active theme's palette, so they work in
//! the Lithic themes and the presets alike.

use iced::{
  Background,
  Border,
  Color,
  Shadow,
  Theme,
  Vector,
  widget::{button, container, pick_list, rule, text, text_editor, text_input},
};

pub const RADIUS: f32 = 8.0;
/// Corner radius of buttons, inputs and other small controls.
pub const CONTROL_RADIUS: f32 = 6.0;

/// `color` scaled towards black; `keep` of 1.0 leaves it unchanged.
fn shade(color: Color, keep: f32) -> Color {
  Color {
    r: color.r * keep,
    g: color.g * keep,
    b: color.b * keep,
    a: color.a,
  }
}

/// Raised surface for grouped content.
pub fn card(theme: &Theme) -> container::Style {
  let p = theme.extended_palette();
  container::Style {
    background: Some(p.background.weakest.color.into()),
    border: Border {
      color:  p.background.weak.color,
      width:  1.0,
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
      color:  p.background.weak.color,
      width:  1.0,
      radius: 0.0.into(),
    },
    ..container::Style::default()
  }
}

/// The navigation column, a shade darker than the content.
pub fn sidebar(theme: &Theme) -> container::Style {
  let p = theme.extended_palette();
  let keep = if p.is_dark { 0.8 } else { 0.955 };
  container::Style {
    background: Some(shade(p.background.base.color, keep).into()),
    text_color: Some(p.background.base.text),
    ..container::Style::default()
  }
}

/// The account strip under the navigation, darker again than the sidebar.
pub fn sidebar_footer(theme: &Theme) -> container::Style {
  let p = theme.extended_palette();
  let keep = if p.is_dark { 0.6 } else { 0.9 };
  container::Style {
    background: Some(shade(p.background.base.color, keep).into()),
    text_color: Some(p.background.base.text),
    ..container::Style::default()
  }
}

pub fn nav(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
  move |theme, status| {
    let p = theme.extended_palette();
    let background = match (active, status) {
      (true, _)
      | (false, button::Status::Hovered | button::Status::Pressed) => {
        Some(Background::Color(p.background.weak.color))
      },
      (false, _) => None,
    };
    button::Style {
      background,
      text_color: p.background.base.text,
      border: Border {
        radius: CONTROL_RADIUS.into(),
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
    button::Status::Hovered | button::Status::Pressed => {
      p.background.weaker.color
    },
    _ => p.background.weakest.color,
  };
  button::Style {
    background: Some(background.into()),
    text_color: p.background.base.text,
    border: Border {
      color:  p.background.weak.color,
      width:  1.0,
      radius: RADIUS.into(),
    },
    ..button::Style::default()
  }
}

/// How much a button stands out. Each view has at most one primary action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
  Primary,
  Secondary,
  Ghost,
  Danger,
}

impl Kind {
  /// The color of a button's label and icon.
  pub fn text(self, theme: &Theme) -> Color {
    let p = theme.extended_palette();
    match self {
      Self::Primary => p.primary.base.text,
      Self::Danger => p.danger.base.text,
      Self::Secondary | Self::Ghost => p.background.base.text,
    }
  }
}

pub fn btn(kind: Kind) -> impl Fn(&Theme, button::Status) -> button::Style {
  move |theme, status| {
    let p = theme.extended_palette();
    let hovered =
      matches!(status, button::Status::Hovered | button::Status::Pressed);
    let (background, border) = match kind {
      Kind::Primary => {
        let pair = if hovered {
          p.primary.strong
        } else {
          p.primary.base
        };
        (Some(pair.color), Color::TRANSPARENT)
      },
      Kind::Danger => {
        let pair = if hovered {
          p.danger.strong
        } else {
          p.danger.base
        };
        (Some(pair.color), Color::TRANSPARENT)
      },
      Kind::Secondary => {
        let fill = if hovered {
          p.background.weak.color
        } else {
          p.background.weakest.color
        };
        (Some(fill), p.background.weak.color)
      },
      Kind::Ghost => {
        (
          hovered.then_some(p.background.weak.color),
          Color::TRANSPARENT,
        )
      },
    };
    let mut style = button::Style {
      background: background.map(Background::Color),
      text_color: kind.text(theme),
      border: Border {
        color:  border,
        width:  1.0,
        radius: CONTROL_RADIUS.into(),
      },
      ..button::Style::default()
    };
    if status == button::Status::Disabled {
      style.text_color = style.text_color.scale_alpha(0.45);
      style.background = style.background.map(|b| b.scale_alpha(0.45));
    }
    style
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

impl Tone {
  pub fn color(self, theme: &Theme) -> Color {
    let p = theme.extended_palette();
    match self {
      Self::Neutral => p.background.base.text.scale_alpha(0.62),
      Self::Accent => p.primary.base.color,
      Self::Good => p.success.base.color,
      Self::Warn => p.warning.base.color,
      Self::Bad => p.danger.base.color,
    }
  }
}

/// Status text colored by tone.
pub fn tone(tone: Tone) -> impl Fn(&Theme) -> text::Style {
  move |theme| {
    text::Style {
      color: Some(tone.color(theme)),
    }
  }
}

/// A panel tinted by tone, for inline notices.
pub fn notice(tone: Tone) -> impl Fn(&Theme) -> container::Style {
  move |theme| {
    let p = theme.extended_palette();
    let pair = match tone {
      Tone::Neutral | Tone::Accent => p.background.weak,
      Tone::Good => p.success.weak,
      Tone::Warn => p.warning.weak,
      Tone::Bad => p.danger.weak,
    };
    container::Style {
      background: Some(pair.color.into()),
      text_color: Some(pair.text),
      border: Border {
        radius: RADIUS.into(),
        ..Border::default()
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
      color:  p.background.weak.color,
      width:  1.0,
      radius: 10.0.into(),
    },
    shadow: Shadow {
      color:       Color::from_rgba(0.0, 0.0, 0.0, 0.35),
      offset:      Vector::new(0.0, 8.0),
      blur_radius: 24.0,
    },
    ..container::Style::default()
  }
}

pub fn toast(tone: Tone) -> impl Fn(&Theme) -> container::Style {
  move |theme| {
    let p = theme.extended_palette();
    container::Style {
      background: Some(p.background.base.color.into()),
      text_color: Some(p.background.base.text),
      border: Border {
        color:  tone.color(theme),
        width:  1.0,
        radius: RADIUS.into(),
      },
      shadow: Shadow {
        color:       Color::from_rgba(0.0, 0.0, 0.0, 0.3),
        offset:      Vector::new(0.0, 4.0),
        blur_radius: 12.0,
      },
      ..container::Style::default()
    }
  }
}

/// Muted text for secondary details.
pub fn muted(theme: &Theme) -> text::Style {
  text::Style {
    color: Some(Tone::Neutral.color(theme)),
  }
}

pub fn tab(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
  move |theme, status| {
    let p = theme.extended_palette();
    let hovered =
      matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
      background: (active || hovered)
        .then_some(Background::Color(p.background.weak.color)),
      text_color: if active {
        p.background.base.text
      } else {
        Tone::Neutral.color(theme)
      },
      border: Border {
        radius: CONTROL_RADIUS.into(),
        ..Border::default()
      },
      ..button::Style::default()
    }
  }
}

pub fn tooltip(theme: &Theme) -> container::Style {
  let p = theme.extended_palette();
  container::Style {
    background: Some(p.background.base.color.into()),
    text_color: Some(p.background.base.text),
    border: Border {
      color:  p.background.strong.color,
      width:  1.0,
      radius: CONTROL_RADIUS.into(),
    },
    shadow: Shadow {
      color:       Color::from_rgba(0.0, 0.0, 0.0, 0.25),
      offset:      Vector::new(0.0, 2.0),
      blur_radius: 8.0,
    },
    ..container::Style::default()
  }
}

pub fn divider(theme: &Theme) -> rule::Style {
  rule::Style {
    color:     theme.extended_palette().background.weak.color,
    radius:    0.0.into(),
    fill_mode: rule::FillMode::Full,
    snap:      true,
  }
}

/// The frame text inputs and drop-downs share, so they match the buttons.
fn field_border(theme: &Theme, focused: bool, hovered: bool) -> Border {
  let p = theme.extended_palette();
  Border {
    color:  if focused {
      p.primary.base.color
    } else if hovered {
      p.background.strong.color
    } else {
      p.background.weak.color
    },
    width:  1.0,
    radius: CONTROL_RADIUS.into(),
  }
}

pub fn input(theme: &Theme, status: text_input::Status) -> text_input::Style {
  let p = theme.extended_palette();
  text_input::Style {
    background:  Background::Color(p.background.weakest.color),
    border:      field_border(
      theme,
      matches!(status, text_input::Status::Focused { .. }),
      status == text_input::Status::Hovered,
    ),
    icon:        Tone::Neutral.color(theme),
    placeholder: Tone::Neutral.color(theme),
    value:       if status == text_input::Status::Disabled {
      Tone::Neutral.color(theme)
    } else {
      p.background.base.text
    },
    selection:   p.primary.weak.color,
  }
}

pub fn editor(
  theme: &Theme,
  status: text_editor::Status,
) -> text_editor::Style {
  let p = theme.extended_palette();
  text_editor::Style {
    background:  Background::Color(p.background.weakest.color),
    border:      field_border(
      theme,
      matches!(status, text_editor::Status::Focused { .. }),
      status == text_editor::Status::Hovered,
    ),
    placeholder: Tone::Neutral.color(theme),
    value:       p.background.base.text,
    selection:   p.primary.weak.color,
  }
}

pub fn select(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
  let p = theme.extended_palette();
  pick_list::Style {
    text_color:        p.background.base.text,
    placeholder_color: Tone::Neutral.color(theme),
    handle_color:      Tone::Neutral.color(theme),
    background:        Background::Color(p.background.weakest.color),
    border:            field_border(
      theme,
      matches!(status, pick_list::Status::Opened { .. }),
      status == pick_list::Status::Hovered,
    ),
  }
}

/// Stands in for an image that has not loaded.
pub fn placeholder(theme: &Theme) -> container::Style {
  container::Style {
    background: Some(theme.extended_palette().background.weak.color.into()),
    border: Border {
      radius: CONTROL_RADIUS.into(),
      ..Border::default()
    },
    ..container::Style::default()
  }
}

/// A small round status light.
pub fn dot(tone: Tone) -> impl Fn(&Theme) -> container::Style {
  move |theme| {
    container::Style {
      background: Some(tone.color(theme).into()),
      border: Border {
        radius: 5.0.into(),
        ..Border::default()
      },
      ..container::Style::default()
    }
  }
}
