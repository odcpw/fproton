//! Desktop integration (DESIGN.md §4 `setup`): idempotent, and never a parse-and-rewrite
//! of the user's own files.
//!
//! `--menu` inserts the entries of `assets/menu.jsonc` **textually** into
//! `~/.config/omarchy/extensions/omarchy-menu.jsonc`, after a verbatim backup: a serde
//! round-trip would delete every comment in a file the owner maintains by hand, and
//! the owner's own `torrents.*` entries live in it.
//!
//! Every step reports `changed` or `already`, so a second run is visibly a no-op.

use std::path::{Path, PathBuf};

use clap::Args;
use fproton_core::proc::{run_with_deadline, CancelToken};
use fproton_core::{Context, Error};
use serde_json::json;

use crate::output::{Out, Paint};

/// The menu entries `--menu` merges. Single-line objects: the merge inserts lines.
pub const MENU_TEMPLATE: &str = include_str!("../assets/menu.jsonc");

/// The plugin id this repo ships, and the one it replaces (§1.1).
const PLUGIN_ID: &str = "oliver.protonvpn";
const OLD_PLUGIN_ID: &str = "io.github.grichard99.omaproton-vpn";
/// Where the bar widget is placed, matching fbittorrent's README (§1.5).
const BAR_ANCHOR: &str = "omarchy.tailscale";
/// The marker line written once, above the first inserted entry.
const MENU_MARKER: &str = "  // fproton";

/// `fproton setup [--plugin] [--menu] [--disable-old-plugin] [--all]`.
#[derive(Args, Debug, Default, Clone)]
pub struct SetupArgs {
    /// Link and enable the bar widget, and place it before omarchy.tailscale
    #[arg(long)]
    pub plugin: bool,
    /// Merge fproton's entries into the Omarchy menu
    #[arg(long)]
    pub menu: bool,
    /// Disable the old io.github.grichard99.omaproton-vpn plugin if it is enabled
    #[arg(long)]
    pub disable_old_plugin: bool,
    /// All of the above
    #[arg(long)]
    pub all: bool,
    /// Set from the global --yes (not a flag of its own).
    #[clap(skip)]
    pub yes: bool,
    /// Set from the global --dry-run (not a flag of its own).
    #[clap(skip)]
    pub dry_run: bool,
}

/// One thing setup did, or would do.
#[derive(Debug)]
struct Step {
    name: &'static str,
    /// `changed`, `already`, `would`, `skipped` or `failed`.
    outcome: &'static str,
    detail: String,
}

impl Step {
    fn json(&self) -> serde_json::Value {
        json!({"step": self.name, "outcome": self.outcome, "detail": self.detail})
    }
}

/// The repo directory this binary was built from, so `--plugin` links the source tree
/// rather than a copy. `CARGO_MANIFEST_DIR` is `crates/fproton`.
fn repo_plugin_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."))
        .join("plugin")
        .join(PLUGIN_ID)
}

fn omarchy(ctx: &Context, args: &[&str]) -> Result<String, Error> {
    let mut argv = vec!["omarchy"];
    argv.extend_from_slice(args);
    let out = run_with_deadline(&argv, ctx.timeouts.probe(), Some(&CancelToken::new()))?;
    if out.timed_out {
        return Err(Error::upstream(format!(
            "omarchy {} was killed at its {}s deadline",
            args.join(" "),
            ctx.timeouts.probe_sec
        ))
        .with_hint("omarchy plugin list"));
    }
    if !out.success() {
        return Err(Error::upstream(format!(
            "omarchy {} failed: {}",
            args.join(" "),
            out.stderr_tail()
        ))
        .with_hint("omarchy plugin list"));
    }
    Ok(out.stdout_string())
}

/// The state column of `omarchy plugin list` for one id, if it is listed at all.
fn plugin_state(ctx: &Context, id: &str) -> Option<String> {
    let listing = omarchy(ctx, &["plugin", "list"]).ok()?;
    listing.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        (fields.next()? == id).then(|| fields.next().unwrap_or("").to_string())
    })
}

// ---------------------------------------------------------------- the steps

