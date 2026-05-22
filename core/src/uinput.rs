/// uinput HID write layer — creates and drives virtual input devices.
///
/// Opens /dev/uinput at daemon startup and holds the file descriptors open
/// for the entire daemon lifetime. No per-event open/close (reduces latency).

#[cfg(target_os = "linux")]
use std::fs::OpenOptions;
#[cfg(target_os = "linux")]
use std::os::unix::io::{IntoRawFd, RawFd};
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
#[cfg(target_os = "linux")]
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use crate::bitmask::{EventType, Peripheral};

#[cfg(not(target_os = "linux"))]
type RawFd = i32;

// ---------------------------------------------------------------------------
// Constants and libc bindings (Linux only)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
const UI_SET_EVBIT: libc::c_ulong = 0x40045564;
#[cfg(target_os = "linux")]
const UI_SET_KEYBIT: libc::c_ulong = 0x40045565;
#[cfg(target_os = "linux")]
const UI_SET_ABSBIT: libc::c_ulong = 0x40045566;
#[cfg(target_os = "linux")]
const UI_DEV_SETUP: libc::c_ulong = 0x405c5503;
#[cfg(target_os = "linux")]
const UI_DEV_CREATE: libc::c_ulong = 0x5501;
#[cfg(target_os = "linux")]
const UI_DEV_DESTROY: libc::c_ulong = 0x5502;

