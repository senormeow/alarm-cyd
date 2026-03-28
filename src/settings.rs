/// Color theme for the clock UI.
#[derive(Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
    pub bg: u32,
    pub clock_text: u32,
    pub date_text: u32,
    pub accent: u32,
    pub accent_bar: u32,
}

pub const THEMES: &[Theme] = &[
    Theme {
        name: "Midnight",
        bg: 0x000000,
        clock_text: 0xc0c0ff,
        date_text: 0x6060a0,
        accent: 0x4040cc,
        accent_bar: 0x101030,
    },
    Theme {
        name: "Sunny",
        bg: 0x000000,
        clock_text: 0xffe080,
        date_text: 0xa08030,
        accent: 0xcc8800,
        accent_bar: 0x1a1400,
    },
    Theme {
        name: "Fire",
        bg: 0x000000,
        clock_text: 0xff6644,
        date_text: 0xa04030,
        accent: 0xcc2200,
        accent_bar: 0x1a0800,
    },
    Theme {
        name: "Water",
        bg: 0x000000,
        clock_text: 0x40e0d0,
        date_text: 0x308080,
        accent: 0x008888,
        accent_bar: 0x001414,
    },
];

/// All user-configurable settings.
pub struct Settings {
    /// Alarm hour (1-12 in 12h mode, 0-23 in 24h mode)
    pub alarm_hour: u8,
    /// Alarm minute (0-59)
    pub alarm_minute: u8,
    /// Alarm AM (true) or PM (false), only used in 12h mode
    pub alarm_am: bool,
    /// Alarm enabled
    pub alarm_enabled: bool,
    /// UTC offset in hours (-12 to +14)
    pub utc_offset: i8,
    /// Snooze duration in minutes
    pub snooze_minutes: u8,
    /// Auto-timeout in minutes
    pub timeout_minutes: u8,
    /// Use 12-hour (AM/PM) format
    pub use_12h: bool,
    /// Theme index into THEMES
    pub theme_index: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            alarm_hour: 7,
            alarm_minute: 0,
            alarm_am: true,
            alarm_enabled: false,
            utc_offset: -6,
            snooze_minutes: 5,
            timeout_minutes: 10,
            use_12h: true,
            theme_index: 0,
        }
    }
}

impl Settings {
    pub fn theme(&self) -> &Theme {
        &THEMES[self.theme_index as usize % THEMES.len()]
    }

    /// Alarm hour in 24h format for time matching.
    pub fn alarm_hour_24(&self) -> u8 {
        if !self.use_12h {
            return self.alarm_hour;
        }
        match (self.alarm_hour, self.alarm_am) {
            (12, true) => 0,   // 12 AM = midnight
            (12, false) => 12, // 12 PM = noon
            (h, true) => h,
            (h, false) => h + 12,
        }
    }
}
