#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

extern crate alloc;

use alloc::rc::Rc;
use core::cell::RefCell;
use defmt::info;
use display_interface_spi::SPIInterface;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use embedded_hal_bus::spi::RefCellDevice;
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::gpio::DriveMode;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig};
use esp_hal::ledc::{
    LSGlobalClkSource, Ledc, LowSpeed,
    channel::{self, ChannelIFace},
    timer::{self, TimerIFace},
};
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use esp_println as _;

use embedded_graphics::{pixelcolor::Rgb565, prelude::*, primitives::Rectangle};
use mipidsi::Builder;
use mipidsi::{
    models::ILI9341Rgb565,
    options::{ColorOrder, Orientation, Rotation},
};

use slint::platform::software_renderer::{
    LineBufferProvider, MinimalSoftwareWindow, RepaintBufferType, Rgb565Pixel,
};

use alarm_cyd::xpt2046::Xpt2046;

slint::include_modules!();

// --- Slint platform backend ---

struct Esp32Platform {
    window: Rc<MinimalSoftwareWindow>,
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

// --- Line-buffer renderer: sends one scanline at a time to the display ---

struct DisplayLine<'a, D> {
    display: &'a mut D,
    line_buffer: [Rgb565Pixel; 320],
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

// ---

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[unsafe(link_section = ".dram2_uninit")] size: 98767);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0);

    info!("Embassy initialized!");

    // RGB LEDs (active-low: High = off, Low = on)
    let mut red_led = Output::new(peripherals.GPIO4, Level::High, OutputConfig::default());
    let mut green_led = Output::new(peripherals.GPIO16, Level::High, OutputConfig::default());
    let mut blue_led = Output::new(peripherals.GPIO17, Level::High, OutputConfig::default());

    // Boot LED test
    red_led.set_low();
    Timer::after(Duration::from_millis(300)).await;
    red_led.set_high();
    green_led.set_low();
    Timer::after(Duration::from_millis(300)).await;
    green_led.set_high();
    blue_led.set_low();
    Timer::after(Duration::from_millis(300)).await;
    blue_led.set_high();

    // SPI2 @ 40 MHz — display (ILI9341)
    let spi_bus = Spi::new(
        peripherals.SPI2,
        SpiConfig::default().with_frequency(Rate::from_mhz(40)),
    )
    .unwrap()
    .with_sck(peripherals.GPIO14)
    .with_mosi(peripherals.GPIO13)
    .with_miso(peripherals.GPIO12);

    // SPI3 @ 1 MHz — touch controller (XPT2046)
    let spi_bus2 = Spi::new(
        peripherals.SPI3,
        SpiConfig::default().with_frequency(Rate::from_mhz(1)),
    )
    .unwrap()
    .with_sck(peripherals.GPIO25)
    .with_mosi(peripherals.GPIO32)
    .with_miso(peripherals.GPIO39);

    let display_cs = Output::new(peripherals.GPIO15, Level::High, OutputConfig::default());
    let display_dc = Output::new(peripherals.GPIO2, Level::High, OutputConfig::default());
    let touch_cs = Output::new(peripherals.GPIO33, Level::High, OutputConfig::default());
    let mut back_light = Output::new(peripherals.GPIO21, Level::Low, OutputConfig::default());
    let touch_irq = Input::new(peripherals.GPIO36, InputConfig::default());

    let spi_bus_ref_cell = RefCell::new(spi_bus);
    let spi_bus2_ref_cell = RefCell::new(spi_bus2);

    let touch_device = RefCellDevice::new(&spi_bus2_ref_cell, touch_cs, Delay::new()).unwrap();
    let display_device = RefCellDevice::new(&spi_bus_ref_cell, display_cs, Delay::new()).unwrap();

    let display_interface = SPIInterface::new(display_device, display_dc);
    let mut delay = Delay::new();

    let mut display = Builder::new(ILI9341Rgb565, display_interface)
        .color_order(ColorOrder::Rgb)
        .display_size(240, 320)
        .orientation(Orientation {
            rotation: Rotation::Deg90,
            mirrored: false,
        })
        .init(&mut delay)
        .unwrap();

    back_light.set_high();
    display.clear(Rgb565::BLACK).unwrap();

    let mut touch_controller = Xpt2046::new(touch_device);
    touch_controller.init(&mut delay).unwrap();

    // Speaker via LEDC PWM on GPIO26
    let mut ledc = Ledc::new(peripherals.LEDC);
    ledc.set_global_slow_clock(LSGlobalClkSource::APBClk);
    let mut lstimer0 = ledc.timer::<LowSpeed>(timer::Number::Timer0);
    lstimer0
        .configure(timer::config::Config {
            duty: timer::config::Duty::Duty10Bit,
            clock_source: timer::LSClockSource::APBClk,
            frequency: Rate::from_khz(1),
        })
        .expect("LEDC timer config failed");
    let mut speaker_channel = ledc.channel(channel::Number::Channel0, peripherals.GPIO26);
    speaker_channel
        .configure(channel::config::Config {
            timer: &lstimer0,
            duty_pct: 1,
            drive_mode: DriveMode::PushPull,
        })
        .expect("LEDC channel config failed");
    Timer::after(Duration::from_millis(500)).await;
    speaker_channel.set_duty(0).expect("speaker silence failed");

    let _ = spawner;

    // --- Set up Slint ---
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    window.set_size(slint::PhysicalSize::new(320, 240));

    slint::platform::set_platform(alloc::boxed::Box::new(Esp32Platform {
        window: window.clone(),
    }))
    .unwrap();

    let app = MainWindow::new().unwrap();
    app.set_time_text("00:00:00".into());
    app.set_date_text("Smart Alarm".into());
    app.show().unwrap();

    // Touch state
    let mut was_touched = false;
    let mut last_touch_pos = slint::LogicalPosition::new(0.0_f32, 0.0_f32);

    // --- Main loop ---
    // Calibration reference: to re-calibrate, switch touch_controller to CALIBRATION mode
    // and draw target circles at screen coordinates (20,25), (160,220), (300,110),
    // then record the raw XPT2046 values and update CalibrationData in src/xpt2046/mod.rs.
    loop {
        let is_touched = touch_irq.is_low();

        if is_touched {
            let pt = touch_controller.read_touch_point().unwrap();
            let pos = slint::LogicalPosition::new(pt.x as f32, pt.y as f32);
            if !was_touched {
                window.dispatch_event(slint::platform::WindowEvent::PointerPressed {
                    position: pos,
                    button: slint::platform::PointerEventButton::Left,
                });
                was_touched = true;
            } else {
                window.dispatch_event(slint::platform::WindowEvent::PointerMoved {
                    position: pos,
                });
            }
            last_touch_pos = pos;
        } else if was_touched {
            window.dispatch_event(slint::platform::WindowEvent::PointerReleased {
                position: last_touch_pos,
                button: slint::platform::PointerEventButton::Left,
            });
            was_touched = false;
        }

        slint::platform::update_timers_and_animations();

        window.draw_if_needed(|renderer: &slint::platform::software_renderer::SoftwareRenderer| {
            renderer.render_by_line(DisplayLine {
                display: &mut display,
                line_buffer: [Rgb565Pixel(0); 320],
            });
        });

        Timer::after(Duration::from_millis(16)).await; // ~60 fps
    }
}
