use core::{cell::RefCell, fmt::Write};
use critical_section::Mutex;
use defmt::info;
use embassy_net::{dns::DnsSocket, tcp::client::TcpClient};
use embassy_time::{Duration, Instant, Timer, with_timeout};
use heapless::{String, Vec};
use reqwless::{Error as ReqwlessError, client::HttpClient, request::Method, response::Status};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeatherError {
    InvalidZip,
    Network,
    Timeout,
    ResponseTooLarge,
    Parse,
    NotFound,
}

#[derive(Debug, Clone)]
pub struct Weather {
    pub temperature_f: i16,
    pub weather_code: u16,
    pub location_name: String<40>,
    pub fetched_unix: u64,
}

const DEFAULT_ZIP: [u8; 5] = *b"00000";

#[derive(Debug, Clone)]
pub struct WeatherState {
    pub zip: [u8; 5],
    pub last_weather: Option<Weather>,
    pub last_error: Option<WeatherError>,
    pub last_attempt: Option<Instant>,
    pub last_ok_unix: Option<u64>,
    pub refresh_now: bool,
}

impl Default for WeatherState {
    fn default() -> Self {
        Self {
            zip: DEFAULT_ZIP,
            last_weather: None,
            last_error: None,
            last_attempt: None,
            last_ok_unix: None,
            refresh_now: false,
        }
    }
}

static WEATHER_STATE: Mutex<RefCell<WeatherState>> = Mutex::new(RefCell::new(WeatherState {
    zip: DEFAULT_ZIP,
    last_weather: None,
    last_error: None,
    last_attempt: None,
    last_ok_unix: None,
    refresh_now: false,
}));

pub fn set_zipcode(zip: [u8; 5]) -> bool {
    if !is_valid_zip(&zip) {
        info!("Weather: invalid ZIP");
        return false;
    }
    critical_section::with(|cs| {
        let mut state = WEATHER_STATE.borrow_ref_mut(cs);
        state.zip = zip;
        state.refresh_now = true;
    });
    info!("Weather: ZIP set to {}", zip_to_string(&zip).as_str());
    true
}

pub fn get_zipcode() -> [u8; 5] {
    critical_section::with(|cs| WEATHER_STATE.borrow_ref(cs).zip)
}

pub fn get_last_weather() -> Option<Weather> {
    critical_section::with(|cs| WEATHER_STATE.borrow_ref(cs).last_weather.clone())
}

fn take_refresh_flag() -> bool {
    critical_section::with(|cs| {
        let mut state = WEATHER_STATE.borrow_ref_mut(cs);
        let flag = state.refresh_now;
        state.refresh_now = false;
        flag
    })
}

pub fn request_refresh() {
    critical_section::with(|cs| {
        WEATHER_STATE.borrow_ref_mut(cs).refresh_now = true;
    });
    info!("Weather: refresh requested");
}

pub fn get_last_error() -> Option<WeatherError> {
    critical_section::with(|cs| WEATHER_STATE.borrow_ref(cs).last_error)
}

pub fn get_last_ok_unix() -> Option<u64> {
    critical_section::with(|cs| WEATHER_STATE.borrow_ref(cs).last_ok_unix)
}

#[derive(Debug, Clone)]
pub struct GeoLocation {
    pub latitude: f32,
    pub longitude: f32,
    pub name: String<40>,
}

#[allow(async_fn_in_trait)]
pub trait HttpGet {
    /// Perform an HTTP GET into `response_buf`, returning the number of bytes written.
    /// Implementations must return `WeatherError::ResponseTooLarge` if the buffer is too small.
    async fn get(&mut self, url: &str, response_buf: &mut [u8]) -> Result<usize, WeatherError>;
}

/// Validate a 5-digit ZIP code (ASCII digits).
pub fn is_valid_zip(zip: &[u8; 5]) -> bool {
    zip.iter().all(|b| b.is_ascii_digit())
}

