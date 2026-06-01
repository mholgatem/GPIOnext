/// theme.rs — Vaporwave / retro-hacker color palette for GPIOnext config UI.
///
/// Cyan and magenta as primary accents, dark-purple selection background.
/// Evokes an 80s/90s hacker terminal aesthetic.

use ratatui::style::{Color, Modifier, Style};

pub const CYAN: Color    = Color::Cyan;
pub const MAGENTA: Color = Color::Magenta;
pub const DIM: Color     = Color::DarkGray;
/// Neon pressed-pin highlight (bright green-cyan).
pub const PRESSED: Color = Color::Rgb(0, 255, 180);
/// Dark purple selection background.
pub const SEL_BG: Color  = Color::Rgb(40, 0, 60);

pub fn border_normal() -> Style  { Style::default().fg(CYAN) }
pub fn border_focused() -> Style { Style::default().fg(MAGENTA) }
pub fn tab_active() -> Style     { Style::default().fg(MAGENTA).add_modifier(Modifier::BOLD) }
pub fn header() -> Style         { Style::default().fg(CYAN).add_modifier(Modifier::BOLD) }
pub fn selected_row() -> Style   { Style::default().bg(SEL_BG).add_modifier(Modifier::BOLD) }
pub fn selected_btn() -> Style   { Style::default().fg(MAGENTA).add_modifier(Modifier::BOLD) }
pub fn hint_text() -> Style      { Style::default().fg(DIM) }
pub fn status_ok() -> Style      { Style::default().fg(Color::LightGreen) }
pub fn status_err() -> Style     { Style::default().fg(Color::Red) }
pub fn pressed_pin() -> Style    { Style::default().fg(PRESSED).add_modifier(Modifier::BOLD) }
pub fn unmapped_pin() -> Style   { Style::default().fg(DIM) }
/// Input field text (e.g. "> value_" prompt).
pub fn input_text() -> Style     { Style::default().fg(CYAN).add_modifier(Modifier::BOLD) }
/// Highlighted/selected list item (dark bg + bold).
pub fn list_selected() -> Style  { Style::default().bg(SEL_BG).fg(Color::White).add_modifier(Modifier::BOLD) }
