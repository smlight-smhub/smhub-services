//! The hub's own hardware on the Linux side (docs/peripherals.md): which side owns each device
//! now, the buzzer's PWM output, the ambient ring's SPI bus, the service LEDs through the
//! libgpiod tools, the buttons through the kernel's key events, the infrared receiver and
//! transmitter through `ir-ctl` and `ir-keytable`, and `rtos-notify` for the second core.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use smhub_model::{Error, Result};
use tokio::io::unix::AsyncFd;
use tokio::process::Child;
use tokio::sync::mpsc;

use crate::{Sys, fsx};

pub const PWM_CHIP: &str = "/sys/class/pwm/pwmchip0";
pub const PWM_CHANNEL: u32 = 2;
pub const SPI_DEV: &str = "/dev/spidev0.0";
pub const RING_LEDS: usize = 12;
/// The ring's data rate: one byte per LED bit (`chase-blue.py` and the C service use the same).
const SPI_HZ: u32 = 6_400_000;
pub const LIRC_RX: &str = "/dev/lirc0";
pub const LIRC_TX: &str = "/dev/lirc1";
pub const RC_PROTOCOLS: &str = "/sys/class/rc/rc0/protocols";
/// The decoders the kernel offers on the receiver, as written to `protocols`. A name without
/// `+` replaces the whole set, so only the first may go without: `lirc` (raw only), then each
/// decoder added. Written without the `+`, only the last one (`rc-mm`) would be on.
pub const IR_DECODERS: &str = "lirc +nec +rc-5 +rc-6 +jvc +sony +sanyo +rc-mm";
pub const RTOS_NOTIFY: &str = "/opt/bin/rtos-notify";
pub const REMOTEPROC_STATE: &str = "/sys/class/remoteproc/remoteproc0/state";
/// Raw infrared sent or received: most pulses and spaces, and the longest code.
pub const IR_MAX_EDGES: usize = 1024;
/// The longest line kept from `ir-keytable` (a decoded code takes about 70 bytes).
const IR_LINE_MAX: usize = 256;
pub const INPUT_CLASS: &str = "/sys/class/input";
/// The buttons' key codes, from the device tree's `gpio-keys`: BTN1, the first from the left
/// ("User Button", line `btn_2`, full-size SMHUB only), is `KEY_SETUP`; BTN2 ("Factory Reset",
/// line `btn_1`, every model) is `KEY_RESTART`.
pub const KEY_BTN1: u16 = 141;
pub const KEY_BTN2: u16 = 408;
/// One `struct input_event`: a `timeval` of two longs, then type, code (16 bits each) and value
/// (32 bits).
const INPUT_EVENT: usize = 2 * std::mem::size_of::<usize>() + 8;
const EV_KEY: u16 = 1;

/// A key event from one `struct input_event`: the code and whether it went down. Repeats
/// (value 2) and other event types are `None`.
pub fn key_event(ev: &[u8]) -> Option<(u16, bool)> {
    let at = 2 * std::mem::size_of::<usize>();
    let u16_at = |i: usize| ev.get(i..i + 2).and_then(|b| b.try_into().ok()).map(u16::from_ne_bytes);
    let kind = u16_at(at)?;
    let code = u16_at(at + 2)?;
    let value = ev.get(at + 4..at + 8).and_then(|b| b.try_into().ok()).map(i32::from_ne_bytes)?;
    match (kind, value) {
        (EV_KEY, 1) => Some((code, true)),
        (EV_KEY, 0) => Some((code, false)),
        _ => None,
    }
}

/// The `keys` file of `gpio-keys`: a list such as `141,408` or `1-3,408`.
pub fn parse_keys(s: &str) -> Vec<u16> {
    let mut out = Vec::new();
    for part in s.trim().split(',').filter(|p| !p.is_empty()) {
        let (a, b) = part.split_once('-').unwrap_or((part, part));
        if let (Ok(a), Ok(b)) = (a.trim().parse::<u16>(), b.trim().parse::<u16>()) {
            // a key list is short; a nonsense range is not expanded
            if b >= a && b - a < 64 {
                out.extend(a..=b);
            }
        }
    }
    out
}

