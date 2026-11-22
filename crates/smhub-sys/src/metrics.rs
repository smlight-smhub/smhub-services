//! CPU, memory, swap, load, uptime and storage, read from `/proc` and `statvfs`.

use serde::Serialize;

use crate::{Sys, lock};

/// One reading of `/proc/stat`'s total line.
#[derive(Debug, Clone, Copy)]
pub struct CpuSample {
    busy: u64,
    total: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Usage {
    pub total: u64,
    pub used: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Storage {
    pub id: &'static str,
    pub path: String,
    pub total: u64,
    pub used: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Metrics {
    pub cpu_percent: f64,
    pub load: [f64; 3],
    pub memory: Usage,
    pub swap: Usage,
    pub uptime_s: f64,
    pub storage: Vec<Storage>,
}

fn cpu_sample(text: &str) -> Option<CpuSample> {
    let line = text.lines().find(|l| l.starts_with("cpu "))?;
    let v: Vec<u64> = line.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
    if v.len() < 4 {
        return None;
    }
    let idle = v[3] + v.get(4).copied().unwrap_or(0);
    let total: u64 = v.iter().take(8).sum();
    Some(CpuSample { busy: total.saturating_sub(idle), total })
}

fn meminfo(text: &str) -> (Usage, Usage) {
    let get = |k: &str| -> u64 {
        text.lines()
            .find(|l| l.starts_with(k))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0)
            * 1024
    };
    let total = get("MemTotal:");
    let available = get("MemAvailable:");
    let st = get("SwapTotal:");
    let sf = get("SwapFree:");
    (Usage { total, used: total.saturating_sub(available) }, Usage { total: st, used: st.saturating_sub(sf) })
}

impl Sys {
    /// Whether something is mounted exactly at `mount_point`.
    pub fn is_mounted(&self, mount_point: &str) -> bool {
        std::fs::read_to_string(self.p("/proc/mounts"))
            .map(|t| t.lines().any(|l| l.split_whitespace().nth(1) == Some(mount_point)))
            .unwrap_or(false)
    }

    fn storage(&self, id: &'static str, path: &str) -> Option<Storage> {
        if self.is_live() && !self.is_mounted(path) {
            return None;
        }
        let st = nix::sys::statvfs::statvfs(&self.p(path)).ok()?;
        let frag = st.fragment_size();
        let total = st.blocks() * frag;
        let free = st.blocks_available() * frag;
        Some(Storage { id, path: path.to_owned(), total, used: total.saturating_sub(free) })
    }

    /// Current metrics. CPU use is measured since the previous call (or since boot).
    pub fn metrics(&self) -> Metrics {
        let stat = std::fs::read_to_string(self.p("/proc/stat")).unwrap_or_default();
        let now = cpu_sample(&stat);
        let cpu_percent = {
            let mut prev = lock(&self.metrics_prev);
            let pct = match (*prev, now) {
                (Some(p), Some(n)) if n.total > p.total => {
                    (n.busy.saturating_sub(p.busy)) as f64 * 100.0 / (n.total - p.total) as f64
                }
                (_, Some(n)) if n.total > 0 => n.busy as f64 * 100.0 / n.total as f64,
                _ => 0.0,
            };
            *prev = now;
            (pct * 10.0).round() / 10.0
        };
        let load_text = std::fs::read_to_string(self.p("/proc/loadavg")).unwrap_or_default();
        let lv: Vec<f64> = load_text.split_whitespace().take(3).filter_map(|v| v.parse().ok()).collect();
        let load = [lv.first().copied().unwrap_or(0.0), lv.get(1).copied().unwrap_or(0.0), lv.get(2).copied().unwrap_or(0.0)];
        let (memory, swap) = meminfo(&std::fs::read_to_string(self.p("/proc/meminfo")).unwrap_or_default());
        let storage = [self.storage("user", "/mnt/user"), self.storage("sdcard", "/mnt/sdcard")].into_iter().flatten().collect();
        Metrics { cpu_percent, load, memory, swap, uptime_s: self.uptime(), storage }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proc() {
        let s = cpu_sample("cpu  100 0 100 700 100 0 0 0 0 0\ncpu0 1 2 3 4\n");
        assert_eq!(s.map(|s| (s.busy, s.total)), Some((200, 1000)));
        let (m, sw) = meminfo("MemTotal: 1000 kB\nMemAvailable: 400 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n");
        assert_eq!((m.total, m.used, sw.total), (1_024_000, 614_400, 0));
    }
}
