#[cfg(feature = "i2c")]
use i2cdev::core::I2CDevice;
/// i2c device drivers: MCP23017 GPIO expander, PCF8574 GPIO expander, and ADS1115 ADC.
///
/// Both chips implement the `IoPin` trait so the rest of the system
/// (bitmask engine, config UI) treats i2c pins identically to physical GPIO pins.

#[cfg(feature = "i2c")]
use i2cdev::linux::LinuxI2CDevice;

#[cfg(feature = "i2c")]
use crate::bitmask;
#[cfg(feature = "i2c")]
use crate::gpio::{board_to_bcm, find_gpio_chip};
#[cfg(feature = "i2c")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "i2c")]
use std::sync::Arc;
#[cfg(feature = "i2c")]
use std::time::Duration;

// ---------------------------------------------------------------------------
// Constants: MCP23017 (IOCON.BANK=0)
// ---------------------------------------------------------------------------

const MCP_IODIRA: u8 = 0x00;
const MCP_IODIRB: u8 = 0x01;
const MCP_GPINTENA: u8 = 0x04;
const MCP_GPINTENB: u8 = 0x05;
const MCP_DEFVALA: u8 = 0x06;
const MCP_DEFVALB: u8 = 0x07;
const MCP_INTCONA: u8 = 0x08;
const MCP_INTCONB: u8 = 0x09;
const MCP_IOCON: u8 = 0x0A;
const MCP_GPPUA: u8 = 0x0C;
const MCP_GPPUB: u8 = 0x0D;
const MCP_GPIOA: u8 = 0x12;
const MCP_GPIOB: u8 = 0x13;

// ---------------------------------------------------------------------------
// Constants: ADS1115
// ---------------------------------------------------------------------------

const ADS_REG_CONVERSION: u8 = 0x00;
const ADS_REG_CONFIG: u8 = 0x01;

// Config register bits
const ADS_OS_SINGLE: u16 = 0x8000;
const ADS_PGA_4_096V: u16 = 0x0200;
const ADS_MODE_CONTINUOUS: u16 = 0x0000;
const ADS_DR_250SPS: u16 = 0x00A0;
const ADS_COMP_QUE_DISABLE: u16 = 0x0003;

// ---------------------------------------------------------------------------
// IoPin trait — common interface for GPIO and i2c pins
// ---------------------------------------------------------------------------

pub trait IoPin: Send + Sync {
    fn pin_id(&self) -> String;
    fn is_pressed(&self) -> bool;
    fn read_analog(&self) -> i16;
    fn virtual_pin(&self) -> u8;
}

// ---------------------------------------------------------------------------
// MCP23017 GPIO expander
// ---------------------------------------------------------------------------

pub struct Mcp23017Pin {
    pub bus: u8,
    pub address: u8,
    pub port: char,
    pub bit: u8,
    pub vpin: u8,
}

impl Mcp23017Pin {
    pub fn new(bus: u8, address: u8, port: char, bit: u8) -> Self {
        let chip_offset = (address.saturating_sub(0x20)) as u8 * 16;
        let port_offset: u8 = if port == 'A' { 0 } else { 8 };
        let vpin = 64 + chip_offset + port_offset + bit;
        Mcp23017Pin {
            bus,
            address,
            port,
            bit,
            vpin,
        }
    }
}

impl IoPin for Mcp23017Pin {
    fn pin_id(&self) -> String {
        format!("i2c-0x{:02X}-{}{}", self.address, self.port, self.bit)
    }

    fn is_pressed(&self) -> bool {
        #[cfg(feature = "i2c")]
        {
            if let Ok(mut dev) =
                LinuxI2CDevice::new(format!("/dev/i2c-{}", self.bus), self.address as u16)
            {
                let reg = if self.port == 'A' {
                    MCP_GPIOA
                } else {
                    MCP_GPIOB
                };
                if let Ok(byte) = dev.smbus_read_byte_data(reg) {
                    return (byte >> self.bit) & 1 == 0; // active-low with pullups
                }
            }
        }
        false
    }

    fn read_analog(&self) -> i16 {
        0
    }
    fn virtual_pin(&self) -> u8 {
        self.vpin
    }
}

pub struct Mcp23017 {
    pub bus: u8,
    pub address: u8,
    pub int_pin: Option<u8>,
    pub pins: Vec<Mcp23017Pin>,
}

