use core::cell::RefCell;
use critical_section::Mutex;
use defmt::info;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{Ipv4Address, Stack};
use embassy_time::{Duration, Instant, Timer, with_timeout};
use esp_radio::wifi::{ClientConfig, ModeConfig, WifiController, WifiDevice};
use time::{OffsetDateTime, UtcOffset};

const WIFI_SSID: &str = "REDACTED_SSID";
const WIFI_PASSWORD: &str = "REDACTED_PASSWORD";

// time.google.com — fixed IPs, no DNS needed
const NTP_SERVER: Ipv4Address = Ipv4Address::new(216, 239, 35, 0);
const NTP_PORT: u16 = 123;
const NTP_TO_UNIX: u64 = 2_208_988_800;

// --- Networking helpers (DNS/TCP-ready scaffolding) ---
// NOTE: DNS/TCP clients will be added here for the weather service.
// Keep this module hardware-agnostic; use Stack-provided APIs.

// --- Shared time state ---

struct TimeSync {
    unix_secs: u64,
    instant: Instant,
}

static TIME_SYNC: Mutex<RefCell<Option<TimeSync>>> = Mutex::new(RefCell::new(None));

/// Get current time as OffsetDateTime with the given UTC offset, or None if NTP hasn't synced.
pub fn now_with_offset(utc_offset_hours: i8) -> Option<OffsetDateTime> {
    let unix_secs = critical_section::with(|cs| {
        TIME_SYNC.borrow_ref(cs).as_ref().map(|sync| {
            let elapsed = (Instant::now() - sync.instant).as_secs();
            sync.unix_secs + elapsed
        })
    })?;
    let offset = UtcOffset::from_hms(utc_offset_hours, 0, 0).ok()?;
    OffsetDateTime::from_unix_timestamp(unix_secs as i64)
        .ok()
        .map(|dt| dt.to_offset(offset))
}

// --- Tasks ---

#[embassy_executor::task]
pub async fn net_task(mut runner: embassy_net::Runner<'static, WifiDevice<'static>>) -> ! {
    runner.run().await
}

#[embassy_executor::task]
pub async fn wifi_task(mut controller: WifiController<'static>, stack: Stack<'static>) {
    let config = ClientConfig::default()
        .with_ssid(WIFI_SSID.into())
        .with_password(WIFI_PASSWORD.into());
    controller
        .set_config(&ModeConfig::Client(config))
        .expect("Wi-Fi set_config failed");

    info!("Wi-Fi: starting...");
    controller.start().expect("Wi-Fi start failed");

    info!("Wi-Fi: connecting...");
    controller.connect().expect("Wi-Fi connect failed");

    loop {
        if controller.is_connected().unwrap_or(false) {
            break;
        }
        Timer::after(Duration::from_millis(100)).await;
    }
    info!("Wi-Fi: connected!");

    loop {
        if let Some(cfg) = stack.config_v4() {
            info!("DHCP: IP = {}", cfg.address);
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    // NTP sync loop: sync now, then every hour
    loop {
        match ntp_sync(stack).await {
            Ok(secs) => info!("NTP: synced, unix={}", secs),
            Err(()) => info!("NTP: failed, retry in 30s"),
        }
        Timer::after(Duration::from_secs(3600)).await;
    }
}

// --- NTP ---

async fn ntp_sync(stack: Stack<'_>) -> Result<u64, ()> {
    let mut rx_meta = [PacketMetadata::EMPTY; 1];
    let mut rx_buf = [0u8; 128];
    let mut tx_meta = [PacketMetadata::EMPTY; 1];
    let mut tx_buf = [0u8; 128];

    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    socket.bind(0).map_err(|_| ())?;

    // NTP request: LI=0, VN=3, Mode=3 (client)
    let mut request = [0u8; 48];
    request[0] = 0x1B;

    socket
        .send_to(&request, (NTP_SERVER, NTP_PORT))
        .await
        .map_err(|_| ())?;

    // Receive with 5s timeout
    let mut response = [0u8; 48];
    let recv = with_timeout(Duration::from_secs(5), socket.recv_from(&mut response)).await;

    let (n, _) = recv.map_err(|_| ())?.map_err(|_| ())?;
    if n < 48 {
        return Err(());
    }

    // Transmit timestamp: bytes 40-43 (seconds since 1900-01-01)
    let ntp_secs =
        u32::from_be_bytes([response[40], response[41], response[42], response[43]]) as u64;
    let unix_secs = ntp_secs - NTP_TO_UNIX;

    critical_section::with(|cs| {
        TIME_SYNC.borrow_ref_mut(cs).replace(TimeSync {
            unix_secs,
            instant: Instant::now(),
        });
    });

    Ok(unix_secs)
}
