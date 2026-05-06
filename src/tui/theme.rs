//! Theming system for the TUI.
//!
//! This module provides the [`Theme`] struct containing all color definitions for the UI.
//! Themes are defined as static data and converted to runtime themes at startup.
//!
//! ## Available Themes
//!
//! The module includes 30+ built-in themes:
//! - Dark: `dark`, `catppuccin-mocha`, `dracula`, `gruvbox-dark`, `nord`, `tokyonight`, `monokai`, `solarized-dark`, `one-dark`, `github-dark`, `ayu-dark`, `material-dark`, `palenight`, `cobalt`, `vs-dark`, `high-contrast-dark`, `zenburn`, `rose-pine`, `kanagawa`, `everforest-dark`, `moonlight`, `night-owl`, `atom-one-dark`, `base16-default`, `tokyonight-storm`, `dracula-soft`
//! - Light: `light`, `catppuccin-latte`, `solarized-light`, `github-light`, `material-light`
//!
//! ## Theme Structure
//!
//! Each theme defines colors for:
//! - `background`, `foreground`: Base colors
//! - `primary`, `secondary`: Accent colors
//! - `success`, `warning`, `error`: Semantic colors
//! - `muted`, `border`, `selection`: UI element colors
//!
//! ## Usage
//!
//! ```rust,ignore
//! // Get a theme by name
//! let theme = Theme::from_name("dracula").unwrap_or_else(Theme::dark);
//!
//! // Apply to a widget
//! widget.set_theme(theme.clone());
//!
//! // Check if dark mode
//! if theme.is_dark() { ... }
//! ```

use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: String,
    pub background: Color,
    pub foreground: Color,
    pub primary: Color,
    pub secondary: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub muted: Color,
    pub border: Color,
    pub selection: Color,
    pub selection_dim: Color,
    pub alternate_bg: Color,
    pub code_theme: &'static str,
    pub link: Color,
}

struct ThemeData {
    name: &'static str,
    background: (u8, u8, u8),
    foreground: (u8, u8, u8),
    primary: (u8, u8, u8),
    secondary: (u8, u8, u8),
    success: (u8, u8, u8),
    warning: (u8, u8, u8),
    error: (u8, u8, u8),
    muted: (u8, u8, u8),
    border: (u8, u8, u8),
    selection: (u8, u8, u8),
    selection_dim: (u8, u8, u8),
    alternate_bg: (u8, u8, u8),
    code_theme: &'static str,
    link: (u8, u8, u8),
}

impl ThemeData {
    fn to_theme(&self) -> Theme {
        Theme {
            name: self.name.to_string(),
            background: Color::Rgb(self.background.0, self.background.1, self.background.2),
            foreground: Color::Rgb(self.foreground.0, self.foreground.1, self.foreground.2),
            primary: Color::Rgb(self.primary.0, self.primary.1, self.primary.2),
            secondary: Color::Rgb(self.secondary.0, self.secondary.1, self.secondary.2),
            success: Color::Rgb(self.success.0, self.success.1, self.success.2),
            warning: Color::Rgb(self.warning.0, self.warning.1, self.warning.2),
            error: Color::Rgb(self.error.0, self.error.1, self.error.2),
            muted: Color::Rgb(self.muted.0, self.muted.1, self.muted.2),
            border: Color::Rgb(self.border.0, self.border.1, self.border.2),
            selection: Color::Rgb(self.selection.0, self.selection.1, self.selection.2),
            selection_dim: Color::Rgb(
                self.selection_dim.0,
                self.selection_dim.1,
                self.selection_dim.2,
            ),
            alternate_bg: Color::Rgb(
                self.alternate_bg.0,
                self.alternate_bg.1,
                self.alternate_bg.2,
            ),
            code_theme: self.code_theme,
            link: Color::Rgb(self.link.0, self.link.1, self.link.2),
        }
    }
}

