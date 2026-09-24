# SSD1306 SysInfo Display

This displays system information on the SSD1306 monochrome OLED display.

By default, this is configured to use RPPAL to interface with the Raspberry Pi. This supports Raspberry Pi 5 or earlier.

You may adapt this to work on the platform of your choice instead.

## Features

This displays the following system information:

- Time
- IP address (IPv4)
- CPU load
- Memory usage
- Disk usage

### Turning off the display

To turn off the display, create a `displayoff` file in the same location as the executable. The screen is cleared and the panel is put to sleep, which saves power and avoids OLED burn-in. To turn it back on, remove the file or rerun the executable.

Stopping the program (for example with Ctrl+C or `systemctl stop`) also turns the display off, rather than leaving the last screen showing.

### Displaying a brief message

To show a temporary message (for example, to indicate a status change), create a `msg.txt` file in the same location as the executable, with the message as the text file’s content. As soon as the message is displayed, the text file will be automatically removed.

By default, the message display duration is 3 seconds. To change it, update `MESSAGE_DURATION` in `src/main.rs`.

## Configuration

### Changing display size

By default, the display size is set to 128x32. If you wish to change this, update `DisplaySize` to use one of the three options that correspond to the display size you need.
