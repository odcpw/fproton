//! The **one** chokepoint every `--fix` mutation flows through (DESIGN.md §7).
//!
//! Nothing else in the doctor writes to disk, creates a symlink, runs a mutating
//! command or calls a D-Bus setter. Every call here
//!
//! 1. refuses a path outside the declared write scopes (bounded blast radius),
//! 2. takes a verbatim backup and records `sha256` before *and* after,
//! 3. appends one line to `runs/<id>/actions.jsonl` and `fsync`s it,
//! 4. records the inverse `doctor undo` will replay.
//!
//! The doctor never deletes: a file this run created is moved to the run's
//! `quarantine/` on undo, never unlinked.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use fproton_core::{index, proc, secrets, state, CancelToken, Context, Error};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::checks::{Class, PlanKind};
use super::probes::{Probes, OLD_PLUGIN_ID, PLUGIN_ID};

/// One mutation. The enum is the complete vocabulary of everything `--fix` can do.
#[derive(Clone, Debug)]
pub enum Action {
    /// Replace a file's bytes (creating it when absent). Reverts from the backup.
    WriteFile { path: PathBuf, bytes: Vec<u8> },
    /// Create a symlink. Reverts by removing the link this run created.
    CreateSymlink { link: PathBuf, target: PathBuf },
    /// Run a command; `inverse_argv` is what undoes it (empty = nothing to undo).
    RunArgv {
        argv: Vec<String>,
        inverse_argv: Vec<String>,
    },
    /// `org.freedesktop.Secret.Service.SetAlias` (signature `so`), run through
    /// `fproton_core::proc` from here — never from `fproton_core::secrets`, which is
    /// read-only by contract (§8).
    DbusSetAlias {
        name: String,
        from: Option<String>,
        to: String,
    },
}

impl Action {
    /// The one-line form printed by `--dry-run` and stored in `actions.jsonl`.
    pub fn describe(&self) -> String {
        match self {
            Action::WriteFile { path, bytes } => {
                format!("write {} ({} bytes)", path.display(), bytes.len())
            }
            Action::CreateSymlink { link, target } => {
                format!("symlink {} → {}", link.display(), target.display())
            }
            Action::RunArgv { argv, inverse_argv } => {
                let mut s = format!("run {}", argv.join(" "));
                if !inverse_argv.is_empty() {
                    let _ = write!(s, "   (undo: {})", inverse_argv.join(" "));
                }
                s
            }
            Action::DbusSetAlias { name, from, to } => format!(
                "SetAlias {name}: {} → {to}",
                from.clone().unwrap_or_else(|| "(unset)".into())
            ),
        }
    }

    /// Every path this action writes to, for the `--dry-run` blast-radius listing.
    pub fn writes_to(&self) -> Vec<String> {
        match self {
            Action::WriteFile { path, .. } => vec![path.display().to_string()],
            Action::CreateSymlink { link, .. } => vec![link.display().to_string()],
            Action::RunArgv { .. } => Vec::new(),
            Action::DbusSetAlias { .. } => {
                vec!["org.freedesktop.secrets (alias `default`)".to_string()]
            }
        }
    }
}

/// The bookkeeping every recorded action carries, kept in one value so a mutation
/// helper takes its own arguments rather than six of these.
#[derive(Clone, Copy)]
struct Step<'a> {
    seq: u64,
    check: &'a str,
    class: Class,
    started_at: &'a str,
}

/// What one `mutate` call did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The mutation ran.
    Applied,
    /// Nothing needed doing (idempotence: run twice = run once).
    AlreadyDone,
    /// `--dry-run`: printed, not run.
    Planned,
}