impl Mcp23017 {
    pub fn new(bus: u8, address: u8, int_pin: Option<u8>) -> Result<Self, I2cError> {
        #[cfg(feature = "i2c")]
        {
            let mut dev =
                LinuxI2CDevice::new(format!("/dev/i2c-{bus}"), address as u16).map_err(|e| {
                    I2cError::BusOpenFailed {
                        bus,
                        reason: e.to_string(),
                    }
                })?;

            // Initialize MCP23017
            // 1. Configure IOCON: MIRROR=1 (bit 6), ODR=0 (bit 2), INTPOL=0 (bit 1, active-low)
            dev.smbus_write_byte_data(MCP_IOCON, 0x40)
                .map_err(|e| I2cError::IoError {
                    address,
                    reason: e.to_string(),
                })?;

            // 2. All pins as inputs
            dev.smbus_write_byte_data(MCP_IODIRA, 0xFF)
                .map_err(|_| I2cError::IoError {
                    address,
                    reason: "IODIRA".into(),
                })?;
            dev.smbus_write_byte_data(MCP_IODIRB, 0xFF)
                .map_err(|_| I2cError::IoError {
                    address,
                    reason: "IODIRB".into(),
                })?;

            // 3. Enable all pullups
            dev.smbus_write_byte_data(MCP_GPPUA, 0xFF)
                .map_err(|_| I2cError::IoError {
                    address,
                    reason: "GPPUA".into(),
                })?;
            dev.smbus_write_byte_data(MCP_GPPUB, 0xFF)
                .map_err(|_| I2cError::IoError {
                    address,
                    reason: "GPPUB".into(),
                })?;

            // 4. Enable interrupt-on-change for all pins
            dev.smbus_write_byte_data(MCP_GPINTENA, 0xFF)
                .map_err(|_| I2cError::IoError {
                    address,
                    reason: "GPINTENA".into(),
                })?;
            dev.smbus_write_byte_data(MCP_GPINTENB, 0xFF)
                .map_err(|_| I2cError::IoError {
                    address,
                    reason: "GPINTENB".into(),
                })?;
        }

        let pins: Vec<Mcp23017Pin> = (0u8..8)
            .map(|b| Mcp23017Pin::new(bus, address, 'A', b))
            .chain((0u8..8).map(|b| Mcp23017Pin::new(bus, address, 'B', b)))
            .collect();

        Ok(Mcp23017 {
            bus,
            address,
            int_pin,
            pins,
        })
    }

    pub fn scan(_bus: u8) -> Vec<u8> {
        let mut _found = Vec::new();
        #[cfg(feature = "i2c")]
        {
            for addr in 0x20u8..=0x27 {
                if let Ok(mut dev) = LinuxI2CDevice::new(format!("/dev/i2c-{_bus}"), addr as u16) {
                    if dev.smbus_read_byte_data(MCP_IODIRA).is_ok() {
                        _found.push(addr);
                    }
                }
            }
        }
        _found
    }

