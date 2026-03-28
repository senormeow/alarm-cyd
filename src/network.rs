use defmt::info;
use embassy_net::Stack;
use embassy_time::{Duration, Timer};
use esp_radio::wifi::{ClientConfig, ModeConfig, WifiController, WifiDevice};

const WIFI_SSID: &str = "REDACTED_SSID";
const WIFI_PASSWORD: &str = "REDACTED_PASSWORD";

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
        if let Some(config) = stack.config_v4() {
            info!("DHCP: IP = {}", config.address);
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    // Keep task alive so controller isn't dropped
    loop {
        Timer::after(Duration::from_secs(60)).await;
    }
}
