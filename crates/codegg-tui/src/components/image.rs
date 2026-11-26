use ratatui::layout::Rect;
use ratatui::prelude::Buffer;
use ratatui::widgets::Widget;

#[cfg(feature = "image")]
use std::cell::RefCell;

#[cfg(feature = "image")]
use ratatui_image::protocol::{ImageSource, StatefulProtocol, StatefulProtocolType};

#[cfg(feature = "image")]
pub struct ImageViewer {
    #[allow(dead_code)]
    state: RefCell<Option<StatefulProtocol>>,
    #[allow(dead_code)]
    font_size: (u16, u16),
}

#[cfg(not(feature = "image"))]
pub struct ImageViewer;

#[cfg(feature = "image")]
impl ImageViewer {
    pub fn new() -> Self {
        Self {
            state: RefCell::new(None),
            font_size: (9, 18),
        }
    }

    pub fn toggle_visible(&mut self) {}

    pub fn zoom_in(&mut self) {
        self.font_size = (self.font_size.0 + 2, self.font_size.1 + 4);
    }

    pub fn zoom_out(&mut self) {
        if self.font_size.0 > 4 {
            self.font_size = (self.font_size.0 - 2, self.font_size.1 - 4);
        }
    }

    pub fn is_visible(&self) -> bool {
        self.state.borrow().is_some()
    }

    pub fn load_from_data_uri(&mut self, uri: &str) -> Result<(), Box<dyn std::error::Error>> {
        let (mime, data) = parse_data_uri(uri).ok_or("Failed to parse data URI")?;
        if !is_supported_image_format(&mime) {
            return Err(format!("Unsupported image format: {}", mime).into());
        }
        let img = image::load_from_memory(&data)?;
        let source = ImageSource::new(img, self.font_size, image::Rgba([0, 0, 0, 0]));
        let protocol_type = match detect_terminal_protocol() {
            "kitty" => StatefulProtocolType::Kitty(
                ratatui_image::protocol::kitty::StatefulKitty::new(rand::random(), false),
            ),
            "iterm2" => {
                StatefulProtocolType::ITerm2(ratatui_image::protocol::iterm2::Iterm2::default())
            }
            _ => StatefulProtocolType::Halfblocks(
                ratatui_image::protocol::halfblocks::Halfblocks::default(),
            ),
        };
        let state = StatefulProtocol::new(source, self.font_size, protocol_type);
        *self.state.borrow_mut() = Some(state);
        Ok(())
    }

    pub fn load_from_path(&mut self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let img = image::open(path)?;
        let source = ImageSource::new(img, self.font_size, image::Rgba([0, 0, 0, 0]));
        let protocol_type = match detect_terminal_protocol() {
            "kitty" => StatefulProtocolType::Kitty(
                ratatui_image::protocol::kitty::StatefulKitty::new(rand::random(), false),
            ),
            "iterm2" => {
                StatefulProtocolType::ITerm2(ratatui_image::protocol::iterm2::Iterm2::default())
            }
            _ => StatefulProtocolType::Halfblocks(
                ratatui_image::protocol::halfblocks::Halfblocks::default(),
            ),
        };
        let state = StatefulProtocol::new(source, self.font_size, protocol_type);
        *self.state.borrow_mut() = Some(state);
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
        Err("Image support not enabled".into())
    }

