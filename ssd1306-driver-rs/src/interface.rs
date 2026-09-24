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

    // Send a sequence of command bytes (commands and their parameters).
    // The default sends them one at a time; transports override this to
    // batch them into fewer bus transactions.
    fn commands(&mut self, cmds: &[u8]) -> Result<(), Self::Error> {
        cmds.iter().try_for_each(|&cmd| self.command(cmd))
    }

    // Send a slice of raw display data bytes.
    fn data(&mut self, data: &[u8]) -> Result<(), Self::Error>;
}

// Maximum payload bytes per I2C write (excluding the control byte). The
// original Python driver used 16 because of SMBus block-size limits; plain
// I2C writes (e.g. Linux i2c-dev, used by rppal) have no such limit, and
// 128 stays under the 255-byte cap common to many microcontroller HALs.
const I2C_CHUNK_SIZE: usize = 128;

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

impl<I2C> I2cInterface<I2C>
where
    I2C: I2c,
{
    // Send `bytes` in chunks of up to `I2C_CHUNK_SIZE`, each prefixed with
    // `control`. With Co = 0 the controller treats everything after the
    // control byte as a stream, so a chunk may hold several commands (or
    // split a command from its parameters) without changing the result.
    fn write_chunked(&mut self, control: u8, bytes: &[u8]) -> Result<(), I2C::Error> {
        let mut chunk_buf = [0u8; I2C_CHUNK_SIZE + 1];
        chunk_buf[0] = control;
        for chunk in bytes.chunks(I2C_CHUNK_SIZE) {
            chunk_buf[1..=chunk.len()].copy_from_slice(chunk);
            self.i2c.write(self.address, &chunk_buf[..=chunk.len()])?;
        }
        Ok(())
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

    fn commands(&mut self, cmds: &[u8]) -> Result<(), Self::Error> {
        self.write_chunked(0x00, cmds)
    }

    fn data(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        // "Data" control byte 0x40: Co = 0, D/C = 1.
        self.write_chunked(0x40, data)
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
        self.commands(&[cmd])
    }

    fn commands(&mut self, cmds: &[u8]) -> Result<(), Self::Error> {
        self.dc.set_low().map_err(SpiInterfaceError::Pin)?;
        self.spi.write(cmds).map_err(SpiInterfaceError::Spi)
    }

    fn data(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        self.dc.set_high().map_err(SpiInterfaceError::Pin)?;
        self.spi.write(data).map_err(SpiInterfaceError::Spi)
    }
}