const THEMES: &[ThemeData] = &[
    ThemeData {
        name: "dark",
        background: (15, 15, 20),
        foreground: (220, 220, 225),
        primary: (120, 180, 255),
        secondary: (180, 140, 255),
        success: (80, 200, 120),
        warning: (255, 180, 60),
        error: (255, 80, 80),
        muted: (100, 100, 110),
        border: (50, 50, 60),
        selection: (40, 60, 90),
        selection_dim: (50, 70, 100),
        alternate_bg: (18, 18, 24),
        code_theme: "base16-ocean.dark",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "light",
        background: (250, 250, 255),
        foreground: (30, 30, 35),
        primary: (30, 80, 180),
        secondary: (100, 40, 200),
        success: (20, 140, 60),
        warning: (200, 130, 0),
        error: (200, 40, 40),
        muted: (140, 140, 150),
        border: (210, 210, 220),
        selection: (200, 220, 250),
        selection_dim: (180, 200, 240),
        alternate_bg: (245, 245, 250),
        code_theme: "base16-github.light",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "catppuccin-mocha",
        background: (30, 30, 46),
        foreground: (205, 214, 244),
        primary: (137, 180, 250),
        secondary: (203, 166, 247),
        success: (166, 227, 161),
        warning: (249, 226, 175),
        error: (243, 139, 168),
        muted: (127, 132, 156),
        border: (69, 71, 90),
        selection: (49, 50, 68),
        selection_dim: (59, 60, 78),
        alternate_bg: (35, 35, 52),
        code_theme: "catppuccin-mocha",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "catppuccin-latte",
        background: (239, 241, 245),
        foreground: (76, 79, 105),
        primary: (30, 102, 245),
        secondary: (136, 57, 239),
        success: (64, 160, 43),
        warning: (223, 142, 29),
        error: (210, 15, 57),
        muted: (156, 160, 176),
        border: (188, 192, 204),
        selection: (204, 208, 218),
        selection_dim: (184, 188, 208),
        alternate_bg: (234, 236, 245),
        code_theme: "catppuccin-latte",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "dracula",
        background: (40, 42, 54),
        foreground: (248, 248, 242),
        primary: (139, 233, 253),
        secondary: (189, 147, 249),
        success: (80, 250, 123),
        warning: (241, 250, 140),
        error: (255, 85, 85),
        muted: (98, 114, 164),
        border: (68, 71, 90),
        selection: (68, 71, 90),
        selection_dim: (78, 81, 100),
        alternate_bg: (45, 47, 59),
        code_theme: "dracula",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "gruvbox-dark",
        background: (40, 40, 40),
        foreground: (235, 219, 178),
        primary: (131, 165, 152),
        secondary: (177, 156, 217),
        success: (152, 151, 26),
        warning: (215, 153, 33),
        error: (204, 36, 29),
        muted: (146, 131, 116),
        border: (80, 73, 69),
        selection: (60, 56, 54),
        selection_dim: (70, 66, 64),
        alternate_bg: (45, 45, 45),
        code_theme: "gruvbox-dark",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "nord",
        background: (46, 52, 64),
        foreground: (216, 222, 233),
        primary: (136, 192, 208),
        secondary: (180, 142, 173),
        success: (163, 190, 140),
        warning: (235, 203, 139),
        error: (191, 97, 106),
        muted: (129, 161, 193),
        border: (67, 76, 94),
        selection: (67, 76, 94),
        selection_dim: (77, 86, 104),
        alternate_bg: (51, 57, 69),
        code_theme: "nord",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "tokyonight",
        background: (26, 27, 38),
        foreground: (192, 202, 245),
        primary: (122, 162, 247),
        secondary: (187, 154, 247),
        success: (49, 176, 138),
        warning: (224, 175, 104),
        error: (247, 118, 142),
        muted: (86, 95, 137),
        border: (50, 52, 72),
        selection: (50, 52, 72),
        selection_dim: (60, 62, 82),
        alternate_bg: (31, 32, 43),
        code_theme: "tokyonight",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "monokai",
        background: (39, 40, 34),
        foreground: (248, 248, 242),
        primary: (102, 217, 239),
        secondary: (174, 129, 255),
        success: (166, 226, 46),
        warning: (249, 226, 175),
        error: (249, 38, 114),
        muted: (117, 113, 94),
        border: (57, 58, 50),
        selection: (57, 58, 50),
        selection_dim: (67, 68, 60),
        alternate_bg: (44, 45, 39),
        code_theme: "monokai",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "solarized-dark",
        background: (0, 43, 54),
        foreground: (131, 148, 150),
        primary: (38, 139, 210),
        secondary: (108, 113, 196),
        success: (133, 153, 0),
        warning: (181, 137, 0),
        error: (220, 50, 47),
        muted: (88, 110, 117),
        border: (7, 54, 66),
        selection: (7, 54, 66),
        selection_dim: (17, 64, 76),
        alternate_bg: (5, 48, 59),
        code_theme: "solarized-dark",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "solarized-light",
        background: (253, 246, 227),
        foreground: (101, 123, 131),
        primary: (38, 139, 210),
        secondary: (108, 113, 196),
        success: (133, 153, 0),
        warning: (181, 137, 0),
        error: (220, 50, 47),
        muted: (147, 161, 161),
        border: (204, 216, 219),
        selection: (211, 226, 230),
        selection_dim: (191, 206, 210),
        alternate_bg: (248, 241, 222),
        code_theme: "solarized-light",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "one-dark",
        background: (40, 44, 52),
        foreground: (171, 178, 191),
        primary: (97, 175, 239),
        secondary: (198, 120, 221),
        success: (152, 195, 121),
        warning: (229, 192, 123),
        error: (224, 108, 117),
        muted: (122, 128, 140),
        border: (60, 64, 74),
        selection: (60, 64, 74),
        selection_dim: (70, 74, 84),
        alternate_bg: (45, 49, 57),
        code_theme: "one-dark",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "github-dark",
        background: (13, 17, 23),
        foreground: (201, 209, 217),
        primary: (88, 166, 255),
        secondary: (188, 140, 255),
        success: (63, 185, 80),
        warning: (210, 153, 34),
        error: (248, 81, 73),
        muted: (100, 108, 118),
        border: (33, 38, 45),
        selection: (33, 38, 45),
        selection_dim: (43, 48, 55),
        alternate_bg: (18, 22, 28),
        code_theme: "base16-ocean.dark",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "github-light",
        background: (255, 255, 255),
        foreground: (36, 41, 47),
        primary: (9, 105, 218),
        secondary: (130, 80, 223),
        success: (33, 139, 58),
        warning: (158, 106, 3),
        error: (207, 34, 46),
        muted: (100, 108, 118),
        border: (208, 215, 222),
        selection: (200, 220, 255),
        selection_dim: (180, 200, 245),
        alternate_bg: (250, 250, 255),
        code_theme: "base16-github.light",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "ayu-dark",
        background: (15, 17, 21),
        foreground: (199, 195, 185),
        primary: (57, 186, 230),
        secondary: (211, 134, 155),
        success: (186, 225, 113),
        warning: (255, 183, 77),
        error: (255, 106, 97),
        muted: (92, 97, 105),
        border: (31, 34, 41),
        selection: (31, 34, 41),
        selection_dim: (41, 44, 51),
        alternate_bg: (20, 22, 26),
        code_theme: "ayu-dark",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "material-dark",
        background: (33, 33, 33),
        foreground: (224, 224, 224),
        primary: (100, 181, 246),
        secondary: (179, 157, 219),
        success: (129, 199, 132),
        warning: (255, 183, 77),
        error: (239, 83, 80),
        muted: (117, 117, 117),
        border: (51, 51, 51),
        selection: (51, 51, 51),
        selection_dim: (61, 61, 61),
        alternate_bg: (38, 38, 38),
        code_theme: "material-dark",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "material-light",
        background: (250, 250, 250),
        foreground: (51, 51, 51),
        primary: (33, 150, 243),
        secondary: (156, 39, 176),
        success: (76, 175, 80),
        warning: (255, 152, 0),
        error: (244, 67, 54),
        muted: (158, 158, 158),
        border: (224, 224, 224),
        selection: (187, 222, 251),
        selection_dim: (167, 202, 231),
        alternate_bg: (245, 245, 245),
        code_theme: "material-light",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "palenight",
        background: (41, 45, 62),
        foreground: (204, 208, 222),
        primary: (130, 170, 255),
        secondary: (199, 146, 234),
        success: (195, 232, 141),
        warning: (255, 191, 105),
        error: (239, 83, 80),
        muted: (105, 113, 140),
        border: (60, 65, 88),
        selection: (60, 65, 88),
        selection_dim: (70, 75, 98),
        alternate_bg: (46, 50, 67),
        code_theme: "palenight",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "cobalt",
        background: (25, 35, 50),
        foreground: (225, 225, 225),
        primary: (0, 180, 255),
        secondary: (200, 120, 255),
        success: (0, 200, 120),
        warning: (255, 200, 60),
        error: (255, 80, 80),
        muted: (100, 120, 140),
        border: (40, 55, 75),
        selection: (40, 55, 75),
        selection_dim: (50, 65, 85),
        alternate_bg: (30, 40, 55),
        code_theme: "cobalt",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "vs-dark",
        background: (30, 30, 30),
        foreground: (204, 204, 204),
        primary: (0, 122, 204),
        secondary: (181, 126, 220),
        success: (97, 175, 87),
        warning: (218, 165, 32),
        error: (244, 67, 54),
        muted: (128, 128, 128),
        border: (50, 50, 50),
        selection: (50, 50, 50),
        selection_dim: (60, 60, 60),
        alternate_bg: (35, 35, 35),
        code_theme: "vs-dark",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "high-contrast-dark",
        background: (0, 0, 0),
        foreground: (255, 255, 255),
        primary: (0, 255, 255),
        secondary: (255, 0, 255),
        success: (0, 255, 0),
        warning: (255, 255, 0),
        error: (255, 0, 0),
        muted: (128, 128, 128),
        border: (255, 255, 255),
        selection: (64, 64, 64),
        selection_dim: (84, 84, 84),
        alternate_bg: (10, 10, 10),
        code_theme: "base16-ocean.dark",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "zenburn",
        background: (63, 63, 63),
        foreground: (220, 220, 204),
        primary: (122, 145, 159),
        secondary: (181, 137, 179),
        success: (127, 179, 71),
        warning: (240, 210, 137),
        error: (224, 108, 117),
        muted: (112, 128, 144),
        border: (80, 80, 80),
        selection: (80, 80, 80),
        selection_dim: (90, 90, 90),
        alternate_bg: (68, 68, 68),
        code_theme: "zenburn",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "rose-pine",
        background: (25, 23, 36),
        foreground: (224, 222, 244),
        primary: (152, 195, 121),
        secondary: (197, 163, 255),
        success: (152, 195, 121),
        warning: (246, 197, 116),
        error: (236, 131, 136),
        muted: (140, 130, 160),
        border: (45, 43, 60),
        selection: (45, 43, 60),
        selection_dim: (55, 53, 70),
        alternate_bg: (30, 28, 41),
        code_theme: "rose-pine",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "kanagawa",
        background: (33, 32, 44),
        foreground: (220, 212, 197),
        primary: (114, 159, 192),
        secondary: (163, 142, 181),
        success: (129, 162, 103),
        warning: (230, 180, 88),
        error: (204, 90, 89),
        muted: (113, 113, 124),
        border: (54, 54, 74),
        selection: (54, 54, 74),
        selection_dim: (64, 64, 84),
        alternate_bg: (38, 37, 49),
        code_theme: "kanagawa",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "everforest-dark",
        background: (45, 52, 50),
        foreground: (211, 206, 190),
        primary: (125, 184, 172),
        secondary: (181, 167, 168),
        success: (142, 183, 119),
        warning: (219, 172, 80),
        error: (230, 109, 91),
        muted: (126, 137, 131),
        border: (66, 75, 72),
        selection: (66, 75, 72),
        selection_dim: (76, 85, 82),
        alternate_bg: (50, 57, 55),
        code_theme: "everforest-dark",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "moonlight",
        background: (22, 24, 36),
        foreground: (220, 220, 225),
        primary: (122, 162, 247),
        secondary: (199, 170, 255),
        success: (80, 200, 120),
        warning: (255, 180, 60),
        error: (255, 100, 100),
        muted: (90, 95, 120),
        border: (40, 43, 60),
        selection: (40, 43, 60),
        selection_dim: (50, 53, 70),
        alternate_bg: (27, 29, 41),
        code_theme: "moonlight",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "night-owl",
        background: (1, 22, 39),
        foreground: (214, 222, 235),
        primary: (130, 170, 255),
        secondary: (199, 146, 234),
        success: (195, 232, 141),
        warning: (255, 203, 107),
        error: (239, 83, 80),
        muted: (105, 120, 140),
        border: (25, 45, 60),
        selection: (25, 45, 60),
        selection_dim: (35, 55, 70),
        alternate_bg: (6, 27, 44),
        code_theme: "night-owl",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "atom-one-dark",
        background: (40, 44, 52),
        foreground: (171, 178, 191),
        primary: (97, 175, 239),
        secondary: (198, 120, 221),
        success: (152, 195, 121),
        warning: (229, 192, 123),
        error: (224, 108, 117),
        muted: (92, 99, 112),
        border: (60, 64, 74),
        selection: (60, 64, 74),
        selection_dim: (70, 74, 84),
        alternate_bg: (45, 49, 57),
        code_theme: "atom-one-dark",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "base16-default",
        background: (24, 24, 24),
        foreground: (208, 208, 208),
        primary: (114, 159, 207),
        secondary: (173, 127, 168),
        success: (163, 190, 140),
        warning: (235, 203, 139),
        error: (191, 97, 106),
        muted: (128, 128, 128),
        border: (56, 56, 56),
        selection: (56, 56, 56),
        selection_dim: (66, 66, 66),
        alternate_bg: (29, 29, 29),
        code_theme: "base16-ocean.dark",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "tokyonight-storm",
        background: (36, 40, 59),
        foreground: (192, 202, 245),
        primary: (125, 207, 255),
        secondary: (187, 154, 247),
        success: (49, 176, 138),
        warning: (224, 175, 104),
        error: (247, 118, 142),
        muted: (86, 95, 137),
        border: (52, 59, 88),
        selection: (52, 59, 88),
        selection_dim: (62, 69, 98),
        alternate_bg: (41, 45, 64),
        code_theme: "tokyonight-storm",
        link: (60, 140, 240),
    },
    ThemeData {
        name: "dracula-soft",
        background: (40, 42, 54),
        foreground: (248, 248, 242),
        primary: (120, 200, 220),
        secondary: (180, 140, 220),
        success: (80, 220, 120),
        warning: (240, 240, 140),
        error: (255, 100, 100),
        muted: (100, 115, 165),
        border: (68, 71, 90),
        selection: (68, 71, 90),
        selection_dim: (78, 81, 100),
        alternate_bg: (45, 47, 59),
        code_theme: "dracula",
        link: (60, 140, 240),
    },
];

