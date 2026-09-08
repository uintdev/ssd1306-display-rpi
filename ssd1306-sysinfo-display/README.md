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

## Turning off the display

If you want to turn off the display visually, create a `displayoff` file in the same location as the executable. To turn it back on, remove the file or rerun the executable.

## Showing a brief message

To show a temporary message (for example, to indicate a status change), create a `msg.txt` in the same location as the executable, with the message as the text file content. As soon as the message is displayed, the text file will be automatically removed.

By default, the message display duration is 3 seconds. This is configurable.

## Configuration

### Changing display size

By default, the display size is set to 128x32. If you wish to change this, update `DisplaySize` to use one of the three options that correspond to the display size you need.
