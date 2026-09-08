use embedded_hal::digital::OutputPin;
use embedded_hal::i2c::I2c;
use embedded_hal::spi::SpiDevice;

// A transport capable of sending command and data bytes to an SSD1306
// display. Implemented for I2C ([`I2cInterface`]) and SPI
// ([`SpiInterface`]) out of the box.
pub trait DisplayInterface {
    // Error type returned by the underlying bus/pins.
    type Error;

    // Send a single command byte.
    fn command(&mut self, cmd: u8) -> Result<(), Self::Error>;

    // Send a slice of raw display data bytes.
    fn data(&mut self, data: &[u8]) -> Result<(), Self::Error>;
}

// I2C transport for the display (the common wiring for most SSD1306
// breakout boards, and the default used by the original Python driver).
pub struct I2cInterface<I2C> {
    i2c: I2C,
    address: u8,
}

impl<I2C> I2cInterface<I2C> {
    // Create a new I2C interface talking to the display at `address`
    // (typically [`crate::command::DEFAULT_I2C_ADDRESS`], `0x3C`).
    pub fn new(i2c: I2C, address: u8) -> Self {
        Self { i2c, address }
    }

    // Release the underlying I2C bus.
    pub fn release(self) -> I2C {
        self.i2c
    }
}

impl<I2C> DisplayInterface for I2cInterface<I2C>
where
    I2C: I2c,
{
    type Error = I2C::Error;

    fn command(&mut self, cmd: u8) -> Result<(), Self::Error> {
        // Control byte 0x00: Co = 0, D/C = 0 (command).
        self.i2c.write(self.address, &[0x00, cmd])
    }

    fn data(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        // The original driver streams the frame buffer in 16-byte chunks,
        // each prefixed with the "data" control byte (0x40: Co = 0, D/C = 1).
        // Chunking keeps each I2C transaction small, which matches common
        // I2C driver/buffer limits on the Pi and elsewhere.
        let mut chunk_buf = [0u8; 17];
        for chunk in data.chunks(16) {
            chunk_buf[0] = 0x40;
            chunk_buf[1..=chunk.len()].copy_from_slice(chunk);
            self.i2c.write(self.address, &chunk_buf[..=chunk.len()])?;
        }
        Ok(())
    }
}

// Combined error type for the SPI transport, distinguishing bus errors
// from D/C pin errors.
#[derive(Debug)]
pub enum SpiInterfaceError<SpiError, PinError> {
    Spi(SpiError),
    Pin(PinError),
}

// 4-wire SPI transport for the display (chip-select is handled by the
// `SpiDevice` implementation; wire the reset pin separately via
// [`crate::Ssd1306::new_with_reset`]).
pub struct SpiInterface<SPI, DC> {
    spi: SPI,
    dc: DC,
}

impl<SPI, DC> SpiInterface<SPI, DC> {
    // Create a new SPI interface using `dc` as the data/command select pin.
    pub fn new(spi: SPI, dc: DC) -> Self {
        Self { spi, dc }
    }

    // Release the underlying SPI device and D/C pin.
    pub fn release(self) -> (SPI, DC) {
        (self.spi, self.dc)
    }
}

impl<SPI, DC> DisplayInterface for SpiInterface<SPI, DC>
where
    SPI: SpiDevice,
    DC: OutputPin,
{
    type Error = SpiInterfaceError<SPI::Error, DC::Error>;

    fn command(&mut self, cmd: u8) -> Result<(), Self::Error> {
        self.dc.set_low().map_err(SpiInterfaceError::Pin)?;
        self.spi.write(&[cmd]).map_err(SpiInterfaceError::Spi)
    }

    fn data(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        self.dc.set_high().map_err(SpiInterfaceError::Pin)?;
        self.spi.write(data).map_err(SpiInterfaceError::Spi)
    }
}