/// `~/.config/omarchy/plugins/oliver.protonvpn` → this repo's `plugin/oliver.protonvpn`.
fn link_plugin(ctx: &Context, dry_run: bool) -> Step {
    let link = ctx.paths.plugin_link();
    let target = repo_plugin_dir();
    if !target.is_dir() {
        return Step {
            name: "plugin_linked",
            outcome: "skipped",
            detail: format!("{} does not exist yet", target.display()),
        };
    }
    if let Ok(existing) = std::fs::read_link(&link) {
        if existing == target {
            return Step {
                name: "plugin_linked",
                outcome: "already",
                detail: format!("{} → {}", link.display(), target.display()),
            };
        }
    }
    if link.exists() && std::fs::read_link(&link).is_err() {
        // A real directory, not a link: never replaced — that would delete someone's files.
        return Step {
            name: "plugin_linked",
            outcome: "failed",
            detail: format!(
                "{} exists and is not a symlink; leaving it alone",
                link.display()
            ),
        };
    }
    if dry_run {
        return Step {
            name: "plugin_linked",
            outcome: "would",
            detail: format!("ln -sfn {} {}", target.display(), link.display()),
        };
    }
    if let Some(parent) = link.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::remove_file(&link);
    match std::os::unix::fs::symlink(&target, &link) {
        Ok(()) => Step {
            name: "plugin_linked",
            outcome: "changed",
            detail: format!("{} → {}", link.display(), target.display()),
        },
        Err(e) => Step {
            name: "plugin_linked",
            outcome: "failed",
            detail: format!("cannot link {}: {e}", link.display()),
        },
    }
}

/// `omarchy plugin enable oliver.protonvpn` + `omarchy bar put … --before omarchy.tailscale`.
fn enable_plugin(ctx: &Context, dry_run: bool) -> Step {
    match plugin_state(ctx, PLUGIN_ID).as_deref() {
        Some("enabled") => {
            return Step {
                name: "plugin_enabled",
                outcome: "already",
                detail: format!("{PLUGIN_ID} is enabled"),
            }
        }
        None => {
            return Step {
                name: "plugin_enabled",
                outcome: "skipped",
                detail: format!("{PLUGIN_ID} is not in `omarchy plugin list` yet"),
            }
        }
        Some(_) => {}
    }
    if dry_run {
        return Step {
            name: "plugin_enabled",
            outcome: "would",
            detail: format!(
                "omarchy plugin enable {PLUGIN_ID} && omarchy bar put {PLUGIN_ID} --before {BAR_ANCHOR}"
            ),
        };
    }
    if let Err(e) = omarchy(ctx, &["plugin", "enable", PLUGIN_ID]) {
        return Step {
            name: "plugin_enabled",
            outcome: "failed",
            detail: e.to_string(),
        };
    }
    match omarchy(ctx, &["bar", "put", PLUGIN_ID, "--before", BAR_ANCHOR]) {
        Ok(_) => Step {
            name: "plugin_enabled",
            outcome: "changed",
            detail: format!("enabled and placed before {BAR_ANCHOR}"),
        },
        Err(e) => Step {
            name: "plugin_enabled",
            outcome: "failed",
            detail: format!("enabled, but not placed: {e}"),
        },
    }
}

/// `omarchy plugin disable io.github.grichard99.omaproton-vpn`, only when enabled (§4).
fn disable_old_plugin(ctx: &Context, dry_run: bool) -> Step {
    match plugin_state(ctx, OLD_PLUGIN_ID).as_deref() {
        None => Step {
            name: "old_plugin_enabled",
            outcome: "already",
            detail: format!("{OLD_PLUGIN_ID} is not installed"),
        },
        Some("enabled") if dry_run => Step {
            name: "old_plugin_enabled",
            outcome: "would",
            detail: format!("omarchy plugin disable {OLD_PLUGIN_ID}"),
        },
        Some("enabled") => match omarchy(ctx, &["plugin", "disable", OLD_PLUGIN_ID]) {
            Ok(_) => Step {
                name: "old_plugin_enabled",
                outcome: "changed",
                detail: format!("{OLD_PLUGIN_ID} disabled"),
            },
            Err(e) => Step {
                name: "old_plugin_enabled",
                outcome: "failed",
                detail: e.to_string(),
            },
        },
        Some(state) => Step {
            name: "old_plugin_enabled",
            outcome: "already",
            detail: format!("{OLD_PLUGIN_ID} is {state}"),
        },
    }
}

