use serde::de::Error as SerdeError;
use serde::{Deserialize, Deserializer, Serialize};

use clauditty_config_derive::ConfigDeserialize;

use crate::display::color::{CellRgb, Rgb};

#[derive(ConfigDeserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Colors {
    pub primary: PrimaryColors,
    pub cursor: InvertedCellColors,
    pub vi_mode_cursor: InvertedCellColors,
    pub selection: InvertedCellColors,
    pub normal: NormalColors,
    pub bright: BrightColors,
    pub dim: Option<DimColors>,
    pub indexed_colors: Vec<IndexedColor>,
    pub search: SearchColors,
    pub line_indicator: LineIndicatorColors,
    pub hints: HintColors,
    pub transparent_background_colors: bool,
    pub draw_bold_text_with_bright_colors: bool,
    footer_bar: BarColors,
}

impl Colors {
    pub fn footer_bar_foreground(&self) -> Rgb {
        self.footer_bar.foreground.unwrap_or(self.primary.background)
    }

    pub fn footer_bar_background(&self) -> Rgb {
        self.footer_bar.background.unwrap_or(self.primary.foreground)
    }
}

#[derive(ConfigDeserialize, Serialize, Copy, Clone, Default, Debug, PartialEq, Eq)]
pub struct LineIndicatorColors {
    pub foreground: Option<Rgb>,
    pub background: Option<Rgb>,
}

#[derive(ConfigDeserialize, Serialize, Default, Copy, Clone, Debug, PartialEq, Eq)]
pub struct HintColors {
    pub start: HintStartColors,
    pub end: HintEndColors,
}

#[derive(ConfigDeserialize, Serialize, Copy, Clone, Debug, PartialEq, Eq)]
pub struct HintStartColors {
    pub foreground: CellRgb,
    pub background: CellRgb,
}

impl Default for HintStartColors {
    fn default() -> Self {
        Self {
            foreground: CellRgb::Rgb(Rgb::new(0x01, 0x04, 0x09)),
            background: CellRgb::Rgb(Rgb::new(0xd2, 0x99, 0x22)),
        }
    }
}

#[derive(ConfigDeserialize, Serialize, Copy, Clone, Debug, PartialEq, Eq)]
pub struct HintEndColors {
    pub foreground: CellRgb,
    pub background: CellRgb,
}

impl Default for HintEndColors {
    fn default() -> Self {
        Self {
            foreground: CellRgb::Rgb(Rgb::new(0x01, 0x04, 0x09)),
            background: CellRgb::Rgb(Rgb::new(0xff, 0x7b, 0x72)),
        }
    }
}

#[derive(Deserialize, Serialize, Copy, Clone, Default, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IndexedColor {
    pub color: Rgb,

    index: ColorIndex,
}

impl IndexedColor {
    #[inline]
    pub fn index(&self) -> u8 {
        self.index.0
    }
}

#[derive(Serialize, Copy, Clone, Default, Debug, PartialEq, Eq)]
struct ColorIndex(u8);

impl<'de> Deserialize<'de> for ColorIndex {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let index = u8::deserialize(deserializer)?;

        if index < 16 {
            Err(SerdeError::custom(
                "Config error: indexed_color's index is {}, but a value bigger than 15 was \
                 expected; ignoring setting",
            ))
        } else {
            Ok(Self(index))
        }
    }
}

#[derive(ConfigDeserialize, Serialize, Debug, Copy, Clone, PartialEq, Eq)]
pub struct InvertedCellColors {
    #[config(alias = "text")]
    pub foreground: CellRgb,
    #[config(alias = "cursor")]
    pub background: CellRgb,
}

impl Default for InvertedCellColors {
    fn default() -> Self {
        Self { foreground: CellRgb::CellBackground, background: CellRgb::CellForeground }
    }
}

#[derive(ConfigDeserialize, Serialize, Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct SearchColors {
    pub focused_match: FocusedMatchColors,
    pub matches: MatchColors,
}

#[derive(ConfigDeserialize, Serialize, Debug, Copy, Clone, PartialEq, Eq)]
pub struct FocusedMatchColors {
    pub foreground: CellRgb,
    pub background: CellRgb,
}