/// Read whole input events (the kernel hands out only whole ones).
async fn read_events(fd: &AsyncFd<std::fs::File>, buf: &mut [u8]) -> std::io::Result<usize> {
    use std::io::Read;
    loop {
        let mut guard = fd.readable().await?;
        match guard.try_io(|f| {
            let mut file: &std::fs::File = f.get_ref();
            file.read(buf)
        }) {
            Ok(r) => return r,
            Err(_would_block) => {}
        }
    }
}
pub const IR_MAX_US: u64 = 2_000_000;

impl Sys {
    /// Linux owns the buzzer's PWM (ESPHome's switch was off at the last start).
    pub fn pwm_owned(&self) -> bool {
        self.p(PWM_CHIP).is_dir()
    }

    /// Linux owns the ring's SPI bus.
    pub fn spi_owned(&self) -> bool {
        self.p(SPI_DEV).exists()
    }

    pub fn ir_present(&self) -> bool {
        self.p(LIRC_RX).exists() && self.p(LIRC_TX).exists()
    }

    /// The second core has firmware to run (the ESPHome package put it there).
    pub fn rtos_firmware(&self) -> bool {
        self.p(crate::rtos::PATCHED_ELF).exists()
    }

    pub fn rtos_running(&self) -> bool {
        fsx::read_trim(&self.p(REMOTEPROC_STATE)).as_deref() == Some("running")
    }

    /// A command to the second core through `smhub-broker`.
    pub async fn rtos_notify(&self, args: &[&str]) -> Result<()> {
        let out = self.exec(RTOS_NOTIFY, args, Duration::from_secs(5)).await?;
        if out.ok() { Ok(()) } else { Err(Error::Unavailable(format!("rtos-notify: {}", out.error_text()))) }
    }

    // --- buzzer through PWM ---------------------------------------------------------------

    fn pwm_file(&self, f: &str) -> std::path::PathBuf {
        self.p(&format!("{PWM_CHIP}/pwm{PWM_CHANNEL}/{f}"))
    }

    /// Make the channel available (the OS's `pwm-buzzer` script usually did at start).
    pub fn pwm_prepare(&self) -> Result<()> {
        if !self.p(&format!("{PWM_CHIP}/pwm{PWM_CHANNEL}")).is_dir() {
            std::fs::write(self.p(&format!("{PWM_CHIP}/export")), PWM_CHANNEL.to_string())?;
        }
        Ok(())
    }

    /// Play a tone (`hz` 0 = silence). The duty cycle sets the loudness a little (1–50 %).
    pub fn pwm_tone(&self, hz: u32, duty_pct: u32) -> Result<()> {
        if hz == 0 {
            std::fs::write(self.pwm_file("enable"), "0")?;
            return Ok(());
        }
        let period = 1_000_000_000u64 / u64::from(hz.clamp(20, 20_000));
        let duty = period * u64::from(duty_pct.clamp(1, 50)) / 100;
        // the duty cycle may never exceed the period, so it goes to 0 first
        std::fs::write(self.pwm_file("duty_cycle"), "0")?;
        std::fs::write(self.pwm_file("period"), period.to_string())?;
        std::fs::write(self.pwm_file("duty_cycle"), duty.to_string())?;
        std::fs::write(self.pwm_file("enable"), "1")?;
        Ok(())
    }

    // --- ambient ring through SPI ---------------------------------------------------------

    pub fn ring_open(&self) -> Result<Ring> {
        use spidev::{SpiModeFlags, Spidev, SpidevOptions};
        let mut dev = Spidev::open(self.p(SPI_DEV)).map_err(|e| Error::Unavailable(format!("{SPI_DEV}: {e}")))?;
        let opts = SpidevOptions::new().bits_per_word(8).max_speed_hz(SPI_HZ).mode(SpiModeFlags::SPI_MODE_0).build();
        dev.configure(&opts).map_err(|e| Error::Unavailable(format!("{SPI_DEV}: {e}")))?;
        Ok(Ring { dev })
    }

    // --- service LEDs and user button through libgpiod -----------------------------------