fn sha256(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

/// The complete set of places `--fix` may write (Axiom 14). Anything else is refused.
pub fn write_scopes(ctx: &Context) -> Vec<PathBuf> {
    vec![
        ctx.paths.state_dir.clone(),
        ctx.paths.config_dir.clone(),
        ctx.paths.cache_dir.clone(),
        ctx.paths.omarchy_config_dir.join("plugins"),
        ctx.paths.omarchy_config_dir.join("extensions"),
        // The single named exception to "never touch ~/.local/share/keyrings/**" (§2):
        // 16 bytes naming a keyring *basename*, never key material.
        ctx.paths.keyring_default_pointer(),
    ]
}

/// True when `path` is inside one of [`write_scopes`].
///
/// `Path::starts_with` compares *components* and does not resolve `..`, so
/// `<state_dir>/../../../etc/x` used to pass this guard. A `..` in a path the doctor
/// is about to write is never legitimate — every caller builds its path from
/// `ctx.paths` — so it is refused outright rather than canonicalised (the target of a
/// write may not exist yet, which `canonicalize` cannot handle).
fn in_scope(ctx: &Context, path: &Path) -> bool {
    if has_parent_dir(path) {
        return false;
    }
    write_scopes(ctx)
        .iter()
        .any(|scope| path == scope || path.starts_with(scope))
}

fn has_parent_dir(path: &Path) -> bool {
    path.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
}

/// The same guard, as an error, for the undo side.
///
/// `doctor undo` takes its targets straight out of `actions.jsonl`, so without this
/// the chokepoint's blast-radius promise held only while applying.
fn require_in_scope(ctx: &Context, path: &Path) -> Result<(), Error> {
    if in_scope(ctx, path) {
        return Ok(());
    }
    Err(Error::doctor_refused(format!(
        "{} is outside the doctor's write scopes; nothing was changed",
        path.display()
    ))
    .with_hint("fproton doctor capabilities --json   # write_scopes"))
}

/// Map an absolute path to the relative one used under `backups/` and `quarantine/`,
/// refusing anything with a `..` component so the mapping cannot escape the run dir.
fn run_relative(path: &Path) -> Result<PathBuf, Error> {
    if has_parent_dir(path) {
        return Err(Error::doctor_refused(format!(
            "{} contains a `..` component; refusing to map it into the run directory",
            path.display()
        )));
    }
    Ok(path.strip_prefix("/").unwrap_or(path).to_path_buf())
}

/// The per-run artifact directory (§7): `report.json`, `actions.jsonl`, `probes/`,
/// `backups/`. Append-only; never edited after the run.
pub struct RunDir {
    pub id: String,
    pub dir: PathBuf,
    actions: std::fs::File,
}

/// Newest runs kept; every write prunes the rest (§7).
pub const KEEP_RUNS: usize = 10;

impl RunDir {
    /// `<UTC ISO8601 basic>__<run-id>` under `~/.local/state/fproton/doctor/runs/`.
    pub fn create(ctx: &Context) -> Result<RunDir, Error> {
        let now = jiff::Timestamp::now();
        let stamp = now.strftime("%Y%m%dT%H%M%SZ").to_string();
        // Content-addressable and unique per invocation: the pid is in the hash, so
        // two runs inside the same second get their own directory rather than
        // appending to each other's `actions.jsonl` (which `undo` replays whole).
        let id = sha256(
            format!(
                "{}|{stamp}|{}",
                ctx.paths.state_dir.display(),
                std::process::id()
            )
            .as_bytes(),
        )[..6]
            .to_string();
        let dir = ctx.paths.doctor_runs_dir().join(format!("{stamp}__{id}"));
        for sub in ["probes", "backups"] {
            std::fs::create_dir_all(dir.join(sub))
                .map_err(|e| Error::failure(format!("cannot create {}: {e}", dir.display())))?;
        }
        let actions = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("actions.jsonl"))
            .map_err(|e| Error::failure(format!("cannot open actions.jsonl: {e}")))?;
        let run = RunDir { id, dir, actions };
        run.point_latest(ctx)?;
        prune(ctx);
        Ok(run)
    }

    /// `doctor/latest` → this run, by symlink-to-temp + rename (§7).
    fn point_latest(&self, ctx: &Context) -> Result<(), Error> {
        let latest = ctx.paths.doctor_latest();
        if let Some(parent) = latest.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = latest.with_extension(format!("tmp.{}", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        std::os::unix::fs::symlink(&self.dir, &tmp)
            .map_err(|e| Error::failure(format!("cannot create {}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, &latest).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            Error::failure(format!("cannot move {} into place: {e}", tmp.display()))
        })
    }

    fn append(&mut self, line: &serde_json::Value) -> Result<(), Error> {
        writeln!(self.actions, "{line}")
            .map_err(|e| Error::failure(format!("cannot append to actions.jsonl: {e}")))?;
        self.actions
            .sync_all()
            .map_err(|e| Error::failure(format!("cannot fsync actions.jsonl: {e}")))
    }

    /// Backups live at `backups/<absolute path without its leading slash>`.
    /// A `..` component is refused rather than joined: it would escape the run dir.
    fn backup_rel(path: &Path) -> Result<PathBuf, Error> {
        Ok(Path::new("backups").join(run_relative(path)?))
    }
}

/// Keep only the newest [`KEEP_RUNS`] run directories. Never touches anything else.
fn prune(ctx: &Context) {
    let Ok(entries) = std::fs::read_dir(ctx.paths.doctor_runs_dir()) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    if dirs.len() <= KEEP_RUNS {
        return;
    }
    // Names begin with a UTC basic timestamp, so lexical order is chronological.
    dirs.sort();
    let drop = dirs.len() - KEEP_RUNS;
    for old in dirs.into_iter().take(drop) {
        let _ = std::fs::remove_dir_all(old);
    }
}

/// The chokepoint itself.
pub struct Mutator<'a> {
    ctx: &'a Context,
    run: Option<RunDir>,
    dry_run: bool,
    seq: u64,
    /// One line per planned or applied action, for the report.
    pub log: Vec<PlannedAction>,
}

/// One entry of the report's `actions` array.
#[derive(Clone, Debug, Serialize)]
pub struct PlannedAction {
    pub seq: u64,
    pub check: String,
    pub class: String,
    pub action: String,
    pub writes_to: Vec<String>,
    pub outcome: String,
    pub error: Option<String>,
}

impl<'a> Mutator<'a> {
    pub fn new(ctx: &'a Context, run: Option<RunDir>, dry_run: bool) -> Mutator<'a> {
        Mutator {
            ctx,
            run,
            dry_run,
            seq: 0,
            log: Vec::new(),
        }
    }

    pub fn run_id(&self) -> Option<&str> {
        self.run.as_ref().map(|r| r.id.as_str())
    }
    pub fn run_dir(&self) -> Option<&Path> {
        self.run.as_ref().map(|r| r.dir.as_path())
    }
    pub fn applied(&self) -> usize {
        self.log.iter().filter(|a| a.outcome == "applied").count()
    }

    /// Apply (or, under `--dry-run`, only record) one action.
    pub fn mutate(&mut self, check: &str, class: Class, action: Action) -> Result<Outcome, Error> {
        self.seq += 1;
        let seq = self.seq;
        let started_at = fproton_core::now_rfc3339();
        let describe = action.describe();
        let writes_to = action.writes_to();

        if self.dry_run {
            self.log.push(PlannedAction {
                seq,
                check: check.to_string(),
                class: class.name().to_string(),
                action: describe,
                writes_to,
                outcome: "planned".to_string(),
                error: None,
            });
            return Ok(Outcome::Planned);
        }

        let result = self.apply(seq, check, class, &action, &started_at);
        let (outcome, error) = match &result {
            Ok(Outcome::Applied) => ("applied", None),
            Ok(Outcome::AlreadyDone) => ("already", None),
            Ok(Outcome::Planned) => ("planned", None),
            Err(e) => ("failed", Some(e.to_string())),
        };
        self.log.push(PlannedAction {
            seq,
            check: check.to_string(),
            class: class.name().to_string(),
            action: describe,
            writes_to,
            outcome: outcome.to_string(),
            error,
        });
        result
    }

    fn apply(
        &mut self,
        seq: u64,
        check: &str,
        class: Class,
        action: &Action,
        started_at: &str,
    ) -> Result<Outcome, Error> {
        match action {
            Action::WriteFile { path, bytes } => {
                self.write_file(seq, check, class, path, bytes, started_at)
            }
            Action::CreateSymlink { link, target } => {
                self.create_symlink(seq, check, class, link, target, started_at)
            }
            Action::RunArgv { argv, inverse_argv } => {
                self.run_argv(seq, check, class, argv, inverse_argv, started_at)
            }
            Action::DbusSetAlias { name, from, to } => self.set_alias(
                Step {
                    seq,
                    check,
                    class,
                    started_at,
                },
                name,
                from.as_deref(),
                to,
            ),
        }
    }

    fn write_file(
        &mut self,
        seq: u64,
        check: &str,
        class: Class,
        path: &Path,
        bytes: &[u8],
        started_at: &str,
    ) -> Result<Outcome, Error> {
        if !in_scope(self.ctx, path) {
            return Err(Error::doctor_refused(format!(
                "{} is outside the doctor's write scopes",
                path.display()
            ))
            .with_hint("fproton doctor capabilities --json   # write_scopes"));
        }
        let existing = std::fs::read(path).ok();
        if existing.as_deref() == Some(bytes) {
            return Ok(Outcome::AlreadyDone);
        }
        let before_sha = existing.as_deref().map(sha256);

        // Axiom 2: the backup is written and verified before anything is touched.
        let mut backup_rel: Option<String> = None;
        if let (Some(run), Some(old)) = (self.run.as_ref(), existing.as_deref()) {
            let rel = RunDir::backup_rel(path)?;
            let dest = run.dir.join(&rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    Error::failure(format!("cannot create {}: {e}", parent.display()))
                })?;
            }
            std::fs::write(&dest, old)
                .map_err(|e| Error::failure(format!("cannot write the backup: {e}")))?;
            let written = std::fs::read(&dest)
                .map_err(|e| Error::failure(format!("cannot re-read the backup: {e}")))?;
            if written != old {
                return Err(Error::doctor_refused(format!(
                    "the backup of {} does not match the file; nothing was changed",
                    path.display()
                )));
            }
            backup_rel = Some(rel.display().to_string());
        } else if existing.is_some() && self.run.is_none() {
            return Err(Error::doctor_refused(
                "refusing to overwrite a file without a run directory to back it up into",
            ));
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::failure(format!("cannot create {}: {e}", parent.display())))?;
        }

        // The journal entry is written and fsynced BEFORE the rename, as this module's
        // own refusal text has always claimed ("a mutation must be recorded before it
        // is applied"). Applying first left a window — a crash, a SIGKILL, ENOSPC —
        // in which the file was replaced with nothing in actions.jsonl, so `undo`
        // silently skipped it. The intent line carries `ok: false` (undo never
        // replays it blindly); the completion line that follows the rename carries
        // `ok: true` and is what `undo` acts on. An intent with no completion is
        // reported as "may or may not have applied", with its backup named.
        let mut line = serde_json::json!({
            "seq": seq, "check": check, "class": class.name(), "started_at": started_at,
            "ok": false, "phase": "intent",
            "action": {"kind": "write_file", "path": path.display().to_string(), "len": bytes.len()},
            "before": {"existed": existing.is_some(), "sha256": before_sha, "backup": backup_rel},
            "after": {"sha256": sha256(bytes), "len": bytes.len()},
            "inverse": if existing.is_some() {
                serde_json::json!({"kind": "restore_file", "path": path.display().to_string(),
                                   "backup": backup_rel, "sha256": before_sha})
            } else {
                serde_json::json!({"kind": "quarantine_file", "path": path.display().to_string()})
            },
        });
        self.record(&line)?;

        // Axiom 5: temp file beside the target, then rename(2).
        let tmp = path.with_extension(format!("fproton-doctor.tmp.{}", std::process::id()));
        std::fs::write(&tmp, bytes)
            .map_err(|e| Error::failure(format!("cannot write {}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            Error::failure(format!("cannot replace {}: {e}", path.display()))
        })?;

        if let Some(map) = line.as_object_mut() {
            map.insert("ok".into(), serde_json::json!(true));
            map.insert("phase".into(), serde_json::json!("done"));
        }
        self.record(&line)?;
        Ok(Outcome::Applied)
    }

    fn create_symlink(
        &mut self,
        seq: u64,
        check: &str,
        class: Class,
        link: &Path,
        target: &Path,
        started_at: &str,
    ) -> Result<Outcome, Error> {
        if !in_scope(self.ctx, link) {
            return Err(Error::doctor_refused(format!(
                "{} is outside the doctor's write scopes",
                link.display()
            )));
        }
        if std::fs::read_link(link).ok().as_deref() == Some(target) {
            return Ok(Outcome::AlreadyDone);
        }
        if std::fs::symlink_metadata(link).is_ok() {
            return Err(Error::doctor_refused(format!(
                "{} already exists and is not the link fproton would make; \
                 the doctor never removes what it did not create",
                link.display()
            ))
            .with_hint(format!("ls -l {}", link.display())));
        }
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::failure(format!("cannot create {}: {e}", parent.display())))?;
        }
        std::os::unix::fs::symlink(target, link)
            .map_err(|e| Error::failure(format!("cannot link {}: {e}", link.display())))?;
        let line = serde_json::json!({
            "seq": seq, "check": check, "class": class.name(), "started_at": started_at, "ok": true,
            "action": {"kind": "create_symlink", "link": link.display().to_string(),
                       "target": target.display().to_string()},
            "before": {"existed": false},
            "after": {"link": link.display().to_string()},
            "inverse": {"kind": "remove_symlink", "link": link.display().to_string()},
        });
        self.record(&line)?;
        Ok(Outcome::Applied)
    }

    fn run_argv(
        &mut self,
        seq: u64,
        check: &str,
        class: Class,
        argv: &[String],
        inverse_argv: &[String],
        started_at: &str,
    ) -> Result<Outcome, Error> {
        let borrowed: Vec<&str> = argv.iter().map(String::as_str).collect();
        let out = proc::run_with_deadline(&borrowed, self.ctx.timeouts.probe(), None)?;
        if out.timed_out {
            return Err(Error::upstream(format!(
                "`{}` was killed at its {}s deadline",
                argv.join(" "),
                self.ctx.timeouts.probe_sec
            )));
        }
        if !out.success() {
            return Err(Error::upstream(format!(
                "`{}` failed: {}",
                argv.join(" "),
                out.stderr_tail()
            )));
        }
        let line = serde_json::json!({
            "seq": seq, "check": check, "class": class.name(), "started_at": started_at, "ok": true,
            "action": {"kind": "run_argv", "argv": argv},
            "before": serde_json::Value::Null,
            "after": {"code": out.code},
            "inverse": if inverse_argv.is_empty() {
                serde_json::json!({"kind": "none", "note": "this step has no inverse"})
            } else {
                serde_json::json!({"kind": "run_argv", "argv": inverse_argv})
            },
        });
        self.record(&line)?;
        Ok(Outcome::Applied)
    }

    fn set_alias(
        &mut self,
        step: Step<'_>,
        name: &str,
        from: Option<&str>,
        to: &str,
    ) -> Result<Outcome, Error> {
        let Step {
            seq,
            check,
            class,
            started_at,
        } = step;
        let out = set_alias_call(self.ctx, name, to)?;
        if !out.success() {
            return Err(Error::upstream(format!(
                "SetAlias {name} → {to}: {}",
                out.stderr_tail()
            )));
        }
        // §7's undo caveat: the previous target is a dangling path, so SetAlias back
        // to it would fail with NoSuchObject. The recorded inverse clears the alias.
        let line = serde_json::json!({
            "seq": seq, "check": check, "class": class.name(), "started_at": started_at, "ok": true,
            "action": {"kind": "dbus_set_alias", "name": name, "to": to},
            "before": {"alias": from},
            "after": {"alias": to},
            "inverse": {"kind": "dbus_set_alias", "name": name, "to": "/",
                        "note": "the previous alias target did not exist; the alias is cleared instead",
                        "previous": from},
        });
        self.record(&line)?;
        Ok(Outcome::Applied)
    }

    fn record(&mut self, line: &serde_json::Value) -> Result<(), Error> {
        match self.run.as_mut() {
            Some(run) => run.append(line),
            // A mutation with nowhere to record it is not allowed to happen.
            None => Err(Error::doctor_refused(
                "no run directory: a mutation must be recorded before it is applied",
            )),
        }
    }
}

