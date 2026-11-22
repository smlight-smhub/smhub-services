//! Line-preserving editor for flat `key value` configuration files (mosquitto.conf and the
//! like). A faithful port of the old backend's `system_config.flatconf.FlatConf`, so a setting
//! change produces the same file as before.

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Entry,
    Disabled,
    Comment,
    Blank,
}

#[derive(Debug, Clone)]
struct Line {
    kind: Kind,
    key: String,
    value: String,
    sep: String,
    pre: String,
    post: String,
    content: String,
}

impl Line {
    fn render(&self) -> String {
        match self.kind {
            Kind::Entry => format!("{}{}{}{}{}", self.key, self.pre, self.sep, self.post, self.value).trim_end().to_owned(),
            Kind::Disabled => format!("#{}{}{}{}{}", self.key, self.pre, self.sep, self.post, self.value).trim_end().to_owned(),
            Kind::Comment | Kind::Blank => self.content.clone(),
        }
    }
}

/// A parsed flat configuration file.
#[derive(Debug, Clone)]
pub struct FlatConf {
    lines: Vec<Line>,
    sep: String,
    pre: String,
    post: String,
}

/// `^([^=:\s]+)(\s*[:=]?\s*)(.*)$` without a regex engine.
fn split_kv(line: &str) -> (String, String, String, String, String) {
    let key_end = line.find(|c: char| c == '=' || c == ':' || c.is_whitespace()).unwrap_or(line.len());
    if key_end == 0 {
        return (line.trim().to_owned(), String::new(), " ".into(), String::new(), String::new());
    }
    let key = &line[..key_end];
    let rest = &line[key_end..];
    let ws1 = rest.len() - rest.trim_start().len();
    let after_ws1 = &rest[ws1..];
    let (sep_char, after_sep) = match after_ws1.chars().next() {
        Some(c @ ('=' | ':')) => (Some(c), &after_ws1[1..]),
        _ => (None, after_ws1),
    };
    let ws2 = after_sep.len() - after_sep.trim_start().len();
    let sep_block_len = ws1 + sep_char.map_or(0, char::len_utf8) + ws2;
    let sep_block = &rest[..sep_block_len];
    let value = &rest[sep_block_len..];
    let (sep, pre, post) = match sep_char {
        Some(c) => {
            let (p, q) = sep_block.split_once(c).unwrap_or((sep_block, ""));
            (c.to_string(), p.to_owned(), q.to_owned())
        }
        None => (" ".to_owned(), String::new(), String::new()),
    };
    (key.trim().to_owned(), value.trim().to_owned(), sep, pre, post)
}