    /// Hold the service LEDs at these values until the returned child ends (dropping it ends
    /// it). `gpioset` keeps the lines while it runs.
    pub fn leds_hold(&self, power: bool, blue: bool) -> Result<Child> {
        let mut c = self.command("gpioset");
        c.args([format!("led_pwr={}", u8::from(power)), format!("led_cus={}", u8::from(blue))]).stdout(Stdio::null()).stderr(Stdio::null());
        c.spawn().map_err(|e| Error::Unavailable(format!("gpioset: {e}")))
    }

    /// The buttons' input device (the kernel's `gpio-keys`): its event file and the key codes it
    /// has. `None` when the hub has none.
    pub fn keys_device(&self) -> Option<(PathBuf, Vec<u16>)> {
        let dir = std::fs::read_dir(self.p(INPUT_CLASS)).ok()?;
        let mut names: Vec<String> = dir.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("event")).collect();
        names.sort();
        names.into_iter().find_map(|n| {
            let dev = self.p(&format!("{INPUT_CLASS}/{n}/device"));
            let phys = std::fs::read_to_string(dev.join("phys")).unwrap_or_default();
            if !phys.starts_with("gpio-keys") {
                return None;
            }
            let keys = std::fs::read_to_string(dev.join("device/keys")).unwrap_or_default();
            Some((self.p(&format!("/dev/input/{n}")), parse_keys(&keys)))
        })
    }

    /// Watch the buttons: a key code and whether it went down. Plain reads of the kernel's input
    /// events, without polling; any number of programs read them side by side (the OS's
    /// `smhub-broker` and `smhub-reset-daemon` do). Ends when the receiver goes.
    pub fn keys_watch(&self, dev: &Path) -> Result<mpsc::Receiver<(u16, bool)>> {
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NONBLOCK)
            .open(dev)
            .map_err(|e| Error::Unavailable(format!("{}: {e}", dev.display())))?;
        #[allow(deprecated)]
        let fd = AsyncFd::new(file).map_err(|e| Error::internal(e.to_string()))?;
        let (tx, rx) = mpsc::channel(16);
        tokio::spawn(async move {
            let mut buf = [0u8; INPUT_EVENT * 16];
            loop {
                let n = tokio::select! {
                    () = tx.closed() => break,
                    n = read_events(&fd, &mut buf) => match n {
                        Ok(n) if n > 0 => n,
                        _ => break,
                    },
                };
                for ev in buf.get(..n).unwrap_or_default().chunks_exact(INPUT_EVENT) {
                    if let Some(k) = key_event(ev) {
                        if tx.send(k).await.is_err() {
                            return;
                        }
                    }
                }
            }
        });
        Ok(rx)
    }

    // --- infrared --------------------------------------------------------------------------

    /// Turn the receiver's kernel decoders on (for learning and listening) or back to raw only.
    pub fn ir_decoders(&self, on: bool) -> Result<()> {
        std::fs::write(self.p(RC_PROTOCOLS), if on { IR_DECODERS } else { "lirc" })?;
        Ok(())
    }

    /// Wait for one code on the receiver, raw and (when a kernel decoder knows it) decoded.
    /// `None` when nothing arrived in time.
    pub async fn ir_receive(&self, limit: Duration) -> Result<Option<IrReceived>> {
        let _ = self.ir_decoders(true);
        let mut decoded = self.ir_decoded_stream()?;
        let end = tokio::time::Instant::now() + limit;
        loop {
            let mut raw = self.command("ir-ctl");
            raw.args(["-d", LIRC_RX, "--receive", "--one-shot"]).stdout(Stdio::piped()).stderr(Stdio::null());
            let child = raw.spawn().map_err(|e| Error::Unavailable(format!("ir-ctl: {e}")))?;
            let out = match tokio::time::timeout_at(end, child.wait_with_output()).await {
                Ok(Ok(o)) => o,
                Ok(Err(e)) => return Err(Error::Unavailable(format!("ir-ctl: {e}"))),
                Err(_) => return Ok(None),
            };
            if !out.status.success() {
                return Err(Error::Unavailable(format!("ir-ctl: {}", out.status)));
            }
            let raw = parse_raw(&String::from_utf8_lossy(&out.stdout));
            if raw.len() < 8 {
                // a few pulses are not a code: light from a lamp or the sun, or the hub's own
                // sending, which the receiver sees as one late pulse (the sender keeps interrupts
                // off while it sends). Wait on for a real code; each capture ends only after the
                // receiver's own silence timeout, so this does not spin.
                while decoded.try_recv().is_ok() {}
                continue;
            }
            // the decoder reports within a few milliseconds of the end of the code
            let dec = tokio::time::timeout(Duration::from_millis(400), decoded.recv()).await.ok().flatten();
            let (protocol, scancode) = match dec.or_else(|| decode_nec(&raw)) {
                Some((p, s)) => (Some(p), Some(s)),
                None => (None, None),
            };
            return Ok(Some(IrReceived { protocol, scancode, raw }));
        }
    }

    /// Decoded codes as the kernel reports them, until the receiver is dropped. Runs
    /// `ir-keytable` in test mode, which prints each decoded code with its protocol. Its output
    /// goes to a pseudo-terminal: into a pipe, `ir-keytable` holds its lines back (checked on the
    /// hub: nothing in 2.5 s), so the codes would arrive long after the press, or never.
    pub fn ir_decoded_stream(&self) -> Result<mpsc::Receiver<(String, u64)>> {
        let pty = crate::terminal::Pty::open(None)?;
        let mut c = self.command("ir-keytable");
        c.args(["-s", "rc0", "-t"]).stdout(pty.stdio()?).stderr(Stdio::null());
        let child = c.spawn().map_err(|e| Error::Unavailable(format!("ir-keytable: {e}")))?;
        let crate::terminal::Pty { master, slave, .. } = pty;
        drop(slave);
        let (tx, rx) = mpsc::channel(16);
        tokio::spawn(async move {
            // `ir-keytable` is killed when this task ends (`kill_on_drop`): at the end of its
            // output, or as soon as the caller drops the receiver, not at the next code
            let _child = child;
            let mut buf = [0u8; 512];
            let mut line: Vec<u8> = Vec::with_capacity(IR_LINE_MAX);
            loop {
                let n = tokio::select! {
                    () = tx.closed() => break,
                    n = crate::terminal::read_pty(&master, &mut buf) => match n {
                        Ok(n) if n > 0 => n,
                        _ => break,
                    },
                };
                for &b in buf.iter().take(n) {
                    if b != b'\n' {
                        if line.len() < IR_LINE_MAX {
                            line.push(b);
                        }
                        continue;
                    }
                    if let Some(d) = parse_decoded(&String::from_utf8_lossy(&line)) {
                        if tx.send(d).await.is_err() {
                            return;
                        }
                    }
                    line.clear();
                }
            }
        });
        Ok(rx)
    }

    pub async fn ir_send_scancode(&self, protocol: &str, scancode: u64) -> Result<()> {
        if !protocol.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') || protocol.is_empty() || protocol.len() > 16 {
            return Err(Error::invalid("protocol", "not a known protocol"));
        }
        let arg = format!("{protocol}:{scancode:#x}");
        let out = self.exec("ir-ctl", &["-d", LIRC_TX, "--scancode", &arg], Duration::from_secs(5)).await?;
        if out.ok() { Ok(()) } else { Err(Error::Unavailable(format!("ir-ctl: {}", out.error_text()))) }
    }

    pub async fn ir_send_raw(&self, raw: &[u32], carrier: u32) -> Result<()> {
        check_raw(raw)?;
        let carrier = carrier.clamp(20_000, 60_000);
        let text: Vec<String> = raw.iter().enumerate().map(|(i, us)| format!("{}{us}", if i % 2 == 0 { '+' } else { '-' })).collect();
        let file = self.p("/run/smhubd-ir.txt");
        fsx::write_plain(&file, text.join(" ").as_bytes())?;
        let f = file.to_string_lossy().into_owned();
        let c = carrier.to_string();
        let out = self.exec("ir-ctl", &["-d", LIRC_TX, "--carrier", &c, "--send", &f], Duration::from_secs(5)).await;
        let _ = std::fs::remove_file(&file);
        let out = out?;
        if out.ok() { Ok(()) } else { Err(Error::Unavailable(format!("ir-ctl: {}", out.error_text()))) }
    }
}

