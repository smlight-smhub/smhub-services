//! GPIO lines through the image's libgpiod tools (`gpioget`, `gpioset`, `gpiomon`, `gpioinfo`),
//! by name or by chip and offset. Reading or setting a line holds it only for that moment
//! (decision D25); watching holds the lines until the watch ends. Also the SG2000's pad
//! function registers (`devmem`) and an I2C bus scan (`i2cdetect`).

use std::process::Stdio;
use std::time::Duration;

use smhub_model::{Error, Result};
use tokio::io::AsyncReadExt;
use tokio::process::Child;
use tokio::sync::mpsc;

use crate::Sys;

/// Where the kernel lists GPIO chips, each a link into its controller's device directory.
pub const GPIO_DEVICES: &str = "/sys/bus/gpio/devices";
/// The same for I2C buses (`i2c-1`) and the chips on them (`1-0050`).
pub const I2C_DEVICES: &str = "/sys/bus/i2c/devices";
/// Changes shorter than this are ignored on watched lines (the kernel's debounce).
pub const DEBOUNCE: &str = "20ms";
/// Changes queued from one watch before the reader waits.
pub const WATCH_QUEUE: usize = 64;
/// The longest line kept from `gpiomon` (a change takes about 6 bytes).
const MON_LINE_MAX: usize = 64;

fn valid_line(name: &str) -> bool {
    !name.is_empty() && name.len() <= 32 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn valid_chip(chip: &str) -> bool {
    chip.strip_prefix("gpiochip").is_some_and(|n| !n.is_empty() && n.len() <= 3 && n.bytes().all(|b| b.is_ascii_digit()))
}

/// One line of `gpiomon --format "%o %e"`: the offset, and whether it went high (event 1 is a
/// rising edge, 2 a falling one).
pub fn parse_mon(line: &str) -> Option<(u32, bool)> {
    let mut it = line.split_whitespace();
    let offset = it.next()?.parse().ok()?;
    let high = match it.next()? {
        "1" => true,
        "2" => false,
        _ => return None,
    };
    it.next().is_none().then_some((offset, high))
}

/// The addresses in `i2cdetect`'s table: a row label (`30:`), then sixteen cells of three
/// characters each, `--` for no answer, the address in hex, `UU` for one a kernel driver uses.
pub fn parse_i2cdetect(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((label, cells)) = line.split_once(':') else { continue };
        let Ok(base) = u8::from_str_radix(label.trim(), 16) else { continue };
        if base % 16 != 0 {
            continue;
        }
        for (i, cell) in cells.as_bytes().chunks(3).take(16).enumerate() {
            let cell = String::from_utf8_lossy(cell);
            let cell = cell.trim();
            let addr = base.saturating_add(u8::try_from(i).unwrap_or(0));
            let present = cell == "UU" || u8::from_str_radix(cell, 16).is_ok_and(|a| a == addr);
            if present && (0x08..=0x77).contains(&addr) {
                out.push(addr);
            }
        }
    }
    out
}

impl Sys {
    /// Read a line by name. `active-low` lines are reported as libgpiod reports them.
    /// `--as-is` keeps the line's direction: a plain `gpioget` makes it an input, which
    /// turns off whatever an output line was driving (an LED, the modem's power).
    pub async fn gpio_get(&self, line: &str) -> Result<bool> {
        if !valid_line(line) {
            return Err(Error::invalid("line", "not a line name"));
        }
        let out = self.exec("gpioget", &["--as-is", "--numeric", line], Duration::from_secs(5)).await?;
        if !out.ok() {
            return Err(Error::Unavailable(format!("gpioget {line}: {}", out.error_text())));
        }
        Ok(out.stdout.trim() == "1")
    }

    /// Whether a line with this name exists on any chip.
    pub async fn gpio_exists(&self, line: &str) -> bool {
        valid_line(line)
            && self.exec("gpioinfo", &[line], Duration::from_secs(5)).await.map(|o| o.ok()).unwrap_or(false)
    }

