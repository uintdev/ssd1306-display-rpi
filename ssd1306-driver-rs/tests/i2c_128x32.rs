use embedded_hal_mock::eh1::delay::NoopDelay;
use embedded_hal_mock::eh1::i2c::{Mock as I2cMock, Transaction as I2cTransaction};
use ssd1306_driver_rs::command::DEFAULT_I2C_ADDRESS;
use ssd1306_driver_rs::{DisplaySize, I2cInterface, Ssd1306, VccState};

fn cmd(addr: u8, byte: u8) -> I2cTransaction {
    I2cTransaction::write(addr, vec![0x00, byte])
}

#[test]
fn init_sequence_matches_128x32_switchcap() {
    let addr: u8 = DEFAULT_I2C_ADDRESS;
    let expected = vec![
        cmd(addr, 0xAE), // DISPLAYOFF
        cmd(addr, 0xD5), // SETDISPLAYCLOCKDIV
        cmd(addr, 0x80),
        cmd(addr, 0xA8), // SETMULTIPLEX
        cmd(addr, 0x1F), // 128x32-specific multiplex ratio
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
        cmd(addr, 0x02), // 128x32-specific COM pins config
        cmd(addr, 0x81), // SETCONTRAST
        cmd(addr, 0x8F), // 128x32 contrast (same for internal/external)
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
    let mut display = Ssd1306::new_without_reset(interface, DisplaySize::Size128x32);

    let mut delay: NoopDelay = NoopDelay::new();
    display.init(VccState::SwitchCap, &mut delay).unwrap();

    // Recover the mock and assert every expected transaction happened.
    let (interface, _reset_pin) = display.release();
    interface.release().done();
}

#[test]
fn init_sequence_matches_128x32_external_vcc() {
    // Same as above, but with an external VCC supply: the charge-pump,
    // precharge, and contrast bytes all switch to their "external" values
    // (contrast stays 0x8F either way for this panel size - see
    // size.rs::SizeParams for 128x32).
    let addr: u8 = DEFAULT_I2C_ADDRESS;
    let expected = vec![
        cmd(addr, 0xAE), // DISPLAYOFF
        cmd(addr, 0xD5), // SETDISPLAYCLOCKDIV
        cmd(addr, 0x80),
        cmd(addr, 0xA8), // SETMULTIPLEX
        cmd(addr, 0x1F),
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
        cmd(addr, 0x02),
        cmd(addr, 0x81), // SETCONTRAST
        cmd(addr, 0x8F),
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
    let mut display = Ssd1306::new_without_reset(interface, DisplaySize::Size128x32);

    let mut delay = NoopDelay::new();
    display.init(VccState::External, &mut delay).unwrap();

    let (interface, _reset_pin) = display.release();
    interface.release().done();
}

#[test]
fn dimensions_and_buffer_size_are_128x32() {
    let mock = I2cMock::new(&[]);
    let interface = I2cInterface::new(mock, DEFAULT_I2C_ADDRESS);
    let display = Ssd1306::new_without_reset(interface, DisplaySize::Size128x32);

    assert_eq!(display.width(), 128);
    assert_eq!(display.height(), 32);
    // 4 pages (32 / 8) * 128 columns.
    assert_eq!(display.buffer().len(), 128 * 4);

    let (interface, _reset_pin) = display.release();
    interface.release().done();
}

#[test]
fn set_pixel_matches_original_bit_packing_128x32() {
    let mock = I2cMock::new(&[]);
    let interface = I2cInterface::new(mock, DEFAULT_I2C_ADDRESS);
    let mut display = Ssd1306::new_without_reset(interface, DisplaySize::Size128x32);

    // Pixel at (5, 17): page = 17 / 8 = 2, bit = 17 % 8 = 1.
    assert!(!display.get_pixel(5, 17));
    display.set_pixel(5, 17, true);
    assert!(display.get_pixel(5, 17));

    let page: usize = 2usize;
    let index: usize = page * display.width() as usize + 5;
    assert_eq!(display.buffer()[index], 0b0000_0010);

    display.set_pixel(5, 17, false);
    assert!(!display.get_pixel(5, 17));
    assert_eq!(display.buffer()[index], 0);

    let (interface, _reset_pin) = display.release();
    interface.release().done();
}

#[test]
fn set_pixel_out_of_bounds_is_ignored_128x32() {
    // Height is only 32, so y=32+ is off-panel and must be a silent no-op
    // rather than an out-of-bounds panic - this is the case most likely to
    // regress on the shortest of the three supported panel heights.
    let mock = I2cMock::new(&[]);
    let interface = I2cInterface::new(mock, DEFAULT_I2C_ADDRESS);
    let mut display = Ssd1306::new_without_reset(interface, DisplaySize::Size128x32);

    display.set_pixel(0, 32, true);
    display.set_pixel(128, 0, true);
    assert!(display.buffer().iter().all(|&b| b == 0));

    let (interface, _reset_pin) = display.release();
    interface.release().done();
}

#[test]
fn set_buffer_rejects_wrong_size_128x32() {
    let mock = I2cMock::new(&[]);
    let interface = I2cInterface::new(mock, DEFAULT_I2C_ADDRESS);
    let mut display = Ssd1306::new_without_reset(interface, DisplaySize::Size128x32);

    let err = display.set_buffer(&[0u8; 4]).unwrap_err();
    match err {
        ssd1306_driver_rs::BufferError::WrongSize { expected, actual } => {
            assert_eq!(expected, 128 * 32 / 8);
            assert_eq!(actual, 4);
        }
    }

    let ok_buf = vec![0u8; 128 * 32 / 8];
    assert!(display.set_buffer(&ok_buf).is_ok());

    let (interface, _reset_pin) = display.release();
    interface.release().done();
}
