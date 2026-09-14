//! The doctor's two outputs: the `report.json` document (also what `--json` prints)
//! and the human table. `doctor capabilities` lives here too, because it describes
//! exactly the table [`checks::CHECKS`](super::checks::CHECKS) declares.

use fproton_core::{Context, Error};
use serde::Serialize;

use super::checks::{CheckDef, CheckStatus, Class, PlanKind, Severity, Verdict, CHECKS};
use super::mutate::{write_scopes, PlannedAction};
use super::DoctorGroup;
use crate::output::{Out, Paint};

/// One row of `report.json::checks` (§7).
#[derive(Clone, Debug, Serialize)]
pub struct CheckReport {
    pub id: String,
    pub group: String,
    pub severity: String,
    pub status: String,
    /// The short machine reason behind a non-pass; `null` on a pass.
    pub reason: Option<String>,
    /// `fail:phantom_collection` — status and reason joined, the way the table prints it.
    pub verdict: String,
    pub detail: String,
    pub fixable: bool,
    pub class: String,
    pub fix_cmd: Option<String>,
    /// What `--fix` did with this check, when it ran.
    pub fix_outcome: Option<String>,
    pub fix_error: Option<String>,
}

impl CheckReport {
    pub fn new(def: &CheckDef, v: &Verdict) -> CheckReport {
        CheckReport {
            id: def.id.to_string(),
            group: def.group.name().to_string(),
            severity: def.severity.name().to_string(),
            status: v.status.name().to_string(),
            reason: v.reason.map(str::to_string),
            verdict: match v.reason {
                Some(r) => format!("{}:{r}", v.status.name()),
                None => v.status.name().to_string(),
            },
            detail: v.detail.clone(),
            fixable: v.plan.is_some(),
            class: def.class.name().to_string(),
            fix_cmd: v.fix_cmd.clone(),
            fix_outcome: None,
            fix_error: None,
        }
    }
}

/// `report.json` in full.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub ok: bool,
    pub schema: u32,
    pub run_id: Option<String>,
    pub run_dir: Option<String>,
    pub generated_at: String,
    pub host_note: String,
    pub fix: bool,
    pub dry_run: bool,
    pub allow_keyring: bool,
    pub only: Option<String>,
    pub checks: Vec<CheckReport>,
    pub actions: Vec<PlannedAction>,
    pub summary: Summary,
    pub exit_code: i32,
    pub commands: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Summary {
    pub total: usize,
    pub pass: usize,
    pub warn: usize,
    pub fail: usize,
    pub skip: usize,
    pub fixable: usize,
    pub fixed: usize,
    pub fix_failed: usize,
    pub planned: usize,
}

impl Summary {
    pub fn of(checks: &[CheckReport], actions: &[PlannedAction]) -> Summary {
        let count = |s: &str| checks.iter().filter(|c| c.status == s).count();
        Summary {
            total: checks.len(),
            pass: count("pass"),
            warn: count("warn"),
            fail: count("fail"),
            skip: count("skip"),
            fixable: checks.iter().filter(|c| c.fixable).count(),
            fixed: actions.iter().filter(|a| a.outcome == "applied").count(),
            fix_failed: actions.iter().filter(|a| a.outcome == "failed").count(),
            planned: actions.iter().filter(|a| a.outcome == "planned").count(),
        }
    }
}

/// The §4a exit code this report implies (§7): 11 on failures, 12 when a fixer
/// failed, 0 otherwise. Warnings alone never fail a run.
pub fn exit_code(summary: &Summary) -> i32 {
    if summary.fix_failed > 0 {
        return 12;
    }
    if summary.fail > 0 {
        return 11;
    }
    0
}

