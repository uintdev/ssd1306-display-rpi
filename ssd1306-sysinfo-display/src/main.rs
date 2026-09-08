use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

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

fn run_shell(cmd: &str) -> String {
    Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
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

    // The original resolved paths relative to the script's own directory;
    // do the same relative to the compiled binary.
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
        } else if display_off_status {
            display_off_status = false;
        }

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
            if sleep_checking_display_off(&mut display, Duration::from_secs(3), &display_off_file)?
            {
                display_off_status = true;
            }
            continue;
        }

        // Monitoring information
        let time_str: String = run_shell("date +\"%H:%M\"");
        let ip: String = run_shell("hostname -I | cut -d' ' -f1");
        let cpu: String = run_shell("cut -f 1 -d ' ' /proc/loadavg");
        let mem_usage: String = run_shell(
            "free -m | awk 'NR==2{printf \"Mem: %s / %s MB  %.2f%%\", $3,$2,$3*100/$2 }'",
        );
        let disk: String =
            run_shell("df -h | awk '$NF==\"/\"{printf \"Disk: %d / %d GB  %s\", $3,$2,$5}'");

        Text::with_text_style(&time_str, Point::new(width - 30, 0), status_font, top_left)
            .draw(&mut display)?;
        Text::with_text_style(
            &format!("IP: {ip}"),
            Point::new(0, 0),
            status_font,
            top_left,
        )
        .draw(&mut display)?;
        Text::with_text_style(
            &format!("CPU load: {cpu}"),
            Point::new(0, 8),
            status_font,
            top_left,
        )
        .draw(&mut display)?;
        Text::with_text_style(&mem_usage, Point::new(0, 16), status_font, top_left)
            .draw(&mut display)?;
        Text::with_text_style(&disk, Point::new(0, 25), status_font, top_left)
            .draw(&mut display)?;

        display.flush()?;
        if sleep_checking_display_off(&mut display, Duration::from_millis(500), &display_off_file)?
        {
            display_off_status = true;
        }
    }
}
