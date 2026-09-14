//! The doctor: detect, report, and apply a small set of reversible repairs (DESIGN.md §7).
//!
//! The shape is the one the design fixes and the doctor skill's kernel demands:
//!
//! * `probes.rs` is the only place a process is spawned, a file read or D-Bus touched;
//! * `checks.rs` holds every detector as a **pure function** of the captured probes;
//! * `mutate.rs` is the single chokepoint every `--fix` mutation flows through, with
//!   verbatim backups, sha256 before and after, an append-only `actions.jsonl` and a
//!   recorded inverse for `doctor undo`;
//! * `report.rs` renders `report.json`, the human table and `doctor capabilities`.
//!
//! Read-only by default: a run without `--fix` writes nothing unless `--save` is
//! passed. The doctor never deletes a file, a NetworkManager profile or a keyring
//! entry, never opens a `*.keyring` file, never unlocks a collection and never
//! restarts a daemon.

pub mod checks;
pub mod mutate;
pub mod probes;
pub mod report;

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand, ValueEnum};
use fproton_core::{state, Context, Error};

use crate::output::{Out, Paint};
use checks::{CheckStatus, Class, CHECKS};
use report::{CheckReport, Report, Summary};

/// The check groups `--only` accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum DoctorGroup {
    Cli,
    Keyring,
    Network,
    Cache,
    Install,
    Connected,
}

/// Every group, in report order.
pub const GROUPS: &[DoctorGroup] = &[
    DoctorGroup::Cli,
    DoctorGroup::Keyring,
    DoctorGroup::Network,
    DoctorGroup::Cache,
    DoctorGroup::Install,
    DoctorGroup::Connected,
];

impl DoctorGroup {
    pub fn name(self) -> &'static str {
        match self {
            DoctorGroup::Cli => "cli",
            DoctorGroup::Keyring => "keyring",
            DoctorGroup::Network => "network",
            DoctorGroup::Cache => "cache",
            DoctorGroup::Install => "install",
            DoctorGroup::Connected => "connected",
        }
    }
}

/// The repo directory this binary was built from — what `plugin_linked` validates
/// the resolved symlink against. `CARGO_MANIFEST_DIR` is `crates/fproton`.
pub fn repo_plugin_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."))
        .join("plugin")
        .join(probes::PLUGIN_ID)
}