/// The ring's SPI device, configured.
pub struct Ring {
    dev: spidev::Spidev,
}

impl Ring {
    /// Show 12 colours (red, green, blue). Blocking: a few hundred bytes at 6.4 MHz.
    pub fn show(&mut self, px: &[[u8; 3]; RING_LEDS]) -> Result<()> {
        use std::io::Write;
        self.dev.write_all(&ws2812(px)).map_err(|e| Error::Unavailable(format!("{SPI_DEV}: {e}")))
    }
}

/// WS2812 data: green, red, blue per LED, each bit as one SPI byte (1 = 0b1110_0000, 0 =
/// 0b1000_0000 at 6.4 MHz), then a quiet line that latches the colours.
pub fn ws2812(px: &[[u8; 3]; RING_LEDS]) -> Vec<u8> {
    let mut out = Vec::with_capacity(RING_LEDS * 24 + 64);
    for [r, g, b] in px {
        for byte in [*g, *r, *b] {
            for bit in (0..8).rev() {
                out.push(if byte & (1 << bit) != 0 { 0b1110_0000 } else { 0b1000_0000 });
            }
        }
    }
    out.extend_from_slice(&[0u8; 64]);
    out
}

/// One received infrared code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrReceived {
    pub protocol: Option<String>,
    pub scancode: Option<u64>,
    pub raw: Vec<u32>,
}