fn starts_like_key(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

impl FlatConf {
    /// Parse text. `comments` are the comment prefixes (`#` and `;` by default).
    pub fn parse(text: &str, comments: &[&str]) -> Self {
        let mut lines = Vec::new();
        for raw in text.split_inclusive('\n') {
            let line = raw.strip_suffix('\n').unwrap_or(raw);
            let stripped = line.trim();
            if stripped.is_empty() {
                lines.push(Line { kind: Kind::Blank, key: String::new(), value: String::new(), sep: " ".into(), pre: String::new(), post: String::new(), content: line.to_owned() });
                continue;
            }
            if let Some(prefix) = comments.iter().find(|p| stripped.starts_with(**p)) {
                let uncommented = &stripped[prefix.len()..];
                if starts_like_key(uncommented) {
                    let (key, value, sep, pre, post) = split_kv(line);
                    let key = key.get(prefix.len()..).unwrap_or("").to_owned();
                    lines.push(Line { kind: Kind::Disabled, key, value, sep, pre, post, content: line.to_owned() });
                } else {
                    lines.push(Line { kind: Kind::Comment, key: String::new(), value: String::new(), sep: " ".into(), pre: String::new(), post: String::new(), content: line.to_owned() });
                }
                continue;
            }
            let (key, value, sep, pre, post) = split_kv(line);
            lines.push(Line { kind: Kind::Entry, key, value, sep, pre, post, content: line.to_owned() });
        }
        let mut conf = Self { lines, sep: " ".into(), pre: String::new(), post: String::new() };
        if let Some(first) = conf.lines.iter().find(|l| l.kind == Kind::Entry) {
            conf.sep = first.sep.clone();
            conf.pre = first.pre.clone();
            conf.post = first.post.clone();
        }
        conf
    }

    /// Values of a key, from active and commented-out lines alike (as the old code did).
    pub fn get(&self, key: &str) -> Vec<String> {
        self.lines
            .iter()
            .filter(|l| matches!(l.kind, Kind::Entry | Kind::Disabled) && l.key == key)
            .map(|l| l.value.clone())
            .collect()
    }

    /// Values of a key from active lines only.
    pub fn get_active(&self, key: &str) -> Vec<String> {
        self.lines.iter().filter(|l| l.kind == Kind::Entry && l.key == key).map(|l| l.value.clone()).collect()
    }

    fn new_line(&self, key: &str, value: &str) -> Line {
        Line {
            kind: Kind::Entry,
            key: key.to_owned(),
            value: value.to_owned(),
            sep: self.sep.clone(),
            pre: self.pre.clone(),
            post: self.post.clone(),
            content: String::new(),
        }
    }

    /// Set a key to one or more values. An empty value comments the line out; existing lines are
    /// updated in place, extra values inserted after the last one, surplus lines removed.
    pub fn set(&mut self, key: &str, values: &[String]) {
        let values: Vec<String> = if values.is_empty() { vec![String::new()] } else { values.to_vec() };
        let existing: Vec<usize> = self
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| matches!(l.kind, Kind::Entry | Kind::Disabled) && l.key == key)
            .map(|(i, _)| i)
            .collect();
        if existing.is_empty() {
            for v in values.iter().filter(|v| !v.is_empty()) {
                let l = self.new_line(key, v);
                self.lines.push(l);
            }
            return;
        }
        let n = values.len().min(existing.len());
        for i in 0..n {
            let line = &mut self.lines[existing[i]];
            if values[i].is_empty() {
                line.kind = Kind::Disabled;
                line.value.clear();
            } else {
                line.kind = Kind::Entry;
                line.value.clone_from(&values[i]);
            }
        }
        if values.len() > existing.len() {
            let start = existing[existing.len() - 1] + 1;
            for (k, v) in values[n..].iter().filter(|v| !v.is_empty()).enumerate() {
                let l = self.new_line(key, v);
                self.lines.insert(start + k, l);
            }
        } else if values.len() < existing.len() {
            for &i in existing[n..].iter().rev() {
                self.lines.remove(i);
            }
        }
    }

    /// Set one value, accepting a JSON array string as a list (old `set` with `_parse_json_list`).
    pub fn set_str(&mut self, key: &str, value: &str) {
        let t = value.trim();
        if t.starts_with('[') {
            if let Ok(serde_json::Value::Array(a)) = serde_json::from_str::<serde_json::Value>(t) {
                let vals: Vec<String> = a
                    .into_iter()
                    .map(|v| match v {
                        serde_json::Value::String(s) => s,
                        other => other.to_string(),
                    })
                    .collect();
                self.set(key, &vals);
                return;
            }
        }
        self.set(key, &[value.to_owned()]);
    }

    /// The file's text: every line rendered, each followed by a newline.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for l in &self.lines {
            out.push_str(&l.render());
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_in_place() {
        let text = "# Mosquitto\nlistener 1883\n#allow_anonymous true\npassword_file /x\n\n";
        let mut c = FlatConf::parse(text, &["#"]);
        assert_eq!(c.get("allow_anonymous"), vec!["true".to_owned()]);
        c.set("allow_anonymous", &["false".into()]);
        c.set("listener", &["1883 0.0.0.0".into()]);
        c.set("password_file", &[String::new()]);
        c.set_str("topic", r#"["a both 0", "b in 1"]"#);
        assert_eq!(
            c.render(),
            "# Mosquitto\nlistener 1883 0.0.0.0\nallow_anonymous false\n#password_file\n\ntopic a both 0\ntopic b in 1\n"
        );
    }

    #[test]
    fn separators() {
        let mut c = FlatConf::parse("a = 1\nb=2\nc: 3\n", &["#", ";"]);
        c.set("d", &["4".into()]);
        assert_eq!(c.render(), "a = 1\nb=2\nc: 3\nd = 4\n");
        assert_eq!(c.get("c"), vec!["3".to_owned()]);
    }
}
