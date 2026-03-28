# Alarm Clock for Cheap Yellow Display

This is a project to write an alarm clock for the ESP32-2432S028 (aka "CYD") written in Rust using Embassy.

So far the display, touch, speaker, and RGB LEDs are working.

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

The display is an ILI9486-based 2.8" 320×480 TFT driven over SPI2. The driver is
configured for RGB565 color, landscape orientation (Rotation::Deg90).

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