//! Running external commands without ever hanging (DESIGN.md §2).
//!
//! Every child runs in **its own process group** with all three stdio handles piped
//! (never inherited), under a deadline: SIGTERM to the group, then SIGKILL 2 s later.
//! A killed child comes back as `Output { timed_out: true }` — a deadline is data, not
//! a panic — while a cancelled one is `Err(Error::Interrupted)`.
//!
//! argv only: there is no shell anywhere in this module.

use std::io::Read;
use std::os::unix::process::{CommandExt, ExitStatusExt};

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::Error;

/// How often the wait loop re-checks the deadline and the cancel flag.
/// The frozen contract promises "at least every 250 ms"; this is 25 ms.
const POLL: Duration = Duration::from_millis(25);
/// Grace between SIGTERM and SIGKILL (§2).
const GRACE: Duration = Duration::from_secs(2);

/// A cooperative cancel flag shared with a running action (the TUI's `StopSignal`
/// bridges to this). Cheap to clone; safe to poll from any thread.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        CancelToken(Arc::new(AtomicBool::new(false)))
    }
    /// Ask the running action to stop. Idempotent.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// What a finished (or killed) child left behind.
#[derive(Clone, Debug)]
pub struct Output {
    /// Exit status, or `None` when the child was killed by a signal.
    pub code: Option<i32>,
    /// The signal that killed it, when one did.
    pub signal: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// True when fproton killed the child at its deadline (§4a maps this to exit 5).
    pub timed_out: bool,
    /// Wall clock from spawn to reap.
    pub duration: Duration,
    /// The child's pid, which is also its process-group id.
    pub pid: u32,
}

impl Output {
    pub fn success(&self) -> bool {
        self.code == Some(0) && !self.timed_out
    }
    pub fn stdout_string(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
    pub fn stderr_string(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
    /// The last non-empty line of stderr, for error messages.
    pub fn stderr_tail(&self) -> String {
        self.stderr_string()
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or_default()
            .trim()
            .to_string()
    }
}

/// Look a program up on `PATH` without spawning anything.
pub fn which(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        let p = PathBuf::from(program);
        return p.is_file().then_some(p);
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(program))
            .find(|c| c.is_file())
    })
}

fn signal_group(pgid: i32, sig: i32) {
    // Negative pid = "every process in this group". ESRCH (already gone) is fine.
    unsafe { libc::kill(-pgid, sig) };
}

