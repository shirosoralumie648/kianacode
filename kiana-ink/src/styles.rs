use ratatui::style::Color as RatatuiColor;
use taffy::prelude::*;

#[derive(Debug, Clone)]
pub enum Color {
    Rgb(u8, u8, u8),
    Ansi(u8),
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
}

impl Color {
    pub fn to_ratatui(&self) -> RatatuiColor {
        match self {
            Color::Rgb(r, g, b) => RatatuiColor::Rgb(*r, *g, *b),
            Color::Ansi(n) => RatatuiColor::Indexed(*n),
            Color::Black => RatatuiColor::Black,
            Color::Red => RatatuiColor::Red,
            Color::Green => RatatuiColor::Green,
            Color::Yellow => RatatuiColor::Yellow,
            Color::Blue => RatatuiColor::Blue,
            Color::Magenta => RatatuiColor::Magenta,
            Color::Cyan => RatatuiColor::Cyan,
            Color::White => RatatuiColor::White,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TextStyle {
    pub color: Option<Color>,
    pub bg_color: Option<Color>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

#[derive(Debug, Clone)]
pub struct BoxStyle {
    pub flex_direction: FlexDirection,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_wrap: FlexWrap,
    pub align_items: Option<AlignItems>,
    pub justify_content: Option<JustifyContent>,
    pub width: Dimension,
    pub height: Dimension,
    pub min_width: Dimension,
    pub min_height: Dimension,
    pub max_width: Dimension,
    pub max_height: Dimension,
    pub margin: Rect<LengthPercentageAuto>,
    pub padding: Rect<LengthPercentage>,
    pub border: Rect<LengthPercentage>,
    pub gap: Size<LengthPercentage>,
    pub position: Position,
    pub display: Display,
}

impl Default for BoxStyle {
    fn default() -> Self {
        Self {
            flex_direction: FlexDirection::Row,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_wrap: FlexWrap::NoWrap,
            align_items: None,
            justify_content: None,
            width: Dimension::Auto,
            height: Dimension::Auto,
            min_width: Dimension::Auto,
            min_height: Dimension::Auto,
            max_width: Dimension::Auto,
            max_height: Dimension::Auto,
            margin: Rect::zero(),
            padding: Rect::zero(),
            border: Rect::zero(),
            gap: Size::zero(),
            position: Position::Relative,
            display: Display::Flex,
        }
    }
}

impl BoxStyle {
    pub fn to_taffy_style(&self) -> Style {
        Style {
            display: self.display,
            position: self.position,
            flex_direction: self.flex_direction,
            flex_wrap: self.flex_wrap,
            flex_grow: self.flex_grow,
            flex_shrink: self.flex_shrink,
            align_items: self.align_items,
            justify_content: self.justify_content,
            size: Size {
                width: self.width,
                height: self.height,
            },
            min_size: Size {
                width: self.min_width,
                height: self.min_height,
            },
            max_size: Size {
                width: self.max_width,
                height: self.max_height,
            },
            margin: self.margin,
            padding: self.padding,
            border: self.border,
            gap: self.gap,
            ..Default::default()
        }
    }
}