    /// Drive a line to a value for a time, then release it (`gpioset -t`).
    pub async fn gpio_pulse(&self, line: &str, value: bool, hold_ms: u64) -> Result<()> {
        if !valid_line(line) {
            return Err(Error::invalid("line", "not a line name"));
        }
        let assign = format!("{line}={}", u8::from(value));
        let hold = format!("{hold_ms}ms,0");
        let out = self.exec("gpioset", &["-t", &hold, &assign], Duration::from_secs(10 + hold_ms / 1000)).await?;
        if out.ok() { Ok(()) } else { Err(Error::Unavailable(format!("gpioset {line}: {}", out.error_text()))) }
    }

    /// The chip of a GPIO controller, by the controller's device name (`3021000.gpio` gives
    /// `gpiochip2`): the chip numbers follow the probe order, the device names do not change.
    pub fn gpio_chip(&self, controller: &str) -> Option<String> {
        let dir = std::fs::read_dir(self.p(GPIO_DEVICES)).ok()?;
        let mut names: Vec<String> = dir.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| valid_chip(n)).collect();
        names.sort();
        names.into_iter().find(|n| {
            std::fs::read_link(self.p(&format!("{GPIO_DEVICES}/{n}"))).is_ok_and(|t| t.components().any(|c| c.as_os_str() == controller))
        })
    }

    /// Read lines of one chip. `as_is` keeps their direction, so an output is read without
    /// being turned into an input; otherwise they become inputs.
    pub async fn gpio_read(&self, chip: &str, offsets: &[u32], as_is: bool) -> Result<Vec<bool>> {
        if !valid_chip(chip) || offsets.is_empty() {
            return Err(Error::invalid("line", "not a chip and offsets"));
        }
        let offs: Vec<String> = offsets.iter().map(u32::to_string).collect();
        let mut args = vec!["--numeric", "-c", chip];
        if as_is {
            args.insert(0, "--as-is");
        }
        args.extend(offs.iter().map(String::as_str));
        let out = self.exec("gpioget", &args, Duration::from_secs(5)).await?;
        if !out.ok() {
            return Err(Error::Unavailable(format!("gpioget {chip}: {}", out.error_text())));
        }
        let values: Vec<bool> = out.stdout.split_whitespace().map(|v| v == "1").collect();
        if values.len() == offsets.len() { Ok(values) } else { Err(Error::Unavailable(format!("gpioget {chip}: unexpected answer"))) }
    }

    /// Whether a line is an output now.
    pub async fn gpio_is_output(&self, chip: &str, offset: u32) -> Result<bool> {
        if !valid_chip(chip) {
            return Err(Error::invalid("line", "not a chip"));
        }
        let out = self.exec("gpioinfo", &["-c", chip, &offset.to_string()], Duration::from_secs(5)).await?;
        if !out.ok() {
            return Err(Error::Unavailable(format!("gpioinfo {chip}: {}", out.error_text())));
        }
        Ok(out.stdout.split_whitespace().any(|w| w == "output"))
    }

    /// Drive lines of one chip and let go of them. The SG2000's controller keeps driving a line
    /// after it is let go (checked on the dev hub), so nothing stays running for an output.
    pub async fn gpio_set(&self, chip: &str, values: &[(u32, bool)]) -> Result<()> {
        if !valid_chip(chip) || values.is_empty() {
            return Err(Error::invalid("line", "not a chip and values"));
        }
        let assign: Vec<String> = values.iter().map(|(o, v)| format!("{o}={}", u8::from(*v))).collect();
        let mut args = vec!["-t", "0", "-c", chip];
        args.extend(assign.iter().map(String::as_str));
        let out = self.exec("gpioset", &args, Duration::from_secs(5)).await?;
        if out.ok() { Ok(()) } else { Err(Error::Unavailable(format!("gpioset {chip}: {}", out.error_text()))) }
    }

    /// Watch lines of one chip (`gpiomon`, both edges, debounced by the kernel). The child holds
    /// the lines until it ends; dropping it ends it. Each change arrives as the offset and
    /// whether the line went high; the queue closes when `gpiomon` ends.
    pub fn gpio_watch(&self, chip: &str, offsets: &[u32]) -> Result<(Child, mpsc::Receiver<(u32, bool)>)> {
        if !valid_chip(chip) || offsets.is_empty() {
            return Err(Error::invalid("line", "not a chip and offsets"));
        }
        let mut c = self.command("gpiomon");
        c.args(["-c", chip, "-e", "both", "-p", DEBOUNCE, "--format", "%o %e"]).args(offsets.iter().map(u32::to_string));
        c.stdout(Stdio::piped()).stderr(Stdio::null());
        let mut child = c.spawn().map_err(|e| Error::Unavailable(format!("gpiomon: {e}")))?;
        let mut out = child.stdout.take().ok_or_else(|| Error::internal("gpiomon has no output"))?;
        let (tx, rx) = mpsc::channel(WATCH_QUEUE);
        tokio::spawn(async move {
            let mut buf = [0u8; 256];
            let mut line = Vec::with_capacity(MON_LINE_MAX);
            loop {
                let n = match out.read(&mut buf).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => n,
                };
                for &b in buf.get(..n).unwrap_or_default() {
                    if b != b'\n' {
                        if line.len() < MON_LINE_MAX {
                            line.push(b);
                        }
                        continue;
                    }
                    if let Some(ev) = parse_mon(&String::from_utf8_lossy(&line)) {
                        if tx.send(ev).await.is_err() {
                            return;
                        }
                    }
                    line.clear();
                }
            }
        });
        Ok((child, rx))
    }

    /// A 32-bit register of the SoC (`devmem`), for the pad function registers of the pin
    /// controller, which the kernel offers no file for.
    pub async fn reg_read(&self, addr: u32) -> Result<u32> {
        let out = self.exec("devmem", &[&format!("{addr:#010x}"), "32"], Duration::from_secs(5)).await?;
        if !out.ok() {
            return Err(Error::Unavailable(format!("devmem: {}", out.error_text())));
        }
        let t = out.stdout.trim();
        u32::from_str_radix(t.trim_start_matches("0x").trim_start_matches("0X"), 16).map_err(|_| Error::Unavailable(format!("devmem: unexpected answer {t}")))
    }

    pub async fn reg_write(&self, addr: u32, value: u32) -> Result<()> {
        let out = self.exec("devmem", &[&format!("{addr:#010x}"), "32", &value.to_string()], Duration::from_secs(5)).await?;
        if out.ok() { Ok(()) } else { Err(Error::Unavailable(format!("devmem: {}", out.error_text()))) }
    }

    /// The number of an I2C bus Linux has, by its controller's device name (`4040000.i2c`).
    /// `None` while the bus is not Linux's (ESPHome's User I2C has I2C4; I2C2 is on only with the
    /// UPS overlay). The numbers follow the device tree's aliases, which not every bus has.
    pub fn i2c_bus(&self, controller: &str) -> Option<u32> {
        let dir = std::fs::read_dir(self.p(I2C_DEVICES)).ok()?;
        dir.flatten().find_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let n: u32 = name.strip_prefix("i2c-")?.parse().ok()?;
            let target = std::fs::read_link(e.path()).ok()?;
            (target.components().any(|c| c.as_os_str() == controller) && self.p(&format!("/dev/i2c-{n}")).exists()).then_some(n)
        })
    }

    /// The addresses that answer on an I2C bus, 0x08 to 0x77 (`i2cdetect`'s usual way of
    /// asking: a read for the ranges where a write could change a chip's state).
    pub async fn i2c_scan(&self, bus: u32) -> Result<Vec<u8>> {
        let out = self.exec("i2cdetect", &["-y", &bus.to_string()], Duration::from_secs(10)).await?;
        if out.ok() { Ok(parse_i2cdetect(&out.stdout)) } else { Err(Error::Unavailable(format!("i2cdetect: {}", out.error_text()))) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_lines() {
        assert_eq!(parse_mon("16 1"), Some((16, true)));
        assert_eq!(parse_mon("18 2"), Some((18, false)));
        assert_eq!(parse_mon("18 3"), None);
        assert_eq!(parse_mon("x 1"), None);
        assert_eq!(parse_mon("1 1 1"), None);
    }

    #[test]
    fn i2c_table() {
        let t = "     0  1  2  3  4  5  6  7  8  9  a  b  c  d  e  f\n\
                 00:                         -- -- -- -- -- -- -- -- \n\
                 10: -- -- -- -- -- -- -- -- -- -- -- -- -- -- -- -- \n\
                 30: -- -- -- -- -- -- -- -- -- -- -- -- 3c -- -- -- \n\
                 40: -- -- -- -- 44 -- -- -- -- -- -- -- -- -- -- -- \n\
                 70: -- -- -- -- -- -- UU 77                         \n";
        assert_eq!(parse_i2cdetect(t), vec![0x3c, 0x44, 0x76, 0x77]);
        assert!(parse_i2cdetect("Error: Could not open file").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn chips_by_controller() {
        let root = std::env::temp_dir().join(format!("smhub-gpiochips-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("sys/bus/gpio/devices");
        std::fs::create_dir_all(&dir).unwrap_or_default();
        for (chip, ctl) in [("gpiochip1", "3020000.gpio"), ("gpiochip2", "3021000.gpio")] {
            let _ = std::os::unix::fs::symlink(format!("../../../devices/platform/soc/{ctl}/{chip}"), dir.join(chip));
        }
        let sys = Sys::new(&root, None);
        assert_eq!(sys.gpio_chip("3021000.gpio").as_deref(), Some("gpiochip2"));
        assert_eq!(sys.gpio_chip("3020000.gpio").as_deref(), Some("gpiochip1"));
        assert_eq!(sys.gpio_chip("3022000.gpio"), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn i2c_buses_by_controller() {
        let root = std::env::temp_dir().join(format!("smhub-i2cbus-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("sys/bus/i2c/devices");
        std::fs::create_dir_all(&dir).unwrap_or_default();
        std::fs::create_dir_all(root.join("dev")).unwrap_or_default();
        // I2C1 aliased, I2C2 numbered after the highest alias, a chip on I2C1
        for (name, target) in [("i2c-1", "4010000.i2c/i2c-1"), ("i2c-5", "4020000.i2c/i2c-5"), ("1-0050", "4010000.i2c/i2c-1/1-0050")] {
            let _ = std::os::unix::fs::symlink(format!("../../../devices/platform/soc/{target}"), dir.join(name));
        }
        for n in [1, 5] {
            std::fs::write(root.join(format!("dev/i2c-{n}")), "").unwrap_or_default();
        }
        let sys = Sys::new(&root, None);
        assert_eq!(sys.i2c_bus("4010000.i2c"), Some(1));
        assert_eq!(sys.i2c_bus("4020000.i2c"), Some(5));
        assert_eq!(sys.i2c_bus("4040000.i2c"), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A stand-in for `gpiomon` that reports two changes and ends.
    #[cfg(unix)]
    #[tokio::test]
    async fn watched_changes_arrive() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("smhub-gpiomon-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap_or_default();
        let p = bin.join("gpiomon");
        std::fs::write(&p, "#!/bin/sh\necho '16 1'\necho 'noise'\necho '13 2'\n").unwrap_or_default();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap_or_default();
        let sys = Sys::new(&root, Some(&bin));
        let got = sys.gpio_watch("gpiochip2", &[13, 16]);
        assert!(got.is_ok(), "the watch did not start");
        if let Ok((_child, mut rx)) = got {
            let mut seen = Vec::new();
            while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(3), rx.recv()).await {
                seen.push(ev);
            }
            assert_eq!(seen, vec![(16, true), (13, false)]);
        }
        assert!(sys.gpio_watch("chip; rm", &[1]).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