/// The human table (§7). One line per check, then a summary and the next moves.
pub fn human(out: &Out, report: &Report) {
    let mut group = String::new();
    let width = report
        .checks
        .iter()
        .map(|c| c.id.len())
        .max()
        .unwrap_or(24)
        .max(24);
    for check in &report.checks {
        if check.group != group {
            group = check.group.clone();
            out.line(out.paint(&format!("[{group}]"), Paint::Dim));
        }
        let tag = match check.status.as_str() {
            "pass" => out.paint("pass", Paint::Green),
            "warn" => out.paint("warn", Paint::Yellow),
            "fail" => out.paint("fail", Paint::Red),
            _ => out.paint("skip", Paint::Dim),
        };
        let reason = match &check.reason {
            Some(r) if check.status != "pass" => format!(":{r}"),
            _ => String::new(),
        };
        out.line(format!(
            "  {tag}{reason:<22} {:<width$}  {}",
            check.id, check.detail
        ));
        if let Some(cmd) = &check.fix_cmd {
            if check.status != "pass" {
                out.line(format!(
                    "{:29}{:<width$}  {}",
                    "",
                    "",
                    out.paint(&format!("fix: {cmd}"), Paint::Dim)
                ));
            }
        }
        if let Some(outcome) = &check.fix_outcome {
            out.line(format!(
                "{:29}{:<width$}  {}",
                "",
                "",
                out.paint(&format!("--fix: {outcome}"), Paint::Dim)
            ));
        }
    }

    let s = &report.summary;
    out.line("");
    out.line(format!(
        "{} checks: {} pass, {} warn, {} fail, {} skip",
        s.total, s.pass, s.warn, s.fail, s.skip
    ));
    if !report.actions.is_empty() {
        out.line("");
        out.line(if report.dry_run {
            "would change:".to_string()
        } else {
            "changed:".to_string()
        });
        for action in &report.actions {
            let tag = match action.outcome.as_str() {
                "applied" => out.paint("applied", Paint::Green),
                "already" => out.paint("already", Paint::Dim),
                "planned" => out.paint("would  ", Paint::Yellow),
                _ => out.paint("failed ", Paint::Red),
            };
            out.line(format!("  {tag} [{}] {}", action.class, action.action));
            if let Some(e) = &action.error {
                out.line(format!("          {}", out.paint(e, Paint::Red)));
            }
        }
        let paths: Vec<&String> = report
            .actions
            .iter()
            .flat_map(|a| a.writes_to.iter())
            .collect();
        if report.dry_run && !paths.is_empty() {
            out.line(format!(
                "  {}",
                out.paint(
                    &format!(
                        "blast radius: {}",
                        paths
                            .iter()
                            .map(|p| p.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    Paint::Dim
                )
            ));
        }
    }
    if let Some(dir) = &report.run_dir {
        out.line("");
        out.line(out.paint(&format!("run artifacts: {dir}"), Paint::Dim));
    }
    if !report.commands.is_empty() {
        out.line("");
        for cmd in &report.commands {
            out.line(format!("  next: {cmd}"));
        }
    }
}

/// `fproton doctor capabilities --json` (§7): every check, group and class, plus the
/// exact set of paths `--fix` is allowed to write to.
pub fn capabilities(ctx: &Context) -> serde_json::Value {
    let checks: Vec<serde_json::Value> = CHECKS
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id,
                "group": c.group.name(),
                "class": c.class.name(),
                "severity": c.severity.name(),
                "title": c.title,
                "auto_fixable": fixable_plan(c).is_some(),
                "plan": fixable_plan(c).map(PlanKind::name),
            })
        })
        .collect();
    serde_json::json!({
        "ok": true,
        "schema": fproton_core::SCHEMA_VERSION,
        "tool": fproton_core::NAME,
        "version": fproton_core::VERSION,
        "contract_version": fproton_core::CONTRACT_VERSION,
        "subcommand": "doctor",
        "groups": (super::GROUPS).iter().map(|g| g.name()).collect::<Vec<_>>(),
        "classes": ([Class::A, Class::B, Class::C, Class::D])
            .iter()
            .map(|c| serde_json::json!({"class": c.name(), "meaning": c.describes()}))
            .collect::<Vec<_>>(),
        "severities": ([Severity::Info, Severity::Warn, Severity::Error])
            .iter().map(|s| s.name()).collect::<Vec<_>>(),
        "statuses": ([CheckStatus::Pass, CheckStatus::Warn, CheckStatus::Fail, CheckStatus::Skip])
            .iter().map(|s| s.name()).collect::<Vec<_>>(),
        "checks": checks,
        "check_count": CHECKS.len(),
        "flags": ["--fix", "--dry-run", "--allow-keyring", "--only <group>", "--yes", "--save"],
        "subcommands": ["undo <run-id|latest>", "capabilities"],
        "exit_codes": (fproton_core::EXIT_CODES)
            .iter()
            .filter(|(c, _, _)| matches!(c, 0 | 1 | 2 | 3 | 11 | 12 | 13 | 130))
            .map(|(code, name, meaning)| serde_json::json!({"code": code, "code_name": name, "meaning": meaning}))
            .collect::<Vec<_>>(),
        "write_scopes": write_scopes(ctx).iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        "never_does": [
            "delete a file, a NetworkManager profile or a keyring entry",
            "open, hash, back up or log any *.keyring file",
            "unlock a Secret Service collection or restart gnome-keyring-daemon",
            "run a shell string: every command is argv",
            "contact Proton's API",
        ],
        "run_artifacts": {
            "dir": ctx.paths.doctor_runs_dir().display().to_string(),
            "latest": ctx.paths.doctor_latest().display().to_string(),
            "written_when": "--fix (always) or --save",
            "keep": (super::mutate::KEEP_RUNS),
            "files": ["report.json", "actions.jsonl", "probes/", "backups/"],
        },
        "health_check": "fproton doctor --json >/dev/null; echo $?",
    })
}

