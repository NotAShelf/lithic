use comfy_table::Cell;
use lithic_core::fsutil;
use lithic_core::mods::InstallOptions;
use lithic_core::pack::ExportOptions;

use crate::Ctx;
use crate::args::PackCommand;
use crate::ui::Result;

pub async fn run(ctx: &Ctx, cmd: PackCommand) -> Result {
   match cmd {
      PackCommand::Export {
         id,
         output,
         config,
         bundle_all,
         description,
      } => {
         let instance = ctx.instance(id.as_deref())?;
         let dest = output.unwrap_or_else(|| {
            let name = match fsutil::slugify(&instance.name) {
               s if s.is_empty() => instance.id.clone(),
               s => s,
            };
            std::env::current_dir()
               .unwrap_or_default()
               .join(format!("{name}.lithicpack.zip"))
         });
         let progress = ctx.ui.progress();
         progress.reporter.log("Checking which mods are on the ModDB");
         let result = ctx
            .lithic
            .export_pack(
               &instance,
               &dest,
               &ExportOptions {
                  description,
                  include_config: config,
                  bundle_all,
               },
            )
            .await;
         drop(progress);
         let manifest = result?;
         let bundled = manifest.mods.iter().filter(|m| m.file.is_some()).count();
         ctx.ui.success(format!(
            "wrote {} with {} mods ({bundled} included as files)",
            dest.display(),
            manifest.mods.len()
         ));
         Ok(())
      }
      PackCommand::Import { file, name } => {
         let progress = ctx.ui.progress();
         let result = ctx
            .lithic
            .import_pack(
               &file,
               name.as_deref(),
               &InstallOptions {
                  reporter: progress.reporter.clone(),
                  ..InstallOptions::default()
               },
            )
            .await;
         drop(progress);
         let imported = result?;
         for f in &imported.install.failures {
            ctx.ui
               .warn(format!("{} could not be installed: {}", f.mod_id, f.error));
         }
         ctx.ui.success(format!(
            "created instance `{}` with {} mods from the ModDB and {} from the pack",
            imported.instance.id,
            imported.install.changes.len(),
            imported.bundled
         ));
         if let Some(v) = &imported.instance.game_version {
            super::warn_if_not_installed(ctx, v);
         }
         Ok(())
      }
      PackCommand::Show { file } => {
         let manifest = ctx.lithic.read_pack(&file)?;
         if ctx.ui.json {
            return ctx.ui.print_json(&manifest);
         }
         println!("{}", manifest.name);
         if let Some(d) = &manifest.description {
            println!("{d}");
         }
         println!(
            "Game version: {}",
            manifest.game_version.as_deref().unwrap_or("not set")
         );
         let mut table = ctx.ui.table();
         table.set_header(vec!["Mod", "Version", "Source", "Enabled"]);
         for m in &manifest.mods {
            table.add_row(vec![
               Cell::new(&m.id),
               Cell::new(if m.version.is_empty() {
                  "latest"
               } else {
                  &m.version
               }),
               Cell::new(if m.file.is_some() { "included" } else { "ModDB" }),
               Cell::new(if m.enabled { "yes" } else { "no" }),
            ]);
         }
         ctx.ui.print_table(&table);
         Ok(())
      }
   }
}
