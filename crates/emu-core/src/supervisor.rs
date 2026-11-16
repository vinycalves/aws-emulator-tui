//! Starts, stops and tails a local `aws-emulator` process.
//!
//! The TUI can attach to an emulator someone else started (the common case), or
//! own the process itself. When it owns the process, stdout and stderr are
//! captured so the Emulator tab can show the JSON log lines.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

const MAX_LOG_LINES: usize = 4_000;

/// Owns an emulator child process, if the user asked the TUI to start one.
pub struct Supervisor {
    binary: Option<PathBuf>,
    port: u16,
    child: Option<Child>,
    logs: Arc<Mutex<VecDeque<String>>>,
    pub last_error: Option<String>,
}

impl Supervisor {
    pub fn new(binary: Option<PathBuf>, port: u16) -> Self {
        Self {
            binary,
            port,
            child: None,
            logs: Arc::new(Mutex::new(VecDeque::new())),
            last_error: None,
        }
    }

    pub fn binary(&self) -> Option<&PathBuf> {
        self.binary.as_ref()
    }

    pub fn can_start(&self) -> bool {
        self.binary.is_some()
    }

    /// Whether this supervisor has a child process (which may have exited).
    pub fn has_child(&self) -> bool {
        self.child.is_some()
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().and_then(Child::id)
    }

    pub fn is_alive(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(Some(status)) => {
                    self.push_log(format!("[process exited: {status}]"));
                    self.child = None;
                    false
                }
                Ok(None) => true,
                Err(_) => false,
            },
            None => false,
        }
    }

    /// Start the emulator. Telemetry is always disabled for processes the TUI
    /// starts; the panel says so.
    pub async fn start(&mut self) -> Result<u32, String> {
        if self.is_alive() {
            return Ok(self.pid().unwrap_or_default());
        }
        let binary = self
            .binary
            .clone()
            .ok_or_else(|| "aws-emulator binary not found (set AWS_EMULATOR_BIN)".to_string())?;

        self.push_log(format!(
            "[starting {} -p {} -t off]",
            binary.display(),
            self.port
        ));

        let mut command = Command::new(&binary);
        command
            .arg("-p")
            .arg(self.port.to_string())
            .arg("-t")
            .arg("off")
            .env("EMULATOR_TELEMETRY", "off")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = command
            .spawn()
            .map_err(|error| format!("cannot start {}: {error}", binary.display()))?;

        if let Some(stdout) = child.stdout.take() {
            spawn_reader(stdout, self.logs.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_reader(stderr, self.logs.clone());
        }

        let pid = child.id();
        self.child = Some(child);
        self.last_error = None;
        Ok(pid.unwrap_or_default())
    }

    /// Stop the child with `SIGTERM`, waiting up to five seconds before killing
    /// it. The emulator deletes its generated files on a clean shutdown.
    pub async fn stop(&mut self) -> Result<(), String> {
        let Some(mut child) = self.child.take() else {
            return Ok(());
        };
        let pid = child.id();
        self.push_log("[stopping emulator]".to_string());

        #[cfg(unix)]
        if let Some(pid) = pid {
            use nix::sys::signal::{kill, Signal};
            use nix::unistd::Pid;
            let _ = kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
        }
        #[cfg(not(unix))]
        {
            let _ = child.start_kill();
        }

        match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
            Ok(Ok(status)) => {
                self.push_log(format!("[process exited: {status}]"));
                Ok(())
            }
            Ok(Err(error)) => Err(format!("cannot wait for emulator: {error}")),
            Err(_) => {
                let _ = child.kill().await;
                self.push_log("[process killed after timeout]".to_string());
                Err("emulator did not stop within 5 seconds; killed it".to_string())
            }
        }
    }

    /// Most recent log lines, oldest first.
    pub fn logs(&self, limit: usize) -> Vec<String> {
        let logs = self.logs.lock().expect("log mutex poisoned");
        let skip = logs.len().saturating_sub(limit);
        logs.iter().skip(skip).cloned().collect()
    }

    pub fn clear_logs(&self) {
        self.logs.lock().expect("log mutex poisoned").clear();
    }

    fn push_log(&self, line: String) {
        let mut logs = self.logs.lock().expect("log mutex poisoned");
        if logs.len() >= MAX_LOG_LINES {
            logs.pop_front();
        }
        logs.push_back(line);
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }
}

fn spawn_reader<R>(reader: R, logs: Arc<Mutex<VecDeque<String>>>)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let mut logs = logs.lock().expect("log mutex poisoned");
            if logs.len() >= MAX_LOG_LINES {
                logs.pop_front();
            }
            logs.push_back(line);
        }
    });
}
