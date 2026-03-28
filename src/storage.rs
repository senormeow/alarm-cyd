//! Non-volatile settings storage using raw SPI flash.
//!
//! Uses a single 4KB flash sector to persist the Settings struct.
//! Format: [MAGIC u8] [version u8] [data...] [checksum u8]
//! On first boot (or corrupt data), returns None and the caller uses defaults.

use crate::settings::Settings;
use bincode::serde::{decode_from_slice, encode_into_slice};
use defmt::info;

/// Flash address for settings storage.
/// 0x3FF000 = last 4KB sector of a 4MB flash chip, well past the app partition.
const SETTINGS_ADDR: u32 = 0x3F_F000;
const SETTINGS_SECTOR: u32 = SETTINGS_ADDR / 4096;

const MAGIC: u8 = 0xA5;
const VERSION: u8 = 2;

// Wire format: [magic, version, bincode_data..., checksum]
const PADDED_LEN: usize = 32; // must be multiple of 4 for flash alignment

fn serialize(settings: &Settings) -> [u8; PADDED_LEN] {
    let mut buf = [0u8; PADDED_LEN];
    buf[0] = MAGIC;
    buf[1] = VERSION;

    // Serialize settings directly using bincode
    let _ = encode_into_slice(
        settings,
        &mut buf[2..PADDED_LEN - 1],
        bincode::config::standard(),
    )
    .expect("Buffer too small for serialization");

    // Simple checksum: XOR of all bytes before checksum
    let mut cksum: u8 = 0;
    for &b in &buf[..PADDED_LEN - 1] {
        cksum ^= b;
    }
    buf[PADDED_LEN - 1] = cksum;
    buf
}

fn deserialize(buf: &[u8; PADDED_LEN]) -> Option<Settings> {
    if buf[0] != MAGIC || buf[1] != VERSION {
        return None;
    }
    // Verify checksum
    let mut cksum: u8 = 0;
    for &b in &buf[..PADDED_LEN - 1] {
        cksum ^= b;
    }
    if cksum != buf[PADDED_LEN - 1] {
        return None;
    }

    // Deserialize settings using bincode
    let (settings, _) =
        decode_from_slice(&buf[2..PADDED_LEN - 1], bincode::config::standard()).ok()?;
    Some(settings)
}

/// Load settings from flash. Returns None on first boot or corrupt data.
pub fn load() -> Option<Settings> {
    let mut buf = [0u8; PADDED_LEN];
    let rc = unsafe {
        esp_hal::rom::spiflash::esp_rom_spiflash_read(
            SETTINGS_ADDR,
            buf.as_mut_ptr() as *mut u32,
            PADDED_LEN as u32,
        )
    };
    if rc != 0 {
        info!("NVS: flash read failed (rc={})", rc);
        return None;
    }
    let result = deserialize(&buf);
    if result.is_some() {
        info!("NVS: loaded settings from flash");
    } else {
        info!("NVS: no valid settings found, using defaults");
    }
    result
}

/// Save settings to flash. Erases the sector first.
pub fn save(settings: &Settings) {
    let buf = serialize(settings);
    unsafe {
        let rc = esp_hal::rom::spiflash::esp_rom_spiflash_unlock();
        if rc != 0 {
            info!("NVS: flash unlock failed (rc={})", rc);
            return;
        }
        let rc = esp_hal::rom::spiflash::esp_rom_spiflash_erase_sector(SETTINGS_SECTOR);
        if rc != 0 {
            info!("NVS: flash erase failed (rc={})", rc);
            return;
        }
        let rc = esp_hal::rom::spiflash::esp_rom_spiflash_write(
            SETTINGS_ADDR,
            buf.as_ptr() as *const u32,
            PADDED_LEN as u32,
        );
        if rc != 0 {
            info!("NVS: flash write failed (rc={})", rc);
            return;
        }
    }
    info!("NVS: settings saved to flash");
}