/// `busctl … SetAlias so <name> <path>` — signature `so`, confirmed by introspection (§7).
fn set_alias_call(ctx: &Context, name: &str, to: &str) -> Result<proc::Output, Error> {
    let deadline = ctx.timeouts.busctl().max(Duration::from_secs(1));
    proc::run_with_deadline(
        &[
            secrets::PROGRAM,
            "--user",
            "--timeout=1",
            "call",
            secrets::SERVICE,
            secrets::OBJECT,
            secrets::IFACE_SERVICE,
            "SetAlias",
            "so",
            name,
            to,
        ],
        deadline,
        None,
    )
}

// ------------------------------------------------------------------ planning

/// Turn a detector's [`PlanKind`] into the actions that would repair it.
///
/// This is the impure half: it may read Proton's 24 MB list (to rebuild the index
/// snapshot in a scratch directory) or re-read the user's menu file. It writes
/// nothing outside the doctor's own scratch space — every real write goes through
/// [`Mutator::mutate`].
pub fn plan(kind: PlanKind, ctx: &Context, probes: &Probes) -> Result<Vec<Action>, Error> {
    match kind {
        PlanKind::CreateStateDir => Ok(vec![Action::RunArgv {
            argv: vec![
                "mkdir".into(),
                "-p".into(),
                ctx.paths.state_dir.display().to_string(),
            ],
            // No inverse: the doctor never removes a directory.
            inverse_argv: Vec::new(),
        }]),

        PlanKind::ResetStateJson => {
            let bytes = pretty_state(&state::StateFile::default())?;
            Ok(vec![Action::WriteFile {
                path: ctx.paths.state_file(),
                bytes,
            }])
        }

        PlanKind::ClearPending => {
            // Re-read state.json HERE, not from `probes.state_json` — that copy was
            // captured at the start of the run, and `cli_session_probe` alone spends
            // 8 s before this point. Rebuilding the whole document from the probe copy
            // rolled back everything written in between: a connect that finished, its
            // history row, a recents entry, a refreshed config cache.
            let text = std::fs::read_to_string(ctx.paths.state_file())
                .map_err(|e| Error::doctor_refused(format!("cannot re-read state.json: {e}")))?;
            let mut file: state::StateFile = serde_json::from_str(&text).map_err(|e| {
                Error::doctor_refused(format!(
                    "state.json does not parse, so it cannot be edited: {e}"
                ))
            })?;
            file.pending = None;
            Ok(vec![Action::WriteFile {
                path: ctx.paths.state_file(),
                bytes: pretty_state(&file)?,
            }])
        }

        PlanKind::NormaliseSettings => {
            let mut settings = ctx.settings.clone();
            let defaults = fproton_core::Settings::default();
            if !fproton_core::settings::ENGINES.contains(&settings.engine.as_str()) {
                settings.engine = defaults.engine.clone();
            }
            if settings
                .default_target
                .parse::<fproton_core::Target>()
                .is_err()
            {
                settings.default_target = defaults.default_target.clone();
            }
            let t = &mut settings.timeouts;
            let d = defaults.timeouts;
            for (v, fallback) in [
                (&mut t.probe_sec, d.probe_sec),
                (&mut t.connect_sec, d.connect_sec),
                (&mut t.disconnect_sec, d.disconnect_sec),
                (&mut t.config_set_sec, d.config_set_sec),
                (&mut t.nmcli_sec, d.nmcli_sec),
                (&mut t.busctl_sec, d.busctl_sec),
            ] {
                if !(1..=600).contains(v) {
                    *v = fallback;
                }
            }
            let text = toml::to_string_pretty(&settings)
                .map_err(|e| Error::failure(format!("cannot serialise settings: {e}")))?;
            Ok(vec![Action::WriteFile {
                path: ctx.paths.config_file(),
                bytes: text.into_bytes(),
            }])
        }

        PlanKind::RebuildIndex => {
            // Build into a scratch cache dir so the only write to the real snapshot
            // is the one the chokepoint makes, with a backup and a hash.
            // Under the fproton-owned cache dir, created with `create_dir` (which
            // fails on an existing directory) rather than in the world-writable,
            // fully predictable `/tmp`: whatever ends up at this path becomes
            // fproton's server index, so a pre-existing directory is never reused.
            let scratch = ctx.paths.cache_dir.join(format!(
                "doctor-index-scratch.{}.{}",
                std::process::id(),
                probes.now_unix
            ));
            let _ = std::fs::create_dir_all(&ctx.paths.cache_dir);
            let _ = std::fs::remove_dir_all(&scratch);
            std::fs::create_dir(&scratch)
                .map_err(|e| Error::failure(format!("cannot create {}: {e}", scratch.display())))?;
            let mut scratch_ctx = ctx.clone();
            scratch_ctx.paths.cache_dir = scratch.clone();
            let built = index::rebuild(&scratch_ctx, &CancelToken::new());
            let bytes = built.and_then(|_| {
                std::fs::read(scratch_ctx.paths.index_file())
                    .map_err(|e| Error::failure(format!("cannot read the rebuilt snapshot: {e}")))
            });
            let _ = std::fs::remove_dir_all(&scratch);
            Ok(vec![Action::WriteFile {
                path: ctx.paths.index_file(),
                bytes: bytes?,
            }])
        }

        PlanKind::LinkPlugin => Ok(vec![Action::CreateSymlink {
            link: ctx.paths.plugin_link(),
            target: super::repo_plugin_dir(),
        }]),

        PlanKind::EnablePlugin => Ok(vec![
            Action::RunArgv {
                argv: vec![
                    "omarchy".into(),
                    "plugin".into(),
                    "enable".into(),
                    PLUGIN_ID.into(),
                ],
                inverse_argv: vec![
                    "omarchy".into(),
                    "plugin".into(),
                    "disable".into(),
                    PLUGIN_ID.into(),
                ],
            },
            Action::RunArgv {
                argv: vec![
                    "omarchy".into(),
                    "bar".into(),
                    "put".into(),
                    PLUGIN_ID.into(),
                    "--before".into(),
                    "omarchy.tailscale".into(),
                ],
                // `omarchy bar` has no `remove`; disabling the plugin takes the widget
                // out of the bar, and that is the inverse of the step above.
                inverse_argv: Vec::new(),
            },
        ]),

        PlanKind::MergeMenu => {
            let path = ctx.paths.omarchy_menu();
            let entries = crate::setup::template_entries();
            let bytes = match &probes.menu_file.text {
                None => crate::setup::MENU_TEMPLATE.as_bytes().to_vec(),
                Some(existing) => crate::setup::merge_menu(existing, &entries)
                    .ok_or_else(|| Error::doctor_refused("every menu entry is already present"))?
                    .into_bytes(),
            };
            Ok(vec![Action::WriteFile { path, bytes }])
        }

        PlanKind::DisableOldPlugin => Ok(vec![Action::RunArgv {
            argv: vec![
                "omarchy".into(),
                "plugin".into(),
                "disable".into(),
                OLD_PLUGIN_ID.into(),
            ],
            inverse_argv: vec![
                "omarchy".into(),
                "plugin".into(),
                "enable".into(),
                OLD_PLUGIN_ID.into(),
            ],
        }]),

        PlanKind::KillswitchDown => Ok(vec![Action::RunArgv {
            argv: vec![
                "nmcli".into(),
                "connection".into(),
                "down".into(),
                fproton_core::KILLSWITCH_PROFILE.into(),
            ],
            inverse_argv: vec![
                "nmcli".into(),
                "connection".into(),
                "up".into(),
                fproton_core::KILLSWITCH_PROFILE.into(),
            ],
        }]),

        PlanKind::SetDefaultAlias => {
            let holders: Vec<&super::probes::CollectionProbe> = probes
                .collections
                .iter()
                .filter(|c| c.exported && c.locked != Some(true) && !c.proton_items.is_empty())
                .collect();
            let target = match holders.as_slice() {
                [one] => *one,
                [] => {
                    return Err(Error::doctor_refused(
                        "no unlocked collection holds a `service=Proton` item, so there is \
                         nothing to point the `default` alias at",
                    )
                    .with_hint("fproton signin"))
                }
                many => {
                    return Err(Error::doctor_refused(format!(
                        "{} unlocked collections hold a `service=Proton` item ({}); \
                         fproton will not guess which one the CLI should read",
                        many.len(),
                        many.iter()
                            .map(|c| c.path.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )))
                }
            };
            let basename = target.keyring_basename().ok_or_else(|| {
                Error::doctor_refused(format!("cannot name a keyring for {}", target.path))
            })?;
            Ok(vec![
                Action::DbusSetAlias {
                    name: "default".into(),
                    from: probes.alias.as_ref().and_then(|a| a.path.clone()),
                    to: target.path.clone(),
                },
                // So the alias survives the next login. This file holds a keyring
                // *basename*; no `*.keyring` file is opened, hashed or logged (§2).
                Action::WriteFile {
                    path: ctx.paths.keyring_default_pointer(),
                    bytes: format!("{basename}\n").into_bytes(),
                },
            ])
        }
    }
}

fn pretty_state(file: &state::StateFile) -> Result<Vec<u8>, Error> {
    serde_json::to_string_pretty(file)
        .map(String::into_bytes)
        .map_err(|e| Error::failure(format!("cannot serialise state: {e}")))
}

// ---------------------------------------------------------------------- undo

#[derive(Debug, Deserialize)]
struct ActionLine {
    seq: u64,
    #[serde(default)]
    check: String,
    #[serde(default)]
    ok: bool,
    /// `"intent"` (written before the mutation) or `"done"` (written after it).
    /// Absent on the variants that still record once, after the fact.
    #[serde(default)]
    phase: String,
    inverse: serde_json::Value,
}

/// One step of an undo, as reported.
#[derive(Debug, Serialize)]
pub struct UndoStep {
    pub seq: u64,
    pub check: String,
    pub inverse: String,
    pub outcome: String,
    pub note: Option<String>,
}

/// Resolve `latest` or a run id to its directory.
pub fn resolve_run(ctx: &Context, run: &str) -> Result<PathBuf, Error> {
    if run == "latest" {
        let latest = ctx.paths.doctor_latest();
        let dir = std::fs::canonicalize(&latest).map_err(|_| {
            Error::not_found(format!("there is no {}", latest.display()))
                .with_hint("fproton doctor --fix --dry-run")
        })?;
        return Ok(dir);
    }
    let root = ctx.paths.doctor_runs_dir();
    let entries = std::fs::read_dir(&root).map_err(|_| {
        Error::not_found(format!("there are no doctor runs under {}", root.display()))
    })?;
    let mut matches: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            name == run || name.ends_with(&format!("__{run}"))
        })
        .collect();
    matches.sort();
    matches.pop().ok_or_else(|| {
        Error::not_found(format!("no doctor run matches '{run}'"))
            .with_hint("ls ".to_string() + &root.display().to_string())
    })
}