/// Raw timings within the limits: an odd number of edges (it ends with a pulse), each between
/// 1 µs and 100 ms.
pub fn check_raw(raw: &[u32]) -> Result<()> {
    if raw.is_empty() || raw.len() > IR_MAX_EDGES || raw.len().is_multiple_of(2) {
        return Err(Error::invalid("raw", format!("1 to {IR_MAX_EDGES} timings, starting and ending with a pulse")));
    }
    if raw.iter().any(|&us| us == 0 || us > 100_000) || raw.iter().map(|&u| u64::from(u)).sum::<u64>() > IR_MAX_US {
        return Err(Error::invalid("raw", "each timing 1 µs to 100 ms, the code at most 2 s"));
    }
    Ok(())
}

/// `ir-ctl` receive output: `+9024 -4512 +564 ...` (also `pulse 9024` / `space 4512` lines and
/// `# timeout` comments). The trailing space and timeout are dropped.
pub fn parse_raw(text: &str) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut tokens = text.split_whitespace().peekable();
    while let Some(t) = tokens.next() {
        let (kind, val) = match t {
            "pulse" | "space" => (if t == "pulse" { '+' } else { '-' }, tokens.next().unwrap_or("")),
            "#" | "timeout" | "carrier" => {
                tokens.next();
                continue;
            }
            _ => match t.chars().next() {
                Some(c @ ('+' | '-')) => (c, t.get(1..).unwrap_or("")),
                _ => continue,
            },
        };
        let Ok(us) = val.parse::<u32>() else { continue };
        // keep pulse/space alternating, starting with a pulse
        let expect_pulse = out.len().is_multiple_of(2);
        if (kind == '+') != expect_pulse {
            if let Some(last) = out.last_mut() {
                *last = last.saturating_add(us);
            }
            continue;
        }
        if out.len() < IR_MAX_EDGES {
            out.push(us);
        }
    }
    if out.len().is_multiple_of(2) {
        out.pop();
    }
    out
}

/// `ir-keytable -t` output: `... lirc protocol(nec): scancode = 0x408` (a repeat says
/// `repeat`; it is skipped).
pub fn parse_decoded(line: &str) -> Option<(String, u64)> {
    let rest = line.split("lirc protocol(").nth(1)?;
    let (proto, rest) = rest.split_once(')')?;
    if rest.contains("repeat") {
        return None;
    }
    let hex = rest.split("scancode = ").nth(1)?.split_whitespace().next()?;
    let v = u64::from_str_radix(hex.trim_start_matches("0x"), 16).ok()?;
    Some((proto.trim().to_owned(), v))
}

fn near(us: u32, target: u32) -> bool {
    let t = i64::from(target);
    (i64::from(us) - t).abs() <= t / 4 + 100
}