impl Theme {
    pub fn from_name(name: &str) -> Option<Self> {
        THEMES.iter().find(|t| t.name == name).map(|t| t.to_theme())
    }

    pub fn dark() -> Self {
        THEMES[0].to_theme()
    }

    pub fn light() -> Self {
        THEMES[1].to_theme()
    }

    pub fn catppuccin_mocha() -> Self {
        THEMES[2].to_theme()
    }

    pub fn catppuccin_latte() -> Self {
        THEMES[3].to_theme()
    }

    pub fn dracula() -> Self {
        THEMES[4].to_theme()
    }

    pub fn gruvbox_dark() -> Self {
        THEMES[5].to_theme()
    }

    pub fn nord() -> Self {
        THEMES[6].to_theme()
    }

    pub fn tokyonight() -> Self {
        THEMES[7].to_theme()
    }

    pub fn monokai() -> Self {
        THEMES[8].to_theme()
    }

    pub fn solarized_dark() -> Self {
        THEMES[9].to_theme()
    }

    pub fn solarized_light() -> Self {
        THEMES[10].to_theme()
    }

    pub fn one_dark() -> Self {
        THEMES[11].to_theme()
    }

    pub fn github_dark() -> Self {
        THEMES[12].to_theme()
    }

    pub fn github_light() -> Self {
        THEMES[13].to_theme()
    }