/// Replay one run's `actions.jsonl` in reverse (§7).
///
/// Refuses with exit 13 when a `WriteFile` backup is missing or its hash does not
/// match what was recorded. Nothing is ever deleted: a file the run created is moved
/// into the run's `quarantine/`.
pub fn undo(ctx: &Context, dir: &Path) -> Result<Vec<UndoStep>, Error> {
    let path = dir.join("actions.jsonl");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| Error::not_found(format!("cannot read {}: {e}", path.display())))?;
    let mut lines: Vec<ActionLine> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        // A crash mid-write leaves a torn last line; ignore it rather than refuse.
        if let Ok(parsed) = serde_json::from_str::<ActionLine>(line) {
            lines.push(parsed);
        }
    }
    lines.reverse();

    // Every seq that reached its completion line. An `intent` without one is a run
    // that stopped between the journal entry and the write.
    let completed: std::collections::HashSet<u64> =
        lines.iter().filter(|l| l.ok).map(|l| l.seq).collect();

    let mut steps = Vec::new();
    for line in lines {
        if !line.ok {
            if line.phase == "intent" && !completed.contains(&line.seq) {
                let backup = line
                    .inverse
                    .get("backup")
                    .and_then(|b| b.as_str())
                    .map(|b| format!("backup available at {}", dir.join(b).display()));
                steps.push(UndoStep {
                    seq: line.seq,
                    check: line.check,
                    inverse: line
                        .inverse
                        .get("kind")
                        .and_then(|k| k.as_str())
                        .unwrap_or("none")
                        .to_string(),
                    outcome: "unknown — the run stopped between the journal entry and \
                              the write; nothing was undone"
                        .to_string(),
                    note: backup,
                });
            }
            continue;
        }
        let kind = line
            .inverse
            .get("kind")
            .and_then(|k| k.as_str())
            .unwrap_or("none");
        let mut note = line
            .inverse
            .get("note")
            .and_then(|n| n.as_str())
            .map(str::to_string);
        let outcome = match kind {
            "restore_file" => restore_file(ctx, dir, &line.inverse)?,
            "quarantine_file" => quarantine(ctx, dir, &line.inverse)?,
            "remove_symlink" => remove_symlink(ctx, &line.inverse)?,
            "run_argv" => {
                let argv: Vec<String> = line
                    .inverse
                    .get("argv")
                    .and_then(|a| serde_json::from_value(a.clone()).ok())
                    .unwrap_or_default();
                let borrowed: Vec<&str> = argv.iter().map(String::as_str).collect();
                let out = proc::run_with_deadline(&borrowed, ctx.timeouts.probe(), None)?;
                if out.success() {
                    "undone".to_string()
                } else {
                    note = Some(out.stderr_tail());
                    "failed".to_string()
                }
            }
            "dbus_set_alias" => {
                let to = line
                    .inverse
                    .get("to")
                    .and_then(|t| t.as_str())
                    .unwrap_or("/");
                let name = line
                    .inverse
                    .get("name")
                    .and_then(|t| t.as_str())
                    .unwrap_or("default");
                let out = set_alias_call(ctx, name, to)?;
                if out.success() {
                    "undone".to_string()
                } else {
                    note = Some(out.stderr_tail());
                    "failed".to_string()
                }
            }
            _ => "nothing to undo".to_string(),
        };
        steps.push(UndoStep {
            seq: line.seq,
            check: line.check,
            inverse: kind.to_string(),
            outcome,
            note,
        });
    }
    Ok(steps)
}

