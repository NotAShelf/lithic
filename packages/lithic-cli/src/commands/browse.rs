use comfy_table::Cell;
use lithic_core::{
  Freshness,
  moddb::{self, ModSummary, Query, Sort},
  mods::resolve::{Target, pick_release},
  version,
};

use crate::{
  Ctx,
  args::{InfoArgs, SearchArgs, SortArg},
  ui::{Result, Ui, format_count},
};

pub async fn search(ctx: &Ctx, args: SearchArgs) -> Result {
  let query = args.query.join(" ");
  let game = if args.any_version {
    None
  } else {
    match args.game {
      Some(v) => Some(super::resolve_game_version(ctx, &v).await?),
      None => ctx.instance(None).ok().and_then(|i| i.game_version),
    }
  };

  let pool: Vec<ModSummary> = match &game {
    Some(game) => {
      let target = version::minor(game);
      let tags: Vec<i64> = ctx
        .lithic
        .moddb
        .game_versions()
        .await?
        .into_iter()
        .filter(|t| target.is_some() && version::minor(&t.name) == target)
        .map(|t| t.tag_id)
        .collect();
      if tags.is_empty() {
        ctx.ui.warn(format!(
          "the ModDB knows no game version like {game}; showing all mods"
        ));
        ctx.lithic.mod_index(freshness(args.refresh)).await?.mods
      } else {
        ctx
          .lithic
          .moddb
          .mods(&Query {
            game_versions: tags,
            ..Query::default()
          })
          .await?
      }
    },
    None => ctx.lithic.mod_index(freshness(args.refresh)).await?.mods,
  };

  let sort = match args.sort {
    SortArg::Relevance if query.trim().is_empty() => Sort::Downloads,
    SortArg::Relevance => Sort::Relevance,
    SortArg::Downloads => Sort::Downloads,
    SortArg::Follows => Sort::Follows,
    SortArg::Trending => Sort::Trending,
    SortArg::Updated => Sort::Updated,
    SortArg::Name => Sort::Name,
  };
  let hits: Vec<&ModSummary> = moddb::search(&pool, &query, sort)
    .into_iter()
    .filter(|m| !m.mod_ids.is_empty())
    .take(args.limit)
    .collect();

  if ctx.ui.json {
    return Ui::print_json(&hits);
  }
  if hits.is_empty() {
    ctx.ui.status("No mods found.");
    return Ok(());
  }
  let style = ctx.ui.style("search");
  let mut table = ctx.ui.table();
  table.set_header(vec![
    style.header("mod_id", "Id"),
    style.header("name", "Name"),
    style.header("author", "Author"),
    style.header("downloads", "Downloads"),
    style.header("summary", "Summary"),
  ]);
  for m in &hits {
    table.add_row(vec![
      style.cell("mod_id", m.mod_ids.first().map_or("", String::as_str)),
      style.cell("name", &m.name),
      style.cell("author", &m.author),
      style.cell("downloads", format_count(m.downloads)),
      style.cell("summary", m.summary.as_deref().unwrap_or("")),
    ]);
  }
  Ui::print_table(&table);
  if let Some(game) = game {
    ctx.ui.status(format!(
      "Showing mods with releases for {}.x; use --any-version to see all",
      version::minor(&game)
        .map_or_else(|| game.clone(), |(a, b)| format!("{a}.{b}"))
    ));
  }
  Ok(())
}

#[expect(
  clippy::print_stdout,
  reason = "mod details and changelogs are CLI output"
)]
pub async fn info(ctx: &Ctx, args: InfoArgs) -> Result {
  let details = ctx.lithic.moddb.mod_details(&args.id).await?;
  if ctx.ui.json {
    return Ui::print_json(&details);
  }
  let instance = ctx.instance(None).ok();
  let settings = ctx.lithic.settings()?;

  let mut table = ctx.ui.table();
  let mut row = |k: &str, v: String| {
    if !v.is_empty() {
      table.add_row(vec![Cell::new(k), Cell::new(v)]);
    }
  };
  row("Name", details.name.clone());
  row("Id", details.mod_id().unwrap_or_default().to_string());
  row("Author", details.author.clone());
  row("Page", details.page_url());
  row("Side", details.side.clone().unwrap_or_default());
  row("Tags", details.tags.join(", "));
  row(
    "Downloads",
    format!(
      "{} ({} follows)",
      format_count(details.downloads),
      format_count(details.follows)
    ),
  );
  row(
    "Latest release",
    details.last_released.clone().unwrap_or_default(),
  );
  for (label, url) in [
    ("Homepage", &details.homepage_url),
    ("Source", &details.source_url),
    ("Issues", &details.issues_url),
    ("Wiki", &details.wiki_url),
  ] {
    row(label, url.clone().unwrap_or_default());
  }
  if let Some(instance) = &instance {
    let target = Target {
      game_version:     instance.game_version.clone(),
      allow_prerelease: settings.mods.allow_prerelease,
    };
    let verdict = match pick_release(&details, &target, None) {
      Ok(r) => {
        format!(
          "{} would be installed",
          r.version.clone().unwrap_or_default()
        )
      },
      Err(e) => e.to_string(),
    };
    row(&format!("For {}", instance.name), verdict);
  }
  Ui::print_table(&table);

  let description = moddb::html_to_text(&details.text, 100);
  if !description.is_empty() {
    println!("\n{description}");
  }

  if args.releases {
    let mut releases = ctx.ui.table();
    releases.set_header(vec![
      "Version",
      "Game versions",
      "Released",
      "Downloads",
    ]);
    for r in &details.releases {
      releases.add_row(vec![
        Cell::new(r.version.clone().unwrap_or_default()),
        Cell::new(summarize_tags(&r.tags)),
        Cell::new(r.created.clone().unwrap_or_default()),
        Cell::new(format_count(r.downloads)),
      ]);
    }
    println!();
    Ui::print_table(&releases);
  }

  if args.changelog {
    for r in details.releases.iter().take(5) {
      let Some(changelog) =
        r.changelog.as_deref().filter(|c| !c.trim().is_empty())
      else {
        continue;
      };
      println!("\n{}", r.version.as_deref().unwrap_or("?"));
      println!("{}", moddb::html_to_text(changelog, 100));
    }
  }
  Ok(())
}

const fn freshness(refresh: bool) -> Freshness {
  if refresh {
    Freshness::Refresh
  } else {
    Freshness::Cached
  }
}

/// `1.21.0, 1.21.1, 1.21.2` becomes `1.21.0 to 1.21.2`.
fn summarize_tags(tags: &[String]) -> String {
  let mut sorted: Vec<&String> = tags.iter().collect();
  sorted.sort_by(|a, b| version::compare(a, b));
  match sorted.as_slice() {
    [] => String::new(),
    [one] => (*one).clone(),
    [first, .., last] => format!("{first} to {last}"),
  }
}

#[cfg(test)]
mod tests {
  use super::summarize_tags;

  #[test]
  fn tag_ranges() {
    let tags =
      |t: &[&str]| t.iter().map(ToString::to_string).collect::<Vec<_>>();
    assert_eq!(
      summarize_tags(&tags(&["1.21.2", "1.21.0", "1.21.10"])),
      "1.21.0 to 1.21.10"
    );
    assert_eq!(summarize_tags(&tags(&["1.20.0"])), "1.20.0");
    assert_eq!(summarize_tags(&[]), "");
  }
}