#[cfg(target_os = "linux")]
#[repr(C)]
struct uinput_setup {
    id: input_id,
    name: [libc::c_char; 80],
    ff_effects_max: u32,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct input_id {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct uinput_abs_setup {
    code: u16,
    absinfo: libc::input_absinfo,
}
#[cfg(target_os = "linux")]
const UI_ABS_SETUP: libc::c_ulong = 0x401c5504;

#[cfg(target_os = "linux")]
const EV_SYN: u16 = 0x00;
#[cfg(target_os = "linux")]
const EV_KEY: u16 = 0x01;
#[cfg(target_os = "linux")]
const EV_ABS: u16 = 0x03;
#[cfg(target_os = "linux")]
const EV_REP: u16 = 0x14;
#[cfg(target_os = "linux")]
const SYN_REPORT: u16 = 0;

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Global file descriptors for the 5 virtual devices.
/// Index 0-3: Joypads, 4: Keyboard.
/// -1 indicates the device is not opened/active.
static DEVICE_FDS: OnceLock<Mutex<[RawFd; 5]>> = OnceLock::new();

fn get_device_fds() -> &'static Mutex<[RawFd; 5]> {
    DEVICE_FDS.get_or_init(|| Mutex::new([-1; 5]))
}

fn device_fd(index: usize) -> RawFd {
    if index >= 5 { return -1; }
    get_device_fds().lock()[index]
}

// ---------------------------------------------------------------------------
// Public API called by bitmask.rs and lib.rs
// ---------------------------------------------------------------------------

/// Initialise uinput devices based on the current configuration.
/// Creates Joypads 1-4 and a Keyboard device if they have peripherals mapped.
pub fn open_all(config: &Arc<crate::bitmask::Config>) {
    let mut fds = get_device_fds().lock();
    
    // Close any existing devices (e.g. on reload)
    for fd in fds.iter_mut() {
        if *fd != -1 {
            #[cfg(target_os = "linux")]
            unsafe {
                libc::ioctl(*fd, UI_DEV_DESTROY as _);
                libc::close(*fd);
            }
            *fd = -1;
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = config;
        eprintln!("[gpionext] WARNING: uinput is only supported on Linux. HID events will be logged to console.");
    }

    #[cfg(target_os = "linux")]
    {
        // Open Joypads (0-3)
        for i in 0..4 {
            let peripherals = &config.device_peripherals[i];
            if !peripherals.is_empty() {
                fds[i] = create_joypad(i, peripherals);
            }
        }

        // Open Keyboard (4)
        let kbd_peripherals = &config.device_peripherals[4];
        if !kbd_peripherals.is_empty() {
            fds[4] = create_keyboard(kbd_peripherals);
        }
    }
}

/// Send a press event for a peripheral, then wait for release.
pub fn dispatch_press(peripheral: &Arc<Peripheral>, key_hold_delay_ms: u64) {
    if peripheral.is_pressed.swap(true, Ordering::SeqCst) {
        return;
    }

    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("[gpionext stub] PRESS: {} (device {})", peripheral.name, peripheral.device_index);
        if let EventType::Command { bash } = &peripheral.event_type {
            let cmd = bash.clone();
            if let Some(pool) = crate::bitmask::get_pool() {
                pool.spawn(move || {
                    eprintln!("[gpionext stub] COMMAND: {}", cmd);
                });
            }
        }
        // Block for Button/Axis to simulate hardware wait
        match &peripheral.event_type {
            EventType::Button { .. } | EventType::Axis { .. } => wait_for_release(peripheral),
            _ => {}
        }
        return;
    }

    #[cfg(target_os = "linux")]
    {
        let fd = device_fd(peripheral.device_index);

        match &peripheral.event_type {
            EventType::Button { evdev_code } => {
                write_event(fd, EV_KEY, *evdev_code as u16, 1);
                write_sync(fd);
                wait_for_release(peripheral);
            }
            EventType::Key { evdev_code } => {
                write_event(fd, EV_KEY, *evdev_code as u16, 1);
                write_sync(fd);

                // Start hold task
                let code = *evdev_code;
                let gen = peripheral.hold_generation.fetch_add(1, Ordering::SeqCst) + 1;
                let p = peripheral.clone();
                if let Some(pool) = crate::bitmask::get_pool() {
                    pool.spawn(move || {
                        std::thread::sleep(Duration::from_millis(key_hold_delay_ms));
                        let fd = device_fd(p.device_index);
                        while p.hold_generation.load(Ordering::Relaxed) == gen
                            && p.is_pressed.load(Ordering::Relaxed)
                        {
                            write_event(fd, EV_KEY, code as u16, 2);
                            write_sync(fd);
                            std::thread::sleep(Duration::from_millis(33));
                        }
                    });
                }
            }
            EventType::Axis { evdev_type, evdev_code, press_value } => {
                write_event(fd, *evdev_type as u16, *evdev_code as u16, *press_value);
                write_sync(fd);
                wait_for_release(peripheral);
            }
            EventType::Command { bash } => {
                let cmd = bash.clone();
                if let Some(pool) = crate::bitmask::get_pool() {
                    pool.spawn(move || {
                        for part in cmd.split("|||") {
                            let part = part.trim();
                            if part.is_empty() {
                                continue;
                            }
                            let _ = std::process::Command::new("/bin/bash")
                                .args(["-c", part])
                                .status();
                        }
                    });
                }
                peripheral.is_pressed.store(false, Ordering::Relaxed);
            }
        }
    }
}

/// Send a release event for a peripheral.
pub fn dispatch_release(peripheral: &Arc<Peripheral>) {
    if !peripheral.is_pressed.swap(false, Ordering::SeqCst) {
        return;
    }

    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("[gpionext stub] RELEASE: {}", peripheral.name);
        return;
    }

    #[cfg(target_os = "linux")]
    {
        let fd = device_fd(peripheral.device_index);
        match &peripheral.event_type {
            EventType::Button { evdev_code } => {
                write_event(fd, EV_KEY, *evdev_code as u16, 0);
                write_sync(fd);
            }
            EventType::Key { evdev_code } => {
                write_event(fd, EV_KEY, *evdev_code as u16, 0);
                write_sync(fd);
            }
            EventType::Axis { evdev_type, evdev_code, .. } => {
                write_event(fd, *evdev_type as u16, *evdev_code as u16, 0);
                write_sync(fd);
            }
            EventType::Command { .. } => {}
        }
    }
}

