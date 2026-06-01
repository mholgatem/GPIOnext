use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
    Frame,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::config::GpioConfig;
use crate::constants::DEVICE_LIST;
use crate::ipc_client::{IpcClient, PinState};
use crate::ui::{
    modals::Modal,
    tabs::{
        devices::DevicesTab,
        i2c_settings::I2cSettingsTab,
        mappings::MappingsTab,
        presets_config::PresetsConfigTab,
        TabHint,
    },
    theme,
};

#[derive(Clone, Copy, PartialEq)]
pub enum TabIndex {
    Devices = 0,
    Mappings = 1,
    I2cSettings = 2,
    PresetsConfig = 3,
}

impl TabIndex {
    pub fn next(self) -> Self {
        match self {
            Self::Devices     => Self::Mappings,
            Self::Mappings    => Self::I2cSettings,
            Self::I2cSettings => Self::PresetsConfig,
            Self::PresetsConfig => Self::Devices,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Devices     => Self::PresetsConfig,
            Self::Mappings    => Self::Devices,
            Self::I2cSettings => Self::Mappings,
            Self::PresetsConfig => Self::I2cSettings,
        }
    }
}

pub struct App {
    pub should_quit: bool,
    pub tab: TabIndex,
    pub modal: Option<Modal>,
    pub config: GpioConfig,
    pub config_path: PathBuf,
    pub pin_state: Arc<Mutex<PinState>>,
    pub devices_tab: DevicesTab,
    pub mappings_tab: MappingsTab,
    pub i2c_settings_tab: I2cSettingsTab,
    pub presets_config_tab: PresetsConfigTab,
    /// Transient status line message; cleared on the next key press.
    pub status_msg: Option<String>,
}

impl App {
    pub fn new(config_path: PathBuf) -> Result<Self> {
        let config = if config_path.exists() {
            crate::config::load(&config_path)?
        } else {
            GpioConfig::default()
        };

        let pin_state = Arc::new(Mutex::new(PinState::default()));
        IpcClient::start(Arc::clone(&pin_state));

        let devices_tab       = DevicesTab::new(&config);
        let mappings_tab      = MappingsTab::new();
        let i2c_settings_tab  = I2cSettingsTab::new(&config);
        let presets_config_tab = PresetsConfigTab::new();

        Ok(Self {
            should_quit: false,
            tab: TabIndex::Devices,
            modal: None,
            config,
            config_path,
            pin_state,
            devices_tab,
            mappings_tab,
            i2c_settings_tab,
            presets_config_tab,
            status_msg: None,
        })
    }

    pub fn save_config(&mut self) -> Result<()> {
        crate::config::save(&self.config_path, &self.config)?;
        self.status_msg = Some("Saved.".into());
        Ok(())
    }