impl Default for FocusedMatchColors {
    fn default() -> Self {
        Self {
            background: CellRgb::Rgb(Rgb::new(0xd2, 0x99, 0x22)),
            foreground: CellRgb::Rgb(Rgb::new(0x01, 0x04, 0x09)),
        }
    }
}

#[derive(ConfigDeserialize, Serialize, Debug, Copy, Clone, PartialEq, Eq)]
pub struct MatchColors {
    pub foreground: CellRgb,
    pub background: CellRgb,
}

impl Default for MatchColors {
    fn default() -> Self {
        Self {
            background: CellRgb::Rgb(Rgb::new(0xff, 0x7b, 0x72)),
            foreground: CellRgb::Rgb(Rgb::new(0x01, 0x04, 0x09)),
        }
    }
}

#[derive(ConfigDeserialize, Serialize, Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct BarColors {
    foreground: Option<Rgb>,
    background: Option<Rgb>,
}

#[derive(ConfigDeserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct PrimaryColors {
    pub foreground: Rgb,
    pub background: Rgb,
    pub bright_foreground: Option<Rgb>,
    pub dim_foreground: Option<Rgb>,
}

// Default colors are the GitHub Dark Default theme.
impl Default for PrimaryColors {
    fn default() -> Self {
        PrimaryColors {
            background: Rgb::new(0x01, 0x04, 0x09),
            foreground: Rgb::new(0xe6, 0xed, 0xf3),
            bright_foreground: Default::default(),
            dim_foreground: Default::default(),
        }
    }
}

#[derive(ConfigDeserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct NormalColors {
    pub black: Rgb,
    pub red: Rgb,
    pub green: Rgb,
    pub yellow: Rgb,
    pub blue: Rgb,
    pub magenta: Rgb,
    pub cyan: Rgb,
    pub white: Rgb,
}

impl Default for NormalColors {
    fn default() -> Self {
        NormalColors {
            black: Rgb::new(0x48, 0x4f, 0x58),
            red: Rgb::new(0xff, 0x7b, 0x72),
            green: Rgb::new(0x3f, 0xb9, 0x50),
            yellow: Rgb::new(0xd2, 0x99, 0x22),
            blue: Rgb::new(0x58, 0xa6, 0xff),
            magenta: Rgb::new(0xbc, 0x8c, 0xff),
            cyan: Rgb::new(0x39, 0xc5, 0xcf),
            white: Rgb::new(0xb1, 0xba, 0xc4),
        }
    }
}

#[derive(ConfigDeserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct BrightColors {
    pub black: Rgb,
    pub red: Rgb,
    pub green: Rgb,
    pub yellow: Rgb,
    pub blue: Rgb,
    pub magenta: Rgb,
    pub cyan: Rgb,
    pub white: Rgb,
}

impl Default for BrightColors {
    fn default() -> Self {
        BrightColors {
            black: Rgb::new(0x6e, 0x76, 0x81),
            red: Rgb::new(0xff, 0xa1, 0x98),
            green: Rgb::new(0x56, 0xd3, 0x64),
            yellow: Rgb::new(0xe3, 0xb3, 0x41),
            blue: Rgb::new(0x79, 0xc0, 0xff),
            magenta: Rgb::new(0xd2, 0xa8, 0xff),
            cyan: Rgb::new(0x56, 0xd4, 0xdd),
            white: Rgb::new(0xff, 0xff, 0xff),
        }
    }
}

#[derive(ConfigDeserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct DimColors {
    pub black: Rgb,
    pub red: Rgb,
    pub green: Rgb,
    pub yellow: Rgb,
    pub blue: Rgb,
    pub magenta: Rgb,
    pub cyan: Rgb,
    pub white: Rgb,
}

impl Default for DimColors {
    fn default() -> Self {
        // Generated with builtin clauditty's color dimming function.
        DimColors {
            black: Rgb::new(0x2f, 0x34, 0x3a),
            red: Rgb::new(0xa8, 0x51, 0x4b),
            green: Rgb::new(0x29, 0x7a, 0x34),
            yellow: Rgb::new(0x8a, 0x64, 0x16),
            blue: Rgb::new(0x3a, 0x6d, 0xa8),
            magenta: Rgb::new(0x7c, 0x5c, 0xa8),
            cyan: Rgb::new(0x25, 0x82, 0x88),
            white: Rgb::new(0x74, 0x7a, 0x81),
        }
    }
}
