//! Inert OSC-8 link metadata from the Markdown delegate; never replay escape sequences.
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    /// Zero-based rendered line and character range (not byte or terminal-cell offsets).
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub target: String,
}

pub fn from_osc8(raw: &str) -> Vec<Link> {
    let mut links = Vec::new();
    let (mut line, mut col) = (0, 0);
    let mut target: Option<String> = None;
    let mut rest = raw;
    while !rest.is_empty() {
        let split = rest.find("\x1b]").unwrap_or(rest.len());
        let plain = crate::render::plain_content(&rest[..split]);
        for chunk in plain.split_inclusive('\n') {
            let body = chunk.strip_suffix('\n').unwrap_or(chunk);
            let end = col + body.chars().count();
            if let Some(target) = &target
                && end > col
            {
                links.push(Link {
                    line,
                    start: col,
                    end,
                    target: target.clone(),
                });
            }
            col = end;
            if chunk.ends_with('\n') {
                line += 1;
                col = 0;
            }
        }
        rest = &rest[split..];
        if rest.is_empty() {
            break;
        }
        let payload = &rest[2..];
        let Some(end) = payload.find(['\x07', '\x1b']) else {
            break;
        };
        let terminator = &payload[end..];
        let n = if terminator.starts_with('\x07') {
            1
        } else if terminator.starts_with("\x1b\\") {
            2
        } else {
            break;
        };
        if let Some(fields) = payload[..end].strip_prefix("8;")
            && let Some((_, uri)) = fields.split_once(';')
        {
            target = (!uri.is_empty()).then(|| uri.to_owned());
        }
        rest = &payload[end + n..];
    }
    links
}

#[derive(Debug, PartialEq, Eq)]
pub enum Destination {
    Web(String),
    Local(PathBuf),
}

/// External protocols are deliberately allowlisted. Local links stay inside the viewed root,
/// including after symlink resolution, and are opened in the viewer, never executed by the OS.
pub fn destination(root: &Path, source: &Path, uri: &str) -> Result<Destination, &'static str> {
    if uri.chars().any(char::is_control) {
        return Err("Link contains control characters");
    }
    let scheme = uri.split_once(':').map(|(s, _)| s.to_ascii_lowercase());
    if matches!(scheme.as_deref(), Some("http" | "https" | "mailto")) {
        return Ok(Destination::Web(uri.to_owned()));
    }
    if scheme.is_some() || uri.starts_with("//") {
        return Err("Unsupported link protocol");
    }
    let path = uri.split(['#', '?']).next().unwrap_or("");
    if path.is_empty() {
        return Err("Heading-only links are not supported yet");
    }
    let mut decoded = Vec::new();
    let mut bytes = path.as_bytes().iter().copied();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            let a = bytes.next().and_then(|b| (b as char).to_digit(16));
            let b = bytes.next().and_then(|b| (b as char).to_digit(16));
            let (Some(a), Some(b)) = (a, b) else {
                return Err("Invalid link encoding");
            };
            decoded.push((a * 16 + b) as u8);
        } else {
            decoded.push(b);
        }
    }
    let decoded = String::from_utf8(decoded).map_err(|_| "Invalid link encoding")?;
    if decoded.chars().any(char::is_control) {
        return Err("Link contains control characters");
    }
    let path = source.parent().unwrap_or(root).join(decoded);
    let path = path
        .canonicalize()
        .map_err(|_| "Linked file does not exist")?;
    let canonical_root = root
        .canonicalize()
        .map_err(|_| "Viewed root is unavailable")?;
    if !path.starts_with(&canonical_root) {
        return Err("Linked file is outside the viewed root");
    }
    if !path.is_file() {
        return Err("Link does not point to a file");
    }
    // Keep the controller's root spelling (e.g. /var vs /private/var on macOS).
    Ok(Destination::Local(
        root.join(path.strip_prefix(&canonical_root).unwrap()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn osc_links_track_unicode_styles_and_lines() {
        let raw =
            "  α \x1b]8;id=1;https://example.org\x07\x1b[32m中文\x1b[0m\nnext\x1b]8;;\x1b\\ end";
        assert_eq!(
            from_osc8(raw),
            vec![
                Link {
                    line: 0,
                    start: 4,
                    end: 6,
                    target: "https://example.org".into()
                },
                Link {
                    line: 1,
                    start: 0,
                    end: 4,
                    target: "https://example.org".into()
                },
            ]
        );
        assert_eq!(crate::render::plain_content(raw), "  α 中文\nnext end");
    }
    #[test]
    fn rejects_executable_protocols_and_controls() {
        for uri in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "//host/file",
            "https://a\n",
            "#heading",
        ] {
            assert!(destination(Path::new("."), Path::new("README.md"), uri).is_err());
        }
        assert_eq!(
            destination(
                Path::new("."),
                Path::new("README.md"),
                "https://example.org"
            ),
            Ok(Destination::Web("https://example.org".into()))
        );
    }
    #[test]
    fn malformed_osc_is_inert() {
        assert!(from_osc8("\x1b]8;;https://evil").is_empty());
        assert!(from_osc8("\x1b]0;title\x07hello").is_empty());
    }
}
