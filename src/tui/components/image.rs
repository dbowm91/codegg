use ratatui::layout::Rect;
use ratatui::prelude::Buffer;
use ratatui::widgets::Widget;

#[cfg(feature = "image")]
use std::cell::RefCell;

#[cfg(feature = "image")]
use ratatui_image::protocol::StatefulProtocol;

#[cfg(feature = "image")]
pub struct ImageViewer {
    #[allow(dead_code)]
    state: RefCell<Option<StatefulProtocol>>,
}

#[cfg(not(feature = "image"))]
pub struct ImageViewer;

#[cfg(feature = "image")]
impl ImageViewer {
    pub fn new() -> Self {
        Self {
            state: RefCell::new(None),
        }
    }

    pub fn toggle_visible(&mut self) {}

    pub fn zoom_in(&mut self) {}
    pub fn zoom_out(&mut self) {}

    pub fn is_visible(&self) -> bool {
        true
    }

    pub fn load_from_data_uri(&mut self, _uri: &str) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

#[cfg(not(feature = "image"))]
impl ImageViewer {
    pub fn new() -> Self {
        Self
    }

    pub fn toggle_visible(&mut self) {}

    pub fn zoom_in(&mut self) {}
    pub fn zoom_out(&mut self) {}

    pub fn is_visible(&self) -> bool {
        true
    }

    pub fn load_from_data_uri(&mut self, _uri: &str) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

impl Default for ImageViewer {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for &ImageViewer {
    fn render(self, _area: Rect, _buf: &mut Buffer) {}
}

pub fn parse_data_uri(uri: &str) -> Option<(String, Vec<u8>)> {
    if !uri.starts_with("data:") {
        return None;
    }

    let uri = &uri[5..];
    let (mime_part, data_part) = uri.split_once(',')?;

    let mime = if mime_part.contains(';') {
        mime_part.split(';').next()?.to_string()
    } else {
        mime_part.to_string()
    };

    let is_base64 = mime_part.contains("base64");
    let data = if is_base64 {
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data_part).ok()?
    } else {
        decode_urlencoded(data_part)?
    };

    Some((mime, data))
}

fn decode_urlencoded(input: &str) -> Option<Vec<u8>> {
    let mut result = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if hex.len() == 2 {
                let byte = u8::from_str_radix(&hex, 16).ok()?;
                result.push(byte);
            } else {
                return None;
            }
        } else if c == '+' {
            result.push(b' ');
        } else {
            result.push(c as u8);
        }
    }

    Some(result)
}

pub fn is_supported_image_format(mime: &str) -> bool {
    matches!(
        mime,
        "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/bmp"
    )
}

pub fn detect_terminal_protocol() -> &'static str {
    #[cfg(feature = "image")]
    {
        if std::env::var("KITTY_WINDOW_ID").is_ok() {
            return "kitty";
        }
        if std::env::var("TERM_PROGRAM")
            .map(|v| v == "iTerm.app")
            .unwrap_or(false)
        {
            return "iterm2";
        }
        if std::env::var("TERM")
            .map(|v| v.starts_with("xterm"))
            .unwrap_or(false)
        {
            return "sixel";
        }
    }
    "none"
}
