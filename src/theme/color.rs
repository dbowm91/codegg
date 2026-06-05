//! Frontend-neutral theme color primitives.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::theme::error::ThemeError;

/// 24-bit RGB color in sRGB space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parse a hex color string.
    ///
    /// Accepts `#rrggbb`, `rrggbb`, `#rgb`/`rgb`, and the 8/4-digit alpha
    /// variants `#rrggbbaa`/`rrggbbaa` and `#rgba`/`rgba`. The alpha channel
    /// is silently discarded — the codebase has no concept of alpha at the
    /// theme layer, and most user-submitted Halloy themes use 8-digit hex for
    /// "transparent" or as a no-op.
    pub fn from_hex(input: &str) -> Result<Self, ThemeError> {
        let trimmed = input.trim();
        let stripped = trimmed.strip_prefix('#').unwrap_or(trimmed);
        let bytes = stripped.as_bytes();
        let len = bytes.len();

        if !matches!(len, 3 | 4 | 6 | 8) {
            return Err(ThemeError::InvalidColor {
                value: input.to_string(),
                reason: "expected #rgb, #rrggbb, #rgba, or #rrggbbaa".to_string(),
            });
        }

        let hex = |i: usize| -> Result<u8, ThemeError> {
            let hi = hex_digit(bytes[i])?;
            let lo = hex_digit(bytes[i + 1])?;
            Ok((hi << 4) | lo)
        };

        match len {
            6 => Ok(Self::new(hex(0)?, hex(2)?, hex(4)?)),
            8 => Ok(Self::new(hex(0)?, hex(2)?, hex(4)?)),
            3 => {
                let r = hex_digit(bytes[0])?;
                let g = hex_digit(bytes[1])?;
                let b = hex_digit(bytes[2])?;
                Ok(Self::new((r << 4) | r, (g << 4) | g, (b << 4) | b))
            }
            4 => {
                let r = hex_digit(bytes[0])?;
                let g = hex_digit(bytes[1])?;
                let b = hex_digit(bytes[2])?;
                Ok(Self::new((r << 4) | r, (g << 4) | g, (b << 4) | b))
            }
            _ => unreachable!("guarded by matches! above"),
        }
    }

    /// Render to a `#rrggbb` string.
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// WCAG relative luminance.
    pub fn relative_luminance(self) -> f64 {
        fn channel(c: u8) -> f64 {
            let s = f64::from(c) / 255.0;
            if s <= 0.03928 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel(self.r) + 0.7152 * channel(self.g) + 0.0722 * channel(self.b)
    }

    /// WCAG contrast ratio between two colors (range 1.0..=21.0).
    pub fn contrast_ratio(self, other: Self) -> f64 {
        let l1 = self.relative_luminance();
        let l2 = other.relative_luminance();
        let (lighter, darker) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
        (lighter + 0.05) / (darker + 0.05)
    }

    /// Approximation: is this a dark color (perceptual luminance < 0.5)?
    pub fn is_dark(self) -> bool {
        self.relative_luminance() < 0.5
    }
}

fn hex_digit(byte: u8) -> Result<u8, ThemeError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(ThemeError::InvalidColor {
            value: (byte as char).to_string(),
            reason: "non-hex character".to_string(),
        }),
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

impl From<Rgb> for ratatui::style::Color {
    fn from(rgb: Rgb) -> Self {
        ratatui::style::Color::Rgb(rgb.r, rgb.g, rgb.b)
    }
}

/// Color slot in a theme. Most slots resolve to a concrete RGB; a small number
/// of fallback slots use [`ThemeColor::Inherit`] to defer to a sibling color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeColor {
    Rgb(Rgb),
    Inherit,
}

impl ThemeColor {
    pub fn rgb_or(self, fallback: Rgb) -> Rgb {
        match self {
            ThemeColor::Rgb(c) => c,
            ThemeColor::Inherit => fallback,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_with_hash() {
        let c = Rgb::from_hex("#ff8800").unwrap();
        assert_eq!(c, Rgb::new(255, 136, 0));
    }

    #[test]
    fn parse_hex_no_hash() {
        let c = Rgb::from_hex("00ff7f").unwrap();
        assert_eq!(c, Rgb::new(0, 255, 127));
    }

    #[test]
    fn parse_short_hex() {
        let c = Rgb::from_hex("#abc").unwrap();
        assert_eq!(c, Rgb::new(0xaa, 0xbb, 0xcc));
    }

    #[test]
    fn parse_short_hex_no_hash() {
        let c = Rgb::from_hex("f0f").unwrap();
        assert_eq!(c, Rgb::new(0xff, 0x00, 0xff));
    }

    #[test]
    fn parse_eight_digit_hex_strips_alpha() {
        let c = Rgb::from_hex("#00000000").unwrap();
        assert_eq!(c, Rgb::new(0, 0, 0));
        let c = Rgb::from_hex("#73000054").unwrap();
        assert_eq!(c, Rgb::new(0x73, 0x00, 0x00));
    }

    #[test]
    fn parse_four_digit_hex_strips_alpha() {
        let c = Rgb::from_hex("#f00a").unwrap();
        assert_eq!(c, Rgb::new(0xff, 0x00, 0x00));
    }

    #[test]
    fn rejects_too_short() {
        // 2 hex digits is unambiguously invalid (not #rgb, not #rrggbb,
        // not #rgba, not #rrggbbaa).
        assert!(Rgb::from_hex("#ab").is_err());
    }

    #[test]
    fn rejects_seven_digit() {
        // 7 digits falls into no supported length.
        assert!(Rgb::from_hex("#abcdef0").is_err());
    }

    #[test]
    fn rejects_invalid_chars() {
        assert!(Rgb::from_hex("#zzzzzz").is_err());
    }

    #[test]
    fn to_hex_round_trip() {
        let c = Rgb::new(0x12, 0x34, 0x56);
        assert_eq!(c.to_hex(), "#123456");
        assert_eq!(Rgb::from_hex(&c.to_hex()).unwrap(), c);
    }

    #[test]
    fn contrast_black_white_is_about_21() {
        let ratio = Rgb::new(0, 0, 0).contrast_ratio(Rgb::new(255, 255, 255));
        assert!((ratio - 21.0).abs() < 0.01, "ratio was {}", ratio);
    }

    #[test]
    fn contrast_identical_is_one() {
        let c = Rgb::new(100, 100, 100);
        assert!((c.contrast_ratio(c) - 1.0).abs() < 1e-6);
    }
}
