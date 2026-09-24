use std::env;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use embedded_graphics::{
    mono_font::{
        MonoTextStyle,
        ascii::{FONT_5X7, FONT_6X10, FONT_7X13},
    },
    pixelcolor::BinaryColor,
    prelude::*,
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};
use rppal::hal::Delay;
use rppal::i2c::I2c;
use signal_hook::{consts::TERM_SIGNALS, flag};
use ssd1306_driver_rs::{
    DisplayInterface, DisplaySize, I2cInterface, Ssd1306, VccState, command::DEFAULT_I2C_ADDRESS,
};

const DISPLAY_OFF_POLL_INTERVAL: Duration = Duration::from_millis(100);
// How long a message from msg.txt stays on screen.
const MESSAGE_DURATION: Duration = Duration::from_secs(3);
// Delay between system information refreshes.
const REFRESH_INTERVAL: Duration = Duration::from_millis(500);
// How often to re-query the IP address and disk usage, which rarely change.
const SLOW_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
// How often to try reinitialising the display after an I2C error.
const I2C_RETRY_INTERVAL: Duration = Duration::from_secs(2);

fn run_shell(cmd: &str) -> String {
    Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default()
}

// 1-minute load average (the first field of /proc/loadavg).
fn read_load_average() -> String {
    fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|s| s.split(' ').next().map(|field| field.trim().to_string()))
        .unwrap_or_default()
}

// POSIX, but not declared by the `libc` crate.
unsafe extern "C" {
    fn tzset();
}

// Local time as HH:MM, like `date +%H:%M`.
fn read_local_time() -> String {
    // SAFETY: `localtime_r` gets valid pointers to locals; the other calls
    // have no preconditions.
    unsafe {
        // Pick up timezone changes made while running.
        tzset();
        let now: libc::time_t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return String::new();
        }
        format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
    }
}

