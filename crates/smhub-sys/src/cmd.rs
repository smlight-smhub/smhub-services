//! Running commands. Every command has a time limit; output is captured with a size cap.

use std::process::Stdio;
use std::time::Duration;

use smhub_model::{Error, Result};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::Sys;

/// Most output kept from a command.
const MAX_OUTPUT: usize = 4 * 1024 * 1024;

/// The usual time limit for a quick command.
pub const QUICK: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.code == 0
    }

    /// stderr, or stdout when stderr is empty, trimmed: the most useful error text.
    pub fn error_text(&self) -> String {
        let e = self.stderr.trim();
        if e.is_empty() { self.stdout.trim().to_owned() } else { e.to_owned() }
    }
}

async fn read_capped<R: tokio::io::AsyncRead + Unpin>(r: Option<R>) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(mut r) = r {
        let mut buf = [0u8; 8192];
        loop {
            match r.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if out.len() < MAX_OUTPUT {
                        let take = n.min(MAX_OUTPUT - out.len());
                        out.extend_from_slice(&buf[..take]);
                    }
                }
            }
        }
    }
    out
}

impl Sys {
    /// A command with the daemon's PATH and stdin closed.
    pub fn command(&self, prog: &str) -> Command {
        let mut c = Command::new(prog);
        c.env("PATH", &self.path_env).stdin(Stdio::null()).kill_on_drop(true);
        c
    }

    /// Run a command and capture its output. Errors only when it cannot be started or exceeds
    /// the time limit; a non-zero exit status is returned in `Output::code`.
    pub async fn exec(&self, prog: &str, args: &[&str], limit: Duration) -> Result<Output> {
        let mut c = self.command(prog);
        c.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
        self.exec_cmd(c, prog, limit).await
    }

    /// Run a prepared command and capture its output.
    pub async fn exec_cmd(&self, mut c: Command, name: &str, limit: Duration) -> Result<Output> {
        let mut child = c.spawn().map_err(|e| Error::Unavailable(format!("cannot run {name}: {e}")))?;
        let so = child.stdout.take();
        let se = child.stderr.take();
        let work = async {
            let (o, e, status) = tokio::join!(read_capped(so), read_capped(se), child.wait());
            (o, e, status)
        };
        match tokio::time::timeout(limit, work).await {
            Ok((o, e, status)) => {
                let status = status.map_err(|e| Error::internal(format!("{name}: {e}")))?;
                Ok(Output {
                    code: status.code().unwrap_or(-1),
                    stdout: String::from_utf8_lossy(&o).into_owned(),
                    stderr: String::from_utf8_lossy(&e).into_owned(),
                })
            }
            Err(_) => Err(Error::Unavailable(format!("{name} did not finish in {} s", limit.as_secs()))),
        }
    }

    /// Run a command and return its stdout; a non-zero exit is an error with its message.
    pub async fn run(&self, prog: &str, args: &[&str], limit: Duration) -> Result<String> {
        let out = self.exec(prog, args, limit).await?;
        if out.ok() {
            Ok(out.stdout)
        } else {
            Err(Error::internal(format!("{prog} failed ({}): {}", out.code, out.error_text())))
        }
    }
}