fn restore_file(ctx: &Context, dir: &Path, inverse: &serde_json::Value) -> Result<String, Error> {
    let target = inverse
        .get("path")
        .and_then(|p| p.as_str())
        .ok_or_else(|| Error::doctor_refused("an undo record has no path"))?;
    // Undo is bounded by the same write scopes as apply: the targets come out of
    // actions.jsonl, which is a file on disk like any other.
    require_in_scope(ctx, Path::new(target))?;
    let rel = inverse
        .get("backup")
        .and_then(|p| p.as_str())
        .ok_or_else(|| {
            Error::doctor_refused(format!(
                "no backup was recorded for {target}; refusing to guess"
            ))
        })?;
    let backup = dir.join(rel);
    let bytes = std::fs::read(&backup).map_err(|e| {
        Error::doctor_refused(format!(
            "the backup {} is missing or unreadable ({e}); {target} was left alone",
            backup.display()
        ))
    })?;
    if let Some(want) = inverse.get("sha256").and_then(|s| s.as_str()) {
        let got = sha256(&bytes);
        if got != want {
            return Err(Error::doctor_refused(format!(
                "the backup of {target} has changed since the run (sha256 {got} ≠ {want}); \
                 nothing was restored"
            )));
        }
    }
    let path = Path::new(target);
    let tmp = path.with_extension(format!("fproton-undo.tmp.{}", std::process::id()));
    std::fs::write(&tmp, &bytes)
        .map_err(|e| Error::failure(format!("cannot write {}: {e}", tmp.display())))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error::failure(format!("cannot restore {target}: {e}"))
    })?;
    Ok("restored".to_string())
}

