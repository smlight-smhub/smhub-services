//! OpenRC services: state, start/stop/restart, autostart (decision D26).
//!
//! State is read from OpenRC's own files under `/run/openrc`, the way `rc-status` (OpenRC 0.63)
//! works it out, without running it: `rc-status -a` takes 0.31 s of the hub's single core (more
//! when the core is busy), this read 25 ms. It is cached for two seconds; the event watcher
//! invalidates the cache when the state folders change. `smhubd --services` prints it, and
//! `tests/hub/services_compare.sh` compares that with `rc-status`.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::Serialize;
use smhub_model::{Error, Result};

use crate::{Sys, lock};

/// How long a status read is reused.
const CACHE: Duration = Duration::from_secs(2);

/// Services the user reaches the hub through. They are not stopped from the API, and a backup
/// keeps them running (the old backend stopped `tailscale` too, which broke a backup download
/// made over Tailscale).
pub const ACCESS_SERVICES: &[&str] = &["networkmanager", "sshd", "nginx", "stunnel", "tailscale", "smhub-services"];

/// A service's state, as the API reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Started,
    Stopped,
    Starting,
    Stopping,
    Crashed,
    Inactive,
    Unknown,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Stopped => "stopped",
            Self::Starting => "starting",
            Self::Stopping => "stopping",
            Self::Crashed => "crashed",
            Self::Inactive => "inactive",
            Self::Unknown => "unknown",
        }
    }
}

/// Service name to state for every service of the hub (callers treat a missing name as stopped).
pub type Snapshot = BTreeMap<String, State>;

/// OpenRC's marker folders under `/run/openrc`, each read once per snapshot. A service is in at
/// most one of the first four; `scheduled` holds folders named after the service that will start
/// others.
const MARK_DIRS: [&str; 8] = ["started", "starting", "stopping", "inactive", "failed", "hotplugged", "wasinactive", "scheduled"];

/// Folder name to the entries in it.
type Marks = BTreeMap<&'static str, std::collections::BTreeSet<String>>;

/// What OpenRC recorded about one daemon of a started service (`/run/openrc/daemons/<svc>/NNN`):
/// the parts `rc_service_daemons_crashed` looks at.
#[derive(Debug, Default, PartialEq)]
struct DaemonRecord {
    exec: Option<String>,
    argv: Vec<String>,
    pidfile: Option<String>,
}

/// Parsed as librc does: `key=value` lines, empty values ignored, every `argv_N` in file order,
/// and nothing after `pidfile=`.
fn parse_daemon_record(text: &str) -> DaemonRecord {
    let mut r = DaemonRecord::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        if value.is_empty() {
            continue;
        }
        match key {
            "exec" => r.exec = Some(value.to_owned()),
            "pidfile" => {
                r.pidfile = Some(value.to_owned());
                break;
            }
            k if k.starts_with("argv_") => r.argv.push(value.to_owned()),
            _ => {}
        }
    }
    r
}

/// The leading integer of a pidfile, as C's `fscanf("%d")` reads it.
fn leading_int(text: &str) -> Option<i64> {
    let t = text.trim_start();
    let digits_from = usize::from(t.starts_with(['-', '+']));
    let end = t[digits_from..].find(|c: char| !c.is_ascii_digit()).map_or(t.len(), |i| i + digits_from);
    if end == digits_from {
        return None;
    }
    t[..end].parse().ok()
}

/// Whether the NUL-separated `cmdline` starts with `argv` (librc's `pid_is_argv`).
fn cmdline_starts_with(cmdline: &[u8], argv: &[String]) -> bool {
    let mut parts = cmdline.split(|b| *b == 0);
    argv.iter().all(|a| parts.next() == Some(a.as_bytes()))
}

/// How a daemon record turned out.
enum Daemon {
    Alive,
    Dead,
    /// Its record or pidfile could not be read: OpenRC does not call that crashed.
    Unreadable,
}

/// The command lines of all processes, read once per snapshot and only when a record has no
/// pidfile. librc reads at most 4095 bytes of each.
struct Procs {
    root: PathBuf,
    lines: Option<Vec<Vec<u8>>>,
}

impl Procs {
    fn any_starts_with(&mut self, argv: &[String]) -> bool {
        let root = &self.root;
        let lines = self.lines.get_or_insert_with(|| {
            let mut out = Vec::new();
            for e in std::fs::read_dir(root.join("proc")).into_iter().flatten().flatten() {
                if !e.file_name().to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
                    continue;
                }
                let mut buf = Vec::new();
                if let Ok(f) = std::fs::File::open(e.path().join("cmdline")) {
                    if f.take(4095).read_to_end(&mut buf).is_ok() {
                        out.push(buf);
                    }
                }
            }
            out
        });
        lines.iter().any(|l| cmdline_starts_with(l, argv))
    }
}

