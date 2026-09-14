//! The one cancellable background worker (DESIGN.md §7a).
//!
//! Long actions — connect, disconnect, reconnect, index rebuild — never run in a
//! `Cmd::task`, because a `Cmd` cannot be cancelled. They run in this `Subscription`:
//! the model holds it only while the action is in flight, so dropping it makes the
//! runtime raise the `StopSignal`, which trips the core `CancelToken`, which
//! SIGTERM/SIGKILLs the child process group and returns `Error::Interrupted`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

use ftui::runtime::{StopSignal, SubId, Subscription};

use fproton_core::{engine, index, Context, Error, Target};

use super::Msg;

/// Everything the TUI can start that is allowed to take a minute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Connect(Target),
    /// `true` = `--force` (nmcli teardown of `proton0` and the leak guard).
    Disconnect(bool),
    /// `true` = `--next`.
    Reconnect(bool),
    Rebuild,
}

impl Action {
    /// The label the status line and the hero show while this runs.
    pub fn label(&self) -> String {
        match self {
            Action::Connect(t) => format!("connecting to {t}"),
            Action::Disconnect(false) => "disconnecting".into(),
            Action::Disconnect(true) => "force disconnecting".into(),
            Action::Reconnect(false) => "reconnecting".into(),
            Action::Reconnect(true) => "reconnecting to the next server".into(),
            Action::Rebuild => "rebuilding the server index".into(),
        }
    }

    /// What the deadline is, so the hero can draw `12s / 90s` before the first
    /// `status` poll carries a `pending` record.
    pub fn deadline_sec(&self, ctx: &Context) -> u64 {
        match self {
            Action::Connect(_) | Action::Reconnect(_) => ctx.timeouts.connect_sec,
            Action::Disconnect(false) => ctx.timeouts.disconnect_sec,
            Action::Disconnect(true) => ctx.timeouts.nmcli_sec,
            Action::Rebuild => 30,
        }
    }

    /// A cancelled `connect` leaves a half-built tunnel behind; §7a says to follow it
    /// with a force disconnect. Nothing else needs cleaning up.
    pub fn cancel_cleanup(&self) -> Option<Action> {
        match self {
            Action::Connect(_) | Action::Reconnect(_) => Some(Action::Disconnect(true)),
            _ => None,
        }
    }
}

/// A failed action, reduced to what the TUI shows and exits with (§4a).
#[derive(Clone, Debug)]
pub struct ActionErr {
    pub code: i32,
    pub code_name: String,
    pub text: String,
    pub hint: String,
    pub interrupted: bool,
}

impl From<Error> for ActionErr {
    fn from(e: Error) -> ActionErr {
        ActionErr {
            code: e.exit_code(),
            code_name: e.code_name().to_string(),
            text: e.to_string(),
            hint: e.hint(),
            interrupted: e.is_interrupted(),
        }
    }
}

pub struct ActionSub {
    pub id: SubId,
    pub ctx: Context,
    pub action: Action,
}

impl Subscription<Msg> for ActionSub {
    fn id(&self) -> SubId {
        self.id
    }

    fn run(&self, tx: Sender<Msg>, stop: StopSignal) {
        let cancel = fproton_core::CancelToken::new();
        let watched = cancel.clone();
        let done = Arc::new(AtomicBool::new(false));
        let watcher_done = done.clone();
        // The runtime has no "cancel me" callback: it drops the subscription and raises
        // the stop signal, so one thread has to translate that into the CancelToken the
        // core polls. It also exits on its own once the action is over.
        let watcher = std::thread::spawn(move || {
            while !watcher_done.load(Ordering::Relaxed) {
                if stop.wait_timeout(Duration::from_millis(200)) {
                    watched.cancel();
                    return;
                }
            }
        });

        let ctx = &self.ctx;
        let outcome = match &self.action {
            Action::Connect(t) => engine::connect(ctx, t, &cancel).map(|c| {
                let where_to = c.server.clone().unwrap_or_else(|| c.target.clone());
                if c.already_connected {
                    format!("already connected to {where_to}")
                } else {
                    format!("connected to {where_to}")
                }
            }),
            Action::Disconnect(force) => engine::disconnect(ctx, *force, &cancel).map(|()| {
                if *force {
                    "tunnel torn down".to_string()
                } else {
                    "disconnected".to_string()
                }
            }),
            Action::Reconnect(next) => engine::reconnect(ctx, *next, &cancel).map(|c| {
                format!(
                    "reconnected to {}",
                    c.server.clone().unwrap_or_else(|| c.target.clone())
                )
            }),
            Action::Rebuild => index::rebuild(ctx, &cancel)
                .map(|s| format!("index rebuilt: {} servers", s.servers)),
        };

        done.store(true, Ordering::Relaxed);
        let _ = watcher.join();
        let _ = tx.send(Msg::ActionDone(
            self.action.clone(),
            Box::new(outcome.map_err(ActionErr::from)),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_name_the_target_and_the_force_flag() {
        let t: Target = "CH#461".parse().expect("target");
        assert_eq!(Action::Connect(t).label(), "connecting to CH#461");
        assert_eq!(Action::Disconnect(true).label(), "force disconnecting");
        assert_eq!(
            Action::Reconnect(true).label(),
            "reconnecting to the next server"
        );
    }

    #[test]
    fn only_a_cancelled_connect_needs_a_teardown() {
        let t: Target = "fastest".parse().expect("target");
        assert_eq!(
            Action::Connect(t).cancel_cleanup(),
            Some(Action::Disconnect(true))
        );
        assert_eq!(Action::Rebuild.cancel_cleanup(), None);
        assert_eq!(Action::Disconnect(false).cancel_cleanup(), None);
    }

    #[test]
    fn an_error_keeps_its_own_exit_code_and_hint() {
        let e: ActionErr = Error::not_found("no such server").into();
        assert_eq!(e.code, 6);
        assert_eq!(e.code_name, "not_found");
        assert!(!e.hint.is_empty());
        assert!(!e.interrupted);
        let i: ActionErr = Error::Interrupted.into();
        assert_eq!(i.code, 130);
        assert!(i.interrupted);
    }
}
