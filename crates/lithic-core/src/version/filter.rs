use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Default)]
pub enum VersionFilter {
   #[default]
   Any,
   Exact(String),
   AtLeast(String),
}

impl VersionFilter {
   /// Render the filter as the user sees it. Returns `None` for `Any` so the
   /// caller (which has access to a localiser) can produce the appropriate
   /// translation rather than embedding the English string in core.
   pub fn label(&self) -> Option<String> {
      match self {
         VersionFilter::Any => None,
         VersionFilter::Exact(v) => Some(v.clone()),
         VersionFilter::AtLeast(v) => Some(format!("{v}+")),
      }
   }

   pub fn minor_key(&self) -> Option<&str> {
      match self {
         VersionFilter::Exact(v) | VersionFilter::AtLeast(v) => Some(v),
         VersionFilter::Any => None,
      }
   }
}

/// Extracts MAJOR.MINOR from any version string.
/// "1.20.4-rc.2" -> Some("1.20"), "1.19" -> Some("1.19"), "v1.18.15" -> Some("1.18")
pub fn minor_version(s: &str) -> Option<String> {
   let s = s.trim_start_matches('v');
   let mut parts = s.splitn(3, '.');
   let major = parts.next()?;
   let minor_raw = parts.next()?;
   let minor = minor_raw.split('-').next().unwrap_or(minor_raw);
   if major.parse::<u32>().is_ok() && minor.parse::<u32>().is_ok() {
      Some(format!("{major}.{minor}"))
   } else {
      None
   }
}

fn parse_minor(s: &str) -> (u32, u32) {
   let mut p = s.split('.');
   let major = p.next().and_then(|x| x.parse().ok()).unwrap_or(0);
   let minor = p.next().and_then(|x| x.parse().ok()).unwrap_or(0);
   (major, minor)
}

/// Returns unique MAJOR.MINOR versions sorted descending (newest first).
pub fn unique_minor_versions(versions: &[String]) -> Vec<String> {
   let mut seen = HashSet::new();
   let mut result: Vec<String> = versions
      .iter()
      .filter_map(|v| minor_version(v))
      .filter(|mv| seen.insert(mv.clone()))
      .collect();
   result.sort_by_key(|v| std::cmp::Reverse(parse_minor(v)));
   result
}

pub fn cmp_minor(a: &str, b: &str) -> std::cmp::Ordering {
   parse_minor(a).cmp(&parse_minor(b))
}

/// Returns all minor versions from `all` that are >= `min`, preserving their order.
pub fn minor_versions_at_least<'a>(all: &'a [String], min: &str) -> Vec<&'a String> {
   all.iter()
      .filter(|v| cmp_minor(v, min) != std::cmp::Ordering::Less)
      .collect()
}
