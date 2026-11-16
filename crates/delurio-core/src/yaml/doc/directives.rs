//! The port of the yaml library's doc/directives.ts: the `%YAML` and `%TAG` directives of a stream, and how a tag
//! is resolved against them.

use indexmap::IndexMap;

/// `Directives`
#[derive(Debug, Clone)]
pub(crate) struct Directives {
    /// the directives-end/doc-start marker `---` (null in the library until a document says it)
    pub doc_start: bool,
    /// the doc-end marker `...`
    pub doc_end: bool,
    pub yaml_explicit: bool,
    pub yaml_version: String,
    pub tags: IndexMap<String, String>,
    at_next_document: bool,
}

const DEFAULT_TAG_HANDLE: &str = "!!";
const DEFAULT_TAG_PREFIX: &str = "tag:yaml.org,2002:";

fn default_tags() -> IndexMap<String, String> {
    IndexMap::from([(DEFAULT_TAG_HANDLE.to_string(), DEFAULT_TAG_PREFIX.to_string())])
}

impl Directives {
    /// `new Directives({ version: '1.2' })`
    pub(crate) fn new() -> Self {
        Directives {
            doc_start: false,
            doc_end: false,
            yaml_explicit: false,
            yaml_version: "1.2".to_string(),
            tags: default_tags(),
            at_next_document: false,
        }
    }

    /// `atDocument()`: the directives of the document that starts, and the stream's state for the next one
    pub(crate) fn at_document(&mut self) -> Directives {
        let res = Directives {
            doc_start: false,
            doc_end: false,
            yaml_explicit: self.yaml_explicit,
            yaml_version: self.yaml_version.clone(),
            tags: self.tags.clone(),
            at_next_document: false,
        };
        match self.yaml_version.as_str() {
            "1.1" => self.at_next_document = true,
            "1.2" => {
                self.at_next_document = false;
                self.yaml_explicit = false;
                self.yaml_version = "1.2".to_string();
                self.tags = default_tags();
            }
            _ => {}
        }
        res
    }

    /// `add(line, onError)`: a directive line; `on_error(offset, message, warning)`
    pub(crate) fn add(&mut self, line: &str, mut on_error: impl FnMut(usize, String, bool)) -> bool {
        if self.at_next_document {
            self.yaml_explicit = false;
            self.yaml_version = "1.1".to_string();
            self.tags = default_tags();
            self.at_next_document = false;
        }
        let trimmed = crate::js::trim(line);
        // `trimmed.split(/[ \t]+/)`
        let mut parts: Vec<&str> = trimmed.split([' ', '\t']).filter(|p| !p.is_empty()).collect();
        if parts.is_empty() {
            parts.push("");
        }
        let name = parts.remove(0);
        match name {
            "%TAG" => {
                if parts.len() != 2 {
                    on_error(0, "%TAG directive should contain exactly two parts".to_string(), false);
                    if parts.len() < 2 {
                        return false;
                    }
                }
                self.tags.insert(parts[0].to_string(), parts[1].to_string());
                true
            }
            "%YAML" => {
                self.yaml_explicit = true;
                if parts.len() != 1 {
                    on_error(0, "%YAML directive should contain exactly one part".to_string(), false);
                    return false;
                }
                let version = parts[0];
                if version == "1.1" || version == "1.2" {
                    self.yaml_version = version.to_string();
                    true
                } else {
                    let is_valid = is_version(version);
                    on_error(6, format!("Unsupported YAML version {version}"), is_valid);
                    false
                }
            }
            _ => {
                on_error(0, format!("Unknown directive {name}"), true);
                false
            }
        }
    }

    /// `tagName(source, onError)`: a tag's full name, `!` for the non-specific tag, None when it cannot be resolved
    pub(crate) fn tag_name(&self, source: &[u16], mut on_error: impl FnMut(String)) -> Option<String> {
        let text = String::from_utf16_lossy(source);
        if text == "!" {
            return Some("!".to_string()); // non-specific tag
        }
        if source.first() != Some(&(b'!' as u16)) {
            on_error(format!("Not a valid tag: {text}"));
            return None;
        }
        if source.get(1) == Some(&(b'<' as u16)) {
            let verbatim =
                String::from_utf16_lossy(source.get(2..source.len().saturating_sub(1).max(2)).unwrap_or(&[]));
            if verbatim == "!" || verbatim == "!!" {
                on_error(format!("Verbatim tags aren't resolved, so {text} is invalid."));
                return None;
            }
            if source.last() != Some(&(b'>' as u16)) {
                on_error("Verbatim tags must end with a >".to_string());
            }
            return Some(verbatim);
        }
        // /^(.*!)([^!]*)$/s: the handle up to the last !, the suffix after it
        let cut = source.iter().rposition(|&c| c == b'!' as u16).unwrap_or(0) + 1;
        let handle = String::from_utf16_lossy(&source[..cut]);
        let suffix = String::from_utf16_lossy(&source[cut..]);
        if suffix.is_empty() {
            on_error(format!("The {text} tag has no suffix"));
        }
        if let Some(prefix) = self.tags.get(&handle).filter(|p| !p.is_empty()) {
            return match decode_uri_component(&suffix) {
                Some(decoded) => Some(format!("{prefix}{decoded}")),
                None => {
                    on_error("URIError: URI malformed".to_string());
                    None
                }
            };
        }
        if handle == "!" {
            return Some(text); // local tag
        }
        on_error(format!("Could not resolve tag: {text}"));
        None
    }
}

/// /^\d+\.\d+$/
fn is_version(s: &str) -> bool {
    match s.split_once('.') {
        Some((a, b)) => {
            !a.is_empty()
                && !b.is_empty()
                && a.bytes().all(|c| c.is_ascii_digit())
                && b.bytes().all(|c| c.is_ascii_digit())
        }
        None => false,
    }
}

/// `decodeURIComponent(s)`, None where JavaScript throws `URIError: URI malformed`
fn decode_uri_component(s: &str) -> Option<String> {
    let units: Vec<u16> = s.encode_utf16().collect();
    let mut out: Vec<u16> = Vec::with_capacity(units.len());
    let hex = |i: usize| -> Option<u8> {
        let h = |c: u16| (c < 0x80).then(|| (c as u8 as char).to_digit(16)).flatten();
        Some((h(*units.get(i + 1)?)? * 16 + h(*units.get(i + 2)?)?) as u8)
    };
    let mut i = 0;
    while i < units.len() {
        if units[i] != b'%' as u16 {
            out.push(units[i]);
            i += 1;
            continue;
        }
        let b = hex(i)?;
        i += 3;
        if b < 0x80 {
            out.push(b as u16);
            continue;
        }
        let n = if b & 0xE0 == 0xC0 {
            2
        } else if b & 0xF0 == 0xE0 {
            3
        } else if b & 0xF8 == 0xF0 {
            4
        } else {
            return None;
        };
        let mut bytes = vec![b];
        for _ in 1..n {
            if units.get(i) != Some(&(b'%' as u16)) {
                return None;
            }
            let c = hex(i)?;
            if c & 0xC0 != 0x80 {
                return None;
            }
            bytes.push(c);
            i += 3;
        }
        let ch = std::str::from_utf8(&bytes).ok()?.chars().next()?;
        let mut buf = [0u16; 2];
        out.extend_from_slice(ch.encode_utf16(&mut buf));
    }
    Some(String::from_utf16_lossy(&out))
}
