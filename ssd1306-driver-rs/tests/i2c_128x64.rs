use embedded_hal_mock::eh1::delay::NoopDelay;
use embedded_hal_mock::eh1::i2c::{Mock as I2cMock, Transaction as I2cTransaction};
use ssd1306_driver_rs::command::DEFAULT_I2C_ADDRESS;
use ssd1306_driver_rs::{DisplaySize, I2cInterface, Ssd1306, VccState};

fn cmd(addr: u8, byte: u8) -> I2cTransaction {
    I2cTransaction::write(addr, vec![0x00, byte])
}

#[test]
fn init_sequence_matches_128x64_switchcap() {
    let addr: u8 = DEFAULT_I2C_ADDRESS;
    let expected = vec![
        cmd(addr, 0xAE), // DISPLAYOFF
        cmd(addr, 0xD5), // SETDISPLAYCLOCKDIV
        cmd(addr, 0x80),
        cmd(addr, 0xA8), // SETMULTIPLEX
        cmd(addr, 0x3F), // 128x64-specific multiplex ratio
        cmd(addr, 0xD3), // SETDISPLAYOFFSET
        cmd(addr, 0x00),
        cmd(addr, 0x40), // SETSTARTLINE | 0
        cmd(addr, 0x8D), // CHARGEPUMP
        cmd(addr, 0x14), // switch-cap
        cmd(addr, 0x20), // MEMORYMODE
        cmd(addr, 0x00),
        cmd(addr, 0xA1), // SEGREMAP | 1
        cmd(addr, 0xC8), // COMSCANDEC
        cmd(addr, 0xDA), // SETCOMPINS
        cmd(addr, 0x12), // 128x64-specific COM pins config
        cmd(addr, 0x81), // SETCONTRAST
        cmd(addr, 0xCF), // internal contrast for 128x64
        cmd(addr, 0xD9), // SETPRECHARGE
        cmd(addr, 0xF1),
        cmd(addr, 0xDB), // SETVCOMDETECT
        cmd(addr, 0x40),
        cmd(addr, 0xA4), // DISPLAYALLON_RESUME
        cmd(addr, 0xA6), // NORMALDISPLAY
        cmd(addr, 0xAF), // DISPLAYON
    ];

    let mock = I2cMock::new(&expected);
    let interface = I2cInterface::new(mock, addr);
    let mut display = Ssd1306::new_without_reset(interface, DisplaySize::Size128x64);

    let mut delay: NoopDelay = NoopDelay::new();
    display.init(VccState::SwitchCap, &mut delay).unwrap();

    // Recover the mock and assert every expected transaction happened.
    let (interface, _reset_pin) = display.release();
    interface.release().done();
}

#[test]
fn init_sequence_matches_128x64_external_vcc() {
    // Same as above, but with an external VCC supply: the charge-pump,
    // precharge, and contrast bytes all switch to their "external" values
    // (unlike 128x32/96x16, 128x64 *does* vary contrast by VCC source:
    // 0xCF internal vs 0x9F external - see size.rs::SizeParams).
    let addr: u8 = DEFAULT_I2C_ADDRESS;
    let expected = vec![
        cmd(addr, 0xAE), // DISPLAYOFF
        cmd(addr, 0xD5), // SETDISPLAYCLOCKDIV
        cmd(addr, 0x80),
        cmd(addr, 0xA8), // SETMULTIPLEX
        cmd(addr, 0x3F),
        cmd(addr, 0xD3), // SETDISPLAYOFFSET
        cmd(addr, 0x00),
        cmd(addr, 0x40), // SETSTARTLINE | 0
        cmd(addr, 0x8D), // CHARGEPUMP
        cmd(addr, 0x10), // external VCC
        cmd(addr, 0x20), // MEMORYMODE
        cmd(addr, 0x00),
        cmd(addr, 0xA1), // SEGREMAP | 1
        cmd(addr, 0xC8), // COMSCANDEC
        cmd(addr, 0xDA), // SETCOMPINS
        cmd(addr, 0x12),
        cmd(addr, 0x81), // SETCONTRAST
        cmd(addr, 0x9F), // external contrast for 128x64
        cmd(addr, 0xD9), // SETPRECHARGE
        cmd(addr, 0x22), // external VCC
        cmd(addr, 0xDB), // SETVCOMDETECT
        cmd(addr, 0x40),
        cmd(addr, 0xA4), // DISPLAYALLON_RESUME
        cmd(addr, 0xA6), // NORMALDISPLAY
        cmd(addr, 0xAF), // DISPLAYON
    ];

    let mock = I2cMock::new(&expected);
    let interface = I2cInterface::new(mock, addr);
    let mut display = Ssd1306::new_without_reset(interface, DisplaySize::Size128x64);

    let mut delay: NoopDelay = NoopDelay::new();
    display.init(VccState::External, &mut delay).unwrap();

    let (interface, _reset_pin) = display.release();
    interface.release().done();
}