/// NEC from raw timings, as the kernel names its scancodes: `nec` (16 bits, address and command
/// checked), `necx` (24 bits) or `nec32`. A fallback when the kernel decoder said nothing.
pub fn decode_nec(raw: &[u32]) -> Option<(String, u64)> {
    if raw.len() < 67 || !near(raw[0], 9000) || !near(raw[1], 4500) {
        return None;
    }
    let mut bits: u32 = 0;
    for i in 0..32 {
        let pulse = raw[2 + i * 2];
        let space = raw[3 + i * 2];
        if !near(pulse, 560) {
            return None;
        }
        let one = if near(space, 1690) {
            true
        } else if near(space, 560) {
            false
        } else {
            return None;
        };
        if one {
            bits |= 1 << i;
        }
    }
    let [a, na, c, nc] = bits.to_le_bytes();
    if c != !nc {
        return Some(("nec32".into(), u64::from(u32::from_be_bytes([a, na, c, nc]))));
    }
    if a == !na {
        Some(("nec".into(), (u64::from(a) << 8) | u64::from(c)))
    } else {
        Some(("necx".into(), (u64::from(a) << 16) | (u64::from(na) << 8) | u64::from(c)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nec_raw(addr: u8, cmd: u8) -> Vec<u32> {
        let mut r = vec![9000, 4500];
        let bits = u32::from_le_bytes([addr, !addr, cmd, !cmd]);
        for i in 0..32 {
            r.push(560);
            r.push(if bits & (1 << i) != 0 { 1690 } else { 560 });
        }
        r.push(560);
        r
    }

    #[test]
    fn nec_decodes() {
        assert_eq!(decode_nec(&nec_raw(0x04, 0x08)), Some(("nec".into(), 0x0408)));
        assert_eq!(decode_nec(&[9000, 4500, 560]), None);
    }

    #[test]
    fn raw_and_decoded_text() {
        let raw = parse_raw("+9024 -4512 +564 -564 +564\n# timeout 125000\n");
        assert_eq!(raw, vec![9024, 4512, 564, 564, 564]);
        assert_eq!(parse_raw("pulse 900\nspace 450\npulse 500\nspace 125000\n"), vec![900, 450, 500]);
        assert_eq!(parse_decoded("1586 8.001234: lirc protocol(nec): scancode = 0x408"), Some(("nec".into(), 0x408)));
        assert_eq!(parse_decoded("lirc protocol(nec): scancode = 0x408 repeat"), None);
        assert!(check_raw(&[9000, 4500, 560]).is_ok());
        assert!(check_raw(&[9000, 4500]).is_err());
    }

    fn input_event(kind: u16, code: u16, value: i32) -> Vec<u8> {
        let mut v = vec![0u8; 2 * std::mem::size_of::<usize>()];
        v.extend_from_slice(&kind.to_ne_bytes());
        v.extend_from_slice(&code.to_ne_bytes());
        v.extend_from_slice(&value.to_ne_bytes());
        v
    }

    #[test]
    fn key_events_and_key_lists() {
        assert_eq!(input_event(EV_KEY, KEY_BTN2, 1).len(), INPUT_EVENT);
        assert_eq!(key_event(&input_event(EV_KEY, KEY_BTN2, 1)), Some((KEY_BTN2, true)));
        assert_eq!(key_event(&input_event(EV_KEY, KEY_BTN1, 0)), Some((KEY_BTN1, false)));
        // a held key's repeats, and the sync events between, are not presses
        assert_eq!(key_event(&input_event(EV_KEY, KEY_BTN1, 2)), None);
        assert_eq!(key_event(&input_event(0, 0, 0)), None);
        assert_eq!(key_event(&[0u8; 4]), None);
        assert_eq!(parse_keys("141,408\n"), vec![141, 408]);
        assert_eq!(parse_keys("1-3,408"), vec![1, 2, 3, 408]);
        assert_eq!(parse_keys(""), Vec::<u16>::new());
        assert_eq!(parse_keys("1-60000"), Vec::<u16>::new());
    }

    /// The sysfs layout of the hub: event0 the infrared receiver, event1 the buttons.
    #[test]
    fn finds_the_buttons_device() {
        let root = std::env::temp_dir().join(format!("smhub-keys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (ev, phys, keys) in [("event0", "gpio_ir_recv/input0", None), ("event1", "gpio-keys/input0", Some("141,408"))] {
            let dev = root.join(format!("sys/class/input/{ev}/device"));
            std::fs::create_dir_all(dev.join("device")).unwrap_or_default();
            std::fs::write(dev.join("phys"), format!("{phys}\n")).unwrap_or_default();
            if let Some(k) = keys {
                std::fs::write(dev.join("device/keys"), format!("{k}\n")).unwrap_or_default();
            }
        }
        let sys = Sys::new(&root, None);
        let got = sys.keys_device();
        assert_eq!(got, Some((root.join("dev/input/event1"), vec![KEY_BTN1, KEY_BTN2])));
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(Sys::new(&root, None).keys_device(), None);
    }

    #[test]
    fn decoders_are_added_not_replaced() {
        // the kernel takes a name without `+` as the whole set (checked on the hub: the list
        // without `+` left only rc-mm on)
        let mut names = IR_DECODERS.split_whitespace();
        assert_eq!(names.next(), Some("lirc"));
        assert!(names.all(|n| n.starts_with('+')));
        assert!(IR_DECODERS.contains("+rc-5") && IR_DECODERS.contains("+rc-6") && IR_DECODERS.contains("+nec"));
    }

    /// A stand-in for `ir-keytable` that, like the real one, says nothing in time unless it
    /// writes to a terminal.
    #[cfg(unix)]
    #[tokio::test]
    async fn decoded_codes_arrive_at_once() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("smhub-irdec-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap_or_default();
        let p = bin.join("ir-keytable");
        let body = "#!/bin/sh\n[ -t 1 ] || exec sleep 30\necho 'Testing events. Please, press CTRL-C to abort.'\necho '1586.123: lirc protocol(rc6_0): scancode = 0x58 toggle=1'\nexec sleep 30\n";
        std::fs::write(&p, body).unwrap_or_default();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap_or_default();
        let sys = Sys::new(&root, Some(&bin));
        let stream = sys.ir_decoded_stream();
        assert!(stream.is_ok(), "the stream did not start");
        if let Ok(mut rx) = stream {
            let got = tokio::time::timeout(Duration::from_secs(3), rx.recv()).await;
            assert_eq!(got, Ok(Some(("rc6_0".to_owned(), 0x58))));
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Stand-ins for `ir-ctl` and `ir-keytable`: the first capture is one lone pulse (what the
    /// receiver sees of the hub's own sending), the second a real NEC code.
    #[cfg(unix)]
    #[tokio::test]
    async fn learning_waits_past_noise_and_ends_its_helper() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("smhub-ir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap_or_default();
        let script = |name: &str, body: String| {
            let p = bin.join(name);
            std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap_or_default();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap_or_default();
        };
        let code: String = nec_raw(0x04, 0x08).iter().enumerate().map(|(i, v)| format!("{}{v} ", if i % 2 == 0 { '+' } else { '-' })).collect();
        let seen = root.join("seen");
        script("ir-ctl", format!("if [ -f {s} ]; then echo '{code}'; else touch {s}; echo '+230 -130000'; fi", s = seen.display()));
        let pid_file = root.join("keytable.pid");
        script("ir-keytable", format!("echo $$ > {}\nexec sleep 30", pid_file.display()));
        let sys = Sys::new(&root, Some(&bin));
        let got = sys.ir_receive(Duration::from_secs(5)).await;
        assert!(matches!(got, Ok(Some(ref r)) if r.raw.len() == 67 && r.scancode == Some(0x0408)), "{got:?}");
        // the decoder's helper ends with the learning, not at the next code
        let pid = std::fs::read_to_string(&pid_file).unwrap_or_default().trim().to_owned();
        let running = || {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
            stat.rsplit_once(')').is_some_and(|(_, rest)| !rest.trim_start().starts_with('Z'))
        };
        for _ in 0..100 {
            if !running() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(!pid.is_empty() && !running(), "ir-keytable {pid} still runs");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ws2812_bytes() {
        let mut px = [[0u8; 3]; RING_LEDS];
        px[0] = [0xff, 0, 0];
        let d = ws2812(&px);
        assert_eq!(d.len(), RING_LEDS * 24 + 64);
        // green first: all zero bits, then red: all one bits
        assert_eq!(d[0], 0b1000_0000);
        assert_eq!(d[8], 0b1110_0000);
    }
}
