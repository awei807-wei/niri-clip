// SPDX-License-Identifier: GPL-3.0-only

//! Clipboard content types shared by capture, storage, search and restore.

/// Semantic clipboard category used by persistence and presentation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentKind {
    Text,
    Image,
    Video,
    Files,
}

impl ContentKind {
    /// Returns the stable SQLite representation for this category.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
            Self::Video => "video",
            Self::Files => "files",
        }
    }

    /// Parses the stable SQLite representation for this category.
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "text" => Some(Self::Text),
            "image" => Some(Self::Image),
            "video" => Some(Self::Video),
            "files" => Some(Self::Files),
            _ => None,
        }
    }

    /// Returns the short, non-visual-only label shown in mixed history rows.
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "TEXT",
            Self::Image => "IMAGE",
            Self::Video => "VIDEO",
            Self::Files => "FILES",
        }
    }
}

/// Selected MIME representation for a Wayland offer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentFormat {
    pub kind: ContentKind,
    pub mime_type: String,
}

/// Validated clipboard bytes together with their semantic category and MIME.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClipboardContent {
    pub kind: ContentKind,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

impl ClipboardContent {
    /// Validates raw bytes for the selected format and constructs a payload.
    pub fn new(format: ContentFormat, bytes: Vec<u8>) -> Option<Self> {
        if bytes.is_empty() {
            return None;
        }
        let content = Self {
            kind: format.kind,
            mime_type: format.mime_type,
            bytes,
        };
        match content.kind {
            ContentKind::Text => {
                std::str::from_utf8(&content.bytes).ok()?;
            }
            ContentKind::Files => {
                if content.file_uris().is_empty() {
                    return None;
                }
            }
            ContentKind::Image | ContentKind::Video => {}
        }
        Some(content)
    }

    /// Produces a concise title suitable for a history row.
    pub fn display_title(&self) -> String {
        match self.kind {
            ContentKind::Text => String::from_utf8_lossy(&self.bytes).into_owned(),
            ContentKind::Image => "图片内容".to_owned(),
            ContentKind::Video => "视频内容".to_owned(),
            ContentKind::Files => {
                let names = self
                    .file_uris()
                    .iter()
                    .filter_map(|uri| file_name_from_uri(uri))
                    .take(3)
                    .collect::<Vec<_>>();
                if names.is_empty() {
                    "文件列表".to_owned()
                } else {
                    names.join(" · ")
                }
            }
        }
    }

    /// Produces searchable text without decoding binary media as UTF-8.
    pub fn search_text(&self) -> String {
        match self.kind {
            ContentKind::Text => String::from_utf8_lossy(&self.bytes).into_owned(),
            ContentKind::Image => format!("IMAGE 图片 {}", self.mime_type),
            ContentKind::Video => format!("VIDEO 视频 {}", self.mime_type),
            ContentKind::Files => format!(
                "FILES 文件 {} {}",
                self.display_title(),
                self.file_uris().join(" ")
            ),
        }
    }

    /// Converts a stored file representation into a canonical URI list.
    pub fn uri_list_bytes(&self) -> Option<Vec<u8>> {
        if self.kind != ContentKind::Files {
            return None;
        }
        let uris = self.file_uris();
        (!uris.is_empty()).then(|| format!("{}\r\n", uris.join("\r\n")).into_bytes())
    }

    /// Converts a stored file representation into GNOME's safe copy form.
    pub fn gnome_file_bytes(&self) -> Option<Vec<u8>> {
        let uri_list = String::from_utf8(self.uri_list_bytes()?).ok()?;
        Some(format!("copy\n{}", uri_list.replace("\r\n", "\n")).into_bytes())
    }

    /// Reports whether a text payload contains only whitespace.
    pub fn is_blank(&self) -> bool {
        self.kind == ContentKind::Text
            && std::str::from_utf8(&self.bytes).is_ok_and(|value| value.trim().is_empty())
    }

    /// Hashes semantic type, canonical MIME representation and original bytes.
    pub fn fingerprint(&self) -> blake3::Hash {
        let mut hasher = blake3::Hasher::new();
        hasher.update(self.kind.as_str().as_bytes());
        hasher.update(&[0]);
        if self.kind == ContentKind::Files {
            hasher.update(b"text/uri-list");
        } else {
            hasher.update(self.mime_type.as_bytes());
        }
        hasher.update(&[0]);
        if self.kind == ContentKind::Files {
            hasher.update(self.uri_list_bytes().as_deref().unwrap_or_default());
        } else {
            hasher.update(&self.bytes);
        }
        hasher.finalize()
    }