    #[cfg(feature = "i2c")]
    pub fn poll(&self, running: Arc<AtomicBool>) {
        let mut dev =
            match LinuxI2CDevice::new(format!("/dev/i2c-{}", self.bus), self.address as u16) {
                Ok(d) => d,
                Err(_) => return,
            };

        // Initialize interrupt line if requested
        let mut int_request = if let Some(board_pin) = self.int_pin {
            if let Some(bcm) = board_to_bcm(board_pin) {
                if let Some(chip_path) = find_gpio_chip() {
                    gpiocdev::Request::builder()
                        .on_chip(&chip_path)
                        .with_consumer("gpionext-i2c-int")
                        .with_line(bcm)
                        .as_input()
                        .with_bias(gpiocdev::line::Bias::PullUp)
                        .with_edge_detection(gpiocdev::line::EdgeDetection::BothEdges)
                        .request()
                        .ok()
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let mut last_state: u16 = 0xFFFF;

        loop {
            if !running.load(Ordering::Relaxed) {
                break;
            }

            // Wait for interrupt or sleep
            if let Some(req) = &mut int_request {
                let _ = req.wait_edge_event(Duration::from_millis(100));
            } else {
                std::thread::sleep(Duration::from_millis(1));
            }

            // Batched read: read both GPIOA and GPIOB in one 16-bit transaction
            if let Ok(state) = dev.smbus_read_word_data(MCP_GPIOA) {
                if state != last_state {
                    for bit in 0..16 {
                        let pressed = (state >> bit) & 1 == 0;
                        let last_pressed = (last_state >> bit) & 1 == 0;
                        if pressed != last_pressed {
                            let vpin = 64 + (self.address.saturating_sub(0x20)) * 16 + bit as u8;
                            if pressed {
                                bitmask::set_pin(vpin);
                                bitmask::on_pin_press(vpin);
                            } else {
                                bitmask::on_pin_release(vpin);
                            }
                        }
                    }
                    last_state = state;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PCF8574 GPIO expander
// ---------------------------------------------------------------------------

pub struct Pcf8574Pin {
    pub bus: u8,
    pub address: u8,
    pub bit: u8,
    pub vpin: u8,
}

impl Pcf8574Pin {
    pub fn new(bus: u8, address: u8, bit: u8) -> Self {
        let chip_offset = (address.saturating_sub(0x20)) * 8;
        let vpin = 192 + chip_offset + bit;
        Pcf8574Pin {
            bus,
            address,
            bit,
            vpin,
        }
    }
}

impl IoPin for Pcf8574Pin {
    fn pin_id(&self) -> String {
        format!("i2c-0x{:02X}-P{}", self.address, self.bit)
    }

    fn is_pressed(&self) -> bool {
        #[cfg(feature = "i2c")]
        {
            if let Ok(mut dev) =
                LinuxI2CDevice::new(format!("/dev/i2c-{}", self.bus), self.address as u16)
            {
                if let Ok(byte) = dev.smbus_read_byte() {
                    return (byte >> self.bit) & 1 == 0; // active-low with pullups
                }
            }
        }
        false
    }

    fn read_analog(&self) -> i16 {
        0
    }
    fn virtual_pin(&self) -> u8 {
        self.vpin
    }
}

pub struct Pcf8574 {
    pub bus: u8,
    pub address: u8,
    pub int_pin: Option<u8>,
    pub pins: Vec<Pcf8574Pin>,
}

impl Pcf8574 {
    pub fn new(bus: u8, address: u8, int_pin: Option<u8>) -> Result<Self, I2cError> {
        #[cfg(feature = "i2c")]
        {
            let mut dev =
                LinuxI2CDevice::new(format!("/dev/i2c-{bus}"), address as u16).map_err(|e| {
                    I2cError::BusOpenFailed {
                        bus,
                        reason: e.to_string(),
                    }
                })?;

            // PCF8574 pins are quasi-bidirectional. Writing 1 releases each pin
            // so external switches/pull-ups can drive the byte state directly.
            dev.smbus_write_byte(0xFF).map_err(|e| I2cError::IoError {
                address,
                reason: e.to_string(),
            })?;
        }

        let pins: Vec<Pcf8574Pin> = (0u8..8).map(|b| Pcf8574Pin::new(bus, address, b)).collect();
        Ok(Pcf8574 {
            bus,
            address,
            int_pin,
            pins,
        })
    }

    pub fn scan(_bus: u8) -> Vec<u8> {
        let mut _found = Vec::new();
        #[cfg(feature = "i2c")]
        {
            for addr in 0x20u8..=0x27 {
                if let Ok(mut dev) = LinuxI2CDevice::new(format!("/dev/i2c-{_bus}"), addr as u16) {
                    if dev.smbus_read_byte().is_ok() {
                        _found.push(addr);
                    }
                }
            }
        }
        _found
    }

    #[cfg(feature = "i2c")]
    pub fn poll(&self, running: Arc<AtomicBool>) {
        let mut dev =
            match LinuxI2CDevice::new(format!("/dev/i2c-{}", self.bus), self.address as u16) {
                Ok(d) => d,
                Err(_) => return,
            };

        let _ = dev.smbus_write_byte(0xFF);

        // Initialize interrupt line if requested
        let mut int_request = if let Some(board_pin) = self.int_pin {
            if let Some(bcm) = board_to_bcm(board_pin) {
                if let Some(chip_path) = find_gpio_chip() {
                    gpiocdev::Request::builder()
                        .on_chip(&chip_path)
                        .with_consumer("gpionext-pcf8574-int")
                        .with_line(bcm)
                        .as_input()
                        .with_bias(gpiocdev::line::Bias::PullUp)
                        .with_edge_detection(gpiocdev::line::EdgeDetection::BothEdges)
                        .request()
                        .ok()
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let mut last_state: u8 = 0xFF;

        loop {
            if !running.load(Ordering::Relaxed) {
                break;
            }

            // Wait for interrupt or sleep. Either way, read the current byte
            // directly from the PCF8574 instead of using register addresses.
            if let Some(req) = &mut int_request {
                let _ = req.wait_edge_event(Duration::from_millis(100));
            } else {
                std::thread::sleep(Duration::from_millis(1));
            }

            if let Ok(state) = dev.smbus_read_byte() {
                if state != last_state {
                    for bit in 0..8 {
                        let pressed = (state >> bit) & 1 == 0;
                        let last_pressed = (last_state >> bit) & 1 == 0;
                        if pressed != last_pressed {
                            let vpin = 192 + (self.address.saturating_sub(0x20)) * 8 + bit as u8;
                            if pressed {
                                bitmask::set_pin(vpin);
                                bitmask::on_pin_press(vpin);
                            } else {
                                bitmask::on_pin_release(vpin);
                            }
                        }
                    }
                    last_state = state;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ADS1115 ADC
// ---------------------------------------------------------------------------

pub struct Ads1115Channel {
    pub bus: u8,
    pub address: u8,
    pub channel: u8,
    pub vpin: u8,
    pub dead_zone: i16,
}

impl Ads1115Channel {
    pub fn new(bus: u8, address: u8, channel: u8, dead_zone: i16) -> Self {
        let chip_offset = (address.saturating_sub(0x48)) * 4;
        let vpin = 128 + chip_offset + channel;
        Ads1115Channel {
            bus,
            address,
            channel,
            vpin,
            dead_zone,
        }
    }

    pub fn scale_to_axis(raw: i16) -> i32 {
        ((raw as i32) * 255) / 32767
    }
}

impl IoPin for Ads1115Channel {
    fn pin_id(&self) -> String {
        format!("i2c-0x{:02X}-ch{}", self.address, self.channel)
    }

    fn is_pressed(&self) -> bool {
        self.read_analog().abs() > self.dead_zone
    }

    fn read_analog(&self) -> i16 {
        #[cfg(feature = "i2c")]
        {
            if let Ok(mut dev) =
                LinuxI2CDevice::new(format!("/dev/i2c-{}", self.bus), self.address as u16)
            {
                let mux = (0x04 + self.channel as u16) << 12;
                let config = ADS_OS_SINGLE
                    | mux
                    | ADS_PGA_4_096V
                    | ADS_MODE_CONTINUOUS
                    | ADS_DR_250SPS
                    | ADS_COMP_QUE_DISABLE;
                let config_swapped = config.swap_bytes();
                if dev
                    .smbus_write_word_data(ADS_REG_CONFIG, config_swapped)
                    .is_ok()
                {
                    std::thread::sleep(Duration::from_millis(5));
                    if let Ok(val) = dev.smbus_read_word_data(ADS_REG_CONVERSION) {
                        return i16::from_be(val as i16);
                    }
                }
            }
        }
        0
    }

    fn virtual_pin(&self) -> u8 {
        self.vpin
    }
}

pub struct Ads1115 {
    pub bus: u8,
    pub address: u8,
    pub channels: Vec<Ads1115Channel>,
}

impl Ads1115 {
    pub fn new(bus: u8, address: u8) -> Result<Self, I2cError> {
        let channels = (0..4)
            .map(|ch| Ads1115Channel::new(bus, address, ch, 2048))
            .collect();
        Ok(Ads1115 {
            bus,
            address,
            channels,
        })
    }

    pub fn scan(_bus: u8) -> Vec<u8> {
        let mut _found = Vec::new();
        #[cfg(feature = "i2c")]
        {
            for addr in 0x48u8..=0x4B {
                if let Ok(mut dev) = LinuxI2CDevice::new(format!("/dev/i2c-{_bus}"), addr as u16) {
                    if dev.smbus_read_word_data(ADS_REG_CONFIG).is_ok() {
                        _found.push(addr);
                    }
                }
            }
        }
        _found
    }

    #[cfg(feature = "i2c")]
    pub fn poll(&self, running: Arc<AtomicBool>) {
        loop {
            if !running.load(Ordering::Relaxed) {
                break;
            }
            for ch in &self.channels {
                let val = ch.read_analog();
                let pressed = val.abs() > ch.dead_zone;
                let vpin = ch.vpin;

                // For analog axes, we'll update the bitmask so live view works,
                // but real axis movement is handled elsewhere or via combos.
                if pressed {
                    bitmask::set_pin(vpin);
                } else {
                    bitmask::clear_pin(vpin);
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum I2cError {
    BusOpenFailed { bus: u8, reason: String },
    DeviceNotFound { bus: u8, address: u8 },
    IoError { address: u8, reason: String },
}

impl std::fmt::Display for I2cError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            I2cError::BusOpenFailed { bus, reason } =>
                write!(f, "Cannot open /dev/i2c-{bus}: {reason}. Run 'raspi-config' → Interface Options → I2C."),
            I2cError::DeviceNotFound { bus, address } =>
                write!(f, "No i2c device at address 0x{address:02X} on bus {bus}. Check wiring and 'i2cdetect -y {bus}'."),
            I2cError::IoError { address, reason } =>
                write!(f, "i2c I/O error on device 0x{address:02X}: {reason}"),
        }
    }
}
