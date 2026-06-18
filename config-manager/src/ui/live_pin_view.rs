/// live_pin_view.rs — Real-time GPIO pin state monitor widget.
///
/// Renders as a table with 4 columns: pin number, pin label, state (●/○),
/// and mapped action. Reads from the shared PinState updated by IpcClient.
///
/// BOARD pins (0-63) come first; I2C virtual pins (64+) follow.
/// Pressed pins are highlighted in neon green-cyan; unmapped pins are dimmed.
/// Supports stateful scrolling via TableState when `focused` is true.

use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Row, Table, TableState},
    Frame,
};
use std::sync::{Arc, Mutex};

use crate::{
    config::GpioConfig,
    constants::{available_pins, board_to_gpio},
    ipc_client::PinState,
    ui::theme,
};

/// Render the live pin monitor table.
///
/// `scroll` is a mutable `TableState` used for keyboard scrolling — the caller
/// owns it and advances it with ↑/↓ keys when `focused` is true.
/// `focused` controls the border highlight color.
pub fn render(
    f: &mut Frame,
    area: Rect,
    cfg: &GpioConfig,
    pin_state: &Arc<Mutex<PinState>>,
    scroll: &mut TableState,
    focused: bool,
) {
    let state = pin_state.lock().unwrap();
    let mapping = build_mapping_label_map(cfg);
    let gpio_map = board_to_gpio();

    let mut rows: Vec<Row> = Vec::new();

    // Physical BOARD pins — sorted numerically
    let mut sorted_pins = available_pins().to_vec();
    sorted_pins.sort_unstable();
    for &board_pin in &sorted_pins {
        let pressed = state.is_pressed(board_pin);
        let gpio_label = gpio_map
            .get(&board_pin)
            .map(|g| format!("GPIO{g}"))
            .unwrap_or_else(|| format!("BOARD{board_pin}"));
        let mapped = mapping
            .get(&(board_pin as u8))
            .map(|s| s.as_str())
            .unwrap_or("unmapped");

        rows.push(pin_row(board_pin.to_string(), gpio_label, pressed, mapped));
    }

    // I2C virtual pins from config
    for i2c_chip in &cfg.i2c.mcp23017 {
        let addr = parse_addr(&i2c_chip.address);
        for port in ['A', 'B'] {
            for bit in 0..8u8 {
                let label = format!("i2c-0x{addr:02X}-{port}{bit}");
                let vpin_id = mcp23017_vpin(addr, port, bit);
                let pressed = state.is_pressed(vpin_id);
                let mapped = mapping.get(&vpin_id).map(|s| s.as_str()).unwrap_or("unmapped");
                rows.push(pin_row(vpin_id.to_string(), label, pressed, mapped));
            }
        }
    }
    for i2c_chip in &cfg.i2c.ads1115 {
        let addr = parse_addr(&i2c_chip.address);
        for ch in 0..4u8 {
            let label = format!("i2c-0x{addr:02X}-ch{ch}");
            let vpin_id = ads1115_vpin(addr, ch);
            let pressed = state.is_pressed(vpin_id);
            let mapped = mapping.get(&vpin_id).map(|s| s.as_str()).unwrap_or("unmapped");
            rows.push(pin_row(vpin_id.to_string(), label, pressed, mapped));
        }
    }
    for i2c_chip in &cfg.i2c.pcf8574 {
        let addr = parse_addr(&i2c_chip.address);
        for pin in 0..8u8 {
            let label = format!("i2c-0x{addr:02X}-P{pin}");
            let vpin_id = pcf8574_vpin(addr, pin);
            let pressed = state.is_pressed(vpin_id);
            let mapped = mapping.get(&vpin_id).map(|s| s.as_str()).unwrap_or("unmapped");
            rows.push(pin_row(vpin_id.to_string(), label, pressed, mapped));
        }
    }

    let conn_indicator = if state.connected {
        " ● Connected "
    } else {
        " ○ Daemon not running "
    };
    let conn_style = if state.connected {
        Style::default().fg(Color::LightGreen)
    } else {
        Style::default().fg(Color::Yellow)
    };

    // Build title with F3 label
    let title = Line::from(vec![
        Span::raw(" Live Pin Monitor [F3]  "),
        Span::styled(conn_indicator, conn_style),
    ]);

    let border_style = if focused {
        theme::border_focused()
    } else {
        theme::border_normal()
    };

    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Percentage(30),
            Constraint::Length(3),
            Constraint::Min(0),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("Pin"),
            Cell::from("Label"),
            Cell::from(""),
            Cell::from("Mapped action"),
        ])
        .style(theme::header()),
    )
    .block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style),
    )
    .highlight_style(theme::selected_row());

    f.render_stateful_widget(table, area, scroll);
}

fn pin_row(pin: String, label: String, pressed: bool, mapped: &str) -> Row {
    let state_symbol = if pressed { "●" } else { "○" };
    let unmapped = mapped == "unmapped";

    let row_style = if pressed {
        theme::pressed_pin()
    } else if unmapped {
        theme::unmapped_pin()
    } else {
        Style::default()
    };

    Row::new(vec![
        Cell::from(pin),
        Cell::from(label),
        Cell::from(state_symbol),
        Cell::from(mapped.to_owned()),
    ])
    .style(row_style)
}

/// Build a map from virtual pin number → "Device / Action" label.
fn build_mapping_label_map(cfg: &GpioConfig) -> std::collections::HashMap<u8, String> {
    let mut map = std::collections::HashMap::new();
    for row in &cfg.devices {
        if let Some(vpin) = crate::config::pin_to_vpin(&row.pins) {
            map.insert(vpin, format!("{} / {}", row.device, row.name));
        }
    }
    map
}

/// Count total rows (BOARD pins + I2C virtual pins) for scroll clamping.
pub fn total_rows(cfg: &GpioConfig) -> usize {
    available_pins().len()
        + cfg.i2c.mcp23017.len() * 16
        + cfg.i2c.ads1115.len() * 4
        + cfg.i2c.pcf8574.len() * 8
}

// ---------------------------------------------------------------------------
// Virtual pin number helpers
// ---------------------------------------------------------------------------

fn parse_addr(s: &str) -> u8 {
    u8::from_str_radix(s.trim_start_matches("0x").trim_start_matches("0X"), 16).unwrap_or(0x20)
}

fn mcp23017_vpin(addr: u8, port: char, bit: u8) -> u8 {
    let port_offset: u8 = if port == 'A' { 0 } else { 8 };
    let addr_offset = addr.saturating_sub(0x20);
    64u8.saturating_add(addr_offset.saturating_mul(16))
        .saturating_add(port_offset)
        .saturating_add(bit)
}

fn ads1115_vpin(addr: u8, ch: u8) -> u8 {
    let addr_offset = addr.saturating_sub(0x48);
    128u8.saturating_add(addr_offset.saturating_mul(4)).saturating_add(ch)
}

fn pcf8574_vpin(addr: u8, pin: u8) -> u8 {
    let addr_offset = addr.saturating_sub(0x20);
    192u8.saturating_add(addr_offset.saturating_mul(8)).saturating_add(pin)
}