/// Close all open uinput file descriptors gracefully.
pub fn close_all() {
    let mut fds = get_device_fds().lock();
    for fd in fds.iter_mut() {
        if *fd != -1 {
            #[cfg(target_os = "linux")]
            unsafe {
                libc::ioctl(*fd, UI_DEV_DESTROY as _);
                libc::close(*fd);
            }
            *fd = -1;
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers (Linux only)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
fn create_joypad(index: usize, peripherals: &[Arc<Peripheral>]) -> RawFd {
    let fd = match open_uinput() {
        Ok(f) => f,
        Err(_) => return -1,
    };

    unsafe {
        libc::ioctl(fd, UI_SET_EVBIT as _, EV_KEY as libc::c_int);
        libc::ioctl(fd, UI_SET_EVBIT as _, EV_ABS as libc::c_int);

        for p in peripherals {
            match &p.event_type {
                EventType::Button { evdev_code } => {
                    libc::ioctl(fd, UI_SET_KEYBIT as _, *evdev_code as libc::c_int);
                }
                EventType::Axis { evdev_type, evdev_code, .. } => {
                    if *evdev_type == EV_ABS as u32 {
                        libc::ioctl(fd, UI_SET_ABSBIT as _, *evdev_code as libc::c_int);
                        let abs_setup = uinput_abs_setup {
                            code: *evdev_code as u16,
                            absinfo: libc::input_absinfo {
                                value: 0,
                                minimum: -255,
                                maximum: 255,
                                fuzz: 0,
                                flat: 15,
                                resolution: 0,
                            },
                        };
                        libc::ioctl(fd, UI_ABS_SETUP as _, &abs_setup);
                    } else if *evdev_type == EV_KEY as u32 {
                        libc::ioctl(fd, UI_SET_KEYBIT as _, *evdev_code as libc::c_int);
                    }
                }
                _ => {}
            }
        }

        let mut setup = uinput_setup {
            id: input_id {
                bustype: 0x0003, // BUS_USB
                vendor: 0x9999,
                product: 0x8888,
                version: 1,
            },
            name: [0; 80],
            ff_effects_max: 0,
        };
        let name_str = format!("GPIOnext Joypad {}\0", index + 1);
        let name_bytes = name_str.as_bytes();
        for (i, &b) in name_bytes.iter().enumerate().take(79) {
            setup.name[i] = b as libc::c_char;
        }

        libc::ioctl(fd, UI_DEV_SETUP as _, &setup);
        libc::ioctl(fd, UI_DEV_CREATE as _);
    }

    fd
}

#[cfg(target_os = "linux")]
fn create_keyboard(peripherals: &[Arc<Peripheral>]) -> RawFd {
    let fd = match open_uinput() {
        Ok(f) => f,
        Err(_) => return -1,
    };

    unsafe {
        libc::ioctl(fd, UI_SET_EVBIT as _, EV_KEY as libc::c_int);
        libc::ioctl(fd, UI_SET_EVBIT as _, EV_REP as libc::c_int);

        for p in peripherals {
            if let EventType::Key { evdev_code } = &p.event_type {
                libc::ioctl(fd, UI_SET_KEYBIT as _, *evdev_code as libc::c_int);
            }
        }

        let mut setup = uinput_setup {
            id: input_id {
                bustype: 0x0003, // BUS_USB
                vendor: 0x0001,
                product: 0x0001,
                version: 1,
            },
            name: [0; 80],
            ff_effects_max: 0,
        };
        let name_str = "GPIOnext Keyboard\0";
        let name_bytes = name_str.as_bytes();
        for (i, &b) in name_bytes.iter().enumerate().take(79) {
            setup.name[i] = b as libc::c_char;
        }

        libc::ioctl(fd, UI_DEV_SETUP as _, &setup);
        libc::ioctl(fd, UI_DEV_CREATE as _);
    }

    fd
}

#[cfg(target_os = "linux")]
fn open_uinput() -> Result<RawFd, std::io::Error> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open("/dev/uinput")?;
    
    Ok(file.into_raw_fd())
}

#[cfg(target_os = "linux")]
fn write_event(fd: RawFd, type_: u16, code: u16, value: i32) {
    if fd == -1 { return; }
    
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let ev = libc::input_event {
        time: libc::timeval {
            tv_sec: now.as_secs() as libc::time_t,
            tv_usec: now.subsec_micros() as libc::suseconds_t,
        },
        type_: type_,
        code: code,
        value: value,
    };

    unsafe {
        libc::write(
            fd,
            &ev as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::input_event>(),
        );
    }
}

#[cfg(target_os = "linux")]
fn write_sync(fd: RawFd) {
    write_event(fd, EV_SYN, SYN_REPORT, 0);
}

fn wait_for_release(peripheral: &Arc<Peripheral>) {
    while peripheral.is_pressed.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(10));
    }
}
