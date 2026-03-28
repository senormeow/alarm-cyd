# Alarm Clock for Cheap Yellow Display

This is a project to write an alarm clock for the ESP32-2432S028 (aka "CYD") written in Rust using Embassy.

Display, touch, speaker, RGB LEDs, Wi-Fi, NTP clock, Slint GUI, full settings screen, color theming, alarm engine, and flash-based settings persistence are all working.

## Roadmap

### ✅ Phase 0 — Hardware Validation (complete)
- [x] ILI9341 display over SPI2
- [x] XPT2046 resistive touch over SPI3
- [x] RGB LED (active-low, GPIO 4/16/17)
- [x] Speaker PWM via LEDC (GPIO26)
- [x] Wi-Fi radio initialized

### ✅ Phase 1 — NTP Time Sync & Clock Display (complete)
- [x] Wi-Fi connection task
- [x] NTP UDP client (port 123, time.google.com)
- [x] Software RTC via embassy-time (NTP sync + elapsed tracking)
- [x] Clock face display (HH:MM:SS, weekday/month/day)

### ✅ Phase 2 — Slint GUI & Settings (complete)
- [x] Slint embedded software renderer
- [x] Main clock UI (.slint file)
- [x] Touch input integration
- [x] AM/PM time format
- [x] Settings screen (alarm time, timezone, snooze duration)
- [x] Color theming

### ✅ Phase 3 — Alarm Engine (complete)

#### Alarm State Machine

The alarm runs as a simple state machine driven from the main loop:

```
              ┌──────────┐
              │   Idle   │ ◄──── Cancel button OR auto-timeout
              └────┬─────┘
                   │ trigger (test button OR time match at :00 of alarm minute)
                   ▼
              ┌──────────┐
              │ Ringing  │ ── speaker beeps, display flashes red, RGB LED red
              └────┬─────┘
                   │ Snooze button
                   ▼
              ┌──────────┐
              │ Snoozed  │ ── silent, 5-min countdown shown on display
              └────┬─────┘
                   │ timer expires
                   ▼
              (back to Ringing)
```

#### Behavior

- **Ringing**: Speaker beeps in a pattern (e.g., 500ms on / 500ms off). Duty ramps
  up gradually over ~30s from 1% to 5%. Display background flashes red/black each
  cycle. RGB LED solid red.
- **Snooze**: Silences speaker, turns off LED, returns display to normal clock face.
  Shows "Snooze 4:59" countdown on the date line. After snooze duration (default 5
  minutes, user-configurable), returns to Ringing.
- **Cancel**: Fully stops the alarm, returns to Idle. Resets all state.
- **Auto-timeout**: If the alarm rings continuously for the auto-timeout duration
  (default 10 minutes, user-configurable) without user interaction, it cancels
  itself (vacation safeguard).

#### UI Layout (during alarm)

The clock face stays visible. The bottom area changes based on state:
- **Idle**: "Test Alarm" button (replaces current "Tap me" button)
- **Ringing**: "Snooze" and "Cancel" buttons side by side, background pulses red
- **Snoozed**: "Cancel" button, date line shows snooze countdown

#### Code Structure

- **`src/alarm.rs`** — alarm state machine (`AlarmState` enum, transition logic,
  timing). Pure logic, no hardware. Exposes `tick()` method called from main loop
  that returns `AlarmAction` (beep on/off, LED on/off, flash on/off). Snooze
  duration and auto-timeout are configurable fields so they can later be wired
  to a settings screen.
- **`ui/main.slint`** — add `alarm-state` property (int: 0=idle, 1=ringing, 2=snoozed),
  `alarm-status-text` property, callbacks for snooze/cancel/test buttons. Conditional
  UI layout based on alarm-state.
- **`src/bin/main.rs`** — main loop calls `alarm.tick()`, applies actions to speaker
  channel and RGB LED, sets Slint properties. Handles Slint callbacks to drive
  state transitions.

#### Settings Screen Design

Accessed via a "Settings" gear/button on the main clock face (bottom-left corner).
Navigates to a full-screen settings view with a "Back" button to return to the clock.