/// Every `"id":` at the start of a line, with comments stripped first — used only to
/// learn which ids already exist. The user's file is never rewritten from this.
pub fn ids_in_menu(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("//") {
            continue;
        }
        let Some(rest) = line.strip_prefix('"') else {
            continue;
        };
        let Some(end) = rest.find('"') else { continue };
        let id = &rest[..end];
        if rest[end + 1..].trim_start().starts_with(':') {
            ids.push(id.to_string());
        }
    }
    ids
}

/// The single-line entries of `assets/menu.jsonc`, as `(id, line)`.
pub fn template_entries() -> Vec<(String, String)> {
    let mut entries = Vec::new();
    for line in MENU_TEMPLATE.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('"') {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix('"') else {
            continue;
        };
        let Some(end) = rest.find('"') else { continue };
        let id = rest[..end].to_string();
        let mut body = trimmed.trim_end().to_string();
        if !body.ends_with(',') {
            body.push(',');
        }
        entries.push((id, format!("  {body}")));
    }
    entries
}

/// True for a line that carries no JSON value: blank, or a `//` / `/* … */` comment.
///
/// §4 chose a textual merge precisely because this file is "user-owned … full of
/// comments", so neither the closing brace nor the comma may be chosen by looking at
/// the last non-blank line: in a commented file that line is very often a comment.
fn is_comment_or_blank(line: &str) -> bool {
    let t = line.trim_start();
    t.is_empty() || t.starts_with("//") || t.starts_with("/*") || t.starts_with('*')
}

/// Every line of `text` paired with the byte offset it starts at.
fn lines_with_offsets(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    for line in text.split_inclusive('\n') {
        out.push((offset, line.trim_end_matches(['\n', '\r'])));
        offset += line.len();
    }
    out
}

/// Insert the missing entries immediately before the file's final `}` (§4).
///
/// Returns the new text, or `None` when every id is already present. Existing lines
/// are left byte for byte alone, with one exception the dry-run diff shows: the entry
/// that was previously last gains the trailing comma the new entries need after it.
pub fn merge_menu(existing: &str, entries: &[(String, String)]) -> Option<String> {
    let have = ids_in_menu(existing);
    let missing: Vec<&(String, String)> = entries
        .iter()
        .filter(|(id, _)| !have.iter().any(|h| h == id))
        .collect();
    if missing.is_empty() {
        return None;
    }
    // The document's closing brace — never one that only appears inside a trailing
    // comment (`// } keep the personal entries above this line`).
    let close = lines_with_offsets(existing)
        .iter()
        .rev()
        .find(|(_, l)| !is_comment_or_blank(l) && l.contains('}'))
        .and_then(|(offset, l)| l.rfind('}').map(|i| offset + i))
        .or_else(|| existing.rfind('}'))?;
    let (head, tail) = existing.split_at(close);

    // The last line that actually *ends a JSON value* must end in `,` (or be the
    // opening `{`) for the file to stay valid JSONC; the owner's last entry usually
    // has no trailing comma. Appending it after `head.trim_end()` would have put the
    // comma inside whatever comment happened to come last, breaking the file.
    let mut head = head.to_string();
    let comma_at = {
        let lines = lines_with_offsets(&head);
        lines
            .iter()
            .rev()
            .find(|(_, l)| !is_comment_or_blank(l))
            .and_then(|(offset, l)| {
                let content = l.trim_end();
                (!content.ends_with(',') && !content.ends_with('{'))
                    .then_some(offset + content.len())
            })
    };
    if let Some(at) = comma_at {
        head.insert(at, ',');
    }
    if !head.ends_with('\n') {
        head.push('\n');
    }
    head.push_str(MENU_MARKER);
    head.push('\n');
    for (i, (_, line)) in missing.iter().enumerate() {
        let last = i + 1 == missing.len();
        if last {
            head.push_str(line.trim_end_matches(','));
        } else {
            head.push_str(line);
        }
        head.push('\n');
    }
    Some(format!("{head}{tail}"))
}

