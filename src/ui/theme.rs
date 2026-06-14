//! Mamba's visual identity: a selectable palette behind a small style API,
//! mirroring `~/Kata`'s theme system so the two tools feel like siblings.
//!
//! All colors live in one [`Palette`] struct; named palettes are `const`s and
//! the active one is chosen once at startup from the `MAMBA_THEME` environment
//! variable (default [`DEFAULT_THEME`]). Widgets never name a `Color`
//! directly — they call [`palette`] or the helpers below — so restyling Mamba
//! is a local change here.
//!
//! Available `MAMBA_THEME` values: `venom` (dark emerald, default),
//! `midnight` (cool dark), `slate` (muted dark).

use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, BorderType, Borders};

/// The palette used when `MAMBA_THEME` is unset or unrecognized.
pub const DEFAULT_THEME: &str = "venom";

/// A complete set of theme colors. Roles, not raw hues, so a widget can ask
/// for `accent` without caring which palette is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Framing chrome behind panes, the top bar, and the status bar.
    pub crust: Color,
    /// Panel background — the surface a pane is drawn on.
    pub base: Color,
    /// Raised interactive surface: selection bar, key chips.
    pub surface0: Color,
    /// Subtle chrome: unfocused borders, separators.
    pub surface1: Color,
    /// Primary body text.
    pub text: Color,
    /// Secondary text: metadata, hints, unfocused titles.
    pub subtext: Color,
    /// Tertiary text: faint labels, placeholders.
    pub overlay: Color,
    /// Brand accent: focus border, wordmark, selection edge.
    pub accent: Color,
    /// Secondary accent for titles and section headers.
    pub accent_deep: Color,
    /// Foreground used on top of a colored badge.
    pub badge_fg: Color,
    /// Success, and the Easy difficulty.
    pub green: Color,
    /// In-progress / caution, and the Medium difficulty.
    pub yellow: Color,
    /// Failure, errors, and the Hard difficulty.
    pub red: Color,
    /// Informational accent (paths, links).
    pub blue: Color,
    /// Cool accent for hints and secondary detail.
    pub teal: Color,
}

/// Dark emerald — "venom": near-black ground, viper-green accent. The default,
/// echoing the black-mamba namesake.
pub const VENOM: Palette = Palette {
    crust: Color::Rgb(0x10, 0x14, 0x12),
    base: Color::Rgb(0x16, 0x1c, 0x19),
    surface0: Color::Rgb(0x20, 0x2a, 0x25),
    surface1: Color::Rgb(0x30, 0x3e, 0x37),
    text: Color::Rgb(0xd6, 0xe4, 0xdc),
    subtext: Color::Rgb(0x96, 0xa9, 0x9f),
    overlay: Color::Rgb(0x64, 0x77, 0x6d),
    accent: Color::Rgb(0x35, 0xd0, 0x8a),
    accent_deep: Color::Rgb(0x5a, 0xe6, 0xa6),
    badge_fg: Color::Rgb(0x10, 0x14, 0x12),
    green: Color::Rgb(0x6c, 0xd6, 0x8f),
    yellow: Color::Rgb(0xe6, 0xc8, 0x6a),
    red: Color::Rgb(0xe2, 0x7d, 0x7d),
    blue: Color::Rgb(0x7a, 0xb6, 0xe6),
    teal: Color::Rgb(0x5f, 0xc8, 0xc0),
};

/// Cool dark — "midnight": deep blue-grey ground with the brand green kept as
/// the accent so Mamba stays recognizable across themes.
pub const MIDNIGHT: Palette = Palette {
    crust: Color::Rgb(0x14, 0x16, 0x1e),
    base: Color::Rgb(0x1a, 0x1b, 0x26),
    surface0: Color::Rgb(0x24, 0x28, 0x3b),
    surface1: Color::Rgb(0x41, 0x48, 0x68),
    text: Color::Rgb(0xc0, 0xca, 0xf5),
    subtext: Color::Rgb(0x9a, 0xa5, 0xce),
    overlay: Color::Rgb(0x56, 0x5f, 0x89),
    accent: Color::Rgb(0x4f, 0xd6, 0x9c),
    accent_deep: Color::Rgb(0x73, 0xe6, 0xb4),
    badge_fg: Color::Rgb(0x14, 0x16, 0x1e),
    green: Color::Rgb(0x9e, 0xce, 0x6a),
    yellow: Color::Rgb(0xe0, 0xaf, 0x68),
    red: Color::Rgb(0xf7, 0x76, 0x8e),
    blue: Color::Rgb(0x7a, 0xa2, 0xf7),
    teal: Color::Rgb(0x7d, 0xcf, 0xff),
};

