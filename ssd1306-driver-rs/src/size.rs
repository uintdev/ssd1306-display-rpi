// Physical display size variants

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplaySize {
    // 128x64 pixels (the most common SSD1306 module).
    Size128x64,
    // 128x32 pixels.
    Size128x32,
    // 96x16 pixels.
    Size96x16,
}

// Register values that differ between panel sizes, pulled out of
// `_initialize()` in the original driver.
pub(crate) struct SizeParams {
    pub clock_div_ratio: u8,
    pub multiplex: u8,
    pub com_pins: u8,
    pub contrast_internal: u8,
    pub contrast_external: u8,
}

impl DisplaySize {
    // Width and height in pixels.
    pub fn dimensions(&self) -> (u32, u32) {
        match self {
            DisplaySize::Size128x64 => (128, 64),
            DisplaySize::Size128x32 => (128, 32),
            DisplaySize::Size96x16 => (96, 16),
        }
    }

    pub(crate) fn params(&self) -> SizeParams {
        match self {
            // Values from SSD1306_128_64._initialize().
            DisplaySize::Size128x64 => SizeParams {
                clock_div_ratio: 0x80,
                multiplex: 0x3F,
                com_pins: 0x12,
                contrast_internal: 0xCF,
                contrast_external: 0x9F,
            },
            // Values from SSD1306_128_32._initialize().
            DisplaySize::Size128x32 => SizeParams {
                clock_div_ratio: 0x80,
                multiplex: 0x1F,
                com_pins: 0x02,
                contrast_internal: 0x8F,
                contrast_external: 0x8F,
            },
            // Values from SSD1306_96_16._initialize().
            DisplaySize::Size96x16 => SizeParams {
                clock_div_ratio: 0x60,
                multiplex: 0x0F,
                com_pins: 0x02,
                contrast_internal: 0x8F,
                contrast_external: 0x8F,
            },
        }
    }
}