fn fixable_plan(def: &CheckDef) -> Option<PlanKind> {
    // The class table is the contract; a Class D check is never auto-fixable.
    match def.class {
        Class::D => None,
        _ => match def.id {
            "index_fresh" => Some(PlanKind::RebuildIndex),
            "state_dir" => Some(PlanKind::CreateStateDir),
            "stale_pending_action" => Some(PlanKind::ClearPending),
            "settings_valid" => Some(PlanKind::NormaliseSettings),
            "plugin_linked" => Some(PlanKind::LinkPlugin),
            "plugin_enabled" => Some(PlanKind::EnablePlugin),
            "menu_entries" => Some(PlanKind::MergeMenu),
            "old_plugin_enabled" => Some(PlanKind::DisableOldPlugin),
            "killswitch_orphan" => Some(PlanKind::KillswitchDown),
            "secret_service_default_alias" => Some(PlanKind::SetDefaultAlias),
            _ => None,
        },
    }
}

/// Write `report.json` into the run directory.
pub fn save(dir: &std::path::Path, report: &Report) -> Result<(), Error> {
    let text = serde_json::to_string_pretty(report)
        .map_err(|e| Error::failure(format!("cannot serialise the report: {e}")))?;
    std::fs::write(dir.join("report.json"), text)
        .map_err(|e| Error::failure(format!("cannot write report.json: {e}")))
}

/// Which group a `--only` value names, for the report envelope.
pub fn only_name(only: Option<DoctorGroup>) -> Option<String> {
    only.map(|g| g.name().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_capability_document_names_every_check_and_no_others() {
        let ctx = Context::load().expect("context");
        let caps = capabilities(&ctx);
        let listed = caps["checks"].as_array().expect("checks").len();
        assert_eq!(listed, CHECKS.len());
        assert_eq!(caps["check_count"].as_u64(), Some(CHECKS.len() as u64));
        // Every fixable check must belong to a class that can run under --fix.
        for check in caps["checks"].as_array().unwrap() {
            if check["auto_fixable"] == serde_json::json!(true) {
                assert_ne!(check["class"], serde_json::json!("D"), "{check}");
            }
        }
    }

    #[test]
    fn exit_codes_follow_the_design_table() {
        let mut s = Summary::default();
        assert_eq!(exit_code(&s), 0);
        s.warn = 3;
        assert_eq!(exit_code(&s), 0, "warnings alone are not a failure");
        s.fail = 1;
        assert_eq!(exit_code(&s), 11);
        s.fix_failed = 1;
        assert_eq!(exit_code(&s), 12);
    }

    #[test]
    fn every_class_d_check_is_reported_as_not_auto_fixable() {
        for def in CHECKS {
            if def.class == Class::D {
                assert!(fixable_plan(def).is_none(), "{}", def.id);
            }
        }
    }
}