**Settings to expose:**
- **Alarm time** — hour and minute pickers (touch +/- buttons), AM/PM toggle, enable/disable
- **Timezone** — UTC offset picker (-12 to +14), shown as "UTC-6" style
- **Snooze duration** — picker in minutes (1–30, default 5)
- **Auto-timeout** — picker in minutes (1–60, default 10)
- **12h/24h format** — toggle between AM/PM and 24-hour display
- **Theme** — preset selector: Midnight (blue), Sunny (amber), Fire (red), Water (teal)

**Themes (memory-efficient):**

No bitmaps — themes are pure color sets applied via Slint properties. Each theme
defines 5 colors. Background styling uses thin colored accent bars (top/bottom
rectangles) rendered by Slint, zero memory overhead.

| Theme    | Background | Clock text | Date text | Accent   | Accent bar     |
|----------|-----------|------------|-----------|----------|----------------|
| Midnight | #000000   | #c0c0ff   | #6060a0   | #4040cc  | #101030        |
| Sunny    | #000000   | #ffe080   | #a08030   | #cc8800  | #1a1400        |
| Fire     | #000000   | #ff6644   | #a04030   | #cc2200  | #1a0800        |
| Water    | #000000   | #40e0d0   | #308080   | #008888  | #001414        |

**Code structure:**
- **`src/settings.rs`** — `Settings` struct (alarm time, tz offset, snooze mins,
  timeout mins, use_12h, theme index), `Theme` struct with const color definitions,
  `THEMES` array. Pure data, no hardware deps. Portable across ESP32/ESP32-S3.
- **`ui/main.slint`** — `settings-visible` bool property toggles settings overlay.
  Theme colors passed as Slint `color` properties from Rust. Settings values are
  `in-out` properties. Callbacks notify Rust on changes.
- **`src/bin/main.rs`** — reads settings from Slint, applies timezone via
  `network::now_with_offset()`, passes snooze/timeout to `Alarm`, formats time
  in 12h/24h. Only file with hardware deps.
- **`src/network.rs`** — `now_with_offset(offset)` replaces hardcoded constant.

**Hardware abstraction principle:** `settings.rs`, `alarm.rs`, `slint_backend.rs`,
and `network.rs` have zero hardware imports. Only `main.rs` touches `esp_hal`
peripherals. Switching to ESP32-S3 should only require changes to `main.rs` and
`Cargo.toml` features.

### ✅ Phase 2b — Settings Persistence (NVS) (complete)
- [x] Reserve a flash sector for settings storage (after app partition)
- [x] Serialize/deserialize Settings struct to raw bytes with magic + checksum
- [x] Use `esp_rom_sys` spiflash functions (already in dep tree) for read/write/erase
- [x] Load settings on boot, save on settings-changed callback
- [x] Handle first-boot (uninitialized flash) gracefully with defaults

### 🔄 Phase 4 — Weather Service
- [ ] HTTP client via embassy-net TCP
- [ ] Open-Meteo API integration (free, no API key)
- [ ] JSON parsing (serde_json_core)
- [ ] Outside temp & forecast display


### 🔄 Phase 5 — Room Temperature (DS18B20)
- [ ] One-wire driver on CN1 connector (GPIO22 or GPIO27, 4.7kΩ pull-up required)
- [ ] DS18B20 temperature read & parse
- [ ] Display on clock face

## Building

Source the ESP toolchain environment before building:

```
. $HOME/export-esp.sh
```

Then build with:

```
cargo build
```

## Board Overview

The ESP32-2432S028 is a low-cost ESP32 development board with a 2.8" TFT display,
resistive touch screen, RGB LED, speaker amplifier, SD card slot, and an LDR light
sensor. Connectors on the board use 1.25mm Molex PicoBlade (often sold as "mx1.25").

## SPI Buses

### SPI2 (HSPI) — Display

Runs at 40 MHz.

| Pin    | Function  | Notes          |
|--------|-----------|----------------|
| GPIO14 | SCK       | TFT_SCK        |
| GPIO13 | MOSI      | TFT_SDI        |
| GPIO12 | MISO      | TFT_SDO        |
| GPIO15 | CS        | TFT_CS         |
| GPIO2  | DC        | TFT_RS / TFT_DC|

### SPI3 (VSPI-alt) — Touch Controller (XPT2046)

Runs at 1 MHz.

