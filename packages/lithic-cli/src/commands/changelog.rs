use crate::commands::arg_structs::changelog_args::ChangeLogArgs;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL_CONDENSED;
use comfy_table::{CellAlignment, ContentArrangement, Row, Table};
use lithic_core::aliases::ModID;
use lithic_core::api::client::ApiClient;
use lithic_core::config::structs::{CellAttr, CellColor};
use lithic_core::errors::LithicError;
use lithic_core::utils::{html_parse, prep_cell};

/// Display the changelog of a single mod, one row per release.
///
/// The standalone `/api/changelogs` endpoint was retired upstream (now returns
/// `statuscode: "410"`), so we surface the per-release changelog field on the
/// per-mod endpoint instead; that data is still served.
///
/// # Errors
///
/// - The `mod_id` is empty or only whitespace.
/// - The Vintage Story mod API is unreachable or returns a non-200 envelope.
/// - The mod has no releases.
/// - Rendering a release's HTML changelog to text fails.
pub async fn changelog(args: &ChangeLogArgs) -> Result<(), LithicError> {
   if args.mod_id.trim().is_empty() {
      return Err(LithicError::SimpleError(
         "changelog: mod id is required".to_string(),
      ));
   }

   let client = ApiClient::new();
   let fetched = client.fetch_mod(ModID::from(args.mod_id.clone())).await?;
   let mod_json = fetched.mod_json;

   let mut table = Table::new();
   table
      .load_preset(UTF8_FULL_CONDENSED)
      .apply_modifier(UTF8_ROUND_CORNERS)
      .set_content_arrangement(ContentArrangement::Dynamic);

   let header_label = mod_json.name.clone().unwrap_or_else(|| args.mod_id.clone());
   table.set_header(Row::from(vec![
      prep_cell(
         header_label,
         Some(CellColor::Green),
         Some(CellAttr::Bold),
         None,
         Some(CellAlignment::Center),
      ),
      prep_cell(
         "Game Versions",
         Some(CellColor::Green),
         Some(CellAttr::Bold),
         None,
         None,
      ),
      prep_cell(
         "Changelog",
         Some(CellColor::Green),
         Some(CellAttr::Bold),
         None,
         Some(CellAlignment::Center),
      ),
   ]));

   let releases: Vec<_> = if args.show_versions == 0 {
      mod_json.releases.iter().collect()
   } else {
      mod_json.releases.iter().take(args.show_versions).collect()
   };

   if releases.is_empty() {
      return Err(LithicError::SimpleError(format!(
         "No releases found for {}",
         args.mod_id
      )));
   }

   let mut rows: Vec<Row> = Vec::with_capacity(releases.len());
   for (index, release) in releases.iter().enumerate() {
      let version = release.mod_version.clone().unwrap_or_default();
      let game_versions = release.tags.join(", ");
      let mut raw = release.changelog.clone().unwrap_or_default();
      let rendered = if raw.trim().is_empty() {
         "(no changelog)".to_string()
      } else {
         html_parse(&mut raw, 100)?
      };

      let color = if index % 2 == 0 {
         CellColor::Green
      } else {
         CellColor::Cyan
      };

      rows.push(Row::from(vec![
         prep_cell(version, Some(CellColor::Magenta), None, None, None),
         prep_cell(game_versions, Some(CellColor::Yellow), None, Some(','), None),
         prep_cell(rendered, Some(color), None, None, Some(CellAlignment::Left)),
      ]));
   }

   table.add_rows(rows);
   println!("{table}");

   Ok(())
}
