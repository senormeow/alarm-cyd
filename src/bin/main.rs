#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

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

// Graphics Imports
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_9X18},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Circle, PrimitiveStyle},
    text::Text,
    text::renderer::CharacterStyle,
};
use mipidsi::Builder;
use mipidsi::{
    models::ILI9486Rgb565,
    options::{ColorOrder, Orientation, Rotation},
};

use alarm_cyd::xpt2046::Xpt2046;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

extern crate alloc;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    // generator version: 0.6.0

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

    // Quick RGB LED cycle test
    info!("LED test: Red");
    red_led.set_low();
    Timer::after(Duration::from_millis(300)).await;
    red_led.set_high();

    info!("LED test: Green");
    green_led.set_low();
    Timer::after(Duration::from_millis(300)).await;
    green_led.set_high();

    info!("LED test: Blue");
    blue_led.set_low();
    Timer::after(Duration::from_millis(300)).await;
    blue_led.set_high();
    info!("LED test done.");

    let spi_config1 = SpiConfig::default().with_frequency(Rate::from_mhz(40));
    let spi_config2 = SpiConfig::default().with_frequency(Rate::from_mhz(1));

    let spi_bus = Spi::new(peripherals.SPI2, spi_config1)
        .unwrap()
        .with_sck(peripherals.GPIO14)
        .with_mosi(peripherals.GPIO13)
        .with_miso(peripherals.GPIO12);

    let spi_bus2 = Spi::new(peripherals.SPI3, spi_config2)
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

    let mut display = Builder::new(ILI9486Rgb565, display_interface)
        .color_order(ColorOrder::Rgb)
        .display_size(320, 480)
        .orientation(Orientation {
            rotation: Rotation::Deg90,
            mirrored: false,
        })
        .init(&mut delay)
        .unwrap();

    back_light.set_high();
    display.clear(Rgb565::BLACK).unwrap();

    let mut touch_controller = Xpt2046::new(touch_device);

    // --- Speaker on GPIO 26 via LEDC PWM ---
    let mut ledc = Ledc::new(peripherals.LEDC);
    ledc.set_global_slow_clock(LSGlobalClkSource::APBClk);

    let mut lstimer0 = ledc.timer::<LowSpeed>(timer::Number::Timer0);
    lstimer0
        .configure(timer::config::Config {
            duty: timer::config::Duty::Duty10Bit,
            clock_source: timer::LSClockSource::APBClk,
            frequency: Rate::from_khz(1), // 1 kHz audible tone
        })
        .expect("Failed to configure LEDC timer for speaker");

    let mut speaker_channel = ledc.channel(channel::Number::Channel0, peripherals.GPIO26);
    speaker_channel
        .configure(channel::config::Config {
            timer: &lstimer0,
            duty_pct: 1, // 1% duty = lower volume
            drive_mode: DriveMode::PushPull,
        })
        .expect("Failed to configure LEDC channel for speaker");

    // Beep for 500 ms then silence
    info!("Beeping speaker...");
    Timer::after(Duration::from_millis(500)).await;
    speaker_channel
        .set_duty(0)
        .expect("Failed to silence speaker");
    info!("Speaker beep done.");

    let radio_init = esp_radio::init().expect("Failed to initialize Wi-Fi/BLE controller");
    let (mut _wifi_controller, _interfaces) =
        esp_radio::wifi::new(&radio_init, peripherals.WIFI, Default::default())
            .expect("Failed to initialize Wi-Fi controller");

    // TODO: Spawn some tasks
    let _ = spawner;

    let style = MonoTextStyle::new(&FONT_9X18, Rgb565::WHITE);
    Text::new("Hello world!", Point::new(5, 10), style)
        .draw(&mut display)
        .unwrap();

    //draw a circle
    Circle::new(Point::new(20, 25), 6)
        .into_styled(PrimitiveStyle::with_stroke(Rgb565::RED, 3))
        .draw(&mut display)
        .unwrap();

    Circle::new(Point::new(160, 220), 6)
        .into_styled(PrimitiveStyle::with_stroke(Rgb565::GREEN, 3))
        .draw(&mut display)
        .unwrap();

    Circle::new(Point::new(300, 110), 6)
        .into_styled(PrimitiveStyle::with_stroke(Rgb565::BLUE, 3))
        .draw(&mut display)
        .unwrap();

    touch_controller.init(&mut delay).unwrap();

    loop {
        let touch_point_raw = touch_controller.read_xy().unwrap();
        let touch_point = touch_controller.read_touch_point().unwrap();
        info!("Touch point: {:?}", touch_point_raw);
        Circle::new(touch_point, 6)
            .into_styled(PrimitiveStyle::with_stroke(Rgb565::WHITE, 3))
            .draw(&mut display)
            .unwrap();

        Timer::after(Duration::from_millis(100)).await;
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.0.0-rc.1/examples/src/bin
}
