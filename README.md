# SSD1306 Display RPi

This repository consists of two different components:

- SSD1306 Display Driver
- System Information Display for Raspberry Pi platforms

Collectively, this will display system information on an SSD1306 monochrome OLED display.

- The display driver works on more devices than Raspberry Pi.
- The system information display component is configured to be used with the Raspberry Pi, but it can be adapted to other platforms.

The instructions here mostly involve configuration for the Raspberry Pi.

For more information, visit the respective repository directories.

## Configuration

This was tested with a Raspberry Pi Zero 2 W. If you are working with a different platform, further modifications will need to be made, and different instructions may need to be followed for it to function.

### Getting i2c ready

The i2c interface first needs to be enabled. You can do this through the `raspi-config` TUI. Alternatively, you can run a command to achieve this.

```bash
sudo raspi-config nonint do_i2c 0
```

Once enabled, the device will need to be powered off.

```bash
sudo shutdown now
```

Connect the display to the respective GPIO header pins, and then power the device back on.

To verify that the display is being detected, run the following:

```bash
sudo apt install i2c-tools
sudo i2cdetect -y 1
```

If all goes well, you should see a mention of `3c` (`0x3c` address).

## Building

To keep the process simple, you should download and compile via the Raspberry Pi you intend to use this with.

1. Install Rust from https://rust-lang.org/ if not done already
2. Download and extract, or clone, the repository
3. Change directory to the root of the repository (where this README file is located)
4. Run `cargo build --release` (use `cargo build --release -j 1` if you are using a more resource-constrained Raspberry Pi, such as the Raspberry Pi Zero 2 W)
5. Run `./target/release/sysinfo_display` - you should then see the OLED display light up with updating system information statistics

## Additional configuration

If your display is not 128x32 (the default), adjust the width and height values under the `disp` variable.
