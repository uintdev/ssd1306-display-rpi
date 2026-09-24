use embedded_hal::digital::{ErrorKind, ErrorType, OutputPin};
use embedded_hal_mock::eh1::delay::NoopDelay;
use embedded_hal_mock::eh1::i2c::{Mock as I2cMock, Transaction as I2cTransaction};
use ssd1306_driver_rs::command::DEFAULT_I2C_ADDRESS;
use ssd1306_driver_rs::{BufferError, DisplaySize, I2cInterface, InitError, Ssd1306, VccState};

const ADDR: u8 = DEFAULT_I2C_ADDRESS;

// Size-specific values expected on the wire and in the frame buffer.
struct Expected {
    width: u32,
    height: u32,
    clock_div_ratio: u8,
    multiplex: u8,
    com_pins: u8,
    contrast_internal: u8,
    contrast_external: u8,
}

fn cmd(byte: u8) -> I2cTransaction {
    I2cTransaction::write(ADDR, vec![0x00, byte])
}

// Several command bytes batched into one write behind a single 0x00
// control byte.
fn cmds(bytes: &[u8]) -> I2cTransaction {
    let mut buf: Vec<u8> = vec![0x00];
    buf.extend_from_slice(bytes);
    I2cTransaction::write(ADDR, buf)
}

// Run `f` against a display backed by an I2C mock that expects exactly
// `transactions`, then assert every expected transaction happened.
fn with_display(
    size: DisplaySize,
    transactions: &[I2cTransaction],
    f: impl FnOnce(&mut Ssd1306<I2cInterface<I2cMock>>),
) {
    let mock = I2cMock::new(transactions);
    let mut display = Ssd1306::new_without_reset(I2cInterface::new(mock, ADDR), size);
    f(&mut display);
    let (interface, _reset_pin) = display.release();
    interface.release().done();
}

// Normal (undimmed) contrast for a panel size and VCC source. Only 128x64
// differs by VCC source: 0xCF internal vs 0x9F external.
fn expected_contrast(e: &Expected, vcc: VccState) -> u8 {
    if vcc == VccState::External {
        e.contrast_external
    } else {
        e.contrast_internal
    }
}

// Transactions sent by `init()`. With an external VCC supply, the
// charge-pump, precharge, and contrast bytes all switch to their "external"
// values.
fn init_transactions(e: &Expected, vcc: VccState) -> Vec<I2cTransaction> {
    let external: bool = vcc == VccState::External;
    vec![
        // Init sequence, sent as one batched write.
        cmds(&[
            0xAE, // DISPLAYOFF
            0xD5, // SETDISPLAYCLOCKDIV
            e.clock_div_ratio,
            0xA8, // SETMULTIPLEX
            e.multiplex,
            0xD3, // SETDISPLAYOFFSET
            0x00, // no offset
            0x40, // SETSTARTLINE | 0
            0x8D, // CHARGEPUMP
            if external { 0x10 } else { 0x14 },
            0x20, // MEMORYMODE
            0x00, // horizontal addressing
            0xA1, // SEGREMAP | 1
            0xC8, // COMSCANDEC
            0xDA, // SETCOMPINS
            e.com_pins,
            0x81, // SETCONTRAST
            expected_contrast(e, vcc),
            0xD9, // SETPRECHARGE
            if external { 0x22 } else { 0xF1 },
            0xDB, // SETVCOMDETECT
            0x40, // VCOMH deselect level
            0xA4, // DISPLAYALLON_RESUME
            0xA6, // NORMALDISPLAY
        ]),
        cmd(0xAF), // DISPLAYON
    ]
}

fn check_init_sequence(size: DisplaySize, e: &Expected, vcc: VccState) {
    with_display(size, &init_transactions(e, vcc), |display| {
        display.init(vcc, &mut NoopDelay::new()).unwrap();
    });
}