#[test]
fn dimensions_and_buffer_size_are_128x64() {
    let mock = I2cMock::new(&[]);
    let interface = I2cInterface::new(mock, DEFAULT_I2C_ADDRESS);
    let display = Ssd1306::new_without_reset(interface, DisplaySize::Size128x64);

    assert_eq!(display.width(), 128);
    assert_eq!(display.height(), 64);
    // 8 pages (64 / 8) * 128 columns.
    assert_eq!(display.buffer().len(), 128 * 8);

    let (interface, _reset_pin) = display.release();
    interface.release().done();
}

#[test]
fn set_pixel_matches_original_bit_packing() {
    // Same bit-packing the Python driver's `image()` method uses: each byte
    // is one vertical column of 8 pixels within a "page", LSB = top pixel.
    let addr: u8 = DEFAULT_I2C_ADDRESS;
    let mock = I2cMock::new(&[]);
    let interface = I2cInterface::new(mock, addr);
    let mut display = Ssd1306::new_without_reset(interface, DisplaySize::Size128x64);

    assert!(!display.get_pixel(10, 9));
    display.set_pixel(10, 9, true);
    assert!(display.get_pixel(10, 9));
    // Byte for column 10, page 1 (y=9 -> page 1, bit 1) should be 0b0000_0010.
    let page: usize = 1usize;
    let index: usize = page * display.width() as usize + 10;
    assert_eq!(display.buffer()[index], 0b0000_0010);

    display.set_pixel(10, 9, false);
    assert!(!display.get_pixel(10, 9));
    assert_eq!(display.buffer()[index], 0);

    let (interface, _reset_pin) = display.release();
    interface.release().done();
}

#[test]
fn set_pixel_out_of_bounds_is_ignored_128x64() {
    // y=64+ or x=128+ is off-panel and must be a silent no-op rather than
    // an out-of-bounds panic.
    let mock = I2cMock::new(&[]);
    let interface = I2cInterface::new(mock, DEFAULT_I2C_ADDRESS);
    let mut display = Ssd1306::new_without_reset(interface, DisplaySize::Size128x64);

    display.set_pixel(0, 64, true);
    display.set_pixel(128, 0, true);
    assert!(display.buffer().iter().all(|&b| b == 0));

    let (interface, _reset_pin) = display.release();
    interface.release().done();
}

#[test]
fn set_buffer_rejects_wrong_size() {
    let mock = I2cMock::new(&[]);
    let interface = I2cInterface::new(mock, DEFAULT_I2C_ADDRESS);
    let mut display = Ssd1306::new_without_reset(interface, DisplaySize::Size128x64);

    let err = display.set_buffer(&[0u8; 4]).unwrap_err();
    match err {
        ssd1306_driver_rs::BufferError::WrongSize { expected, actual } => {
            assert_eq!(expected, 128 * 64 / 8);
            assert_eq!(actual, 4);
        }
    }

    let ok_buf = vec![0u8; 128 * 64 / 8];
    assert!(display.set_buffer(&ok_buf).is_ok());

    let (interface, _reset_pin) = display.release();
    interface.release().done();
}
