use embassy_time::{Duration, Instant};

/// What the main loop should do this frame.
pub struct AlarmAction {
    /// Speaker duty (0 = silent, 1..=100 = percent)
    pub speaker_duty: u8,
    /// Red LED on
    pub led_red: bool,
    /// Flash the display background red (toggles each beep cycle)
    pub flash_red: bool,
    /// Status text for the UI date line (empty = show normal date)
    pub status_text: Option<&'static str>,
    /// Snooze countdown text (e.g. "Snooze 4:32"), None when not snoozed
    pub snooze_text: Option<(u32, u32)>,
}

#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum AlarmState {
    Idle,
    Ringing,
    Snoozed,
}

pub struct Alarm {
    state: AlarmState,
    /// When the alarm started ringing (for ramp-up and auto-timeout)
    ring_start: Option<Instant>,
    /// When snooze was pressed
    snooze_start: Option<Instant>,
    /// Snooze duration (user-configurable)
    pub snooze_duration: Duration,
    /// Auto-timeout: cancel after this much continuous ringing (user-configurable)
    pub auto_timeout: Duration,
}

impl Alarm {
    pub fn new() -> Self {
        Self {
            state: AlarmState::Idle,
            ring_start: None,
            snooze_start: None,
            snooze_duration: Duration::from_secs(5 * 60),
            auto_timeout: Duration::from_secs(10 * 60),
        }
    }

    pub fn state(&self) -> AlarmState {
        self.state
    }

    /// Trigger the alarm (from test button or time match).
    pub fn trigger(&mut self) {
        if self.state == AlarmState::Idle {
            self.state = AlarmState::Ringing;
            self.ring_start = Some(Instant::now());
        }
    }

    /// Snooze the alarm.
    pub fn snooze(&mut self) {
        if self.state == AlarmState::Ringing {
            self.state = AlarmState::Snoozed;
            self.snooze_start = Some(Instant::now());
        }
    }

    /// Cancel the alarm entirely.
    pub fn cancel(&mut self) {
        self.state = AlarmState::Idle;
        self.ring_start = None;
        self.snooze_start = None;
    }

    /// Call every frame from the main loop. Returns what the hardware should do.
    pub fn tick(&mut self) -> AlarmAction {
        let now = Instant::now();

        match self.state {
            AlarmState::Idle => AlarmAction {
                speaker_duty: 0,
                led_red: false,
                flash_red: false,
                status_text: None,
                snooze_text: None,
            },

            AlarmState::Ringing => {
                // Auto-timeout check
                if let Some(start) = self.ring_start {
                    if now - start > self.auto_timeout {
                        self.cancel();
                        return self.tick();
                    }
                }

                // Ramp duty: 1% at start, up to 5% over 30 seconds
                let elapsed_ms = self.ring_start.map(|s| (now - s).as_millis()).unwrap_or(0);
                let ramp = (elapsed_ms / 7500) as u8; // 0..4 over 30s
                let duty = 1 + ramp.min(4);

                // Beep pattern: 500ms on / 500ms off
                let cycle_ms = (elapsed_ms % 1000) as u32;
                let beep_on = cycle_ms < 500;

                AlarmAction {
                    speaker_duty: if beep_on { duty } else { 0 },
                    led_red: true,
                    flash_red: beep_on,
                    status_text: Some("ALARM"),
                    snooze_text: None,
                }
            }

            AlarmState::Snoozed => {
                // Check if snooze expired
                if let Some(start) = self.snooze_start {
                    let elapsed = now - start;
                    if elapsed >= self.snooze_duration {
                        self.state = AlarmState::Ringing;
                        self.ring_start = Some(now);
                        self.snooze_start = None;
                        return self.tick();
                    }

                    let remaining = self.snooze_duration - elapsed;
                    let secs = remaining.as_secs() as u32;
                    let mins = secs / 60;
                    let secs = secs % 60;

                    AlarmAction {
                        speaker_duty: 0,
                        led_red: false,
                        flash_red: false,
                        status_text: None,
                        snooze_text: Some((mins, secs)),
                    }
                } else {
                    // Shouldn't happen, but recover
                    self.cancel();
                    self.tick()
                }
            }
        }
    }
}