fn check_dim(size: DisplaySize, e: &Expected, vcc: VccState) {
    // Dimming drops contrast to 0; undimming restores the same size- and
    // VCC-specific contrast that init() set.
    let mut expected = init_transactions(e, vcc);
    expected.push(cmds(&[0x81, 0x00]));
    expected.push(cmds(&[0x81, expected_contrast(e, vcc)]));

    with_display(size, &expected, |display| {
        display.init(vcc, &mut NoopDelay::new()).unwrap();
        display.dim(true).unwrap();
        display.dim(false).unwrap();
    });
}

fn check_dim_custom_contrast(size: DisplaySize, e: &Expected) {
    // After set_contrast, undimming returns to that value, not the default.
    let mut expected = init_transactions(e, VccState::SwitchCap);
    expected.push(cmds(&[0x81, 0x42])); // set_contrast(0x42)
    expected.push(cmds(&[0x81, 0x00])); // dim(true)
    expected.push(cmds(&[0x81, 0x42])); // dim(false)

    with_display(size, &expected, |display| {
        display
            .init(VccState::SwitchCap, &mut NoopDelay::new())
            .unwrap();
        display.set_contrast(0x42).unwrap();
        display.dim(true).unwrap();
        display.dim(false).unwrap();
    });
}

fn check_flush(size: DisplaySize, e: &Expected) {
    // A distinct value per byte, so misordered or dropped data is caught.
    let len: usize = (e.width * e.height / 8) as usize;
    let frame: Vec<u8> = (0..len).map(|i| i as u8).collect();

    // Address window covering the whole panel, sent as one batched write.
    let mut expected = vec![cmds(&[
        0x21,                     // COLUMNADDR
        0x00,                     // start column
        (e.width - 1) as u8,      // end column
        0x22,                     // PAGEADDR
        0x00,                     // start page
        (e.height / 8 - 1) as u8, // end page
    ])];
    // Then the frame in chunks of up to 128 bytes, each behind a 0x40
    // (data) control byte. 96x16's 192-byte frame ends in a partial chunk.
    expected.extend(frame.chunks(128).map(|chunk| {
        let mut buf: Vec<u8> = vec![0x40];
        buf.extend_from_slice(chunk);
        I2cTransaction::write(ADDR, buf)
    }));

    with_display(size, &expected, |display| {
        display.set_buffer(&frame).unwrap();
        display.flush().unwrap();
    });
}

fn check_dimensions_and_buffer_size(size: DisplaySize, e: &Expected) {
    with_display(size, &[], |display| {
        assert_eq!(display.width(), e.width);
        assert_eq!(display.height(), e.height);
        // One byte per column per 8-pixel page.
        assert_eq!(display.buffer().len(), (e.width * e.height / 8) as usize);
    });
}

fn check_set_pixel_bit_packing(size: DisplaySize, e: &Expected) {
    // Each byte is one vertical column of 8 pixels within a "page",
    // LSB = top pixel.
    // Use the last page so the page offset is exercised on every size.
    let (x, y): (u32, u32) = (5, e.height - 7);
    let page: usize = (e.height / 8 - 1) as usize;
    let index: usize = page * e.width as usize + x as usize;

    with_display(size, &[], |display| {
        assert!(!display.get_pixel(x, y));
        display.set_pixel(x, y, true);
        assert!(display.get_pixel(x, y));
        // y % 8 == 1, so bit 1.
        assert_eq!(display.buffer()[index], 0b0000_0010);

        display.set_pixel(x, y, false);
        assert!(!display.get_pixel(x, y));
        assert_eq!(display.buffer()[index], 0);
    });
}

fn check_set_pixel_out_of_bounds_is_ignored(size: DisplaySize, e: &Expected) {
    // Off-panel pixels must be a silent no-op rather than an out-of-bounds
    // panic.
    with_display(size, &[], |display| {
        display.set_pixel(0, e.height, true);
        display.set_pixel(e.width, 0, true);
        assert!(display.buffer().iter().all(|&b| b == 0));
        assert!(!display.get_pixel(0, e.height));
        assert!(!display.get_pixel(e.width, 0));
    });
}

