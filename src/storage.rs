//! Non-volatile settings storage using raw SPI flash.
//!
//! Uses a single 4KB flash sector to persist the Settings struct.
//! Format: [MAGIC u8] [version u8] [data...] [checksum u8]
//! On first boot (or corrupt data), returns None and the caller uses defaults.

use crate::settings::Settings;
use defmt::info;

/// Flash address for settings storage.
/// 0x3FF000 = last 4KB sector of a 4MB flash chip, well past the app partition.
const SETTINGS_ADDR: u32 = 0x3F_F000;
const SETTINGS_SECTOR: u32 = SETTINGS_ADDR / 4096;

const MAGIC: u8 = 0xA5;
const VERSION: u8 = 1;

// Wire format: [magic, version, alarm_hour, alarm_minute, alarm_am, alarm_enabled,
//               utc_offset(as u8), snooze_minutes, timeout_minutes, use_12h,
//               theme_index, checksum]
const DATA_LEN: usize = 12; // must be multiple of 4 for flash alignment
const PADDED_LEN: usize = 12; // already aligned

fn serialize(settings: &Settings) -> [u8; PADDED_LEN] {
    let mut buf = [0u8; PADDED_LEN];
    buf[0] = MAGIC;
    buf[1] = VERSION;
    buf[2] = settings.alarm_hour;
    buf[3] = settings.alarm_minute;
    buf[4] = settings.alarm_am as u8;
    buf[5] = settings.alarm_enabled as u8;
    buf[6] = settings.utc_offset as u8; // i8 → u8 bit pattern
    buf[7] = settings.snooze_minutes;
    buf[8] = settings.timeout_minutes;
    buf[9] = settings.use_12h as u8;
    buf[10] = settings.theme_index;
    // Simple checksum: XOR of bytes 0..11
    let mut cksum: u8 = 0;
    for &b in &buf[..11] {
        cksum ^= b;
    }
    buf[11] = cksum;
    buf
}

fn deserialize(buf: &[u8; PADDED_LEN]) -> Option<Settings> {
    if buf[0] != MAGIC || buf[1] != VERSION {
        return None;
    }
    // Verify checksum
    let mut cksum: u8 = 0;
    for &b in &buf[..11] {
        cksum ^= b;
    }
    if cksum != buf[11] {
        return None;
    }
    Some(Settings {
        alarm_hour: buf[2],
        alarm_minute: buf[3],
        alarm_am: buf[4] != 0,
        alarm_enabled: buf[5] != 0,
        utc_offset: buf[6] as i8,
        snooze_minutes: buf[7],
        timeout_minutes: buf[8],
        use_12h: buf[9] != 0,
        theme_index: buf[10],
    })
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
