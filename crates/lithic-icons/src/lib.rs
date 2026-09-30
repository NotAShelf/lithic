use std::sync::LazyLock;

use iced::{
  Color,
  Theme,
  widget::{Svg, svg},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
  Instances,
  Browse,
  Games,
  Accounts,
  Settings,
  Play,
  Stop,
  Pin,
  Unpin,
  Trash,
  Folder,
  Refresh,
  Update,
  Download,
  Plus,
  LogOut,
  Star,
  StarFilled,
  Help,
  Back,
  Close,
  User,
  Copy,
  Export,
  Import,
  Select,
  Info,
}

const SOURCES: [&[u8]; 27] = [
  include_bytes!("../assets/icons/boxes.svg"),
  include_bytes!("../assets/icons/search.svg"),
  include_bytes!("../assets/icons/gamepad-2.svg"),
  include_bytes!("../assets/icons/users.svg"),
  include_bytes!("../assets/icons/settings.svg"),
  include_bytes!("../assets/icons/play.svg"),
  include_bytes!("../assets/icons/square.svg"),
  include_bytes!("../assets/icons/pin.svg"),
  include_bytes!("../assets/icons/pin-off.svg"),
  include_bytes!("../assets/icons/trash-2.svg"),
  include_bytes!("../assets/icons/folder-open.svg"),
  include_bytes!("../assets/icons/refresh-cw.svg"),
  include_bytes!("../assets/icons/circle-arrow-up.svg"),
  include_bytes!("../assets/icons/download.svg"),
  include_bytes!("../assets/icons/plus.svg"),
  include_bytes!("../assets/icons/log-out.svg"),
  include_bytes!("../assets/icons/star.svg"),
  include_bytes!("../assets/icons/star.svg"),
  include_bytes!("../assets/icons/circle-help.svg"),
  include_bytes!("../assets/icons/arrow-left.svg"),
  include_bytes!("../assets/icons/x.svg"),
  include_bytes!("../assets/icons/user.svg"),
  include_bytes!("../assets/icons/copy.svg"),
  include_bytes!("../assets/icons/package.svg"),
  include_bytes!("../assets/icons/package-open.svg"),
  include_bytes!("../assets/icons/circle-check.svg"),
  include_bytes!("../assets/icons/info.svg"),
];

static HANDLES: LazyLock<Vec<svg::Handle>> = LazyLock::new(|| {
  SOURCES
    .iter()
    .enumerate()
    .map(|(i, bytes)| {
      if i == Icon::StarFilled as usize {
        let filled = String::from_utf8_lossy(bytes)
          .replace(r#"fill="none""#, r#"fill="currentColor""#);
        svg::Handle::from_memory(filled.into_bytes())
      } else {
        svg::Handle::from_memory(*bytes)
      }
    })
    .collect()
});

impl Icon {
  #[must_use]
  pub fn handle(self) -> svg::Handle {
    HANDLES[self as usize].clone()
  }
}

/// `icon` drawn in the theme's text color.
#[must_use]
pub fn icon<'a>(icon: Icon, size: f32) -> Svg<'a> {
  colored(icon, size, |theme| {
    theme.extended_palette().background.base.text
  })
}

pub fn colored<'a>(
  icon: Icon,
  size: f32,
  color: impl Fn(&Theme) -> Color + 'a,
) -> Svg<'a> {
  svg(icon.handle())
    .width(size)
    .height(size)
    .style(move |theme: &Theme, _| {
      svg::Style {
        color: Some(color(theme)),
      }
    })
}