/// A minimal line diff — every line that goes away, then every line that appears.
///
/// Not a unified diff: there is no context and no hunk header. It only has to be
/// *complete*, and the `+`-only version was not: the merge appends a comma to the
/// entry that was previously last, and showing that rewritten line as a pure addition
/// made it read as a duplicate of the owner's own entry. This is the only preview of
/// the only user-owned file fproton writes, so it must show removals too.
fn diff(path: &Path, before: &str, after: &str) -> String {
    let mut out = format!(
        "--- {}\n+++ {} (fproton setup --menu)\n",
        path.display(),
        path.display()
    );
    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();
    for line in &before_lines {
        if after_lines.contains(line) {
            continue;
        }
        out.push('-');
        out.push_str(line);
        out.push('\n');
    }
    for line in &after_lines {
        if before_lines.contains(line) {
            continue;
        }
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn merge_menu_step(ctx: &Context, dry_run: bool) -> (Step, Option<String>) {
    let path = ctx.paths.omarchy_menu();
    let entries = template_entries();
    let existing = std::fs::read_to_string(&path).ok();

    let Some(existing) = existing else {
        // No file at all: write the template verbatim (§4 step 4).
        if dry_run {
            return (
                Step {
                    name: "menu_entries",
                    outcome: "would",
                    detail: format!("write {} from the template", path.display()),
                },
                Some(MENU_TEMPLATE.to_string()),
            );
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        return match std::fs::write(&path, MENU_TEMPLATE) {
            Ok(()) => (
                Step {
                    name: "menu_entries",
                    outcome: "changed",
                    detail: format!("{} written from the template", path.display()),
                },
                None,
            ),
            Err(e) => (
                Step {
                    name: "menu_entries",
                    outcome: "failed",
                    detail: format!("cannot write {}: {e}", path.display()),
                },
                None,
            ),
        };
    };

    let Some(merged) = merge_menu(&existing, &entries) else {
        return (
            Step {
                name: "menu_entries",
                outcome: "already",
                detail: format!("every fproton entry is already in {}", path.display()),
            },
            None,
        );
    };
    if dry_run {
        return (
            Step {
                name: "menu_entries",
                outcome: "would",
                detail: format!("insert {} entries into {}", entries.len(), path.display()),
            },
            Some(diff(&path, &existing, &merged)),
        );
    }

    // A verbatim backup first, named by UTC basic time (§4 step 1).
    let stamp = jiff::Timestamp::now()
        .strftime("%Y%m%dT%H%M%SZ")
        .to_string();
    let backups = ctx.paths.backups_dir();
    if let Err(e) = std::fs::create_dir_all(&backups) {
        return (
            Step {
                name: "menu_entries",
                outcome: "failed",
                detail: format!("cannot create {}: {e}", backups.display()),
            },
            None,
        );
    }
    let backup = backups.join(format!("omarchy-menu.jsonc.{stamp}"));
    if let Err(e) = std::fs::write(&backup, &existing) {
        return (
            Step {
                name: "menu_entries",
                outcome: "failed",
                detail: format!("cannot back up to {}: {e}", backup.display()),
            },
            None,
        );
    }
    match std::fs::write(&path, &merged) {
        Ok(()) => (
            Step {
                name: "menu_entries",
                outcome: "changed",
                detail: format!(
                    "inserted into {}; backup at {}",
                    path.display(),
                    backup.display()
                ),
            },
            None,
        ),
        Err(e) => (
            Step {
                name: "menu_entries",
                outcome: "failed",
                detail: format!("cannot write {}: {e}", path.display()),
            },
            None,
        ),
    }
}

/// `~/.local/bin/fproton` is what the Omarchy shell's PATH can see (§1.5).
fn binary_on_path(ctx: &Context) -> Step {
    let installed = ctx.paths.local_bin.join(fproton_core::NAME);
    if installed.exists() {
        Step {
            name: "binary_on_path",
            outcome: "already",
            detail: format!("{} exists", installed.display()),
        }
    } else {
        Step {
            name: "binary_on_path",
            outcome: "failed",
            detail: format!(
                "{} is missing; the Omarchy shell's PATH does not include ~/.cargo/bin",
                installed.display()
            ),
        }
    }
}

// ---------------------------------------------------------------- frozen API

/// Run the requested integration steps. Idempotent: a second run changes nothing.
pub fn run(args: SetupArgs, ctx: Context) -> anyhow::Result<i32> {
    let out = Out::new(ctx.json, false, ctx.verbose);
    let (plugin, menu, old) = if args.all {
        (true, true, true)
    } else if !args.plugin && !args.menu && !args.disable_old_plugin {
        // A bare `fproton setup` reports without changing anything.
        (false, false, false)
    } else {
        (args.plugin, args.menu, args.disable_old_plugin)
    };
    let nothing_asked = !plugin && !menu && !old;
    // Without --yes, every step runs as a dry run and says so (§2).
    let dry_run = args.dry_run || nothing_asked || !args.yes;

    let mut steps = vec![binary_on_path(&ctx)];
    let mut diffs = Vec::new();
    if plugin || nothing_asked {
        steps.push(link_plugin(&ctx, dry_run));
        steps.push(enable_plugin(&ctx, dry_run));
    }
    if menu || nothing_asked {
        let (step, text) = merge_menu_step(&ctx, dry_run);
        steps.push(step);
        if let Some(text) = text {
            diffs.push(text);
        }
    }
    if old || nothing_asked {
        steps.push(disable_old_plugin(&ctx, dry_run));
    }

    let failed = steps.iter().any(|s| s.outcome == "failed");
    let value = json!({
        "ok": !failed,
        "schema": fproton_core::SCHEMA_VERSION,
        "action": "setup",
        "dry_run": dry_run,
        "steps": steps.iter().map(Step::json).collect::<Vec<_>>(),
        "diff": diffs.join("\n"),
    });
    out.emit(value, |o| {
        for step in &steps {
            let tag = match step.outcome {
                "changed" => o.paint("changed", Paint::Green),
                "already" => o.paint("already", Paint::Dim),
                "would" => o.paint("would  ", Paint::Yellow),
                "failed" => o.paint("failed ", Paint::Red),
                other => o.paint(other, Paint::Dim),
            };
            o.line(format!("{tag} {:<18} {}", step.name, step.detail));
        }
        for d in &diffs {
            o.line(d);
        }
        if dry_run && !nothing_asked {
            o.note("nothing was changed; add --yes to apply");
        }
        if nothing_asked {
            o.note("nothing was changed; pass --plugin, --menu, --disable-old-plugin or --all");
        }
    });
    Ok(if failed { 1 } else { 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER_MENU: &str = r#"{
  // the owner's own entries
  "torrents": {"icon":"","label":"Torrents"},
  "torrents.open": {"icon":"","label":"Open","action":"fbittorrent tui"}
}
"#;

    #[test]
    fn the_template_parses_into_five_single_line_entries() {
        let entries = template_entries();
        let ids: Vec<&str> = entries.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "protonvpn",
                "protonvpn.connect",
                "protonvpn.disconnect",
                "protonvpn.open",
                "protonvpn.doctor"
            ]
        );
        for (_, line) in &entries {
            assert!(line.starts_with("  \""), "one line per entry: {line}");
        }
    }

    /// The last entry before the insertion point is a `//` comment — the shape §4
    /// warns about, and the one the first cut got wrong: the comma was appended after
    /// `head.trim_end()`, which put it *inside* the comment and left the entry above
    /// with no comma at all, so the merged file no longer parsed.
    const COMMENTED_TAIL_MENU: &str = r#"{
  // the owner's own entries
  "torrents.stop": {"icon":"","label":"Stop","action":"fbittorrent down"}
  // keep the personal entries above this line
}
"#;

    /// Strip whole-line `//` comments the way a JSONC reader may, then parse.
    fn parse_jsonc(text: &str) -> serde_json::Value {
        let stripped: String = text
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        serde_json::from_str(&stripped)
            .unwrap_or_else(|e| panic!("merged menu is not JSON: {e}\n{stripped}"))
    }

    #[test]
    fn a_merge_keeps_every_existing_line_except_the_trailing_comma() {
        let merged = merge_menu(USER_MENU, &template_entries()).expect("something to insert");
        // Exactly one line may change, and only by gaining a `,`: the entry that was
        // last. Every other line survives byte for byte. (README says the same.)
        let mut rewritten = Vec::new();
        for line in USER_MENU.lines().filter(|l| !l.trim().is_empty()) {
            if line.trim() == "}" {
                continue;
            }
            // The newline matters: without it `…tui"}` matches inside `…tui"},`
            // and a rewritten line reads as an untouched one.
            if merged.contains(&format!("{line}\n")) {
                continue;
            }
            assert!(
                merged.contains(&format!("{line},\n")),
                "the merge dropped or rewrote: {line}"
            );
            rewritten.push(line);
        }
        assert_eq!(
            rewritten,
            [r#"  "torrents.open": {"icon":"","label":"Open","action":"fbittorrent tui"}"#],
            "only the previously-last entry may gain a comma"
        );
        assert!(
            merged.contains("// the owner's own entries"),
            "comments survive"
        );
        assert!(merged.contains(MENU_MARKER));
        assert!(merged.contains("\"protonvpn.doctor\""));
        assert!(merged.trim_end().ends_with('}'));
    }

    #[test]
    fn a_comment_as_the_last_line_does_not_swallow_the_comma() {
        let merged =
            merge_menu(COMMENTED_TAIL_MENU, &template_entries()).expect("something to insert");
        assert!(
            merged.contains("  // keep the personal entries above this line\n"),
            "the trailing comment must be untouched:\n{merged}"
        );
        assert!(
            merged.contains(r#""action":"fbittorrent down"},"#),
            "the comma belongs on the last real entry:\n{merged}"
        );
        let v = parse_jsonc(&merged);
        assert_eq!(v["torrents.stop"]["action"], "fbittorrent down");
        assert_eq!(v["protonvpn.connect"]["action"], "fproton connect");
    }

    #[test]
    fn a_closing_brace_inside_a_trailing_comment_is_not_the_insertion_point() {
        let menu = "{\n  \"torrents\": {\"label\":\"T\"}\n}\n// } that brace is prose\n";
        let merged = merge_menu(menu, &template_entries()).expect("something to insert");
        assert!(
            merged.trim_end().ends_with("// } that brace is prose"),
            "the comment must stay after the document:\n{merged}"
        );
        let v = parse_jsonc(&merged);
        assert_eq!(v["protonvpn.connect"]["action"], "fproton connect");
    }

    #[test]
    fn the_dry_run_diff_shows_the_rewritten_line_as_a_removal_and_an_addition() {
        let merged = merge_menu(USER_MENU, &template_entries()).unwrap();
        let text = diff(Path::new("/tmp/omarchy-menu.jsonc"), USER_MENU, &merged);
        assert!(
            text.contains("\n-  \"torrents.open\""),
            "the line that gains a comma must show as a removal:\n{text}"
        );
        assert!(
            text.contains("\n+  \"torrents.open\""),
            "…and as the addition that replaces it:\n{text}"
        );
        assert!(text.contains("\n+  // fproton"));
    }

    #[test]
    fn the_merged_file_is_still_valid_jsonc() {
        let merged = merge_menu(USER_MENU, &template_entries()).unwrap();
        // Strip whole-line comments the way the design says a reader may, then parse.
        let stripped: String = merged
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let v: serde_json::Value = serde_json::from_str(&stripped)
            .unwrap_or_else(|e| panic!("merged menu is not JSON: {e}\n{stripped}"));
        assert!(v["torrents"].is_object(), "the owner's entries survive");
        // ids and actions are the contract; labels and glyphs belong to the docs agent.
        assert_eq!(v["protonvpn.connect"]["action"], "fproton connect");
        assert!(v["protonvpn"]["label"].is_string());
    }

    #[test]
    fn a_second_merge_is_a_no_op() {
        let once = merge_menu(USER_MENU, &template_entries()).unwrap();
        assert!(
            merge_menu(&once, &template_entries()).is_none(),
            "re-running setup --menu must change nothing"
        );
    }

    #[test]
    fn ids_are_read_from_the_file_but_never_from_its_comments() {
        let text = "{\n  // \"commented\": {},\n  \"real\": {\"a\":1}\n}\n";
        assert_eq!(ids_in_menu(text), vec!["real".to_string()]);
    }

    #[test]
    fn every_menu_action_is_a_constant_literal_argv_never_a_computed_string() {
        // §1.5: only constant literals may reach omarchy-launch-or-focus-tui.
        for (_, line) in template_entries() {
            if let Some(i) = line.find("\"action\":") {
                let action = &line[i..];
                assert!(
                    !action.contains('$') && !action.contains('`') && !action.contains(';'),
                    "a menu action must be a constant literal: {line}"
                );
            }
        }
    }
}
