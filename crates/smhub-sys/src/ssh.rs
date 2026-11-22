//! SSH access: three `sshd_config` directives through the line-preserving editor, and the
//! device user's `authorized_keys` (reference §4.10, §16). Unlike the old code, the key file and
//! folder get the owner and modes sshd expects.

use base64::Engine;
use serde::Serialize;
use sha2::{Digest, Sha256};
use smhub_model::{Error, Result};

use crate::flatconf::FlatConf;
use crate::{DEVICE_USER, Sys, fsx};

pub const SSHD_CONFIG: &str = "/etc/ssh/sshd_config";

#[derive(Debug, Clone, Serialize)]
pub struct Key {
    pub fingerprint: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub comment: Option<String>,
    #[serde(skip)]
    pub line: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SshAccess {
    pub password_auth: bool,
    pub root_login: bool,
    pub pubkey_auth: bool,
}

const KEY_TYPES: &[&str] = &[
    "ssh-ed25519",
    "ssh-rsa",
    "ecdsa-sha2-nistp256",
    "ecdsa-sha2-nistp384",
    "ecdsa-sha2-nistp521",
    "sk-ssh-ed25519@openssh.com",
    "sk-ecdsa-sha2-nistp256@openssh.com",
];

/// Parse one `authorized_keys` line (options before the key type are allowed).
pub fn parse_key(line: &str) -> Option<Key> {
    let l = line.trim();
    if l.is_empty() || l.starts_with('#') {
        return None;
    }
    let parts: Vec<&str> = l.split_whitespace().collect();
    let i = parts.iter().position(|p| KEY_TYPES.contains(p))?;
    let blob = base64::engine::general_purpose::STANDARD.decode(parts.get(i + 1)?).ok()?;
    let fp = base64::engine::general_purpose::STANDARD_NO_PAD.encode(Sha256::digest(&blob));
    let comment = parts.get(i + 2..).map(|c| c.join(" ")).filter(|c| !c.is_empty());
    Some(Key { fingerprint: format!("SHA256:{fp}"), kind: parts[i].to_owned(), comment, line: l.to_owned() })
}

impl Sys {
    fn home(&self) -> (std::path::PathBuf, Option<(u32, u32)>) {
        match fsx::user_ids(&self.p("/etc/passwd"), DEVICE_USER) {
            Some((u, g, home)) => (self.p(&home), Some((u, g))),
            None => (self.p(&format!("/home/{DEVICE_USER}")), None),
        }
    }

    fn keys_file(&self) -> std::path::PathBuf {
        self.home().0.join(".ssh/authorized_keys")
    }

    pub fn ssh_access(&self) -> SshAccess {
        let text = fsx::read_opt(&self.p(SSHD_CONFIG)).ok().flatten().unwrap_or_default();
        let c = FlatConf::parse(&text, &["#"]);
        let get = |k: &str| c.get_active(k).into_iter().next().map(|v| v.to_ascii_lowercase());
        SshAccess {
            password_auth: get("PasswordAuthentication").is_none_or(|v| v != "no"),
            root_login: get("PermitRootLogin").is_some_and(|v| v == "yes"),
            pubkey_auth: get("PubkeyAuthentication").is_none_or(|v| v != "no"),
        }
    }

    /// Set the directives that were given; returns whether the file changed.
    pub fn set_ssh_access(&self, password: Option<bool>, root: Option<bool>, pubkey: Option<bool>) -> Result<bool> {
        let path = self.p(SSHD_CONFIG);
        let text = fsx::read_opt(&path)?.ok_or_else(|| Error::Unavailable("sshd_config not found".into()))?;
        let mut c = FlatConf::parse(&text, &["#"]);
        let yn = |b: bool| if b { "yes".to_owned() } else { "no".to_owned() };
        if let Some(v) = password {
            c.set("PasswordAuthentication", &[yn(v)]);
        }
        if let Some(v) = root {
            c.set("PermitRootLogin", &[yn(v)]);
        }
        if let Some(v) = pubkey {
            c.set("PubkeyAuthentication", &[yn(v)]);
        }
        let out = c.render();
        if out == text {
            return Ok(false);
        }
        fsx::write_plain(&path, out.as_bytes())?;
        Ok(true)
    }

    pub fn ssh_keys(&self) -> Vec<Key> {
        fsx::read_opt(&self.keys_file()).ok().flatten().unwrap_or_default().lines().filter_map(parse_key).collect()
    }

    fn write_keys(&self, lines: &[String]) -> Result<()> {
        let (_, ids) = self.home();
        let file = self.keys_file();
        let dir = file.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
        std::fs::create_dir_all(&dir)?;
        let mut text = lines.join("\n");
        if !text.is_empty() {
            text.push('\n');
        }
        fsx::write_atomic(&file, text.as_bytes())?;
        std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
        std::fs::set_permissions(&file, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
        if let Some((u, g)) = ids {
            fsx::chown(&dir, u, g);
            fsx::chown(&file, u, g);
        }
        Ok(())
    }

    pub fn ssh_add_key(&self, line: &str) -> Result<()> {
        let l = line.trim();
        if l.contains('\n') {
            return Err(Error::invalid("key", "paste one key on one line"));
        }
        let k = parse_key(l).ok_or_else(|| Error::invalid("key", "not an SSH public key (e.g. ssh-ed25519 AAAA... name)"))?;
        let existing = self.ssh_keys();
        if existing.iter().any(|e| e.fingerprint == k.fingerprint) {
            return Err(Error::Conflict("this key is already added".into()));
        }
        let mut lines: Vec<String> = fsx::read_opt(&self.keys_file())?.unwrap_or_default().lines().map(str::to_owned).filter(|s| !s.trim().is_empty()).collect();
        lines.push(k.line);
        self.write_keys(&lines)
    }

    pub fn ssh_delete_key(&self, fingerprint: &str) -> Result<()> {
        let text = fsx::read_opt(&self.keys_file())?.unwrap_or_default();
        let mut found = false;
        let lines: Vec<String> = text
            .lines()
            .filter(|l| {
                let hit = parse_key(l).is_some_and(|k| k.fingerprint == fingerprint);
                found |= hit;
                !hit && !l.trim().is_empty()
            })
            .map(str::to_owned)
            .collect();
        if !found {
            return Err(Error::not_found("key"));
        }
        self.write_keys(&lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key() {
        let k = parse_key("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl me@pc");
        assert!(k.as_ref().is_some_and(|k| k.fingerprint.starts_with("SHA256:")));
        assert_eq!(k.and_then(|k| k.comment), Some("me@pc".into()));
        assert!(parse_key("not a key").is_none());
    }
}