// Memory usage from /proc/meminfo, as "Mem: <used> / <total> MB  <percent>%".
// Used is total minus available, in whole MB, matching `free -m` (procps 4.x).
fn read_memory_usage() -> String {
    let Ok(meminfo) = fs::read_to_string("/proc/meminfo") else {
        return String::new();
    };
    // Value in kB of a line like "MemTotal:  948304 kB".
    let field = |name: &str| -> Option<u64> {
        meminfo.lines().find_map(|line| {
            line.strip_prefix(name)?
                .strip_prefix(':')?
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
    };
    let (Some(total_kb), Some(available_kb)) = (field("MemTotal"), field("MemAvailable")) else {
        return String::new();
    };
    let total: u64 = total_kb / 1024;
    let used: u64 = total_kb.saturating_sub(available_kb) / 1024;
    let percent: f64 = used as f64 * 100.0 / total as f64;
    format!("Mem: {used} / {total} MB  {percent:.2}%")
}

// Remove `path`, logging any error. A missing file counts as removed.
fn try_remove_file(path: &Path) -> bool {
    match fs::remove_file(path) {
        Ok(()) => true,
        Err(e) if e.kind() == io::ErrorKind::NotFound => true,
        Err(e) => {
            eprintln!("Could not remove {}: {e}", path.display());
            false
        }
    }
}

fn modified_time(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

// Blank the panel and put it to sleep (saves power, avoids burn-in).
fn turn_display_off<I>(display: &mut Ssd1306<I>) -> Result<(), I::Error>
where
    I: DisplayInterface,
{
    display.clear();
    display.flush_changed()?;
    display.off()
}

fn sleep_checking_display_off<I>(
    display: &mut Ssd1306<I>,
    total: Duration,
    display_off_file: &Path,
    shutdown: &AtomicBool,
) -> Result<bool, I::Error>
where
    I: DisplayInterface,
{
    let mut waited: Duration = Duration::ZERO;
    while waited < total && !shutdown.load(Ordering::Relaxed) {
        if display_off_file.is_file() {
            turn_display_off(display)?;
            return Ok(true);
        }
        let step: Duration = DISPLAY_OFF_POLL_INTERVAL.min(total - waited);
        thread::sleep(step);
        waited += step;
    }
    Ok(false)
}

fn main() -> Result<(), Box<dyn Error>> {
    println!("--- SSD1306 SysInfo Display ---\n");

    // Exit cleanly on SIGTERM/SIGINT/SIGQUIT so the display can be turned
    // off. A second signal exits immediately.
    let shutdown: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    for &signal in TERM_SIGNALS {
        flag::register_conditional_shutdown(signal, 1, Arc::clone(&shutdown))?;
        flag::register(signal, Arc::clone(&shutdown))?;
    }

    // Look for the flag and message files next to the binary.
    let base_path: PathBuf = env::current_exe()?
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let display_off_file: PathBuf = base_path.join("displayoff");
    let msg_file: PathBuf = base_path.join("msg.txt");

    println!(
        "Display off file to check for: {}\n",
        display_off_file.display()
    );
    println!("Message file to check for: {}\n", msg_file.display());

    // Always show the display on boot
    if display_off_file.is_file() {
        try_remove_file(&display_off_file);
    }

    // Create the I2C interface and the SSD1306 driver. Change the size
    // here to match your display.
    let i2c: I2c = I2c::new()?;
    let interface: I2cInterface<I2c> = I2cInterface::new(i2c, DEFAULT_I2C_ADDRESS);
    let mut display: Ssd1306<I2cInterface<I2c>> =
        Ssd1306::new_without_reset(interface, DisplaySize::Size128x32);
    display.init(VccState::SwitchCap, &mut Delay)?;

    while let Err(err) = run(&mut display, &display_off_file, &msg_file, &shutdown) {
        // On an I2C error (e.g. the panel lost power), reinitialise until it
        // responds. Other errors are fatal.
        let Some(i2c_err) = err.downcast_ref::<rppal::i2c::Error>() else {
            return Err(err);
        };
        eprintln!("I2C error: {i2c_err}; retrying every {I2C_RETRY_INTERVAL:?}");
        loop {
            thread::sleep(I2C_RETRY_INTERVAL);
            if shutdown.load(Ordering::Relaxed) {
                // The display isn't responding, so leave it as is.
                println!("Shutting down");
                return Ok(());
            }
            if display.init(VccState::SwitchCap, &mut Delay).is_ok() {
                eprintln!("Display reinitialised");
                break;
            }
        }
    }

    // Don't leave the last frame on screen.
    println!("Shutting down");
    turn_display_off(&mut display)?;
    Ok(())
}

// Draw frames until shutdown is requested or an error occurs.
fn run(
    display: &mut Ssd1306<I2cInterface<I2c>>,
    display_off_file: &Path,
    msg_file: &Path,
    shutdown: &AtomicBool,
) -> Result<(), Box<dyn Error>> {
    display.clear();
    display.flush()?;

    let width: i32 = display.width() as i32;
    let height: i32 = display.height() as i32;

    let status_font = MonoTextStyle::new(&FONT_5X7, BinaryColor::On);
    let message_font_normal = MonoTextStyle::new(&FONT_7X13, BinaryColor::On);
    let message_font_small = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);

    let top_left = TextStyleBuilder::new()
        .alignment(Alignment::Left)
        .baseline(Baseline::Top)
        .build();
    let centered = TextStyleBuilder::new()
        .alignment(Alignment::Center)
        .baseline(Baseline::Middle)
        .build();

    let mut display_off_status: bool = false;

    let mut ip: String = String::new();
    let mut disk: String = String::new();
    let mut last_slow_refresh: Option<Instant> = None;
    // Modification time of a shown msg.txt that couldn't be removed, so it
    // isn't shown again.
    let mut undeletable_msg_modified: Option<SystemTime> = None;

    while !shutdown.load(Ordering::Relaxed) {
        display.clear();

        // Keep the display off while the flag file exists
        if display_off_file.is_file() {
            if !display_off_status {
                turn_display_off(display)?;
                display_off_status = true;
            }
            thread::sleep(DISPLAY_OFF_POLL_INTERVAL);
            continue;
        }
        if display_off_status {
            display.on()?;
            display_off_status = false;
        }

        // Show a one-off message from msg.txt, if present and not already shown
        let msg_already_shown: bool =
            undeletable_msg_modified.is_some_and(|shown| modified_time(msg_file) == Some(shown));
        if msg_file.is_file() && !msg_already_shown {
            let content: String = fs::read_to_string(msg_file)
                .unwrap_or_default()
                .lines()
                .next()
                .unwrap_or("")
                .trim_end_matches(['\r', '\t', ' '])
                .to_string();

            // The fonts are monospace, so count characters, not bytes.
            let font = if content.chars().count() >= 20 {
                message_font_small
            } else {
                message_font_normal
            };

            Text::with_text_style(&content, Point::new(width / 2, height / 2), font, centered)
                .draw(display)?;
            display.flush_changed()?;

            undeletable_msg_modified = if try_remove_file(msg_file) {
                None
            } else {
                modified_time(msg_file)
            };
            display_off_status |=
                sleep_checking_display_off(display, MESSAGE_DURATION, display_off_file, shutdown)?;
            continue;
        }

        // Monitoring information
        let time_str: String = read_local_time();
        let cpu: String = read_load_average();
        let mem_usage: String = read_memory_usage();
        // Also retry while the IP is empty (e.g. network not up yet at boot).
        if ip.is_empty() || last_slow_refresh.is_none_or(|t| t.elapsed() >= SLOW_REFRESH_INTERVAL) {
            ip = run_shell("hostname -I | cut -d' ' -f1");
            disk =
                run_shell("df -h / | awk '$NF==\"/\"{printf \"Disk: %d / %d GB  %s\", $3,$2,$5}'");
            last_slow_refresh = Some(Instant::now());
        }

        let lines: [(&str, Point); 5] = [
            (&time_str, Point::new(width - 30, 0)),
            (&format!("IP: {ip}"), Point::new(0, 0)),
            (&format!("CPU load: {cpu}"), Point::new(0, 8)),
            (&mem_usage, Point::new(0, 16)),
            (&disk, Point::new(0, 25)),
        ];
        for (text, position) in lines {
            Text::with_text_style(text, position, status_font, top_left).draw(display)?;
        }

        // Only rows that changed are sent (often none).
        display.flush_changed()?;
        display_off_status |=
            sleep_checking_display_off(display, REFRESH_INTERVAL, display_off_file, shutdown)?;
    }
    Ok(())
}