/// Muted dark — "slate": warm grey ground, gentle pastel accents, the lowest
/// contrast of the set; the most forgiving for long sessions.
pub const SLATE: Palette = Palette {
    crust: Color::Rgb(0x1d, 0x20, 0x21),
    base: Color::Rgb(0x28, 0x28, 0x28),
    surface0: Color::Rgb(0x3c, 0x38, 0x36),
    surface1: Color::Rgb(0x50, 0x49, 0x45),
    text: Color::Rgb(0xeb, 0xdb, 0xb2),
    subtext: Color::Rgb(0xa8, 0x99, 0x84),
    overlay: Color::Rgb(0x7c, 0x6f, 0x64),
    accent: Color::Rgb(0x8e, 0xc0, 0x7c),
    accent_deep: Color::Rgb(0xb8, 0xbb, 0x26),
    badge_fg: Color::Rgb(0x1d, 0x20, 0x21),
    green: Color::Rgb(0xb8, 0xbb, 0x26),
    yellow: Color::Rgb(0xfa, 0xbd, 0x2f),
    red: Color::Rgb(0xfb, 0x49, 0x34),
    blue: Color::Rgb(0x83, 0xa5, 0x98),
    teal: Color::Rgb(0x8e, 0xc0, 0x7c),
};

/// Resolves a `MAMBA_THEME` value (case-insensitive) to its palette, falling
/// back to the default for unknown names.
pub fn palette_by_name(name: &str) -> &'static Palette {
    match name.trim().to_ascii_lowercase().as_str() {
        "midnight" => &MIDNIGHT,
        "slate" => &SLATE,
        _ => &VENOM,
    }
}

/// The active palette, selected once from `MAMBA_THEME`.
pub fn palette() -> &'static Palette {
    static ACTIVE: OnceLock<&'static Palette> = OnceLock::new();
    ACTIVE.get_or_init(|| {
        let name = std::env::var("MAMBA_THEME").unwrap_or_else(|_| DEFAULT_THEME.to_string());
        palette_by_name(&name)
    })
}

// ---------------------------------------------------------------------------
// Reusable styles (all read the active palette)
// ---------------------------------------------------------------------------

/// Style of the framing chrome painted behind panes (canvas, bars).
pub fn canvas() -> Style {
    Style::default().bg(palette().crust)
}

/// Base text style inside panes (ink on the page).
pub fn body() -> Style {
    let p = palette();
    Style::default().fg(p.text).bg(p.base)
}

/// A rounded pane. The focused pane gets an accent border + title; unfocused
/// panes recede into the chrome with a faint border.
pub fn pane_block(title: impl std::fmt::Display, focused: bool) -> Block<'static> {
    let p = palette();
    let (border, title_style) = if focused {
        (
            Style::default().fg(p.accent),
            Style::default()
                .fg(p.accent_deep)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            Style::default().fg(p.surface1),
            Style::default().fg(p.subtext),
        )
    };
    Block::default()
        .title(Span::styled(format!(" {title} "), title_style))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border)
        .style(Style::default().bg(p.base))
}

/// Selection bar in lists: a raised surface with an accent-bold label.
pub fn selection() -> Style {
    let p = palette();
    Style::default()
        .bg(p.surface0)
        .fg(p.accent_deep)
        .add_modifier(Modifier::BOLD)
}

/// A pill-style badge (` CORRECT `, ` WRONG `, …): badge_fg on a color.
pub fn badge(bg: Color) -> Style {
    Style::default()
        .fg(palette().badge_fg)
        .bg(bg)
        .add_modifier(Modifier::BOLD)
}

/// One `key → action` hint pair for the status bar: an accent key chip then a
/// muted action label.
pub fn key_hint(key: &str, action: &str) -> [Span<'static>; 3] {
    let p = palette();
    [
        Span::styled(
            format!(" {key} "),
            Style::default().fg(p.accent_deep).bg(p.surface0),
        ),
        Span::styled(format!(" {action}"), Style::default().fg(p.subtext)),
        Span::styled("  ", Style::default().fg(p.subtext)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_theme_falls_back_to_default() {
        assert_eq!(palette_by_name("nonsense"), &VENOM);
        assert_eq!(palette_by_name("VENOM"), &VENOM);
        assert_eq!(palette_by_name("midnight"), &MIDNIGHT);
        assert_eq!(palette_by_name(" Slate "), &SLATE);
    }
}
