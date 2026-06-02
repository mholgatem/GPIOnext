/// theme.rs — Vaporwave / retro-hacker color palette for GPIOnext config UI.
///
/// Cyan and magenta as primary accents, orange for chrome, dark-purple selection bg.

use ratatui::style::{Color, Modifier, Style};

pub const CYAN: Color       = Color::Cyan;
pub const MAGENTA: Color    = Color::Magenta;
pub const DIM: Color        = Color::DarkGray;
/// Vaporwave orange — used for the tab bar chrome and footer hints.
pub const ORANGE: Color     = Color::Rgb(255, 140, 0);
/// Slightly dimmed orange for footer hint text.
pub const ORANGE_DIM: Color = Color::Rgb(160, 80, 0);
/// Neon pressed-pin highlight (bright green-cyan).
pub const PRESSED: Color    = Color::Rgb(0, 255, 180);
/// Dark purple selection background.
pub const SEL_BG: Color     = Color::Rgb(40, 0, 60);

pub fn border_normal() -> Style  { Style::default().fg(CYAN) }
pub fn border_focused() -> Style { Style::default().fg(MAGENTA) }
/// Tab bar border (orange chrome).
pub fn tab_border() -> Style     { Style::default().fg(ORANGE) }
/// Selected/highlighted tab label (cyan bold).
pub fn tab_active() -> Style     { Style::default().fg(CYAN).add_modifier(Modifier::BOLD) }
pub fn header() -> Style         { Style::default().fg(CYAN).add_modifier(Modifier::BOLD) }
pub fn selected_row() -> Style   { Style::default().bg(SEL_BG).add_modifier(Modifier::BOLD) }
pub fn selected_btn() -> Style   { Style::default().fg(MAGENTA).add_modifier(Modifier::BOLD) }
pub fn hint_text() -> Style      { Style::default().fg(DIM) }
/// Footer command legend — orange, dimmed.
pub fn footer_hint() -> Style    { Style::default().fg(ORANGE_DIM) }
pub fn status_ok() -> Style      { Style::default().fg(Color::LightGreen) }
pub fn status_err() -> Style     { Style::default().fg(Color::Red) }
pub fn pressed_pin() -> Style    { Style::default().fg(PRESSED).add_modifier(Modifier::BOLD) }
pub fn unmapped_pin() -> Style   { Style::default().fg(DIM) }
/// Input field text (e.g. "> value_" prompt).
pub fn input_text() -> Style     { Style::default().fg(CYAN).add_modifier(Modifier::BOLD) }
/// Highlighted/selected list item (dark bg + bold white).
pub fn list_selected() -> Style  { Style::default().bg(SEL_BG).fg(Color::White).add_modifier(Modifier::BOLD) }