    pub fn ayu_dark() -> Self {
        THEMES[14].to_theme()
    }

    pub fn material_dark() -> Self {
        THEMES[15].to_theme()
    }

    pub fn material_light() -> Self {
        THEMES[16].to_theme()
    }

    pub fn palenight() -> Self {
        THEMES[17].to_theme()
    }

    pub fn cobalt() -> Self {
        THEMES[18].to_theme()
    }

    pub fn vs_dark() -> Self {
        THEMES[19].to_theme()
    }

    pub fn high_contrast_dark() -> Self {
        THEMES[20].to_theme()
    }

    pub fn zenburn() -> Self {
        THEMES[21].to_theme()
    }

    pub fn rose_pine() -> Self {
        THEMES[22].to_theme()
    }

    pub fn kanagawa() -> Self {
        THEMES[23].to_theme()
    }

    pub fn everforest_dark() -> Self {
        THEMES[24].to_theme()
    }

    pub fn moonlight() -> Self {
        THEMES[25].to_theme()
    }

    pub fn night_owl() -> Self {
        THEMES[26].to_theme()
    }

    pub fn atom_one_dark() -> Self {
        THEMES[27].to_theme()
    }

    pub fn base16_default() -> Self {
        THEMES[28].to_theme()
    }

    pub fn tokyonight_storm() -> Self {
        THEMES[29].to_theme()
    }