/// Run `argv` with a deadline, in its own process group, with piped stdio.
///
/// * returns `Ok(Output { timed_out: true })` when the deadline killed the child;
/// * returns `Err(Error::Interrupted)` when `cancel` fired (the group is killed first);
/// * returns `Err(Error::Backend)` when the program could not be spawned at all.
pub fn run_with_deadline(
    argv: &[&str],
    timeout: Duration,
    cancel: Option<&CancelToken>,
) -> Result<Output, Error> {
    let Some((program, args)) = argv.split_first() else {
        return Err(Error::failure(
            "run_with_deadline called with an empty argv",
        ));
    };
    let started = Instant::now();
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::backend(format!("{program} is not installed"))
                    .with_hint(format!("omarchy pkg add {program}"))
            } else {
                Error::backend(format!("cannot run {program}: {e}"))
            }
        })?;
    let pid = child.id();
    let pgid = pid as i32;

    // The child must never wait on stdin: close it immediately.
    drop(child.stdin.take());

    // Drain both pipes on their own threads so a chatty child cannot fill a pipe
    // buffer and deadlock against our wait loop.
    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();
    let out_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = out_pipe.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = err_pipe.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });

    let mut timed_out = false;
    let mut cancelled = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {}
            Err(e) => return Err(Error::failure(format!("waiting for {program}: {e}"))),
        }
        if cancel.map(|c| c.is_cancelled()).unwrap_or(false) {
            cancelled = true;
        } else if started.elapsed() < timeout {
            std::thread::sleep(POLL);
            continue;
        } else {
            timed_out = true;
        }
        // Deadline or cancel: SIGTERM the whole group, SIGKILL it 2 s later.
        signal_group(pgid, libc::SIGTERM);
        let killed_at = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if killed_at.elapsed() >= GRACE => {
                    signal_group(pgid, libc::SIGKILL);
                    let _ = child.wait();
                    break;
                }
                Ok(None) => std::thread::sleep(POLL),
                Err(e) => return Err(Error::failure(format!("waiting for {program}: {e}"))),
            }
        }
        break child
            .wait()
            .map_err(|e| Error::failure(format!("waiting for {program}: {e}")))?;
    };

    let stdout = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();
    if cancelled {
        return Err(Error::Interrupted);
    }
    Ok(Output {
        code: status.code(),
        signal: status.signal(),
        stdout,
        stderr,
        timed_out,
        duration: started.elapsed(),
        pid,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(f: f64) -> Duration {
        Duration::from_secs_f64(f)
    }

    #[test]
    fn captures_stdout_and_exit_zero() {
        let out = run_with_deadline(&["echo", "hello fproton"], secs(5.0), None).unwrap();
        assert!(out.success());
        assert_eq!(out.stdout_string().trim(), "hello fproton");
        assert!(out.stderr.is_empty());
        assert!(!out.timed_out);
    }

    #[test]
    fn captures_stderr_and_a_non_zero_code() {
        let out =
            run_with_deadline(&["cat", "/nonexistent/fproton-test"], secs(5.0), None).unwrap();
        assert!(!out.success());
        assert_eq!(out.code, Some(1));
        assert!(!out.stderr_tail().is_empty(), "stderr must be captured");
    }

    #[test]
    fn a_deadline_kills_a_sleeping_child() {
        let t = Instant::now();
        let out = run_with_deadline(&["sleep", "30"], secs(0.4), None).unwrap();
        let elapsed = t.elapsed();
        assert!(out.timed_out, "must be reported as a deadline kill");
        assert_eq!(out.code, None);
        assert_eq!(out.signal, Some(libc::SIGTERM));
        assert!(
            elapsed < secs(3.0),
            "SIGTERM must land promptly, took {elapsed:?}"
        );
    }

    #[test]
    fn the_whole_process_group_dies_not_just_the_child() {
        // `timeout` forks its own child; killing only the direct child would leave
        // `sleep 30` running. After the group kill nothing may answer kill(-pgid, 0).
        let out = run_with_deadline(&["timeout", "30", "sleep", "30"], secs(0.4), None).unwrap();
        assert!(out.timed_out);
        std::thread::sleep(secs(0.3));
        let alive = unsafe { libc::kill(-(out.pid as i32), 0) };
        let err = std::io::Error::last_os_error().raw_os_error();
        assert_eq!(
            (alive, err),
            (-1, Some(libc::ESRCH)),
            "process group {} still has members",
            out.pid
        );
    }

    #[test]
    fn a_cancel_token_stops_a_long_child() {
        let cancel = CancelToken::new();
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(secs(0.3));
            c2.cancel();
        });
        let t = Instant::now();
        let err = run_with_deadline(&["sleep", "30"], secs(60.0), Some(&cancel)).unwrap_err();
        assert!(err.is_interrupted());
        assert_eq!(err.exit_code(), 130);
        assert!(
            t.elapsed() < secs(3.0),
            "cancel must not wait for the deadline"
        );
    }

    #[test]
    fn a_missing_program_is_a_backend_error_not_a_panic() {
        let err = run_with_deadline(&["fproton-no-such-binary"], secs(5.0), None).unwrap_err();
        assert_eq!(err.exit_code(), 4);
        assert!(err.hint().starts_with("omarchy pkg add"));
    }

    #[test]
    fn which_finds_a_real_program_and_misses_a_fake_one() {
        assert!(which("sh").is_some());
        assert!(which("fproton-no-such-binary").is_none());
    }
}
