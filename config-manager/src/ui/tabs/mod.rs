pub mod devices;
pub mod mappings;
pub mod i2c_settings;
pub mod presets_config;

/// Trait for per-tab dynamic footer hint text.
/// Each tab returns a context-sensitive string describing the active hotkeys.
pub trait TabHint {
    fn hint(&self) -> &str;
}