/// The OpenRC service of a package: `system.service.name` from its schema, a fixed mapping, or
/// the package name (old `Packages._resolve_service_name`).
pub fn service_for_package(schema: Option<&serde_json::Value>, package: &str) -> String {
    if let Some(n) = schema
        .and_then(|s| s.pointer("/system/service/name"))
        .and_then(serde_json::Value::as_str)
        .filter(|n| !n.is_empty())
    {
        return n.to_owned();
    }
    match package {
        "smhub-ui" => "nginx".to_owned(),
        _ => package.to_owned(),
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-+".contains(c))
}

impl Sys {
    /// All service states, cached for two seconds. The read does not wait on anything, so it
    /// cannot overlap a change announced in the meantime.
    pub async fn services(&self) -> Snapshot {
        if let Some((at, snap)) = lock(&self.services_cache).as_ref() {
            if at.elapsed() < CACHE {
                return snap.clone();
            }
        }
        let snap = self.read_services();
        *lock(&self.services_cache) = Some((Instant::now(), snap.clone()));
        snap
    }

    /// Forget the cached service states (something changed).
    pub fn services_changed(&self) {
        *lock(&self.services_cache) = None;
    }

    /// Every service with an init script or a state folder entry, and its state now.
    pub fn read_services(&self) -> Snapshot {
        let entries = |dir: &str| -> std::collections::BTreeSet<String> {
            std::fs::read_dir(self.p(dir))
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| valid_name(n) && n != "functions.sh" && !n.starts_with('.'))
                .collect()
        };
        let marks: Marks = MARK_DIRS.iter().map(|d| (*d, entries(&format!("/run/openrc/{d}")))).collect();
        let mut names = entries("/etc/init.d");
        for (dir, set) in &marks {
            if *dir != "scheduled" {
                names.extend(set.iter().cloned());
            }
        }
        let mut procs = Procs { root: self.p("/"), lines: None };
        names
            .into_iter()
            .map(|n| {
                let s = self.service_state(&n, &marks, &mut procs);
                (n, s)
            })
            .collect()
    }

    /// One service's state, as `rc-status` prints it (`print_service`, OpenRC 0.63), mapped to
    /// the API's states. "scheduled" (stopped, waiting for another service) counts as stopped;
    /// "failed" and "unsupervised" (the supervisor is gone) are unknown, as they were when the
    /// state came from `rc-status`.
    fn service_state(&self, name: &str, marks: &Marks, procs: &mut Procs) -> State {
        // librc tests with access(F_OK), which follows the marker's symlink to the init script
        let marked = |dir: &str| marks.get(dir).is_some_and(|s| s.contains(name)) && self.p(&format!("/run/openrc/{dir}/{name}")).exists();
        if marked("stopping") {
            State::Stopping
        } else if marked("starting") {
            State::Starting
        } else if marked("inactive") {
            State::Inactive
        } else if marked("started") {
            if !self.daemons_crashed(name, procs) {
                State::Started
            } else if self.p(&format!("/run/openrc/options/{name}/child_pid")).exists()
                && self.p(&format!("/run/openrc/options/{name}/start_time")).exists()
            {
                State::Unknown
            } else {
                State::Crashed
            }
        } else if marked("scheduled") {
            State::Stopped
        } else if marked("failed") {
            State::Unknown
        } else {
            State::Stopped
        }
    }

    /// librc's `rc_service_daemons_crashed`: a started service whose recorded daemon is gone.
    /// With a pidfile, that pid must exist; without one, a process whose command line starts with
    /// the recorded `argv` (or `exec`) must.
    fn daemons_crashed(&self, name: &str, procs: &mut Procs) -> bool {
        let Ok(rd) = std::fs::read_dir(self.p(&format!("/run/openrc/daemons/{name}"))) else {
            return false;
        };
        let mut files: Vec<_> = rd.flatten().filter(|e| !e.file_name().to_string_lossy().starts_with('.')).map(|e| e.path()).collect();
        files.sort();
        let chroot = std::fs::read_to_string(self.p(&format!("/run/openrc/options/{name}/chroot"))).unwrap_or_default();
        for f in files {
            let Ok(text) = std::fs::read_to_string(&f) else { break };
            match self.daemon(&parse_daemon_record(&text), chroot.trim_end_matches('\n'), procs) {
                Daemon::Alive => {}
                Daemon::Dead => return true,
                Daemon::Unreadable => return false,
            }
        }
        false
    }

    fn daemon(&self, r: &DaemonRecord, chroot: &str, procs: &mut Procs) -> Daemon {
        if let Some(pidfile) = &r.pidfile {
            let text = match std::fs::read_to_string(self.p(&format!("{chroot}{pidfile}"))) {
                Ok(t) => t,
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Daemon::Unreadable,
                Err(_) => return Daemon::Dead,
            };
            return match leading_int(&text) {
                None => Daemon::Dead,
                // librc then matches any process
                Some(0) => Daemon::Alive,
                Some(pid) if self.p(&format!("/proc/{pid}")).exists() => Daemon::Alive,
                Some(_) => Daemon::Dead,
            };
        }
        let argv: Vec<String> = if r.argv.is_empty() { r.exec.iter().cloned().collect() } else { r.argv.clone() };
        if argv.is_empty() || procs.any_starts_with(&argv) { Daemon::Alive } else { Daemon::Dead }
    }

    /// Waits for a service that is starting or stopping to finish doing so, up to `limit`.
    /// Answers its state then.
    async fn settled(&self, name: &str, limit: Duration) -> Option<State> {
        let end = Instant::now() + limit;
        loop {
            self.services_changed();
            let st = self.services().await.get(name).copied();
            if !matches!(st, Some(State::Starting | State::Stopping)) || Instant::now() >= end {
                return st;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    /// Whether an init script exists for the service.
    pub fn service_exists(&self, name: &str) -> bool {
        valid_name(name) && self.p(&format!("/etc/init.d/{name}")).exists()
    }

    /// `rc-service <name> <action>`.
    pub async fn service_action(&self, name: &str, action: &str) -> Result<()> {
        if !valid_name(name) {
            return Err(Error::invalid("name", "not a service name"));
        }
        if !matches!(action, "start" | "stop" | "restart" | "status") {
            return Err(Error::invalid("action", "use start, stop or restart"));
        }
        if !self.service_exists(name) {
            return Err(Error::not_found(format!("service {name}")));
        }
        // OpenRC refuses an action while the service is starting or stopping (its lock is
        // taken): an app that restarts itself, or a slow stop, first finishes
        if action != "status" {
            if let Some(st @ (State::Starting | State::Stopping)) = self.settled(name, Duration::from_secs(30)).await {
                return Err(Error::Busy(format!("{name} is still {}; try again in a moment", st.as_str())));
            }
        }
        let out = self.exec("rc-service", &[name, action], Duration::from_secs(90)).await?;
        self.services_changed();
        if out.ok() {
            return Ok(());
        }
        let text = out.error_text();
        if text.contains("flock") || text.contains("is stopping") || text.contains("is starting") {
            return Err(Error::Busy(format!("{name} is busy starting or stopping; try again in a moment")));
        }
        Err(Error::internal(format!("rc-service {name} {action} failed: {text}")))
    }

    /// Whether a service is in the `default` (or `boot`) runlevel.
    pub fn service_autostart(&self, name: &str) -> bool {
        valid_name(name)
            && (self.p(&format!("/etc/runlevels/default/{name}")).exists()
                || self.p(&format!("/etc/runlevels/boot/{name}")).exists())
    }

    /// `rc-update add|del <name> default`.
    pub async fn set_service_autostart(&self, name: &str, enabled: bool) -> Result<()> {
        if !valid_name(name) {
            return Err(Error::invalid("name", "not a service name"));
        }
        let action = if enabled { "add" } else { "del" };
        let out = self.exec("rc-update", &[action, name, "default"], Duration::from_secs(20)).await?;
        if !out.ok() && enabled {
            return Err(Error::internal(format!("rc-update {action} {name}: {}", out.error_text())));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_records() {
        let r = parse_daemon_record("exec=/a\nargv_0=/a\nargv_1=\nname=x\npidfile=/p\nargv_2=late\n");
        assert_eq!(r, DaemonRecord { exec: Some("/a".into()), argv: vec!["/a".into()], pidfile: Some("/p".into()) });
        assert_eq!(parse_daemon_record("exec=/a\npidfile=\n").pidfile, None);
        assert_eq!(leading_int(" 42\n"), Some(42));
        assert_eq!(leading_int("+7abc"), Some(7));
        assert_eq!(leading_int("-3"), Some(-3));
        assert_eq!(leading_int("x"), None);
        assert_eq!(leading_int(""), None);
        assert!(cmdline_starts_with(b"/usr/sbin/m\0-c\0/etc/m.conf\0", &["/usr/sbin/m".into(), "-c".into()]));
        assert!(!cmdline_starts_with(b"/usr/sbin/m\0", &["/usr/sbin/m".into(), "-c".into()]));
        assert!(!cmdline_starts_with(b"/usr/sbin/mx\0", &["/usr/sbin/m".into()]));
    }

    /// OpenRC state, init scripts and processes laid out as on a hub, one case per service.
    #[test]
    fn states_as_rc_status_reports_them() {
        let root = std::env::temp_dir().join(format!("smhub-services-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let put = |path: &str, text: &str| {
            let f = root.join(path);
            if let Some(d) = f.parent() {
                std::fs::create_dir_all(d).unwrap_or_default();
            }
            std::fs::write(f, text).unwrap_or_default();
        };
        put("proc/100/cmdline", "/usr/sbin/mosquitto\0-c\0/etc/mosquitto/mosquitto.conf\0");
        put("proc/200/cmdline", "/opt/bin/thing\0");
        put("run/alive.pid", "100\n");
        put("run/dead.pid", "999\n");
        put("run/garbage.pid", "x\n");
        put("run/zero.pid", "0\n");
        let started: [(&str, &[&str]); 11] = [
            ("alive_pid", &["exec=/usr/sbin/mosquitto\nargv_0=/usr/sbin/mosquitto\npidfile=/run/alive.pid"]),
            ("dead_pid", &["exec=/usr/sbin/mosquitto\npidfile=/run/dead.pid"]),
            ("missing_pidfile", &["exec=/x\npidfile=/run/nope.pid"]),
            ("garbage_pid", &["pidfile=/run/garbage.pid"]),
            ("zero_pid", &["pidfile=/run/zero.pid"]),
            ("argv_alive", &["exec=/usr/sbin/mosquitto\nargv_0=/usr/sbin/mosquitto\nargv_1=-c\npidfile="]),
            ("argv_dead", &["exec=/usr/sbin/mosquitto\nargv_0=/usr/sbin/mosquitto\nargv_1=-d\npidfile="]),
            ("exec_only", &["exec=/opt/bin/thing\npidfile="]),
            ("unsupervised", &["exec=/opt/bin/sup\nargv_0=/opt/bin/sup\npidfile=/run/dead.pid"]),
            ("second_dead", &["pidfile=/run/alive.pid", "pidfile=/run/dead.pid"]),
            ("nothing_recorded", &[]),
        ];
        for (name, records) in started {
            put(&format!("etc/init.d/{name}"), "#!/sbin/openrc-run\n");
            put(&format!("run/openrc/started/{name}"), "");
            for (i, r) in records.iter().enumerate() {
                put(&format!("run/openrc/daemons/{name}/{:03}", i + 1), r);
            }
        }
        put("run/openrc/options/unsupervised/child_pid", "5");
        put("run/openrc/options/unsupervised/start_time", "1");
        for (name, dir) in [("stopping", "stopping"), ("inactive", "inactive"), ("failed", "failed")] {
            put(&format!("etc/init.d/{name}"), "");
            put(&format!("run/openrc/{dir}/{name}"), "");
        }
        put("etc/init.d/scheduled", "");
        put("run/openrc/scheduled/scheduled/other", "");
        put("etc/init.d/plain", "");
        put("etc/init.d/functions.sh", "");

        let s = Sys::new(&root, None).read_services();
        let want = [
            ("alive_pid", State::Started),
            ("dead_pid", State::Crashed),
            ("missing_pidfile", State::Crashed),
            ("garbage_pid", State::Crashed),
            ("zero_pid", State::Started),
            ("argv_alive", State::Started),
            ("argv_dead", State::Crashed),
            ("exec_only", State::Started),
            ("unsupervised", State::Unknown),
            ("second_dead", State::Crashed),
            ("nothing_recorded", State::Started),
            ("stopping", State::Stopping),
            ("inactive", State::Inactive),
            ("failed", State::Unknown),
            ("scheduled", State::Stopped),
            ("plain", State::Stopped),
        ];
        for (name, state) in want {
            assert_eq!(s.get(name), Some(&state), "{name}");
        }
        assert_eq!(s.len(), want.len(), "functions.sh is not a service");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn service_names() {
        assert_eq!(service_for_package(None, "smhub-ui"), "nginx");
        let schema = serde_json::json!({"system": {"service": {"name": "otbr-agent"}}});
        assert_eq!(service_for_package(Some(&schema), "openthread"), "otbr-agent");
    }
}
