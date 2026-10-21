use ratatui::{
    style::{Color, Style},
    text::Span,
};

// Monokai accents with a warm, subdued foreground instead of pure white.
pub const BACKGROUND: Color = Color::Rgb(39, 40, 34);
pub const FOREGROUND: Color = Color::Rgb(214, 211, 196);
pub const MUTED: Color = Color::Rgb(163, 159, 140);
pub const BORDER: Color = Color::Rgb(88, 89, 77);
pub const PINK: Color = Color::Rgb(249, 38, 114);
pub const GREEN: Color = Color::Rgb(166, 226, 46);
pub const CYAN: Color = Color::Rgb(102, 217, 239);
pub const YELLOW: Color = Color::Rgb(230, 219, 116);
pub const ORANGE: Color = Color::Rgb(253, 151, 31);
pub const PURPLE: Color = Color::Rgb(174, 129, 255);

pub fn fg(color: Color) -> Style {
    Style::default().fg(color)
}

pub fn base() -> Style {
    fg(FOREGROUND).bg(BACKGROUND)
}

pub fn selection(active: bool) -> Style {
    // Only change the background, retaining each span's semantic foreground.
    Style::default().bg(if active {
        Color::Rgb(65, 70, 54)
    } else {
        Color::Rgb(51, 54, 46)
    })
}

pub fn span(text: impl Into<String>, color: Color) -> Span<'static> {
    Span::styled(text.into(), fg(color))
}

pub fn notice(text: &str) -> Style {
    let lower = text.to_ascii_lowercase();
    fg(
        if ["failed", "error", "cannot"]
            .iter()
            .any(|word| lower.contains(word))
        {
            PINK
        } else if ["refresh", "loading", "checking", "waiting"]
            .iter()
            .any(|word| lower.contains(word))
        {
            ORANGE
        } else {
            MUTED
        },
    )
}
