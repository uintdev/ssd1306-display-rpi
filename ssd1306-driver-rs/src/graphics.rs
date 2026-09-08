// Optional [`embedded-graphics`](https://docs.rs/embedded-graphics) support,
// enabled with the `graphics` feature.

use crate::display::Ssd1306;
use crate::interface::DisplayInterface;
use embedded_graphics::{
    Pixel,
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::BinaryColor,
};

impl<I, RST> OriginDimensions for Ssd1306<I, RST>
where
    I: DisplayInterface,
{
    fn size(&self) -> Size {
        Size::new(self.width(), self.height())
    }
}

impl<I, RST> DrawTarget for Ssd1306<I, RST>
where
    I: DisplayInterface,
{
    type Color = BinaryColor;
    type Error = core::convert::Infallible;

    fn draw_iter<Iter>(&mut self, pixels: Iter) -> Result<(), Self::Error>
    where
        Iter: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            if point.x >= 0 && point.y >= 0 {
                self.set_pixel(point.x as u32, point.y as u32, color.is_on());
            }
        }
        Ok(())
    }
}