fn zip_to_string(zip: &[u8; 5]) -> String<5> {
    let mut s: String<5> = String::new();
    for &b in zip {
        let _ = s.push(b as char);
    }
    s
}

/// Build Open-Meteo geocoding URL for ZIP.
pub fn build_geocode_url(zip: &[u8; 5]) -> Result<String<128>, WeatherError> {
    if !is_valid_zip(zip) {
        return Err(WeatherError::InvalidZip);
    }
    let mut url: String<128> = String::new();
    let zip_s = zip_to_string(zip);
    write!(
        url,
        "http://geocoding-api.open-meteo.com/v1/search?name={}&count=1&format=json",
        zip_s
    )
    .map_err(|_| WeatherError::ResponseTooLarge)?;
    Ok(url)
}

/// Build Open-Meteo forecast URL for the given coordinates.
pub fn build_forecast_url(latitude: f32, longitude: f32) -> Result<String<200>, WeatherError> {
    let mut url: String<200> = String::new();
    write!(
        url,
        "http://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,weather_code&temperature_unit=fahrenheit&forecast_days=1",
        latitude, longitude
    )
    .map_err(|_| WeatherError::ResponseTooLarge)?;
    Ok(url)
}

#[derive(Deserialize)]
struct GeoResponse {
    results: Option<Vec<GeoResult, 4>>,
}

#[derive(Deserialize)]
struct GeoResult {
    name: String<32>,
    latitude: f32,
    longitude: f32,
    admin1: Option<String<24>>,
}

#[derive(Deserialize)]
struct ForecastResponse {
    current: Option<ForecastCurrent>,
}

#[derive(Deserialize)]
struct ForecastCurrent {
    temperature_2m: f32,
    weather_code: i32,
}

/// Parse geocoding response into a `GeoLocation`.
pub fn parse_geocode(body: &[u8]) -> Result<GeoLocation, WeatherError> {
    let (parsed, _) =
        serde_json_core::from_slice::<GeoResponse>(body).map_err(|_| WeatherError::Parse)?;
    let mut results = parsed.results.ok_or(WeatherError::NotFound)?;
    if results.is_empty() {
        return Err(WeatherError::NotFound);
    }
    let first = results.swap_remove(0);
    let mut location_name: String<40> = String::new();
    let _ = location_name.push_str(first.name.as_str());
    if let Some(admin1) = first.admin1 {
        let _ = location_name.push_str(", ");
        let _ = location_name.push_str(admin1.as_str());
    }
    Ok(GeoLocation {
        latitude: first.latitude,
        longitude: first.longitude,
        name: location_name,
    })
}

/// Parse forecast response into temperature (°F) and weather code.
pub fn parse_forecast(body: &[u8]) -> Result<(i16, u16), WeatherError> {
    let (parsed, _) =
        serde_json_core::from_slice::<ForecastResponse>(body).map_err(|_| WeatherError::Parse)?;
    let current = parsed.current.ok_or(WeatherError::NotFound)?;
    let temp_f = if current.temperature_2m >= 0.0 {
        (current.temperature_2m + 0.5) as i16
    } else {
        (current.temperature_2m - 0.5) as i16
    };
    let code = current.weather_code as u16;
    Ok((temp_f, code))
}

/// Fetch current weather from Open-Meteo, using the provided HTTP client.
/// This function never panics and returns errors instead.
pub async fn fetch_weather<H: HttpGet>(
    http: &mut H,
    zip: &[u8; 5],
    now_unix: u64,
    response_buf: &mut [u8],
) -> Result<Weather, WeatherError> {
    let geo_url = build_geocode_url(zip)?;
    let n = http.get(geo_url.as_str(), response_buf).await?;
    let geo = parse_geocode(&response_buf[..n])?;

    let forecast_url = build_forecast_url(geo.latitude, geo.longitude)?;
    let n = http.get(forecast_url.as_str(), response_buf).await?;
    let (temp_f, code) = parse_forecast(&response_buf[..n])?;

    Ok(Weather {
        temperature_f: temp_f,
        weather_code: code,
        location_name: geo.name,
        fetched_unix: now_unix,
    })
}