    fn file_uris(&self) -> Vec<String> {
        let Ok(value) = std::str::from_utf8(&self.bytes) else {
            return Vec::new();
        };
        value
            .lines()
            .map(str::trim)
            .filter(|line| {
                !line.is_empty()
                    && !line.starts_with('#')
                    && !line.eq_ignore_ascii_case("copy")
                    && !line.eq_ignore_ascii_case("cut")
            })
            .filter(|line| is_valid_uri(line))
            .map(str::to_owned)
            .collect()
    }
}

fn is_valid_uri(value: &str) -> bool {
    let Some((scheme, remainder)) = value.split_once(':') else {
        return false;
    };
    !remainder.is_empty()
        && scheme.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphabetic()
                || (index > 0 && (byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.')))
        })
}

/// Selects the best persistable representation from a Wayland MIME offer.
pub fn select_format(mime_types: &[String]) -> Option<ContentFormat> {
    find_base_mime(mime_types, "text/uri-list")
        .map(|mime_type| format(ContentKind::Files, mime_type))
        .or_else(|| {
            find_base_mime(mime_types, "x-special/gnome-copied-files")
                .map(|mime_type| format(ContentKind::Files, mime_type))
        })
        .or_else(|| {
            ["image/png", "image/jpeg", "image/webp", "image/svg+xml"]
                .into_iter()
                .find_map(|preferred| find_base_mime(mime_types, preferred))
                .or_else(|| find_family_mime(mime_types, "image/"))
                .map(|mime_type| format(ContentKind::Image, mime_type))
        })
        .or_else(|| {
            ["video/mp4", "video/webm"]
                .into_iter()
                .find_map(|preferred| find_base_mime(mime_types, preferred))
                .or_else(|| find_family_mime(mime_types, "video/"))
                .map(|mime_type| format(ContentKind::Video, mime_type))
        })
        .or_else(|| {
            find_base_mime(mime_types, "text/plain;charset=utf-8")
                .or_else(|| {
                    mime_types
                        .iter()
                        .find(|mime| mime.eq_ignore_ascii_case("UTF8_STRING"))
                        .cloned()
                })
                .or_else(|| {
                    mime_types
                        .iter()
                        .find(|mime| {
                            !base_mime(mime).eq_ignore_ascii_case("text/uri-list")
                                && wl_clipboard_rs::utils::is_text(mime)
                        })
                        .cloned()
                })
                .map(|mime_type| format(ContentKind::Text, mime_type))
        })
}

fn format(kind: ContentKind, mime_type: String) -> ContentFormat {
    ContentFormat { kind, mime_type }
}

fn base_mime(mime_type: &str) -> &str {
    mime_type.split(';').next().unwrap_or(mime_type).trim()
}

fn find_base_mime(mime_types: &[String], expected: &str) -> Option<String> {
    mime_types
        .iter()
        .find(|mime| {
            mime.eq_ignore_ascii_case(expected) || base_mime(mime).eq_ignore_ascii_case(expected)
        })
        .cloned()
}

fn find_family_mime(mime_types: &[String], family: &str) -> Option<String> {
    mime_types
        .iter()
        .find(|mime| base_mime(mime).to_ascii_lowercase().starts_with(family))
        .cloned()
}

fn file_name_from_uri(uri: &str) -> Option<String> {
    let without_suffix = uri.split(['?', '#']).next().unwrap_or(uri);
    let segment = without_suffix
        .rsplit('/')
        .find(|segment| !segment.is_empty())?;
    let decoded = percent_decode(segment.as_bytes());
    Some(String::from_utf8_lossy(&decoded).into_owned())
}

fn percent_decode(value: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(value.len());
    let mut index = 0;
    while index < value.len() {
        if value[index] == b'%' && index + 2 < value.len() {
            let high = hex_value(value[index + 1]);
            let low = hex_value(value[index + 2]);
            if let (Some(high), Some(low)) = (high, low) {
                output.push((high << 4) | low);
                index += 3;
                continue;
            }
        }
        output.push(value[index]);
        index += 1;
    }
    output
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
