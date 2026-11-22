//! The OS layer (decision D23): the only crate that reads or writes OS files, runs commands, or
//! talks to devices. Every absolute path is resolved under a root directory given at start, so
//! the daemon can run against a prepared directory in tests.

pub mod appconfig;
pub mod backups;
pub mod cmd;
pub mod esphome;
pub mod files;
pub mod flatconf;
pub mod fsx;
pub mod gpio;
pub mod logs;
pub mod metrics;
pub mod modem;
pub mod mqtt;
pub mod network;
pub mod packages;
pub mod periph;
pub mod radios;
pub mod rtos;
pub mod ser2net;
pub mod services;
pub mod ssh;
pub mod stunnel;
pub mod system;
pub mod tailscale;
pub mod terminal;
pub mod tz;
pub mod updates;
pub mod usb;
pub mod watch;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use smhub_model::hardware::{self, ModelInfo};

pub use system::Eeprom;

/// The device user that owns files in the home folder and runs the apps.
pub const DEVICE_USER: &str = "smlight";

/// Handle to the operating system. Shared behind an `Arc`.
pub struct Sys {
    root: PathBuf,
    path_env: String,
    eeprom: Option<Eeprom>,
    model: ModelInfo,
    metrics_prev: Mutex<Option<metrics::CpuSample>>,
    services_cache: Mutex<Option<(std::time::Instant, services::Snapshot)>>,
    updates_cache: Mutex<Option<updates::Cached>>,
}

impl Sys {
    /// `root` is `/` in production. `bin_dir`, when given, is searched first for commands (tests
    /// put stand-ins for `rc-service` and `opkg` there).
    pub fn new(root: &Path, bin_dir: Option<&Path>) -> Self {
        let base = "/opt/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
        let path_env = match bin_dir {
            Some(d) => format!("{}:{base}", d.display()),
            None => base.to_owned(),
        };
        let mut sys = Self {
            root: root.to_path_buf(),
            path_env,
            eeprom: None,
            model: hardware::lookup(None),
            metrics_prev: Mutex::new(None),
            services_cache: Mutex::new(None),
            updates_cache: Mutex::new(None),
        };
        sys.eeprom = system::read_eeprom(&sys);
        sys.model = hardware::lookup(sys.eeprom.as_ref().map(|e| e.model.as_str()));
        sys
    }

    /// An absolute OS path under the configured root.
    pub fn p(&self, abs: &str) -> PathBuf {
        if self.root == Path::new("/") {
            PathBuf::from(abs)
        } else {
            self.root.join(abs.trim_start_matches('/'))
        }
    }

    /// The configured root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether the daemon runs on a real hub (root `/`).
    pub fn is_live(&self) -> bool {
        self.root == Path::new("/")
    }

    pub fn eeprom(&self) -> Option<&Eeprom> {
        self.eeprom.as_ref()
    }

    pub fn model(&self) -> &ModelInfo {
        &self.model
    }
}

/// Lock a mutex, recovering from poisoning (a panic elsewhere must not stop the daemon).
pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}