fn record_success(weather: Weather) {
    critical_section::with(|cs| {
        let mut state = WEATHER_STATE.borrow_ref_mut(cs);
        state.last_ok_unix = Some(weather.fetched_unix);
        state.last_weather = Some(weather);
        state.last_error = None;
        state.last_attempt = Some(Instant::now());
    });
}

fn record_error(err: WeatherError) {
    critical_section::with(|cs| {
        let mut state = WEATHER_STATE.borrow_ref_mut(cs);
        state.last_error = Some(err);
        state.last_attempt = Some(Instant::now());
    });
}

/// Background weather polling task. Never panics; all failures update shared state.
pub async fn weather_task<H: HttpGet>(
    mut http: H,
    poll_interval: Duration,
    now_unix: fn() -> Option<u64>,
) -> ! {
    let mut response_buf = [0u8; 768];

    info!("Weather: task start (poll {:?})", poll_interval);

    loop {
        let refresh = take_refresh_flag();
        let had_success = get_last_ok_unix().is_some();

        if !refresh {
            let wait = if had_success {
                poll_interval
            } else {
                Duration::from_secs(7)
            };
            Timer::after(wait).await;
            if take_refresh_flag() {
                continue;
            }
        }

        let zip = get_zipcode();
        info!("Weather: fetching for ZIP {}", zip_to_string(&zip).as_str());
        let now = if let Some(n) = now_unix() {
            n
        } else {
            info!("Weather: skipping fetch (time unavailable)");
            Timer::after(Duration::from_secs(5)).await;
            continue;
        };

        let result = with_timeout(
            Duration::from_secs(10),
            fetch_weather(&mut http, &zip, now, &mut response_buf),
        )
        .await;

        match result {
            Ok(Ok(weather)) => {
                info!(
                    "Weather fetch ok: {}F code {}",
                    weather.temperature_f, weather.weather_code
                );
                record_success(weather)
            }
            Ok(Err(e)) => {
                info!("Weather fetch error");
                record_error(e)
            }
            Err(_) => {
                info!("Weather fetch timeout");
                record_error(WeatherError::Timeout)
            }
        }

        // If we have never succeeded yet, back off briefly before the next attempt.
        if get_last_ok_unix().is_none() {
            Timer::after(Duration::from_secs(5)).await;
        }
    }
}

fn map_reqwless_error(err: ReqwlessError) -> WeatherError {
    match err {
        ReqwlessError::BufferTooSmall => WeatherError::ResponseTooLarge,
        _ => WeatherError::Network,
    }
}

pub struct WeatherHttpClient<'a, const N: usize, const TX: usize, const RX: usize> {
    client: HttpClient<'a, TcpClient<'a, N, TX, RX>, DnsSocket<'a>>,
}

impl<'a, const N: usize, const TX: usize, const RX: usize> WeatherHttpClient<'a, N, TX, RX> {
    pub fn new(tcp: &'a TcpClient<'a, N, TX, RX>, dns: &'a DnsSocket<'a>) -> Self {
        Self {
            client: HttpClient::new(tcp, dns),
        }
    }
}

impl<'a, const N: usize, const TX: usize, const RX: usize> HttpGet
    for WeatherHttpClient<'a, N, TX, RX>
{
    async fn get(&mut self, url: &str, response_buf: &mut [u8]) -> Result<usize, WeatherError> {
        let mut request = self
            .client
            .request(Method::GET, url)
            .await
            .map_err(map_reqwless_error)?;
        let response = request
            .send(response_buf)
            .await
            .map_err(map_reqwless_error)?;

        if response.status != Status::Ok {
            return Err(WeatherError::NotFound);
        }

        let body = response
            .body()
            .read_to_end()
            .await
            .map_err(map_reqwless_error)?;

        Ok(body.len())
    }
}
