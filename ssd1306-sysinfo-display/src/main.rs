use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

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
use ssd1306_driver_rs::{
    DisplayInterface, DisplaySize, I2cInterface, Ssd1306, VccState, command::DEFAULT_I2C_ADDRESS,
};

const DISPLAY_OFF_POLL_INTERVAL: Duration = Duration::from_millis(100);
// How long a message from msg.txt stays on screen.
const MESSAGE_DURATION: Duration = Duration::from_secs(3);
// Delay between system information refreshes.
const REFRESH_INTERVAL: Duration = Duration::from_millis(500);
// How often to re-query values that rarely change (IP address, disk usage).
// Time, CPU load and memory are still refreshed on every loop.
const SLOW_REFRESH_INTERVAL: Duration = Duration::from_secs(30);

fn run_shell(cmd: &str) -> String {
    Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default()
}

// 1-minute load average: the first field of /proc/loadavg. Read directly
// rather than via `cut` to avoid spawning a shell every refresh.
fn read_load_average() -> String {
    fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|s| s.split(' ').next().map(|field| field.trim().to_string()))
        .unwrap_or_default()
}

fn sleep_checking_display_off<I>(
    display: &mut Ssd1306<I>,
    total: Duration,
    display_off_file: &Path,
) -> Result<bool, I::Error>
where
    I: DisplayInterface,
{
    let mut waited: Duration = Duration::ZERO;
    while waited < total {
        if display_off_file.is_file() {
            display.clear();
            display.flush()?;
            return Ok(true);
        }
        let step: Duration = DISPLAY_OFF_POLL_INTERVAL.min(total - waited);
        thread::sleep(step);
        waited += step;
    }
    Ok(false)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("--- SSD1306 SysInfo Display ---\n");

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
        fs::remove_file(&display_off_file)?;
    }

    // Create the I2C interface and the SSD1306 driver. Change the size
    // here to match your display.
    let i2c: I2c = I2c::new()?;
    let interface: I2cInterface<I2c> = I2cInterface::new(i2c, DEFAULT_I2C_ADDRESS);
    let mut display: Ssd1306<I2cInterface<I2c>> =
        Ssd1306::new_without_reset(interface, DisplaySize::Size128x32);
    display.init(VccState::SwitchCap, &mut Delay)?;

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

    loop {
        display.clear();

        // Keep the display off while the flag file exists
        if display_off_file.is_file() {
            if !display_off_status {
                display.flush()?;
                display_off_status = true;
            }
            thread::sleep(DISPLAY_OFF_POLL_INTERVAL);
            continue;
        }
        display_off_status = false;

        // Show a one-off message from msg.txt, if present
        if msg_file.is_file() {
            let content: String = fs::read_to_string(&msg_file)
                .unwrap_or_default()
                .lines()
                .next()
                .unwrap_or("")
                .trim_end_matches(['\r', '\t', ' '])
                .to_string();

            let font = if content.len() >= 20 {
                message_font_small
            } else {
                message_font_normal
            };

            Text::with_text_style(&content, Point::new(width / 2, height / 2), font, centered)
                .draw(&mut display)?;
            display.flush()?;

            fs::remove_file(&msg_file)?;
            display_off_status |=
                sleep_checking_display_off(&mut display, MESSAGE_DURATION, &display_off_file)?;
            continue;
        }

        // Monitoring information
        let time_str: String = run_shell("date +\"%H:%M\"");
        let cpu: String = read_load_average();
        let mem_usage: String = run_shell(
            "free -m | awk 'NR==2{printf \"Mem: %s / %s MB  %.2f%%\", $3,$2,$3*100/$2 }'",
        );
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
            Text::with_text_style(text, position, status_font, top_left).draw(&mut display)?;
        }

        display.flush()?;
        display_off_status |=
            sleep_checking_display_off(&mut display, REFRESH_INTERVAL, &display_off_file)?;
    }
}
