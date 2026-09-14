//! Every path fproton reads or writes, in one place (DESIGN.md §3).
//!
//! `FPROTON_STATE_DIR`, `FPROTON_CONFIG_DIR` and `FPROTON_CACHE_DIR` override the XDG
//! locations so a test (and every e2e case, §4b) can run in its own directory.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::Error;

/// Resolved locations. Plain data: cloneable, no open handles.
#[derive(Clone, Debug, Serialize)]
pub struct Paths {
    /// `$XDG_STATE_HOME/fproton` — pending action, history, caches, doctor runs.
    pub state_dir: PathBuf,
    /// `$XDG_CONFIG_HOME/fproton` — `config.toml`.
    pub config_dir: PathBuf,
    /// `$XDG_CACHE_HOME/fproton` — the postcard server-index snapshot.
    pub cache_dir: PathBuf,
    /// `~/.cache/Proton/VPN` — Proton's own cache (read-only for fproton).
    pub proton_cache_dir: PathBuf,
    /// `~/.config/omarchy` — plugin directory and menu extension live below this.
    pub omarchy_config_dir: PathBuf,
    /// `~/.local/bin` — where the shell's PATH expects the binary (§1.5).
    pub local_bin: PathBuf,
    /// `~` — used only to build the paths above.
    pub home: PathBuf,
}

fn env_dir(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

impl Paths {
    /// Resolve every path from the environment. Creates nothing.
    pub fn discover() -> Result<Paths, Error> {
        let home =
            dirs::home_dir().ok_or_else(|| Error::failure("no home directory (HOME is unset)"))?;
        let state_dir = env_dir("FPROTON_STATE_DIR").unwrap_or_else(|| {
            dirs::state_dir()
                .unwrap_or_else(|| home.join(".local/state"))
                .join("fproton")
        });
        let config_dir = env_dir("FPROTON_CONFIG_DIR").unwrap_or_else(|| {
            dirs::config_dir()
                .unwrap_or_else(|| home.join(".config"))
                .join("fproton")
        });
        let cache_dir = env_dir("FPROTON_CACHE_DIR").unwrap_or_else(|| {
            dirs::cache_dir()
                .unwrap_or_else(|| home.join(".cache"))
                .join("fproton")
        });
        let proton_cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| home.join(".cache"))
            .join("Proton/VPN");
        let omarchy_config_dir = dirs::config_dir()
            .unwrap_or_else(|| home.join(".config"))
            .join("omarchy");
        Ok(Paths {
            local_bin: home.join(".local/bin"),
            home,
            state_dir,
            config_dir,
            cache_dir,
            proton_cache_dir,
            omarchy_config_dir,
        })
    }

    /// Create the three fproton-owned directories. Idempotent.
    pub fn ensure_dirs(&self) -> Result<(), Error> {
        for d in [&self.state_dir, &self.config_dir, &self.cache_dir] {
            std::fs::create_dir_all(d)
                .map_err(|e| Error::failure(format!("cannot create {}: {e}", d.display())))?;
        }
        Ok(())
    }

    // ---- fproton's own files -------------------------------------------------

    /// `config.toml` (fproton's settings, not Proton's).
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }
    /// Pending action, history, recents, caches, probe results.
    pub fn state_file(&self) -> PathBuf {
        self.state_dir.join("state.json")
    }
    /// The postcard server-index snapshot (§4 `index`).
    pub fn index_file(&self) -> PathBuf {
        self.cache_dir.join("index.bin")
    }
    /// Backups written before any doctor/setup file edit.
    pub fn backups_dir(&self) -> PathBuf {
        self.state_dir.join("backups")
    }
    /// One directory per `doctor --fix` (or `--save`) run.
    pub fn doctor_runs_dir(&self) -> PathBuf {
        self.state_dir.join("doctor/runs")
    }
    /// Symlink to the newest doctor run.
    pub fn doctor_latest(&self) -> PathBuf {
        self.state_dir.join("doctor/latest")
    }

    // ---- files owned by Proton (read-only) -----------------------------------

    /// Proton's 24 MB logical-server list.
    pub fn serverlist(&self) -> PathBuf {
        self.proton_cache_dir.join("serverlist.json")
    }
    /// Proton's client config (WireGuard ports, feature flags).
    pub fn clientconfig(&self) -> PathBuf {
        self.proton_cache_dir.join("clientconfig.json")
    }
    /// Last connection, including the NetworkManager UUID of its profile (§5).
    pub fn connection_persistence(&self) -> PathBuf {
        self.proton_cache_dir
            .join("connection/connection_persistence.json")
    }

    // ---- files owned by the desktop ------------------------------------------

    /// The user's menu extension, edited textually by `setup --menu` (§4).
    pub fn omarchy_menu(&self) -> PathBuf {
        self.omarchy_config_dir
            .join("extensions/omarchy-menu.jsonc")
    }
    /// Where `setup --plugin` links `plugin/oliver.protonvpn`.
    pub fn plugin_link(&self) -> PathBuf {
        self.omarchy_config_dir.join("plugins/oliver.protonvpn")
    }
    /// The Secret Service *pointer* file: 16 bytes naming a keyring basename.
    ///
    /// The single named exception to "never touch `~/.local/share/keyrings/**`" (§2):
    /// the doctor's `secret_service_default_alias` fixer rewrites this file. No
    /// `*.keyring` file is ever opened, hashed, backed up or logged.
    pub fn keyring_default_pointer(&self) -> PathBuf {
        self.home.join(".local/share/keyrings/default")
    }
    /// sysfs counters for a tunnel device; present only while the link is up.
    pub fn sysfs_device(&self, device: &str) -> PathBuf {
        Path::new("/sys/class/net").join(device)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_overrides_win() {
        // Serialised by the process-wide env: keep this the only test that sets it.
        std::env::set_var("FPROTON_STATE_DIR", "/tmp/fproton-test-state");
        let p = Paths::discover().expect("paths");
        assert_eq!(p.state_dir, PathBuf::from("/tmp/fproton-test-state"));
        assert_eq!(
            p.state_file(),
            PathBuf::from("/tmp/fproton-test-state/state.json")
        );
        std::env::remove_var("FPROTON_STATE_DIR");
    }

    #[test]
    fn proton_paths_are_read_only_locations() {
        let p = Paths::discover().expect("paths");
        assert!(p.serverlist().ends_with("Proton/VPN/serverlist.json"));
        assert!(p
            .connection_persistence()
            .ends_with("Proton/VPN/connection/connection_persistence.json"));
    }
}
