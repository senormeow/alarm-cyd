#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

extern crate alloc;

use alloc::rc::Rc;
use core::cell::{Cell, RefCell};
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

use embedded_graphics::{pixelcolor::Rgb565, prelude::*};
use mipidsi::Builder;
use mipidsi::{
    models::ILI9341Rgb565,
    options::{ColorOrder, Orientation, Rotation},
};

use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType, Rgb565Pixel};

use alarm_cyd::alarm::{Alarm, AlarmState};
use alarm_cyd::network;
use alarm_cyd::settings::Settings;
use alarm_cyd::slint_backend::{DisplayLine, Esp32Platform};
use alarm_cyd::storage;
use alarm_cyd::xpt2046::Xpt2046;

slint::include_modules!();

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
        SpiConfig::default().with_frequency(Rate::from_mhz(80)),
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

    // --- Wi-Fi + embassy-net ---
    static RADIO: static_cell::StaticCell<esp_radio::Controller<'static>> =
        static_cell::StaticCell::new();
    let radio_init = RADIO.init(esp_radio::init().expect("radio init failed"));
    let (wifi_controller, interfaces) =
        esp_radio::wifi::new(radio_init, peripherals.WIFI, Default::default())
            .expect("Wi-Fi init failed");

    static NET_RESOURCES: static_cell::StaticCell<embassy_net::StackResources<3>> =
        static_cell::StaticCell::new();
    let (stack, runner) = embassy_net::new(
        interfaces.sta,
        embassy_net::Config::dhcpv4(Default::default()),
        NET_RESOURCES.init(embassy_net::StackResources::new()),
        1234u64,
    );

    spawner.spawn(network::net_task(runner)).unwrap();
    spawner
        .spawn(network::wifi_task(wifi_controller, stack))
        .unwrap();

    // --- Set up Slint ---
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    window.set_size(slint::PhysicalSize::new(320, 240));

    slint::platform::set_platform(alloc::boxed::Box::new(Esp32Platform {
        window: window.clone(),
    }))
    .unwrap();

    let app = MainWindow::new().unwrap();
    app.set_time_text("--:--:--".into());
    app.set_date_text("Connecting...".into());
    app.show().unwrap();

    // Settings + alarm — load from flash or use defaults
    let mut settings = storage::load().unwrap_or_default();
    let mut alarm = Alarm::new();
    alarm.snooze_duration = Duration::from_secs(settings.snooze_minutes as u64 * 60);
    alarm.auto_timeout = Duration::from_secs(settings.timeout_minutes as u64 * 60);

    // Apply default settings to Slint
    fn apply_theme(app: &MainWindow, settings: &Settings) {
        let t = settings.theme();
        app.set_theme_bg(slint_color(t.bg));
        app.set_theme_clock(slint_color(t.clock_text));
        app.set_theme_date(slint_color(t.date_text));
        app.set_theme_accent(slint_color(t.accent));
        app.set_theme_accent_bar(slint_color(t.accent_bar));
        app.set_setting_theme_name(t.name.into());
    }

    fn push_settings_to_slint(app: &MainWindow, settings: &Settings) {
        app.set_setting_alarm_hour(settings.alarm_hour as i32);
        app.set_setting_alarm_minute(settings.alarm_minute as i32);
        app.set_setting_alarm_am(settings.alarm_am);
        app.set_setting_alarm_enabled(settings.alarm_enabled);
        app.set_setting_utc_offset(settings.utc_offset as i32);
        app.set_setting_snooze_min(settings.snooze_minutes as i32);
        app.set_setting_timeout_min(settings.timeout_minutes as i32);
        app.set_setting_use_12h(settings.use_12h);
        app.set_setting_theme_index(settings.theme_index as i32);
        apply_theme(app, settings);
    }

    fn pull_settings_from_slint(app: &MainWindow, settings: &mut Settings) {
        settings.alarm_hour = app.get_setting_alarm_hour() as u8;
        settings.alarm_minute = app.get_setting_alarm_minute() as u8;
        settings.alarm_am = app.get_setting_alarm_am();
        settings.alarm_enabled = app.get_setting_alarm_enabled();
        settings.utc_offset = app.get_setting_utc_offset() as i8;
        settings.snooze_minutes = app.get_setting_snooze_min() as u8;
        settings.timeout_minutes = app.get_setting_timeout_min() as u8;
        settings.use_12h = app.get_setting_use_12h();
        settings.theme_index = app.get_setting_theme_index() as u8;
    }

    push_settings_to_slint(&app, &settings);

    // Slint callbacks — use shared flags polled in main loop
    let alarm_trigger = Rc::new(Cell::new(false));
    let alarm_snooze = Rc::new(Cell::new(false));
    let alarm_cancel = Rc::new(Cell::new(false));
    let settings_dirty = Rc::new(Cell::new(false));

    app.on_test_alarm({
        let flag = alarm_trigger.clone();
        move || flag.set(true)
    });
    app.on_snooze_alarm({
        let flag = alarm_snooze.clone();
        move || flag.set(true)
    });
    app.on_cancel_alarm({
        let flag = alarm_cancel.clone();
        move || flag.set(true)
    });
    app.on_settings_changed({
        let flag = settings_dirty.clone();
        move || flag.set(true)
    });

    // Touch state
    let mut was_touched = false;
    let mut last_touch_pos = slint::LogicalPosition::new(0.0_f32, 0.0_f32);
    let mut last_sec: u8 = 255;

    // --- Main loop ---
    // Calibration reference: to re-calibrate, switch touch_controller to CALIBRATION mode
    // and draw target circles at screen coordinates (20,25), (160,220), (300,110),
    // then record the raw XPT2046 values and update CalibrationData in src/xpt2046/mod.rs.
    loop {
        // Sync settings when changed in UI
        if settings_dirty.get() {
            settings_dirty.set(false);
            pull_settings_from_slint(&app, &mut settings);
            apply_theme(&app, &settings);
            alarm.snooze_duration = Duration::from_secs(settings.snooze_minutes as u64 * 60);
            alarm.auto_timeout = Duration::from_secs(settings.timeout_minutes as u64 * 60);
            storage::save(&settings);
        }

        // Update clock display when the second changes
        if let Some(dt) = network::now_with_offset(settings.utc_offset) {
            let sec = dt.second();
            if sec != last_sec {
                last_sec = sec;
                use core::fmt::Write;
                let mut buf = heapless::String::<16>::new();
                if settings.use_12h {
                    let h = dt.hour();
                    let (h12, ampm) = match h {
                        0 => (12, "AM"),
                        1..=11 => (h, "AM"),
                        12 => (12, "PM"),
                        _ => (h - 12, "PM"),
                    };
                    let _ = write!(buf, "{:2}:{:02}:{:02} {}", h12, dt.minute(), sec, ampm);
                } else {
                    let _ = write!(buf, "{:02}:{:02}:{:02}", dt.hour(), dt.minute(), sec);
                }
                app.set_time_text(buf.as_str().into());

                let mut dbuf = heapless::String::<16>::new();
                let _ = write!(
                    dbuf,
                    "{} {} {}",
                    match dt.weekday() {
                        time::Weekday::Sunday => "Sun",
                        time::Weekday::Monday => "Mon",
                        time::Weekday::Tuesday => "Tue",
                        time::Weekday::Wednesday => "Wed",
                        time::Weekday::Thursday => "Thu",
                        time::Weekday::Friday => "Fri",
                        time::Weekday::Saturday => "Sat",
                    },
                    match dt.month() {
                        time::Month::January => "Jan",
                        time::Month::February => "Feb",
                        time::Month::March => "Mar",
                        time::Month::April => "Apr",
                        time::Month::May => "May",
                        time::Month::June => "Jun",
                        time::Month::July => "Jul",
                        time::Month::August => "Aug",
                        time::Month::September => "Sep",
                        time::Month::October => "Oct",
                        time::Month::November => "Nov",
                        time::Month::December => "Dec",
                    },
                    dt.day(),
                );
                app.set_date_text(dbuf.as_str().into());

                // Time-based alarm trigger (fires at :00 of the alarm minute)
                if sec == 0 && settings.alarm_enabled && alarm.state() == AlarmState::Idle {
                    if dt.hour() == settings.alarm_hour_24() && dt.minute() == settings.alarm_minute
                    {
                        alarm.trigger();
                    }
                }
            }
        }

        // Process alarm callbacks from Slint
        if alarm_trigger.get() {
            alarm_trigger.set(false);
            alarm.trigger();
        }
        if alarm_snooze.get() {
            alarm_snooze.set(false);
            alarm.snooze();
        }
        if alarm_cancel.get() {
            alarm_cancel.set(false);
            alarm.cancel();
        }

        // Tick alarm state machine
        let action = alarm.tick();

        // Apply speaker
        speaker_channel.set_duty(action.speaker_duty).ok();

        // Apply RGB LED
        if action.led_red {
            red_led.set_low(); // active-low
        } else {
            red_led.set_high();
        }

        // Update Slint alarm state
        let state_int = match alarm.state() {
            AlarmState::Idle => 0,
            AlarmState::Ringing => 1,
            AlarmState::Snoozed => 2,
        };
        app.set_alarm_state(state_int);
        app.set_alarm_flash(action.flash_red);

        // Update alarm status text
        if let Some(text) = action.status_text {
            app.set_alarm_status_text(text.into());
        } else if let Some((mins, secs)) = action.snooze_text {
            use core::fmt::Write;
            let mut sbuf = heapless::String::<20>::new();
            let _ = write!(sbuf, "Snooze {}:{:02}", mins, secs);
            app.set_alarm_status_text(sbuf.as_str().into());
        } else {
            app.set_alarm_status_text("".into());
        }

        // Touch input
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
                window.dispatch_event(slint::platform::WindowEvent::PointerMoved { position: pos });
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

        window.draw_if_needed(
            |renderer: &slint::platform::software_renderer::SoftwareRenderer| {
                renderer.render_by_line(DisplayLine {
                    display: &mut display,
                    line_buffer: [Rgb565Pixel(0); 320],
                });
            },
        );

        Timer::after(Duration::from_millis(16)).await; // ~60 fps
    }
}

fn slint_color(rgb: u32) -> slint::Color {
    slint::Color::from_rgb_u8(
        ((rgb >> 16) & 0xFF) as u8,
        ((rgb >> 8) & 0xFF) as u8,
        (rgb & 0xFF) as u8,
    )
}
