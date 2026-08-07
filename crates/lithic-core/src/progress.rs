use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{Error, Result};

/// A stage of a long-running operation. Frontends turn these into their own
/// wording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
   Resolving,
   Downloading,
   Verifying,
   Extracting,
   Installing,
   Cleaning,
}

#[derive(Debug, Clone)]
pub enum Event {
   Step(Step),
   /// Byte progress for one transfer, keyed by what is being fetched.
   Transfer {
      label: String,
      done: u64,
      total: Option<u64>,
   },
   /// A line worth showing in a detail log.
   Log(String),
}

/// Receives [`Event`]s. Cheap to clone; the no-op reporter drops everything.
#[derive(Clone, Default)]
pub struct Reporter(Option<Arc<dyn Fn(Event) + Send + Sync>>);

impl Reporter {
   pub fn new(f: impl Fn(Event) + Send + Sync + 'static) -> Self {
      Self(Some(Arc::new(f)))
   }

   pub fn none() -> Self {
      Self(None)
   }

   pub fn emit(&self, event: Event) {
      if let Some(f) = &self.0 {
         f(event);
      }
   }

   pub fn step(&self, step: Step) {
      self.emit(Event::Step(step));
   }

   pub fn log(&self, line: impl Into<String>) {
      self.emit(Event::Log(line.into()));
   }
}

impl std::fmt::Debug for Reporter {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      f.write_str("Reporter")
   }
}

/// Shared cancellation flag. Long operations check it between chunks and
/// stop with [`Error::Cancelled`].
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
   pub fn new() -> Self {
      Self::default()
   }

   pub fn cancel(&self) {
      self.0.store(true, Ordering::Relaxed);
   }

   pub fn is_cancelled(&self) -> bool {
      self.0.load(Ordering::Relaxed)
   }

   pub fn check(&self) -> Result<()> {
      if self.is_cancelled() {
         Err(Error::Cancelled)
      } else {
         Ok(())
      }
   }
}
