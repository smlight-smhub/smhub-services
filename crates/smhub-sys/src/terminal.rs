//! The web terminal: a login shell for the device user on a pseudo-terminal.
//!
//! The shell is started as `setsid -c su -l smlight` with the terminal's slave side as its
//! standard streams, so it gets a controlling terminal without any `unsafe` code in the daemon.
//! Closing the master side hangs the session up.

use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::process::Stdio;

use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::pty::{Winsize, openpty};
use smhub_model::{Error, Result};
use tokio::io::unix::AsyncFd;
use tokio::process::Child;

use crate::{DEVICE_USER, Sys};

/// A running terminal session.
pub struct Terminal {
    master: AsyncFd<OwnedFd>,
    slave_path: String,
    child: Child,
}

fn io_err(e: nix::Error) -> std::io::Error {
    std::io::Error::from_raw_os_error(e as i32)
}

/// A pseudo-terminal: the master side, non-blocking for the daemon, and the slave side for a
/// child's standard streams.
pub(crate) struct Pty {
    pub master: AsyncFd<OwnedFd>,
    pub slave: OwnedFd,
    pub slave_path: String,
}

impl Pty {
    pub fn open(ws: Option<&Winsize>) -> Result<Self> {
        let pty = openpty(ws, None).map_err(|e| Error::internal(format!("openpty: {e}")))?;
        let slave_path = std::fs::read_link(format!("/proc/self/fd/{}", pty.slave.as_raw_fd()))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let flags = fcntl(pty.master.as_fd(), FcntlArg::F_GETFL).map_err(|e| Error::internal(e.to_string()))?;
        fcntl(pty.master.as_fd(), FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK))
            .map_err(|e| Error::internal(e.to_string()))?;
        #[allow(deprecated)]
        let master = AsyncFd::new(pty.master).map_err(|e| Error::internal(e.to_string()))?;
        Ok(Self { master, slave: pty.slave, slave_path })
    }

    /// The slave side once more, for one of a child's streams.
    pub fn stdio(&self) -> Result<Stdio> {
        self.slave.try_clone().map(Stdio::from).map_err(|e| Error::internal(e.to_string()))
    }
}

/// Read from a pseudo-terminal's master side. `Ok(0)` means the other side has closed.
pub(crate) async fn read_pty(master: &AsyncFd<OwnedFd>, buf: &mut [u8]) -> std::io::Result<usize> {
    loop {
        let mut guard = master.readable().await?;
        match guard.try_io(|fd| nix::unistd::read(fd.get_ref(), buf).map_err(io_err)) {
            Ok(Ok(n)) => return Ok(n),
            Ok(Err(e)) if e.raw_os_error() == Some(nix::libc::EIO) => return Ok(0),
            Ok(Err(e)) => return Err(e),
            Err(_would_block) => {}
        }
    }
}

impl Sys {
    /// Start a login shell for the device user.
    pub fn open_terminal(&self, cols: u16, rows: u16) -> Result<Terminal> {
        let ws = Winsize { ws_row: rows.max(2), ws_col: cols.max(10), ws_xpixel: 0, ws_ypixel: 0 };
        let pty = Pty::open(Some(&ws))?;
        let mut c = self.command("setsid");
        c.args(["-c", "su", "-l", DEVICE_USER])
            .env("TERM", "xterm-256color")
            .stdin(pty.stdio()?)
            .stdout(pty.stdio()?)
            .stderr(pty.stdio()?)
            .kill_on_drop(true);
        let child = c.spawn().map_err(|e| Error::Unavailable(format!("cannot start a shell: {e}")))?;
        let Pty { master, slave, slave_path } = pty;
        drop(slave);
        Ok(Terminal { master, slave_path, child })
    }
}

impl Terminal {
    /// Read output from the shell. `Ok(0)` means it has ended.
    pub async fn read(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        read_pty(&self.master, buf).await
    }

    /// Send input to the shell.
    pub async fn write(&self, mut data: &[u8]) -> std::io::Result<()> {
        while !data.is_empty() {
            let mut guard = self.master.writable().await?;
            match guard.try_io(|fd| nix::unistd::write(fd.get_ref(), data).map_err(io_err)) {
                Ok(Ok(n)) => data = &data[n..],
                Ok(Err(e)) => return Err(e),
                Err(_would_block) => {}
            }
        }
        Ok(())
    }

    /// Change the terminal size (the shell receives SIGWINCH).
    pub async fn resize(&self, sys: &Sys, cols: u16, rows: u16) {
        if self.slave_path.is_empty() {
            return;
        }
        let (r, c) = (rows.clamp(2, 500).to_string(), cols.clamp(10, 1000).to_string());
        let _ = sys
            .exec("stty", &["-F", &self.slave_path, "rows", &r, "cols", &c], std::time::Duration::from_secs(3))
            .await;
    }

    /// End the session.
    pub async fn close(mut self) {
        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), self.child.wait()).await;
    }
}