    pub fn load_from_path(&mut self, _path: &str) -> Result<(), Box<dyn std::error::Error>> {
        Err("Image support not enabled".into())
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

#[cfg(test)]
mod tests {
    use super::{is_supported_image_format, parse_data_uri};

    #[test]
    fn supported_mime_allowlist_matches_retained_set() {
        for mime in [
            "image/png",
            "image/jpeg",
            "image/gif",
            "image/webp",
            "image/bmp",
        ] {
            assert!(
                is_supported_image_format(mime),
                "{mime} must stay supported"
            );
        }
        for mime in [
            "image/svg+xml",
            "image/x-icon",
            "image/tiff",
            "image/avif",
            "text/plain",
            "",
        ] {
            assert!(
                !is_supported_image_format(mime),
                "{mime} must stay rejected"
            );
        }
    }

    #[test]
    fn parse_data_uri_handles_base64_and_rejects_malformed() {
        let uri = "data:image/png;base64,aGk=";
        let (mime, data) = parse_data_uri(uri).expect("valid data URI parses");
        assert_eq!(mime, "image/png");
        assert_eq!(data, b"hi");

        assert!(parse_data_uri("https://example.com/x.png").is_none());
        assert!(parse_data_uri("data:image/png;base64").is_none());
        assert!(parse_data_uri("data:image/png;base64,!!!not-base64!!!").is_none());
    }

    #[cfg(feature = "image")]
    mod image_feature_tests {
        use image::{ExtendedColorType, ImageEncoder};

        fn red_2x2_rgb() -> Vec<u8> {
            vec![
                255, 0, 0, 255, 0, 0, //
                255, 0, 0, 255, 0, 0, //
            ]
        }

        fn decode_ok(bytes: &[u8]) -> image::DynamicImage {
            image::load_from_memory(bytes).expect("retained format must decode")
        }

        #[test]
        fn png_roundtrip_decodes() {
            let mut buf = Vec::new();
            image::codecs::png::PngEncoder::new(&mut buf)
                .write_image(&red_2x2_rgb(), 2, 2, ExtendedColorType::Rgb8)
                .expect("png encode");
            let img = decode_ok(&buf);
            assert_eq!((img.width(), img.height()), (2, 2));
        }

        #[test]
        fn jpeg_roundtrip_decodes() {
            let mut buf = Vec::new();
            image::codecs::jpeg::JpegEncoder::new(&mut buf)
                .write_image(&red_2x2_rgb(), 2, 2, ExtendedColorType::Rgb8)
                .expect("jpeg encode");
            let img = decode_ok(&buf);
            assert_eq!((img.width(), img.height()), (2, 2));
        }

        #[test]
        fn gif_roundtrip_decodes() {
            let mut buf = Vec::new();
            image::codecs::gif::GifEncoder::new(&mut buf)
                .write_image(&red_2x2_rgb(), 2, 2, ExtendedColorType::Rgb8)
                .expect("gif encode");
            let img = decode_ok(&buf);
            assert_eq!((img.width(), img.height()), (2, 2));
        }

        #[test]
        fn webp_roundtrip_decodes() {
            let mut buf = Vec::new();
            image::codecs::webp::WebPEncoder::new_lossless(&mut buf)
                .write_image(&red_2x2_rgb(), 2, 2, ExtendedColorType::Rgb8)
                .expect("webp encode");
            let img = decode_ok(&buf);
            assert_eq!((img.width(), img.height()), (2, 2));
        }

        #[test]
        fn bmp_roundtrip_decodes() {
            let mut buf = Vec::new();
            image::codecs::bmp::BmpEncoder::new(&mut buf)
                .write_image(&red_2x2_rgb(), 2, 2, ExtendedColorType::Rgb8)
                .expect("bmp encode");
            let img = decode_ok(&buf);
            assert_eq!((img.width(), img.height()), (2, 2));
        }

        #[test]
        fn render_preparation_accepts_decoded_image() {
            let mut buf = Vec::new();
            image::codecs::png::PngEncoder::new(&mut buf)
                .write_image(&red_2x2_rgb(), 2, 2, ExtendedColorType::Rgb8)
                .expect("png encode");
            let img = decode_ok(&buf);
            let _source =
                ratatui_image::protocol::ImageSource::new(img, (9, 18), image::Rgba([0, 0, 0, 0]));
        }

        #[test]
        fn malformed_and_unsupported_inputs_fail_safely() {
            assert!(image::load_from_memory(b"not an image").is_err());
            assert!(image::load_from_memory(&[0u8; 32]).is_err());
            // Truncated PNG header must not panic.
            assert!(image::load_from_memory(&[0x89, b'P', b'N', b'G']).is_err());

            let mut viewer = super::super::ImageViewer::new();
            assert!(viewer
                .load_from_data_uri("data:image/svg+xml;base64,aGk=")
                .is_err());
            assert!(viewer.load_from_data_uri("not-a-data-uri").is_err());
            // Supported mime with corrupt payload fails through the decode path.
            assert!(viewer
                .load_from_data_uri("data:image/png;base64,!!!not-base64!!!")
                .is_err());
        }
    }
}