    pub fn tick(&mut self) {
        if let Some(Modal::PinCapture(ref mut cap)) = self.modal {
            cap.tick(&self.pin_state);
        }
        if matches!(&self.modal, Some(Modal::PinCapture(cap)) if cap.hold_complete()) {
            if let Some(Modal::PinCapture(cap)) = self.modal.take() {
                let held = cap.hold_pins.clone();
                let (next_modal, action) = (cap.on_capture)(held, &mut self.config);
                self.modal = next_modal;
                if let Some(a) = action {
                    self.apply_modal_action(a);
                }
            }
        }

        if self.tab == TabIndex::I2cSettings {
            self.i2c_settings_tab.tick();
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        self.status_msg = None;

        // Modal open → route all input there.
        if let Some(modal) = self.modal.take() {
            let (next_modal, action, quit) = modal.handle_key(key, &mut self.config);
            self.modal = next_modal;
            if let Some(a) = action {
                self.apply_modal_action(a);
            }
            return quit;
        }

        match key.code {
            // ── Global navigation ────────────────────────────────────────────
            KeyCode::Tab => {
                self.tab = self.tab.next();
            }
            KeyCode::BackTab => {
                self.tab = self.tab.prev();
            }
            KeyCode::Char('q') | KeyCode::Char('Q') => return true,
            KeyCode::Char('s') | KeyCode::Char('S') => {
                if let Err(e) = self.save_config() {
                    self.status_msg = Some(format!("Save failed: {e}"));
                }
            }

            // ── Devices tab: Enter or n both navigate to Mappings ─────────────
            KeyCode::Enter if self.tab == TabIndex::Devices => {
                let device = DEVICE_LIST[self.devices_tab.selected].to_owned();
                self.mappings_tab.set_filter(Some(device));
                self.tab = TabIndex::Mappings;
            }
            KeyCode::Char('n') | KeyCode::Char('N') if self.tab == TabIndex::Devices => {
                let device = DEVICE_LIST[self.devices_tab.selected].to_owned();
                self.mappings_tab.set_filter(Some(device));
                self.tab = TabIndex::Mappings;
            }

            // ── Delegate everything else to the active tab ───────────────────
            _ => {
                let modal = match self.tab {
                    TabIndex::Devices => self.devices_tab.handle_key(key, &mut self.config),
                    TabIndex::Mappings => self.mappings_tab.handle_key(key, &mut self.config),
                    TabIndex::I2cSettings => {
                        self.i2c_settings_tab.handle_key(key, &mut self.config)
                    }
                    TabIndex::PresetsConfig => {
                        self.presets_config_tab.handle_key(key, &mut self.config)
                    }
                };
                if let Some(m) = modal {
                    self.modal = Some(m);
                }
            }
        }
        false
    }

    pub fn apply_modal_action(&mut self, action: crate::ui::ModalAction) {
        use crate::ui::ModalAction::*;
        match action {
            Save => {
                if let Err(e) = self.save_config() {
                    self.status_msg = Some(format!("Save failed: {e}"));
                }
            }
            StatusMsg(msg) => {
                self.status_msg = Some(msg);
            }
            RefreshDevicesTab => {
                self.devices_tab = DevicesTab::new(&self.config);
            }
            RefreshMappingsTab(device) => {
                self.mappings_tab.set_filter(Some(device));
                self.tab = TabIndex::Mappings;
            }
            SetMappingsFilter(device) => {
                self.mappings_tab.set_filter(device);
            }
            DaemonAction(cmd) => {
                if let Err(e) = crate::init_sys::run_daemon_cmd(cmd) {
                    self.status_msg = Some(format!("Daemon error: {e}"));
                }
            }
        }
    }

    pub fn render(&mut self, f: &mut Frame) {
        let area = f.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // tab bar
                Constraint::Min(0),    // content
                Constraint::Length(1), // status / help bar
            ])
            .split(area);

        self.render_tabs(f, chunks[0]);
        self.render_active_tab(f, chunks[1]);
        self.render_status(f, chunks[2]);

        if let Some(modal) = &mut self.modal {
            modal.render(f, area, &self.pin_state);
        }
    }

    fn render_tabs(&self, f: &mut Frame, area: Rect) {
        let titles = vec![
            Line::from(" Devices "),
            Line::from(" Mappings "),
            Line::from(" I2C & Settings "),
            Line::from(" Presets & Config "),
        ];
        let tabs = Tabs::new(titles)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(theme::border_normal())
                    .title(Span::styled(" GPIOnext Config ", theme::tab_active())),
            )
            .select(self.tab as usize)
            .highlight_style(theme::tab_active())
            .divider(Span::styled(" | ", theme::hint_text()));
        f.render_widget(tabs, area);
    }

    fn render_active_tab(&mut self, f: &mut Frame, area: Rect) {
        match self.tab {
            TabIndex::Devices => self.devices_tab.render(f, area, &self.config),
            TabIndex::Mappings => self.mappings_tab.render(f, area, &self.config),
            TabIndex::I2cSettings => {
                self.i2c_settings_tab.render(f, area, &self.config, &self.pin_state)
            }
            TabIndex::PresetsConfig => self.presets_config_tab.render(f, area, &self.config),
        }
    }

    fn render_status(&self, f: &mut Frame, area: Rect) {
        let (text, style) = if let Some(msg) = &self.status_msg {
            if msg.starts_with("Save failed") || msg.starts_with("Daemon error") {
                (msg.as_str(), theme::status_err())
            } else {
                (msg.as_str(), theme::status_ok())
            }
        } else {
            (self.tab_hint(), theme::hint_text())
        };

        let para = Paragraph::new(Line::from(Span::styled(
            format!(" {text}"),
            style,
        )))
        .style(Style::default().bg(ratatui::style::Color::Black));
        f.render_widget(para, area);
    }

    fn tab_hint(&self) -> &str {
        match self.tab {
            TabIndex::Devices     => self.devices_tab.hint(),
            TabIndex::Mappings    => self.mappings_tab.hint(),
            TabIndex::I2cSettings => self.i2c_settings_tab.hint(),
            TabIndex::PresetsConfig => self.presets_config_tab.hint(),
        }
    }
}
