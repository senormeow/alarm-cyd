# CLAUDE.md

## Project
ESP32-2432S028 ("CYD") smart alarm clock in Rust using Embassy async runtime.

## Build
```
. $HOME/export-esp.sh   # source ESP toolchain first
cargo build
```

## Hardware
- **MCU**: ESP32 dual-core 240MHz, 520KB SRAM, 98KB heap (`.dram2_uninit`)
- **Display**: ILI9341 240x320 TFT on SPI2 @ 40MHz, landscape via Rotation::Deg90 = 320x240
- **Touch**: XPT2046 resistive on SPI3 @ 1MHz, calibrated with affine transform
- **Speaker**: GPIO26 via LEDC PWM (1% duty = comfortable volume)
- **RGB LED**: Active-low on GPIO 4/16/17
- **Wi-Fi**: esp-radio + embassy-net, credentials in `src/network.rs`

## Architecture
- `src/bin/main.rs` — entry point, hardware init, Slint main loop, alarm time-match trigger
- `src/slint_backend.rs` — Esp32Platform + DisplayLine (Slint rendering adapter)
- `src/network.rs` — Wi-Fi connection task, embassy-net stack runner, `now_with_offset()`
- `src/xpt2046/mod.rs` — touch driver with calibration (do not modify calibration data without re-calibrating)
- `src/alarm.rs` — AlarmState machine (Idle/Ringing/Snoozed), speaker ramp, snooze countdown, auto-timeout
- `src/settings.rs` — Settings struct, Theme/THEMES array (Midnight/Sunny/Fire/Water), Default impl
- `src/storage.rs` — flash persistence via `esp_rom_spiflash_*` ROM calls; sector 0x3FF000, bincode/serde serialization with magic+version+XOR checksum
- `ui/main.slint` — Slint UI: clock face, alarm buttons, full settings screen with all pickers
- `build.rs` — must use `EmbedForSoftwareRenderer` (fonts are pre-baked at build time; without this, runtime font rendering OOMs the 98KB heap)

## Key Constraints
- `#![no_std]` — no standard library
- Heap is exactly 98767 bytes (full capacity of `dram2_seg`), cannot increase without using a different DRAM region
- Slint uses `unsafe-single-threaded` feature — all Slint access must stay on the main task
- `RepaintBufferType::NewBuffer` is required (no retained framebuffer)
- Display controller is ILI9341 (NOT ILI9486 — the CYD board was misidentified initially)
