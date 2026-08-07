use std::io::stdin;

use comfy_table::Cell;
use lithic_core::auth::{Account, AuthError};
use lithic_core::{Error, Kind};

use crate::Ctx;
use crate::args::AccountCommand;
use crate::ui::{Failure, Result, Ui, fail};

pub async fn run(ctx: &Ctx, cmd: AccountCommand) -> Result {
   match cmd {
      AccountCommand::Login {
         email,
         password_stdin,
         code,
      } => login(ctx, email, password_stdin, code).await,
      AccountCommand::List => {
         let accounts = ctx.lithic.accounts()?;
         if ctx.ui.json {
            return Ui::print_json(&accounts);
         }
         if accounts.accounts.is_empty() {
            ctx.ui.status("No accounts. Log in with `lithic account login`.");
            return Ok(());
         }
         let mut table = ctx.ui.table();
         table.set_header(vec!["", "Player", "Email", "Id", "Session"]);
         for a in &accounts.accounts {
            let active = accounts.active.as_deref() == Some(a.uid.as_str());
            table.add_row(vec![
               Cell::new(if active { "*" } else { "" }),
               Cell::new(&a.playername),
               Cell::new(&a.email),
               Cell::new(&a.uid),
               Cell::new(if ctx.lithic.has_session(&a.uid) {
                  "stored"
               } else {
                  "missing, log in again"
               }),
            ]);
         }
         Ui::print_table(&table);
         Ok(())
      }
      AccountCommand::Switch { account } => {
         let a = find(ctx, &account)?;
         ctx.lithic.set_active_account(&a.uid)?;
         ctx.ui
            .success(format!("{} is now the active account", a.playername));
         Ok(())
      }
      AccountCommand::Logout { account } => {
         let a = find(ctx, &account)?;
         if !ctx
            .ui
            .confirm(&format!("Log out {} and forget its session?", a.playername))?
         {
            return fail("");
         }
         ctx.lithic.logout(&a.uid)?;
         ctx.ui.success(format!("logged out {}", a.playername));
         Ok(())
      }
   }
}

fn find(ctx: &Ctx, wanted: &str) -> Result<Account> {
   ctx.lithic
      .accounts()?
      .accounts
      .into_iter()
      .find(|a| {
         a.uid == wanted || a.playername.eq_ignore_ascii_case(wanted) || a.email.eq_ignore_ascii_case(wanted)
      })
      .ok_or_else(|| Error::not_found(Kind::Account, wanted).into())
}

async fn login(ctx: &Ctx, email: Option<String>, password_stdin: bool, code: Option<String>) -> Result {
   let email = match email {
      Some(e) => e,
      None => ctx.ui.prompt("Email")?,
   };
   let password = if password_stdin {
      let mut line = String::new();
      stdin().read_line(&mut line).map_err(|e| Failure(e.to_string()))?;
      line.trim_end_matches(['\r', '\n']).to_string()
   } else {
      ctx.ui.prompt_secret(&format!("Password for {email}"))?
   };
   if password.is_empty() {
      return fail("no password given");
   }

   let account = match ctx.lithic.login(&email, &password, None).await {
      Ok(account) => account,
      Err(Error::Auth(AuthError::TwoFactorRequired { prelogintoken })) => {
         let code = match code {
            Some(c) => c,
            None => ctx.ui.prompt("Two-factor code")?,
         };
         ctx.lithic
            .login(&email, &password, Some((&prelogintoken, code.trim())))
            .await?
      }
      Err(e) => return Err(e.into()),
   };
   ctx.ui.success(format!("logged in as {}", account.playername));
   let accounts = ctx.lithic.accounts()?;
   if accounts.active.as_deref() != Some(account.uid.as_str()) {
      ctx.ui.status(format!(
         "Use it by default with `lithic account switch {}`",
         account.playername
      ));
   }
   Ok(())
}
