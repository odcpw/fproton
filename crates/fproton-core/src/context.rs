//! The one value every core call takes (DESIGN.md §8).
//!
//! Plain data: `Clone + Send + Sync + 'static`, no open handles, no interior state.
//! The TUI owns its own clone; the binary builds one per invocation.

use serde::Serialize;

use crate::error::Error;
use crate::paths::Paths;
use crate::settings::{Settings, Timeouts};

/// Settings, paths and deadlines, plus how the caller wants to be talked to.
#[derive(Clone, Debug, Serialize)]
pub struct Context {
    pub settings: Settings,
    pub paths: Paths,
    pub timeouts: Timeouts,
    /// The caller asked for machine output. Core never prints; the TUI and the
    /// doctor use it to decide their own rendering.
    pub json: bool,
    /// `-v` repetitions.
    pub verbose: u8,
}

impl Context {
    /// Discover paths, read `config.toml`, take the deadlines from it.
    pub fn load() -> Result<Context, Error> {
        let paths = Paths::discover()?;
        let settings = Settings::load(&paths)?;
        let timeouts = settings.timeouts;
        Ok(Context {
            settings,
            paths,
            timeouts,
            json: false,
            verbose: 0,
        })
    }

    /// Apply the global flags. `timeout` clamps **every** external deadline (§4).
    #[must_use]
    pub fn with_flags(mut self, json: bool, verbose: u8, timeout: Option<u64>) -> Context {
        self.json = json;
        self.verbose = verbose;
        if let Some(secs) = timeout {
            self.timeouts.override_all(secs);
        }
        self
    }

    /// A context for tests: every path under `dir`, default settings.
    pub fn for_test(dir: &std::path::Path) -> Result<Context, Error> {
        let mut ctx = Context::load()?;
        ctx.paths.state_dir = dir.join("state");
        ctx.paths.config_dir = dir.join("config");
        ctx.paths.cache_dir = dir.join("cache");
        ctx.paths.ensure_dirs()?;
        Ok(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_context_is_send_and_sync_and_clonable() {
        fn assert_context<T: Send + Sync + Clone + 'static>() {}
        assert_context::<Context>();
    }

    #[test]
    fn the_timeout_flag_clamps_every_deadline() {
        let ctx = Context::load()
            .expect("context")
            .with_flags(true, 1, Some(3));
        assert_eq!(ctx.timeouts.connect_sec, 3);
        assert_eq!(ctx.timeouts.probe_sec, 3);
        assert!(ctx.json);
        assert_eq!(ctx.verbose, 1);
    }
}
