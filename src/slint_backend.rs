use alloc::rc::Rc;
use embedded_graphics::{pixelcolor::Rgb565, prelude::*, primitives::Rectangle};
use slint::platform::software_renderer::{LineBufferProvider, MinimalSoftwareWindow, Rgb565Pixel};

/// Slint platform backend for ESP32 using Embassy for timekeeping.
pub struct Esp32Platform {
    pub window: Rc<MinimalSoftwareWindow>,
}

impl slint::platform::Platform for Esp32Platform {
    fn create_window_adapter(
        &self,
    ) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> core::time::Duration {
        let d = embassy_time::Duration::from_ticks(embassy_time::Instant::now().as_ticks());
        core::time::Duration::from_millis(d.as_millis())
    }
}

/// Line-buffer renderer: sends one scanline at a time to the display.
pub struct DisplayLine<'a, D> {
    pub display: &'a mut D,
    pub line_buffer: [Rgb565Pixel; 320],
}

impl<D> LineBufferProvider for DisplayLine<'_, D>
where
    D: DrawTarget<Color = Rgb565>,
{
    type TargetPixel = Rgb565Pixel;

    fn process_line(
        &mut self,
        y: usize,
        range: core::ops::Range<usize>,
        render_fn: impl FnOnce(&mut [Rgb565Pixel]),
    ) {
        let start = range.start;
        let end = range.end;
        render_fn(&mut self.line_buffer[start..end]);

        self.display
            .fill_contiguous(
                &Rectangle::new(
                    Point::new(start as i32, y as i32),
                    Size::new((end - start) as u32, 1),
                ),
                self.line_buffer[start..end].iter().map(|p| {
                    Rgb565::new(
                        (p.0 >> 11) as u8 & 0x1F,
                        (p.0 >> 5) as u8 & 0x3F,
                        p.0 as u8 & 0x1F,
                    )
                }),
            )
            .ok();
    }
}