    pub fn dracula_soft() -> Self {
        THEMES[30].to_theme()
    }

    pub fn is_dark(&self) -> bool {
        matches!(self.background, Color::Rgb(r, _, _) if r < 128)
    }

    pub fn default_style(&self) -> Style {
        Style::default().fg(self.foreground).bg(self.background)
    }

    pub fn dim_style(&self) -> Style {
        Style::default().fg(self.muted).bg(self.background)
    }

    pub fn highlight_style(&self) -> Style {
        Style::default().fg(self.primary).bg(self.selection)
    }

    pub fn selection_style(&self) -> Style {
        Style::default()
            .fg(self.primary)
            .bg(self.selection)
            .add_modifier(Modifier::BOLD)
            .add_modifier(Modifier::UNDERLINED)
    }

    pub fn alternate_bg_style(&self) -> Style {
        Style::default().bg(self.alternate_bg)
    }

    pub fn code_theme(&self) -> &'static str {
        self.code_theme
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

pub fn all_themes() -> Vec<Theme> {
    THEMES.iter().map(|t| t.to_theme()).collect()
}

pub fn find_theme(name: &str) -> Option<Theme> {
    Theme::from_name(name)
}

pub fn theme_names() -> Vec<String> {
    THEMES.iter().map(|t| t.name.to_string()).collect()
}