fn check_set_buffer_rejects_wrong_size(size: DisplaySize, e: &Expected) {
    let len: usize = (e.width * e.height / 8) as usize;

    with_display(size, &[], |display| {
        assert_eq!(
            display.set_buffer(&[0u8; 4]),
            Err(BufferError::WrongSize {
                expected: len,
                actual: 4
            })
        );
        assert!(display.set_buffer(&vec![0u8; len]).is_ok());
    });
}

// Generate a test module per panel size, so failures name the size.
macro_rules! size_tests {
    ($name:ident, $size:expr, $expected:expr) => {
        mod $name {
            use super::*;

            const SIZE: DisplaySize = $size;
            const EXPECTED: Expected = $expected;

            #[test]
            fn init_sequence_switchcap() {
                check_init_sequence(SIZE, &EXPECTED, VccState::SwitchCap);
            }

            #[test]
            fn init_sequence_external_vcc() {
                check_init_sequence(SIZE, &EXPECTED, VccState::External);
            }

            #[test]
            fn dim_restores_init_contrast_switchcap() {
                check_dim(SIZE, &EXPECTED, VccState::SwitchCap);
            }

            #[test]
            fn dim_restores_init_contrast_external_vcc() {
                check_dim(SIZE, &EXPECTED, VccState::External);
            }

            #[test]
            fn dim_restores_custom_contrast() {
                check_dim_custom_contrast(SIZE, &EXPECTED);
            }

            #[test]
            fn flush_sends_address_window_and_frame() {
                check_flush(SIZE, &EXPECTED);
            }

            #[test]
            fn dimensions_and_buffer_size() {
                check_dimensions_and_buffer_size(SIZE, &EXPECTED);
            }

            #[test]
            fn set_pixel_bit_packing() {
                check_set_pixel_bit_packing(SIZE, &EXPECTED);
            }

            #[test]
            fn set_pixel_out_of_bounds_is_ignored() {
                check_set_pixel_out_of_bounds_is_ignored(SIZE, &EXPECTED);
            }

            #[test]
            fn set_buffer_rejects_wrong_size() {
                check_set_buffer_rejects_wrong_size(SIZE, &EXPECTED);
            }
        }
    };
}

size_tests!(
    size_128x64,
    DisplaySize::Size128x64,
    Expected {
        width: 128,
        height: 64,
        clock_div_ratio: 0x80,
        multiplex: 0x3F,
        com_pins: 0x12,
        contrast_internal: 0xCF,
        contrast_external: 0x9F,
    }
);

size_tests!(
    size_128x32,
    DisplaySize::Size128x32,
    Expected {
        width: 128,
        height: 32,
        clock_div_ratio: 0x80,
        multiplex: 0x1F,
        com_pins: 0x02,
        contrast_internal: 0x8F,
        contrast_external: 0x8F,
    }
);

size_tests!(
    size_96x16,
    DisplaySize::Size96x16,
    Expected {
        width: 96,
        height: 16,
        clock_div_ratio: 0x60,
        multiplex: 0x0F,
        com_pins: 0x02,
        contrast_internal: 0x8F,
        contrast_external: 0x8F,
    }
);

// A reset pin whose every operation fails.
struct FailingPin;

impl ErrorType for FailingPin {
    type Error = ErrorKind;
}

impl OutputPin for FailingPin {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Err(ErrorKind::Other)
    }

    fn set_high(&mut self) -> Result<(), Self::Error> {
        Err(ErrorKind::Other)
    }
}

#[test]
fn init_reports_reset_pin_error_without_touching_the_bus() {
    // The mock expects no transactions: init must stop at the failed reset.
    let mock = I2cMock::new(&[]);
    let interface = I2cInterface::new(mock, ADDR);
    let mut display = Ssd1306::new_with_reset(interface, DisplaySize::Size128x32, FailingPin);

    let result = display.init(VccState::SwitchCap, &mut NoopDelay::new());
    assert!(matches!(result, Err(InitError::Pin(ErrorKind::Other))));

    let (interface, _reset_pin) = display.release();
    interface.release().done();
}
