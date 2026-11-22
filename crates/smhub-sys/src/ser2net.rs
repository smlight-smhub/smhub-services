//! The serial bridge: `/etc/ser2net/ser2net.yaml` (old `Ser2netManager`, reference §4.11; D58).
//!
//! ser2net 4 reads one top-level `connection: &<name>` entry per shared port. That key repeats,
//! which a YAML map cannot hold, so the file is written as text and read entry by entry. The old
//! backend wrote a `connections:` map, which ser2net 4 refuses ("Invalid token at the main
//! level"); such a file is still read, and the next save writes it in ser2net's format.

use std::collections::BTreeSet;
use std::io::BufRead;

use serde::Serialize;
use serde_json::Value;
use smhub_model::{Error, Result};

use crate::appconfig::parse_yaml;
use crate::{Sys, fsx};

pub const YAML: &str = "/etc/ser2net/ser2net.yaml";
const HEADER: &str = "%YAML 1.1\n---\n";
/// Ports the bridge shares at most.
pub const MAX_PORTS: usize = 4;
const BAUDS: [u32; 11] = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 115_200, 230_400, 460_800, 921_600];
/// Top-level entries a save keeps from a hand-edited file: they set things up but name no
/// connection. Everything else (`connection`, the old `connections`, `rotator`, ...) is
/// replaced by the page's ports.
const KEPT: [&str; 4] = ["define", "default", "admin", "led"];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Port {
    pub device: String,
    pub tcp_port: u16,
    pub baud: u32,
}

/// One top-level entry of the file: its key, and its lines with the comments above it.
#[derive(Debug)]
struct Entry {
    key: String,
    text: String,
}

/// The file's top-level entries in order. Directives, document markers, blank lines between
/// entries and comments after the last one are left out.
fn entries(text: &str) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    // blank lines and comments at column 0 belong to what follows them
    let mut pending: Vec<&str> = Vec::new();
    let push = |to: &mut String, line: &str| {
        to.push_str(line);
        to.push('\n');
    };
    for line in text.lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            pending.push(line);
        } else if line.starts_with([' ', '\t']) {
            if let Some(e) = out.last_mut() {
                pending.drain(..).for_each(|l| push(&mut e.text, l));
                push(&mut e.text, line);
            }
            pending.clear();
        } else if line.starts_with('%') || line.starts_with("---") || line.starts_with("...") {
            pending.clear();
        } else {
            let key = line.split(':').next().unwrap_or_default().trim().to_owned();
            let mut text = String::new();
            pending.drain(..).skip_while(|l| l.trim().is_empty()).for_each(|l| push(&mut text, l));
            push(&mut text, line);
            out.push(Entry { key, text });
        }
    }
    out
}

/// Splits a gensio string at the commas outside parentheses: `tcp(nodelay),6638`.
fn gensio_parts(s: &str) -> Vec<&str> {
    let (mut out, mut depth, mut start) = (Vec::new(), 0u32, 0);
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(s[start..].trim());
    out
}

/// A gensio's name without its options: `tcp(nodelay)` is `tcp`.
fn gensio_name(part: &str) -> &str {
    part.split('(').next().unwrap_or(part).trim()
}

/// One connection: `accepter: tcp,<port>` (a stack such as `telnet,tcp,<port>` too) and
/// `connector: serialdev,<device>,<baud>n81,...`. A connection with `enable: off` shares nothing.
fn parse_conn(v: &Value) -> Option<Port> {
    let off = match v.get("enable") {
        Some(Value::Bool(b)) => !b,
        Some(Value::String(s)) => matches!(s.trim(), "off" | "false" | "no"),
        _ => false,
    };
    if off {
        return None;
    }
    let acc = gensio_parts(v.get("accepter")?.as_str()?);
    if !acc.iter().any(|g| gensio_name(g) == "tcp") {
        return None;
    }
    let tcp_port = acc.last()?.parse().ok()?;
    let con = gensio_parts(v.get("connector")?.as_str()?);
    if gensio_name(con.first()?) != "serialdev" {
        return None;
    }
    let device = (*con.get(1)?).to_owned();
    if device.is_empty() {
        return None;
    }
    let digits = |s: &&str| s.chars().take_while(char::is_ascii_digit).collect::<String>();
    let baud = con.get(2).map(digits).and_then(|d| d.parse().ok()).unwrap_or(115_200);
    Some(Port { device, tcp_port, baud })
}