/// A file the run created: moved aside, never deleted (§2, Axiom 3).
fn quarantine(ctx: &Context, dir: &Path, inverse: &serde_json::Value) -> Result<String, Error> {
    let target = inverse
        .get("path")
        .and_then(|p| p.as_str())
        .ok_or_else(|| Error::doctor_refused("an undo record has no path"))?;
    let path = Path::new(target);
    require_in_scope(ctx, path)?;
    if !path.exists() {
        return Ok("already absent".to_string());
    }
    let rel = run_relative(path)?;
    let dest = dir.join("quarantine").join(rel);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::failure(format!("cannot create {}: {e}", parent.display())))?;
    }
    std::fs::rename(path, &dest)
        .map_err(|e| Error::failure(format!("cannot move {target} aside: {e}")))?;
    Ok(format!("moved to {}", dest.display()))
}

fn remove_symlink(ctx: &Context, inverse: &serde_json::Value) -> Result<String, Error> {
    let link = inverse
        .get("link")
        .and_then(|p| p.as_str())
        .ok_or_else(|| Error::doctor_refused("an undo record has no link"))?;
    let path = Path::new(link);
    require_in_scope(ctx, path)?;
    match std::fs::symlink_metadata(path) {
        Err(_) => Ok("already absent".to_string()),
        Ok(meta) if meta.file_type().is_symlink() => {
            std::fs::remove_file(path)
                .map_err(|e| Error::failure(format!("cannot remove {link}: {e}")))?;
            Ok("removed".to_string())
        }
        Ok(_) => Err(Error::doctor_refused(format!(
            "{link} is no longer a symlink; the doctor will not remove it"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx(name: &str) -> (Context, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "fproton-doctor-mutate-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let ctx = Context::for_test(&dir).expect("test context");
        (ctx, dir)
    }

    #[test]
    fn a_write_outside_the_scopes_is_refused() {
        let (ctx, dir) = test_ctx("scope");
        let run = RunDir::create(&ctx).expect("run dir");
        let mut m = Mutator::new(&ctx, Some(run), false);
        let err = m
            .mutate(
                "x",
                Class::A,
                Action::WriteFile {
                    path: PathBuf::from("/etc/passwd"),
                    bytes: b"nope".to_vec(),
                },
            )
            .expect_err("must refuse");
        assert_eq!(err.exit_code(), 13, "{err}");
        assert!(!Path::new("/etc/passwd")
            .metadata()
            .map(|m| m.len() == 4)
            .unwrap_or(false));

        // `Path::starts_with` compares components and does not resolve `..`, so this
        // escaped the guard while looking like an in-scope path.
        let escape = ctx.paths.state_dir.join("../../../etc/fproton-escape");
        let err = m
            .mutate(
                "x",
                Class::A,
                Action::WriteFile {
                    path: escape.clone(),
                    bytes: b"nope".to_vec(),
                },
            )
            .expect_err("a `..` component must be refused");
        assert_eq!(err.exit_code(), 13, "{err}");
        assert!(!escape.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn undo_refuses_a_target_outside_the_write_scopes() {
        // Undo takes its targets from actions.jsonl, which is a file on disk like any
        // other. It must be bounded by the same scopes as apply.
        let (ctx, dir) = test_ctx("undo-scope");
        let run = RunDir::create(&ctx).expect("run dir");
        let run_dir = run.dir.clone();
        drop(run);
        let forged = serde_json::json!({
            "seq": 1, "check": "forged", "ok": true, "phase": "done",
            "inverse": {"kind": "restore_file", "path": "/etc/passwd", "backup": "backups/etc/passwd"},
        });
        std::fs::write(run_dir.join("actions.jsonl"), format!("{forged}\n")).unwrap();
        let err = undo(&ctx, &run_dir).expect_err("must refuse");
        assert_eq!(err.exit_code(), 13, "{err}");
        assert!(err.to_string().contains("write scopes"), "{err}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_journal_entry_is_written_before_the_mutation() {
        // A crash between the rename and the journal append left a file replaced with
        // nothing in actions.jsonl, so `undo` silently skipped it — the opposite of
        // what this module's own refusal text claims.
        let (ctx, dir) = test_ctx("journal-order");
        let target = ctx.paths.state_dir.join("ordered.json");
        std::fs::write(&target, b"before").unwrap();
        let run = RunDir::create(&ctx).expect("run dir");
        let run_dir = run.dir.clone();
        let mut m = Mutator::new(&ctx, Some(run), false);
        m.mutate(
            "x",
            Class::A,
            Action::WriteFile {
                path: target.clone(),
                bytes: b"after".to_vec(),
            },
        )
        .expect("apply");
        drop(m);

        let journal = std::fs::read_to_string(run_dir.join("actions.jsonl")).unwrap();
        let lines: Vec<&str> = journal.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 2, "intent then completion:\n{journal}");
        assert!(lines[0].contains(r#""phase":"intent""#), "{}", lines[0]);
        assert!(lines[0].contains(r#""ok":false"#), "{}", lines[0]);
        assert!(lines[1].contains(r#""phase":"done""#), "{}", lines[1]);
        assert!(lines[1].contains(r#""ok":true"#), "{}", lines[1]);

        // With only the intent line — exactly what a crash mid-write leaves — undo
        // touches nothing and says so, naming the backup.
        std::fs::write(run_dir.join("actions.jsonl"), format!("{}\n", lines[0])).unwrap();
        let steps = undo(&ctx, &run_dir).expect("undo reports rather than refusing");
        assert_eq!(steps.len(), 1);
        assert!(steps[0].outcome.starts_with("unknown"), "{:?}", steps[0]);
        assert!(steps[0].note.as_deref().unwrap_or("").contains("backups/"));
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"after",
            "nothing restored"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_write_backs_up_verbatim_and_undo_restores_it_byte_for_byte() {
        let (ctx, dir) = test_ctx("roundtrip");
        let target = ctx.paths.state_file();
        std::fs::write(&target, b"{\"pending\": \"corrupt\"}").expect("seed");
        let run = RunDir::create(&ctx).expect("run dir");
        let run_dir = run.dir.clone();
        let mut m = Mutator::new(&ctx, Some(run), false);
        let outcome = m
            .mutate(
                "state_dir",
                Class::A,
                Action::WriteFile {
                    path: target.clone(),
                    bytes: b"{}".to_vec(),
                },
            )
            .expect("write");
        assert_eq!(outcome, Outcome::Applied);
        assert_eq!(std::fs::read(&target).unwrap(), b"{}");

        let steps = undo(&ctx, &run_dir).expect("undo");
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].outcome, "restored");
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"{\"pending\": \"corrupt\"}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn undo_refuses_when_a_backup_was_altered() {
        let (ctx, dir) = test_ctx("tamper");
        let target = ctx.paths.state_file();
        std::fs::write(&target, b"original").expect("seed");
        let run = RunDir::create(&ctx).expect("run dir");
        let run_dir = run.dir.clone();
        let mut m = Mutator::new(&ctx, Some(run), false);
        m.mutate(
            "state_dir",
            Class::A,
            Action::WriteFile {
                path: target.clone(),
                bytes: b"replaced".to_vec(),
            },
        )
        .expect("write");
        let backup = run_dir.join(RunDir::backup_rel(&target).unwrap());
        std::fs::write(&backup, b"tampered").expect("tamper");
        let err = undo(&ctx, &run_dir).expect_err("must refuse");
        assert_eq!(err.exit_code(), 13, "{err}");
        assert_eq!(std::fs::read(&target).unwrap(), b"replaced");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_created_file_is_quarantined_by_undo_never_deleted() {
        let (ctx, dir) = test_ctx("quarantine");
        let target = ctx.paths.state_file();
        let run = RunDir::create(&ctx).expect("run dir");
        let run_dir = run.dir.clone();
        let mut m = Mutator::new(&ctx, Some(run), false);
        m.mutate(
            "state_dir",
            Class::A,
            Action::WriteFile {
                path: target.clone(),
                bytes: b"new".to_vec(),
            },
        )
        .expect("write");
        let steps = undo(&ctx, &run_dir).expect("undo");
        assert!(steps[0].outcome.starts_with("moved to"), "{:?}", steps[0]);
        assert!(!target.exists());
        let kept = run_dir
            .join("quarantine")
            .join(target.strip_prefix("/").unwrap());
        assert_eq!(std::fs::read(kept).unwrap(), b"new");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn writing_the_same_bytes_twice_is_already_done() {
        let (ctx, dir) = test_ctx("idempotent");
        let target = ctx.paths.state_file();
        let run = RunDir::create(&ctx).expect("run dir");
        let mut m = Mutator::new(&ctx, Some(run), false);
        let action = || Action::WriteFile {
            path: target.clone(),
            bytes: b"same".to_vec(),
        };
        assert_eq!(
            m.mutate("x", Class::A, action()).expect("first"),
            Outcome::Applied
        );
        assert_eq!(
            m.mutate("x", Class::A, action()).expect("second"),
            Outcome::AlreadyDone
        );
        assert_eq!(m.applied(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_dry_run_mutator_touches_nothing() {
        let (ctx, dir) = test_ctx("dryrun");
        let target = ctx.paths.state_file();
        let mut m = Mutator::new(&ctx, None, true);
        let outcome = m
            .mutate(
                "x",
                Class::A,
                Action::WriteFile {
                    path: target.clone(),
                    bytes: b"never".to_vec(),
                },
            )
            .expect("planned");
        assert_eq!(outcome, Outcome::Planned);
        assert!(!target.exists(), "a dry run wrote a file");
        assert_eq!(m.log[0].outcome, "planned");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The §7 Class A round trip, for every Class A plan: corrupt → `--fix` →
    /// assert → `doctor undo latest` → the file is byte-for-byte what it was.
    /// `FPROTON_STATE_DIR` is not needed here because `Context::for_test` puts every
    /// fproton path under a temp dir already; the binary-level gate uses the env var.
    #[test]
    fn class_a_plans_round_trip_through_fix_and_undo() {
        let (ctx, dir) = test_ctx("class-a");
        let probes = super::super::probes::Probes {
            state_json: super::super::probes::FileProbe {
                path: ctx.paths.state_file().display().to_string(),
                exists: true,
                text: Some(
                    r#"{"pending":{"action":"connect","target":"CH#461","started_at":"2026-09-14T22:29:58+02:00","deadline_sec":90,"elapsed_sec":900,"pid":null}}"#
                        .to_string(),
                ),
                taken: true,
                ..Default::default()
            },
            ..Default::default()
        };

        for (kind, path, assert_after) in [
            (
                PlanKind::ClearPending,
                ctx.paths.state_file(),
                "\"pending\": null" as &str,
            ),
            (
                PlanKind::ResetStateJson,
                ctx.paths.state_file(),
                "\"history\": []",
            ),
            (
                PlanKind::NormaliseSettings,
                ctx.paths.config_file(),
                "engine = \"cli\"",
            ),
        ] {
            // Corrupt (or seed) the file, then remember its exact bytes.
            std::fs::write(&path, probes.state_json.text.clone().unwrap()).expect("seed");
            let before = std::fs::read(&path).expect("before");

            let run = RunDir::create(&ctx).expect("run dir");
            let run_dir = run.dir.clone();
            let mut m = Mutator::new(&ctx, Some(run), false);
            for action in plan(kind, &ctx, &probes).expect("plan") {
                m.mutate("class_a", Class::A, action).expect("apply");
            }
            let after = std::fs::read_to_string(&path).expect("after");
            assert!(
                after.contains(assert_after),
                "{kind:?} did not produce {assert_after}:\n{after}"
            );

            let steps = undo(&ctx, &run_dir).expect("undo");
            assert!(steps.iter().all(|s| s.outcome != "failed"), "{steps:?}");
            assert_eq!(
                std::fs::read(&path).expect("restored"),
                before,
                "{kind:?} did not restore byte for byte"
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_state_dir_plan_creates_the_directory_and_has_no_inverse() {
        let (ctx, dir) = test_ctx("statedir");
        let run = RunDir::create(&ctx).expect("run dir");
        let run_dir = run.dir.clone();
        let mut m = Mutator::new(&ctx, Some(run), false);
        let target = ctx.paths.state_dir.join("nested");
        let mut scratch = ctx.clone();
        scratch.paths.state_dir = target.clone();
        for action in plan(PlanKind::CreateStateDir, &scratch, &Default::default()).expect("plan") {
            m.mutate("state_dir", Class::A, action).expect("apply");
        }
        assert!(target.is_dir());
        let steps = undo(&ctx, &run_dir).expect("undo");
        assert_eq!(steps[0].outcome, "nothing to undo");
        assert!(target.is_dir(), "the doctor must never remove a directory");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The index snapshot is Class A too: rebuilt in scratch, written through the
    /// chokepoint, restorable. Skipped (loudly) when Proton's cache is absent.
    #[test]
    fn the_index_plan_writes_a_real_snapshot_through_the_chokepoint() {
        let (ctx, dir) = test_ctx("index");
        if !ctx.paths.serverlist().exists() {
            eprintln!("skipped: {} is absent", ctx.paths.serverlist().display());
            return;
        }
        let run = RunDir::create(&ctx).expect("run dir");
        let run_dir = run.dir.clone();
        let mut m = Mutator::new(&ctx, Some(run), false);
        for action in plan(PlanKind::RebuildIndex, &ctx, &Default::default()).expect("plan") {
            m.mutate("index_fresh", Class::A, action).expect("apply");
        }
        let snapshot = ctx.paths.index_file();
        assert!(snapshot.exists());
        assert_eq!(&std::fs::read(&snapshot).unwrap()[..4], b"FPIX");
        // No scratch directory is left behind, and none of it was ever in /tmp.
        let leftovers: Vec<_> = std::fs::read_dir(&ctx.paths.cache_dir)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("doctor-index-scratch.")
            })
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        // The snapshot did not exist before, so undo quarantines it rather than deleting.
        let steps = undo(&ctx, &run_dir).expect("undo");
        assert!(steps[0].outcome.starts_with("moved to"), "{:?}", steps[0]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn runs_are_pruned_to_the_newest_ten() {
        let (ctx, dir) = test_ctx("prune");
        let root = ctx.paths.doctor_runs_dir();
        std::fs::create_dir_all(&root).expect("root");
        for i in 0..14 {
            std::fs::create_dir_all(root.join(format!("2026010{i:02}T000000Z__aaaaaa")))
                .expect("fake run");
        }
        prune(&ctx);
        let left = std::fs::read_dir(&root).unwrap().count();
        assert_eq!(left, KEEP_RUNS);
        let _ = std::fs::remove_dir_all(dir);
    }
}