| Pin    | Function  | Notes          |
|--------|-----------|----------------|
| GPIO25 | SCK       | XPT2046_CLK    |
| GPIO32 | MOSI      | XPT2046_MOSI   |
| GPIO39 | MISO      | XPT2046_MISO   |
| GPIO33 | CS        | XPT2046_CS     |
| GPIO36 | IRQ       | XPT2046_IRQ (active low) |

## IO

### Display

The display is an ILI9341-based 2.8" 240×320 TFT driven over SPI2. The driver is
configured for RGB565 color, landscape orientation (Rotation::Deg90), giving a
320×240 usable area.

| Pin    | Function       |
|--------|----------------|
| GPIO21 | Backlight (active high) |

### Touch Screen

Resistive touch panel using an XPT2046 controller on SPI3. Calibration is handled
in software with affine transform coefficients.

### Speaker

| Pin    | Function       | Notes |
|--------|----------------|-------|
| GPIO26 | Speaker output | Digital IO → high-pass filter → low-pass filter → amplifier → 2P Molex PicoBlade (P4) |

Driven via the LEDC PWM peripheral (LowSpeed Timer0 / Channel0). The on-board
amplifier has significant gain — a duty cycle of ~1% is sufficient for comfortable
volume. 50% is *very* loud.

### RGB LED

accent LEDs are active-low (HIGH = off, LOW = on).

| Pin    | Color |
|--------|-------|
| GPIO4  | Red   |
| GPIO16 | Green |
| GPIO17 | Blue  |

All three LEDs confirmed working via RGB cycle test on boot.

### SD Card (VSPI)

| Pin    | Function |
|--------|----------|
| GPIO5  | CS (SS)  |
| GPIO18 | SCK      |
| GPIO19 | MISO     |
| GPIO23 | MOSI     |

> **Note:** These pins are from the [upstream CYD reference](https://github.com/witnessmenow/ESP32-Cheap-Yellow-Display/blob/main/PINS.md) and have not yet been verified on this board.

### LDR (Light Dependent Resistor)

| Pin    | Function           |
|--------|--------------------|
| GPIO34 | Analog light sensor |

> **Note:** This pin is from the [upstream CYD reference](https://github.com/witnessmenow/ESP32-Cheap-Yellow-Display/blob/main/PINS.md) and has not yet been verified on this board.

### Buttons

| Pin   | Function | Notes |
|-------|----------|-------|
| IO0   | BOOT     | Can be used as a general input |
| RESET | Reset    | Hardware reset only            |

> **Note:** These are from the [upstream CYD reference](https://github.com/witnessmenow/ESP32-Cheap-Yellow-Display/blob/main/PINS.md) and have not yet been verified on this board.

## Connectors

> **Note:** Connector pinouts below are from the [upstream CYD reference](https://github.com/witnessmenow/ESP32-Cheap-Yellow-Display/blob/main/PINS.md) and have not been independently verified.

### P1 — Serial (4P Molex PicoBlade)

| Pin  | Use |
|------|-----|
| VIN  | 5V  |
| IO1  | TX  |
| IO3  | RX  |
| GND  |     |

### P3 — GPIO (4P Molex PicoBlade)

| Pin   | Notes |
|-------|-------|
| GND   |       |
| IO35  | Input only, no internal pull-up |
| IO22  | Also on CN1 |
| IO21  | Shared with TFT backlight |

### P4 — Speaker (2P Molex PicoBlade)

Connected to the amplifier output, not directly to GPIO.

### CN1 — GPIO / I2C (4P Molex PicoBlade)

| Pin   | Notes |
|-------|-------|
| GND   |       |
| IO22  | Also on P3 |
| IO27  |       |
| 3.3V  |       |

## Test Points

> **Note:** Connector pinouts below are from the [upstream CYD reference](https://github.com/witnessmenow/ESP32-Cheap-Yellow-Display/blob/main/PINS.md) and have not been independently verified.

| Pad | Use   | Location           |
|-----|-------|--------------------|
| S1  | GND   | Near USB-serial    |
| S2  | 3.3V  | ESP32 rail         |
| S3  | 5V    | Near USB-serial    |
| S4  | GND   | ESP32 rail         |
| S5  | 3.3V  | TFT rail           |
| JP0 | 5V / 3.3V | TFT LDO        |
| JP3 | 5V / 3.3V | ESP32 LDO      |