/// The entry's text with its key renamed.
fn rekey(e: &Entry, key: &str) -> String {
    let mut out = String::new();
    let mut done = false;
    for line in e.text.lines() {
        match line.strip_prefix(e.key.as_str()) {
            Some(rest) if !done && !line.starts_with('#') => {
                done = true;
                out.push_str(key);
                out.push_str(rest);
            }
            _ => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

/// The ports shared in a file of either format.
fn ports_in(text: &str) -> Vec<Port> {
    let all = entries(text);
    // a connection may use `*name` from a `define: &name`: each connection is read together with
    // the defines, which get keys of their own so that they do not repeat
    let defines: String = all.iter().filter(|e| e.key == "define").enumerate().map(|(i, e)| rekey(e, &format!("define{i}"))).collect();
    let mut out = Vec::new();
    for e in all {
        if e.key != "connection" && e.key != "connections" {
            continue;
        }
        let Some(v) = parse_yaml(&format!("{defines}{}", e.text)).or_else(|| parse_yaml(&e.text)) else { continue };
        match v.get(&e.key) {
            Some(c) if e.key == "connection" => out.extend(parse_conn(c)),
            Some(Value::Object(m)) => out.extend(m.values().filter_map(parse_conn)),
            _ => {}
        }
    }
    out.sort_by(|a, b| a.device.cmp(&b.device));
    out
}

/// A device ser2net and the YAML plain scalar both take as it is: no spaces, no commas.
fn valid_device(d: &str) -> bool {
    d.starts_with("/dev/") && !d.ends_with('/') && !d.contains("..") && d.len() < 128 && d.bytes().all(|b| b.is_ascii_graphic() && b != b',')
}

/// The connection's name (its YAML anchor): the device's file name, made unique.
fn anchor(device: &str, taken: &mut BTreeSet<String>) -> String {
    let base: String = device
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') { c } else { '_' })
        .collect();
    let base = if base.is_empty() { "port".to_owned() } else { base };
    let mut name = base.clone();
    let mut n = 2;
    while !taken.insert(name.clone()) {
        name = format!("{base}-{n}");
        n += 1;
    }
    name
}

/// The whole file: the kept entries, then one `connection` per port.
fn render(old: &str, ports: &[Port]) -> String {
    let kept: Vec<Entry> = entries(old).into_iter().filter(|e| KEPT.contains(&e.key.as_str())).collect();
    // anchors the kept entries define (`define: &banner ...`) are not reused
    let mut taken: BTreeSet<String> = kept
        .iter()
        .filter_map(|e| e.text.lines().find(|l| !l.starts_with('#') && !l.trim().is_empty()))
        .filter_map(|l| l.split_once(':').and_then(|(_, r)| r.trim_start().strip_prefix('&')))
        .filter_map(|r| r.split_whitespace().next())
        .map(str::to_owned)
        .collect();
    let mut blocks: Vec<String> = kept.into_iter().map(|e| e.text).collect();
    for p in ports {
        let name = anchor(&p.device, &mut taken);
        blocks.push(format!(
            "connection: &{name}\n  accepter: tcp,{}\n  connector: serialdev,{},{}n81,local\n  options:\n    kickolduser: true\n",
            p.tcp_port, p.device, p.baud
        ));
    }
    format!("{HEADER}{}", blocks.join("\n"))
}

impl Sys {
    /// Ports in the bridge's file.
    pub fn ser2net_ports(&self) -> Vec<Port> {
        let Ok(Some(text)) = fsx::read_opt(&self.p(YAML)) else { return Vec::new() };
        ports_in(&text)
    }

    pub fn ser2net_configured(&self) -> bool {
        self.p(YAML).exists()
    }

    /// Which of `ports` something listens on now, from the kernel's TCP tables. ser2net counts
    /// as started even when it refused its file; then its ports are not open.
    pub fn tcp_listening(&self, ports: &[u16]) -> BTreeSet<u16> {
        let want: BTreeSet<u16> = ports.iter().copied().collect();
        let mut found = BTreeSet::new();
        for table in ["/proc/net/tcp", "/proc/net/tcp6"] {
            let Ok(f) = std::fs::File::open(self.p(table)) else { continue };
            // `sl local_address rem_address st ...`; 0A is LISTEN
            for line in std::io::BufReader::new(f).lines().skip(1).map_while(std::result::Result::ok) {
                let mut cols = line.split_whitespace();
                let local = cols.nth(1);
                if cols.nth(1) != Some("0A") {
                    continue;
                }
                let port = local.and_then(|l| l.rsplit(':').next()).and_then(|h| u16::from_str_radix(h, 16).ok());
                if let Some(p) = port.filter(|p| want.contains(p)) {
                    found.insert(p);
                    if found.len() == want.len() {
                        return found;
                    }
                }
            }
        }
        found
    }

    /// Write the bridge's ports. No ports removes the file. Returns the old page value
    /// (`ports` as JSON) for the database.
    pub fn set_ser2net_ports(&self, ports: &[Port]) -> Result<String> {
        if ports.len() > MAX_PORTS {
            return Err(Error::invalid("ports", format!("share at most {MAX_PORTS} ports")));
        }
        let mut seen = BTreeSet::new();
        for p in ports {
            if !valid_device(&p.device) {
                return Err(Error::invalid("ports", format!("{} is not a serial device", p.device)));
            }
            if p.tcp_port == 0 || !seen.insert(p.tcp_port) {
                return Err(Error::invalid("ports", format!("TCP port {} is used twice or invalid", p.tcp_port)));
            }
            if !BAUDS.contains(&p.baud) {
                return Err(Error::invalid("ports", format!("{} is not a standard baud rate", p.baud)));
            }
        }
        let path = self.p(YAML);
        let old: Vec<Value> = ports
            .iter()
            .map(|p| serde_json::json!({ "serial_port": p.device, "baudrate": p.baud, "tcp_port": p.tcp_port }))
            .collect();
        if ports.is_empty() {
            let _ = std::fs::remove_file(&path);
            return Ok(serde_json::to_string(&old)?);
        }
        let text = render(&fsx::read_opt(&path)?.unwrap_or_default(), ports);
        fsx::write_atomic(&path, text.as_bytes())?;
        Ok(serde_json::to_string(&old)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AEOTEC: &str = "/dev/serial/by-id/usb-0658_0200-if00-port0";

    fn port(device: &str, tcp_port: u16, baud: u32) -> Port {
        Port { device: device.into(), tcp_port, baud }
    }

    fn sys(name: &str, file: Option<&str>) -> (Sys, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("smhub-ser2net-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("etc/ser2net")).unwrap_or_default();
        if let Some(t) = file {
            std::fs::write(root.join("etc/ser2net/ser2net.yaml"), t).unwrap_or_default();
        }
        (Sys::new(&root, None), root)
    }

    fn file(root: &std::path::Path) -> String {
        std::fs::read_to_string(root.join("etc/ser2net/ser2net.yaml")).unwrap_or_default()
    }

    #[test]
    fn connector() {
        let v = serde_json::json!({"accepter": "tcp,6638", "connector": "serialdev,/dev/ttyS1,115200n81,local"});
        assert_eq!(parse_conn(&v), Some(port("/dev/ttyS1", 6638, 115_200)));
        let v = serde_json::json!({"accepter": "telnet(rfc2217),tcp(nodelay),3333", "connector": "serialdev(nobreak), /dev/ttyUSB0, 9600e71,local"});
        assert_eq!(parse_conn(&v), Some(port("/dev/ttyUSB0", 3333, 9600)));
        let v = serde_json::json!({"accepter": "tcp,2000", "enable": "off", "connector": "serialdev,/dev/ttyS0,9600n81,local"});
        assert_eq!(parse_conn(&v), None);
        let v = serde_json::json!({"accepter": "udp,2000", "connector": "serialdev,/dev/ttyS0,9600n81,local"});
        assert_eq!(parse_conn(&v), None);
    }

    #[test]
    fn writes_one_port_as_ser2net_4_reads_it() {
        let (s, root) = sys("one", None);
        let old = s.set_ser2net_ports(&[port(AEOTEC, 6638, 115_200)]).unwrap_or_default();
        assert_eq!(
            file(&root),
            "%YAML 1.1\n---\nconnection: &usb-0658_0200-if00-port0\n  accepter: tcp,6638\n  \
             connector: serialdev,/dev/serial/by-id/usb-0658_0200-if00-port0,115200n81,local\n  options:\n    kickolduser: true\n"
        );
        assert_eq!(old, r#"[{"serial_port":"/dev/serial/by-id/usb-0658_0200-if00-port0","baudrate":115200,"tcp_port":6638}]"#);
        assert_eq!(s.ser2net_ports(), [port(AEOTEC, 6638, 115_200)]);
    }

    #[test]
    fn writes_several_ports_with_unique_names() {
        let (s, root) = sys("several", None);
        let ports = [port("/dev/ttyS2", 6638, 115_200), port("/dev/ttyUSB0", 6639, 9600), port("/dev/usb/ttyUSB0", 6640, 460_800)];
        s.set_ser2net_ports(&ports).unwrap_or_default();
        let text = file(&root);
        assert_eq!(text.matches("\nconnection: &").count(), 3, "{text}");
        assert!(text.contains("connection: &ttyS2\n  accepter: tcp,6638\n  connector: serialdev,/dev/ttyS2,115200n81,local\n"), "{text}");
        assert!(text.contains("connection: &ttyUSB0\n  accepter: tcp,6639\n  connector: serialdev,/dev/ttyUSB0,9600n81,local\n"), "{text}");
        assert!(text.contains("connection: &ttyUSB0-2\n  accepter: tcp,6640\n  connector: serialdev,/dev/usb/ttyUSB0,460800n81,local\n"), "{text}");
        assert_eq!(text.matches("    kickolduser: true\n").count(), 3);
        // blocks are separated by one blank line
        assert!(text.contains("kickolduser: true\n\nconnection: &ttyUSB0\n"), "{text}");
        assert_eq!(s.ser2net_ports(), ports);
        // saving again gives the same file
        s.set_ser2net_ports(&ports).unwrap_or_default();
        assert_eq!(file(&root), text);
    }

    #[test]
    fn reads_the_old_backends_map_and_rewrites_it() {
        let old = "%YAML 1.1\n---\nconnections:\n  ttyS1:\n    accepter: tcp,5001\n    connector: serialdev,/dev/ttyS1,115200n81,local\n  \
                   ttyUSB0:\n    accepter: tcp,6002\n    connector: serialdev,/dev/ttyUSB0,9600n81,local\n";
        let (s, root) = sys("old", Some(old));
        let ports = s.ser2net_ports();
        assert_eq!(ports, [port("/dev/ttyS1", 5001, 115_200), port("/dev/ttyUSB0", 6002, 9600)]);
        s.set_ser2net_ports(&ports).unwrap_or_default();
        let text = file(&root);
        assert!(!text.contains("connections"), "{text}");
        assert!(text.starts_with("%YAML 1.1\n---\nconnection: &ttyS1\n  accepter: tcp,5001\n"), "{text}");
        assert_eq!(s.ser2net_ports(), ports);
    }

    #[test]
    fn reads_a_hand_written_file_and_keeps_its_setup() {
        let hand = "%YAML 1.1\n---\n# banner for every port\ndefine: &banner \\r\\nser2net port \\p\\r\\n\n\n\
                    connection: &zigbee\n    accepter: telnet(rfc2217),tcp,2000\n    options:\n      banner: *banner\n    \
                    connector: serialdev,\n              /dev/ttyS0,\n              9600n81,local\n\n\
                    connection: &spare\n    accepter: tcp,2001\n    enable: off\n    connector: serialdev,/dev/ttyS1,9600n81,local\n\n\
                    admin:\n    accepter: tcp,127.0.0.1,2999\n\nrotator: &rot\n    accepter: tcp,2020\n    connections: [ *zigbee ]\n";
        let (s, root) = sys("hand", Some(hand));
        // `zigbee` uses the define's `*banner` and folds its connector over three lines; `spare` is off
        assert_eq!(s.ser2net_ports(), [port("/dev/ttyS0", 2000, 9600)]);
        s.set_ser2net_ports(&[port("/dev/ttyS0", 2000, 9600)]).unwrap_or_default();
        let text = file(&root);
        assert_eq!(
            text,
            "%YAML 1.1\n---\n# banner for every port\ndefine: &banner \\r\\nser2net port \\p\\r\\n\n\nadmin:\n    accepter: tcp,127.0.0.1,2999\n\n\
             connection: &ttyS0\n  accepter: tcp,2000\n  connector: serialdev,/dev/ttyS0,9600n81,local\n  options:\n    kickolduser: true\n"
        );
        assert_eq!(s.ser2net_ports(), [port("/dev/ttyS0", 2000, 9600)]);
        s.set_ser2net_ports(&[port("/dev/ttyS0", 2000, 9600)]).unwrap_or_default();
        assert_eq!(file(&root), text);
    }

    #[test]
    fn several_defines_are_read_with_each_connection() {
        let hand = "%YAML 1.1\n---\ndefine: &a one\ndefine: &dev /dev/ttyS3\nconnection: &x\n  accepter: tcp,7000\n  connector: serialdev,/dev/ttyS3,57600n81,local\n  \
                    options:\n    banner: *a\n";
        let (s, _root) = sys("defines", Some(hand));
        assert_eq!(s.ser2net_ports(), [port("/dev/ttyS3", 7000, 57_600)]);
    }

    #[test]
    fn a_kept_anchor_is_not_reused() {
        let (s, root) = sys("anchor", Some("%YAML 1.1\n---\ndefine: &ttyS2 x\n"));
        s.set_ser2net_ports(&[port("/dev/ttyS2", 6638, 115_200)]).unwrap_or_default();
        assert!(file(&root).contains("connection: &ttyS2-2\n"), "{}", file(&root));
    }

    #[test]
    fn no_ports_removes_the_file() {
        let (s, root) = sys("none", Some("%YAML 1.1\n---\n"));
        s.set_ser2net_ports(&[]).unwrap_or_default();
        assert!(!root.join("etc/ser2net/ser2net.yaml").exists());
        assert_eq!(s.ser2net_ports(), []);
    }

    #[test]
    fn refuses_what_the_file_cannot_hold() {
        let (s, root) = sys("refuse", None);
        for bad in [
            vec![port("/dev/tty S1", 6638, 115_200)],
            vec![port("/dev/ttyS1,9600", 6638, 115_200)],
            vec![port("/dev/", 6638, 115_200)],
            vec![port("/etc/passwd", 6638, 115_200)],
            vec![port("/dev/../etc/passwd", 6638, 115_200)],
            vec![port("/dev/ttyS1", 0, 115_200)],
            vec![port("/dev/ttyS1", 6638, 115_201)],
            vec![port("/dev/ttyS1", 6638, 115_200), port("/dev/ttyS2", 6638, 115_200)],
            (0..5).map(|i| port(&format!("/dev/ttyS{i}"), 6638 + i, 115_200)).collect(),
        ] {
            assert!(s.set_ser2net_ports(&bad).is_err(), "{bad:?}");
        }
        assert!(!root.join("etc/ser2net/ser2net.yaml").exists());
    }

    #[test]
    fn listening_ports_from_the_tcp_tables() {
        let (s, root) = sys("listen", None);
        std::fs::create_dir_all(root.join("proc/net")).unwrap_or_default();
        let head = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n";
        // 6638 = 19EE listening on IPv6; 6639 = 19EF only connected; 1883 = 075B listening on IPv4
        std::fs::write(root.join("proc/net/tcp"), format!("{head}   0: 00000000:075B 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 1\n   1: 0100007F:19EF 0100007F:9C40 01 00000000:00000000 00:00000000 00000000     0        0 2\n")).unwrap_or_default();
        std::fs::write(root.join("proc/net/tcp6"), format!("{head}   0: 00000000000000000000000000000000:19EE 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 3\n")).unwrap_or_default();
        assert_eq!(s.tcp_listening(&[6638, 6639, 1883]), BTreeSet::from([6638, 1883]));
        assert_eq!(s.tcp_listening(&[6639]), BTreeSet::new());
        assert_eq!(s.tcp_listening(&[]), BTreeSet::new());
    }
}