/// `fproton doctor [FLAGS]` / `fproton doctor undo RUN` / `fproton doctor capabilities`.
#[derive(Args, Debug, Default, Clone)]
pub struct DoctorArgs {
    /// Apply the fixable findings (each one reversible with `doctor undo latest`)
    #[arg(long)]
    pub fix: bool,
    /// Also allow the foreign-state keyring fixer (Class C)
    #[arg(long)]
    pub allow_keyring: bool,
    /// Only run one group of checks
    #[arg(long, value_enum, value_name = "GROUP")]
    pub only: Option<DoctorGroup>,
    /// Write the run artifacts even without --fix
    #[arg(long)]
    pub save: bool,
    #[command(subcommand)]
    pub sub: Option<DoctorCmd>,
    /// Set from the global --yes (not a flag of its own).
    #[clap(skip)]
    pub yes: bool,
    /// Set from the global --dry-run (not a flag of its own).
    #[clap(skip)]
    pub dry_run: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum DoctorCmd {
    /// Replay a `--fix` run in reverse (exit 13 if a backup is missing or altered)
    Undo {
        /// A run id, or `latest`
        #[arg(default_value = "latest")]
        run: String,
    },
    /// List every check, its group and its safety class
    Capabilities,
}

/// Run the doctor. Exit 0 when everything passes, 11 on failures, 12 when a fixer
/// failed, 13 when a precondition or an undo integrity check refused.
pub fn run(args: DoctorArgs, ctx: Context) -> anyhow::Result<i32> {
    let out = Out::new(ctx.json, false, ctx.verbose);
    match &args.sub {
        Some(DoctorCmd::Capabilities) => {
            let out = out.forced_json();
            out.emit(report::capabilities(&ctx), |_| {});
            Ok(0)
        }
        Some(DoctorCmd::Undo { run }) => undo(run, &ctx, &out),
        None => diagnose(args, ctx, out),
    }
}

fn undo(run: &str, ctx: &Context, out: &Out) -> anyhow::Result<i32> {
    let dir = mutate::resolve_run(ctx, run)?;
    let steps = mutate::undo(ctx, &dir)?;
    let failed = steps.iter().filter(|s| s.outcome == "failed").count();
    let value = serde_json::json!({
        "ok": failed == 0,
        "schema": fproton_core::SCHEMA_VERSION,
        "action": "doctor undo",
        "run_dir": dir.display().to_string(),
        "steps": steps,
    });
    out.emit(value, |o| {
        if steps.is_empty() {
            o.line("nothing to undo: that run changed nothing");
        }
        for step in &steps {
            let tag = match step.outcome.as_str() {
                "failed" => o.paint("failed ", Paint::Red),
                "nothing to undo" => o.paint("skipped", Paint::Dim),
                _ => o.paint("undone ", Paint::Green),
            };
            o.line(format!(
                "{tag} [{}] {} — {}",
                step.seq, step.check, step.outcome
            ));
            if let Some(note) = &step.note {
                o.line(format!("        {}", o.paint(note, Paint::Dim)));
            }
        }
        o.note(format!("run: {}", dir.display()));
    });
    Ok(if failed > 0 { 12 } else { 0 })
}

fn diagnose(args: DoctorArgs, ctx: Context, out: Out) -> anyhow::Result<i32> {
    let selected: Vec<&checks::CheckDef> = CHECKS
        .iter()
        .filter(|c| args.only.is_none() || args.only == Some(c.group))
        .collect();

    let mut probes = probes::Probes::capture(&ctx, args.only);
    let mut rows: Vec<CheckReport> = Vec::new();
    let mut plans: Vec<(&checks::CheckDef, checks::PlanKind)> = Vec::new();
    for def in &selected {
        let verdict = (def.detect)(&probes);
        if let Some(plan) = verdict.plan {
            plans.push((def, plan));
        }
        rows.push(CheckReport::new(def, &verdict));
    }

    // The `killswitch_orphan` fixer needs to have seen the condition in an earlier
    // run (§7), so the observation timestamp is recorded here — the one field a
    // detect-only run writes, and never under --dry-run.
    record_killswitch_observation(&ctx, &rows, args.dry_run);

    // `--fix` without `--yes` shows the plan and refuses (§2).
    let refuse_without_yes = args.fix && !args.yes && !args.dry_run;
    let planning_only = args.dry_run || refuse_without_yes;

    let mut mutator = None;
    let mut refused = false;
    if args.fix {
        let run = if planning_only {
            None
        } else {
            Some(mutate::RunDir::create(&ctx)?)
        };
        let mut m = mutate::Mutator::new(&ctx, run, planning_only);
        // §7: say what a Class C repair means before touching foreign state.
        if args.allow_keyring && plans.iter().any(|(d, _)| d.class == Class::C) {
            out.note(
                "class C: this switches which keyring the Proton CLI reads; \
                 you may need to run `protonvpn signin` again",
            );
        }
        for (def, kind) in &plans {
            if let Some(reason) = skip_reason(def, &args) {
                if let Some(row) = rows.iter_mut().find(|r| r.id == def.id) {
                    row.fix_outcome = Some(reason);
                }
                continue;
            }
            let outcome = apply_one(&mut m, &ctx, &probes, def, *kind);
            if let Some(row) = rows.iter_mut().find(|r| r.id == def.id) {
                match outcome {
                    Ok(text) => row.fix_outcome = Some(text),
                    Err(e) => {
                        row.fix_outcome = Some("refused".to_string());
                        row.fix_error = Some(e.to_string());
                        if e.exit_code() == 13 {
                            refused = true;
                        }
                    }
                }
            }
        }
        mutator = Some(m);
    }

    // Anything actually applied changes the world: re-probe and re-run just the
    // checks that were fixed, so `fixed` is a measurement rather than a claim (§7).
    let applied = mutator.as_ref().map(|m| m.applied()).unwrap_or(0);
    if applied > 0 {
        probes = probes::Probes::capture(&ctx, args.only);
        for def in &selected {
            let Some(row) = rows.iter_mut().find(|r| r.id == def.id) else {
                continue;
            };
            if row.fix_outcome.as_deref() != Some("applied") {
                continue;
            }
            let after = (def.detect)(&probes);
            row.status = after.status.name().to_string();
            row.reason = after.reason.map(str::to_string);
            row.verdict = match after.reason {
                Some(r) => format!("{}:{r}", after.status.name()),
                None => after.status.name().to_string(),
            };
            row.detail = after.detail.clone();
            row.fixable = after.plan.is_some();
            row.fix_outcome = Some(
                if matches!(after.status, CheckStatus::Pass | CheckStatus::Skip) {
                    "fixed".to_string()
                } else {
                    "applied, still failing".to_string()
                },
            );
        }
    }

    let actions = mutator.as_ref().map(|m| m.log.clone()).unwrap_or_default();
    let summary = Summary::of(&rows, &actions);
    let mut code = report::exit_code(&summary);
    if refused {
        code = 13;
    }
    if refuse_without_yes {
        code = 3;
    }
    let commands = next_commands(&summary, &args, refuse_without_yes);

    let report = Report {
        ok: summary.fail == 0 && summary.fix_failed == 0 && !refused,
        schema: fproton_core::SCHEMA_VERSION,
        run_id: mutator
            .as_ref()
            .and_then(|m| m.run_id().map(str::to_string)),
        run_dir: mutator
            .as_ref()
            .and_then(|m| m.run_dir().map(|d| d.display().to_string())),
        generated_at: fproton_core::now_rfc3339(),
        host_note: host_note(&rows),
        fix: args.fix,
        dry_run: planning_only,
        allow_keyring: args.allow_keyring,
        only: report::only_name(args.only),
        checks: rows,
        actions,
        summary,
        exit_code: code,
        commands,
    };

    // A run without --fix writes nothing unless --save is passed (§7).
    let mut saved_dir: Option<PathBuf> = None;
    if let Some(dir) = mutator.as_ref().and_then(|m| m.run_dir()) {
        write_artifacts(dir, &report, &probes)?;
    } else if args.save {
        let run = mutate::RunDir::create(&ctx)?;
        write_artifacts(&run.dir, &report, &probes)?;
        saved_dir = Some(run.dir.clone());
    }

    out.emit(
        serde_json::to_value(&report).unwrap_or(serde_json::Value::Null),
        |o| report::human(o, &report),
    );
    if let Some(dir) = saved_dir {
        out.note(format!("run artifacts: {}", dir.display()));
    }
    if refuse_without_yes {
        out.note("nothing was changed; add --yes to apply, or --dry-run to keep planning");
    }
    Ok(code)
}

fn next_commands(summary: &Summary, args: &DoctorArgs, refused_without_yes: bool) -> Vec<String> {
    let mut cmds = Vec::new();
    if refused_without_yes {
        cmds.push("fproton doctor --fix --yes".to_string());
        return cmds;
    }
    if summary.fixable > 0 && !args.fix {
        cmds.push("fproton doctor --fix --dry-run".to_string());
    }
    if summary.fail > 0 || summary.warn > 0 {
        cmds.push(
            "fproton doctor --json | jq '.checks[] | select(.status != \"pass\")'".to_string(),
        );
    }
    if args.fix && !args.dry_run && args.yes && summary.fixed > 0 {
        cmds.push("fproton doctor undo latest".to_string());
    }
    cmds
}

/// Why a fixable check is not being fixed on this invocation.
fn skip_reason(def: &checks::CheckDef, args: &DoctorArgs) -> Option<String> {
    match def.class {
        Class::C if !args.allow_keyring => {
            Some("skipped: class C touches foreign state and needs --allow-keyring".to_string())
        }
        Class::D => Some("skipped: class D is never auto-fixed".to_string()),
        _ => None,
    }
}

fn apply_one(
    m: &mut mutate::Mutator,
    ctx: &Context,
    probes: &probes::Probes,
    def: &checks::CheckDef,
    kind: checks::PlanKind,
) -> Result<String, Error> {
    let actions = mutate::plan(kind, ctx, probes)?;
    let mut applied = 0;
    let mut already = 0;
    for action in actions {
        match m.mutate(def.id, def.class, action)? {
            mutate::Outcome::Applied => applied += 1,
            mutate::Outcome::AlreadyDone => already += 1,
            mutate::Outcome::Planned => {}
        }
    }
    Ok(if applied > 0 {
        "applied".to_string()
    } else if already > 0 {
        "already done".to_string()
    } else {
        "planned".to_string()
    })
}

/// Remember when `killswitch_orphan` was first seen, and forget it once it clears.
fn record_killswitch_observation(ctx: &Context, rows: &[CheckReport], dry_run: bool) {
    if dry_run {
        return;
    }
    let Some(row) = rows.iter().find(|r| r.id == "killswitch_orphan") else {
        return;
    };
    let orphaned = row
        .reason
        .as_deref()
        .is_some_and(|r| r.starts_with("orphan"));
    // Nothing to record and nothing recorded: do not touch state.json at all, so a
    // healthy machine's state dir is byte-identical before and after a doctor run.
    let already = state::load(ctx)
        .map(|f| f.killswitch_orphan_seen_at.is_some())
        .unwrap_or(false);
    if orphaned == already {
        return;
    }
    let _ = state::update(ctx, |file| {
        file.killswitch_orphan_seen_at = orphaned.then(fproton_core::now_rfc3339);
    });
}

/// One line the report carries so a reader of `report.json` alone knows what this
/// machine looked like, without re-deriving it from every row.
fn host_note(rows: &[CheckReport]) -> String {
    let bad: Vec<&str> = rows
        .iter()
        .filter(|r| r.status == "fail")
        .map(|r| r.id.as_str())
        .collect();
    if bad.is_empty() {
        "no check failed".to_string()
    } else {
        format!("failing: {}", bad.join(", "))
    }
}

fn write_artifacts(dir: &Path, report: &Report, probes: &probes::Probes) -> Result<(), Error> {
    report::save(dir, report)?;
    let probe_dir = dir.join("probes");
    std::fs::create_dir_all(&probe_dir)
        .map_err(|e| Error::failure(format!("cannot create {}: {e}", probe_dir.display())))?;
    for (name, probe) in probes::named_commands(probes) {
        let _ = std::fs::write(
            probe_dir.join(format!("{name}.txt")),
            probes::artifact_text(probe),
        );
    }
    // The 24 MB serverlist is recorded as facts, never as bytes (§7).
    if let Ok(text) = serde_json::to_string_pretty(&serde_json::json!({
        "taken_at": probes.taken_at,
        "groups": probes.groups,
        "serverlist": probes.serverlist,
        "index": probes.index,
        "collections": probes.collections,
        "alias": probes.alias,
        "proton_items": probes.proton_items.len(),
        "state_dir": probes.state_dir,
        "plugin_link": probes.plugin_link,
        "menu_missing_ids": probes.menu_missing_ids,
        "settings_problems": probes.settings_problems,
    })) {
        let _ = std::fs::write(probe_dir.join("facts.json"), probes::truncate(text));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_group_has_a_name_and_the_list_is_complete() {
        assert_eq!(GROUPS.len(), 6);
        for g in GROUPS {
            assert!(!g.name().is_empty());
        }
    }

    #[test]
    fn the_repo_plugin_dir_points_at_this_checkout() {
        let dir = repo_plugin_dir();
        assert!(
            dir.ends_with("plugin/oliver.protonvpn"),
            "{}",
            dir.display()
        );
    }
}
