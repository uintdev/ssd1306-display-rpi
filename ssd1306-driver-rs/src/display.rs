use crate::command::*;
use crate::interface::DisplayInterface;
use crate::size::DisplaySize;
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{ErrorType, OutputPin};

// Charge pump power source, mirroring `SSD1306_EXTERNALVCC` /
// `SSD1306_SWITCHCAPVCC` in the original driver. Most breakout boards
// (and the Python driver's default) use `SwitchCap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VccState {
    // Display is powered from an external VCC supply.
    External,
    // Display uses its internal charge pump (the common case).
    SwitchCap,
}

// A no-op GPIO pin, used when a display's reset line isn't wired up.
//
// Some SSD1306 boards (especially small I2C ones) tie reset to the
// microcontroller's own reset line and don't expose a separate pin; this
// lets [`Ssd1306::new_without_reset`] skip the reset pulse instead of
// requiring a dummy pin from the caller.
#[derive(Debug, Default)]
pub struct NoResetPin;

impl ErrorType for NoResetPin {
    type Error = core::convert::Infallible;
}

impl OutputPin for NoResetPin {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

// Error returned by [`Ssd1306::set_buffer`] when the supplied slice
// doesn't match the display's buffer size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferError {
    // The supplied buffer had the wrong length.
    WrongSize {
        // Expected length (`width * height / 8` bytes).
        expected: usize,
        // Length actually supplied.
        actual: usize,
    },
}

impl core::fmt::Display for BufferError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BufferError::WrongSize { expected, actual } => write!(
                f,
                "buffer has wrong size: expected {expected} bytes, got {actual}"
            ),
        }
    }
}

impl std::error::Error for BufferError {}

// `I` is the transport ([`crate::I2cInterface`] or [`crate::SpiInterface`])
// and `RST` is the (optional) reset pin's type.
pub struct Ssd1306<I, RST = NoResetPin> {
    interface: I,
    reset_pin: Option<RST>,
    size: DisplaySize,
    width: u32,
    height: u32,
    pages: usize,
    buffer: Vec<u8>,
    vcc_state: VccState,
}

impl<I> Ssd1306<I, NoResetPin>
where
    I: DisplayInterface,
{
    // Create a driver for a display whose reset line isn't connected to a
    // controllable GPIO pin.
    pub fn new_without_reset(interface: I, size: DisplaySize) -> Self {
        Self::new(interface, size, None)
    }
}

// Methods that don't touch the reset pin: these only need a working
// transport (`I: DisplayInterface`), regardless of what `RST` is or whether
// it implements `OutputPin`. Keeping this bound minimal is what lets the
// `embedded-graphics` `DrawTarget` impl (which only calls `set_pixel`) work
// for `Ssd1306<I, RST>` for *any* `RST`, not just ones with a real pin.
impl<I, RST> Ssd1306<I, RST>
where
    I: DisplayInterface,
{
    fn new(interface: I, size: DisplaySize, reset_pin: Option<RST>) -> Self {
        let (width, height) = size.dimensions();
        let pages: usize = (height / 8) as usize;
        let buffer: Vec<u8> = vec![0u8; width as usize * pages];
        Self {
            interface,
            reset_pin,
            size,
            width,
            height,
            pages,
            buffer,
            vcc_state: VccState::SwitchCap,
        }
    }

    // Consume the driver and return the underlying transport (and reset
    // pin, if one was configured), e.g. to reuse the bus for something
    // else or to inspect a mock in tests.
    pub fn release(self) -> (I, Option<RST>) {
        (self.interface, self.reset_pin)
    }

    // Display width and height in pixels
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }

    fn initialize(&mut self) -> Result<(), I::Error> {
        let p = self.size.params();
        let external: bool = self.vcc_state == VccState::External;

        self.interface.command(DISPLAYOFF)?;
        self.interface.command(SETDISPLAYCLOCKDIV)?;
        self.interface.command(p.clock_div_ratio)?;
        self.interface.command(SETMULTIPLEX)?;
        self.interface.command(p.multiplex)?;
        self.interface.command(SETDISPLAYOFFSET)?;
        self.interface.command(0x00)?; // No offset.
        self.interface.command(SETSTARTLINE)?; // Line #0.
        self.interface.command(CHARGEPUMP)?;
        self.interface.command(if external { 0x10 } else { 0x14 })?;
        self.interface.command(MEMORYMODE)?;
        self.interface.command(0x00)?; // Act like ks0108.
        self.interface.command(SEGREMAP | 0x01)?;
        self.interface.command(COMSCANDEC)?;
        self.interface.command(SETCOMPINS)?;
        self.interface.command(p.com_pins)?;
        self.interface.command(SETCONTRAST)?;
        self.interface.command(if external {
            p.contrast_external
        } else {
            p.contrast_internal
        })?;
        self.interface.command(SETPRECHARGE)?;
        self.interface.command(if external { 0x22 } else { 0xF1 })?;
        self.interface.command(SETVCOMDETECT)?;
        self.interface.command(0x40)?;
        self.interface.command(DISPLAYALLON_RESUME)?;
        self.interface.command(NORMALDISPLAY)
    }

    // Push the in-memory frame buffer to the physical display.
    pub fn flush(&mut self) -> Result<(), I::Error> {
        self.interface.command(COLUMNADDR)?;
        self.interface.command(0)?; // Column start address (0 = reset).
        self.interface.command((self.width - 1) as u8)?; // Column end address.
        self.interface.command(PAGEADDR)?;
        self.interface.command(0)?; // Page start address (0 = reset).
        self.interface.command((self.pages - 1) as u8)?; // Page end address.
        self.interface.data(&self.buffer)
    }

    // Zero the in-memory frame buffer. Call [`Ssd1306::flush`] afterwards
    // to clear the physical display.
    pub fn clear(&mut self) {
        self.buffer.iter_mut().for_each(|b| *b = 0);
    }

    // Set (or clear) a single pixel in the frame buffer. Call
    // [`Ssd1306::flush`] to send the change to the display.
    pub fn set_pixel(&mut self, x: u32, y: u32, on: bool) {
        if x >= self.width || y >= self.height {
            return;
        }
        let page = (y / 8) as usize;
        let bit = (y % 8) as u8;
        let index = page * self.width as usize + x as usize;
        if on {
            self.buffer[index] |= 1 << bit;
        } else {
            self.buffer[index] &= !(1 << bit);
        }
    }

    // Read back whether a pixel in the frame buffer is set.
    pub fn get_pixel(&self, x: u32, y: u32) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let page: usize = (y / 8) as usize;
        let bit: u8 = (y % 8) as u8;
        let index: usize = page * self.width as usize + x as usize;
        self.buffer[index] & (1 << bit) != 0
    }

    // Overwrite the whole frame buffer with already page-packed data
    // (`width * height / 8` bytes, same layout the display expects on the
    // wire).
    pub fn set_buffer(&mut self, data: &[u8]) -> Result<(), BufferError> {
        if data.len() != self.buffer.len() {
            return Err(BufferError::WrongSize {
                expected: self.buffer.len(),
                actual: data.len(),
            });
        }
        self.buffer.copy_from_slice(data);
        Ok(())
    }

    // Borrow the raw frame buffer
    pub fn buffer(&self) -> &[u8] {
        &self.buffer
    }

    // Mutably borrow the raw frame buffer for direct manipulation.
    pub fn buffer_mut(&mut self) -> &mut [u8] {
        &mut self.buffer
    }

    // Set display contrast, from 0 to 255.
    pub fn set_contrast(&mut self, contrast: u8) -> Result<(), I::Error> {
        self.interface.command(SETCONTRAST)?;
        self.interface.command(contrast)
    }

    // Dim the display, or restore normal brightness if `dim` is `false`.
    // Equivalent to the original driver's `dim()`.
    pub fn dim(&mut self, dim: bool) -> Result<(), I::Error> {
        let contrast: u8 = if dim {
            0
        } else if self.vcc_state == VccState::External {
            0x9F
        } else {
            0xCF
        };
        self.set_contrast(contrast)
    }

    // Turn the whole display on regardless of buffer contents (`0xA5`).
    pub fn display_all_on(&mut self) -> Result<(), I::Error> {
        self.interface.command(DISPLAYALLON)
    }

    // Resume displaying frame buffer contents after [`Ssd1306::display_all_on`]
    // (`0xA4`).
    pub fn display_all_on_resume(&mut self) -> Result<(), I::Error> {
        self.interface.command(DISPLAYALLON_RESUME)
    }

    // Invert the display (swap on/off pixels) without touching the buffer.
    pub fn invert(&mut self, invert: bool) -> Result<(), I::Error> {
        self.interface
            .command(if invert { INVERTDISPLAY } else { NORMALDISPLAY })
    }

    // Turn the physical display panel off (low power mode). The frame
    // buffer is preserved and will reappear on the next [`Ssd1306::on`].
    pub fn off(&mut self) -> Result<(), I::Error> {
        self.interface.command(DISPLAYOFF)
    }

    // Turn the physical display panel back on.
    pub fn on(&mut self) -> Result<(), I::Error> {
        self.interface.command(DISPLAYON)
    }
}

// Methods that need to actually drive the reset pin
impl<I, RST> Ssd1306<I, RST>
where
    I: DisplayInterface,
    RST: OutputPin,
{
    // Create a driver with a reset pin wired up.
    pub fn new_with_reset(interface: I, size: DisplaySize, reset_pin: RST) -> Self {
        Self::new(interface, size, Some(reset_pin))
    }

    // Pulse the reset pin (high, then low for 10ms, then high again), as
    // done in the original driver's `reset()`. A no-op if this display was
    // constructed with [`Ssd1306::new_without_reset`].
    pub fn reset(&mut self, delay: &mut impl DelayNs) {
        if let Some(rst) = self.reset_pin.as_mut() {
            let _ = rst.set_high();
            delay.delay_ms(1);
            let _ = rst.set_low();
            delay.delay_ms(10);
            let _ = rst.set_high();
        }
    }

    // Reset, run the size-specific initialization sequence, and turn the display on.
    pub fn init(&mut self, vcc_state: VccState, delay: &mut impl DelayNs) -> Result<(), I::Error> {
        self.vcc_state = vcc_state;
        self.reset(delay);
        self.initialize()?;
        self.interface.command(DISPLAYON)
    }
}
