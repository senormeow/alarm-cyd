/*
    A rough re-implementation of the xpt2046 crate from   https://github.com/VersBinarii/xpt2046
*/

use defmt::{Format, Formatter, write};

use embedded_graphics_core::geometry::Point;
use embedded_hal::delay::DelayNs;
use embedded_hal::spi::SpiDevice;

const CHANNEL_SETTING_X: u8 = 0b10010000;
const CHANNEL_SETTING_Y: u8 = 0b11010000;

const MAX_SAMPLES: usize = 32;
const TX_BUFF_LEN: usize = 5;

#[derive(Debug)]
pub enum BusError<SPIError> {
    Spi(SPIError),
}

#[derive(Debug)]
pub enum Error<E> {
    /// SPI bus error
    Bus(E),
    /// Error when calculating new calibration values
    //Calibration(CalibrationError),
    /// Delay error
    Delay,
}

impl<E> Format for Error<E> {
    fn format(&self, fmt: Formatter) {
        match self {
            Error::Bus(_) => write!(fmt, "Bus error"),
            //Error::Calibration(e) => write!(fmt, "Error when calculating calibration for: {}", e),
            Error::Delay => write!(fmt, "Delay error"),
        }
    }
}

#[derive(Debug)]
pub enum TouchScreenState {
    /// Driver waith for touch
    IDLE,
    /// Driver debounces the touch
    PRESAMPLING,
    /// Confirmed touch
    TOUCHED,
    /// Touch released
    RELEASED,
}

#[derive(Debug)]
pub struct TouchSamples {
    /// All the touch samples
    samples: [Point; MAX_SAMPLES],
    /// current number of captured samples
    counter: usize,
}

impl Default for TouchSamples {
    fn default() -> Self {
        Self {
            counter: 0,
            samples: [Point::default(); MAX_SAMPLES],
        }
    }
}

impl TouchSamples {
    pub fn average(&self) -> Point {
        let mut x = 0;
        let mut y = 0;

        for point in self.samples {
            x += point.x;
            y += point.y;
        }
        x /= MAX_SAMPLES as i32;
        y /= MAX_SAMPLES as i32;
        Point::new(x, y)
    }
}

#[derive(Debug)]
pub struct Xpt2046<SPI> {
    /// THe SPI interface
    spi: SPI,
    /// Control pin
    /// Interrupt control pin
    /// Internall buffers tx
    tx_buff: [u8; TX_BUFF_LEN],
    /// Internal buffer for rx
    rx_buff: [u8; TX_BUFF_LEN],
    /// Current driver state
    screen_state: TouchScreenState,
    /// Buffer for the touch data samples
    ts: TouchSamples,
    //calibration_data: CalibrationData,
    //operation_mode: TouchScreenOperationMode,
    // Location of the touch points used for
    // performing manual calibration
    //calibration_point: CalibrationPoint,
}

impl<SPI> Xpt2046<SPI>
where
    SPI: SpiDevice,
{
    pub fn new(spi: SPI) -> Self {
        Self {
            spi,
            tx_buff: [0; TX_BUFF_LEN],
            rx_buff: [0; TX_BUFF_LEN],
            screen_state: TouchScreenState::IDLE,
            ts: TouchSamples::default(),
            //calibration_data: orientation.calibration_data(),
            //operation_mode: TouchScreenOperationMode::NORMAL,
            //calibration_point: orientation.calibration_point(),
        }
    }
}
impl<SPI, SPIError> Xpt2046<SPI>
where
    SPI: SpiDevice<Error = SPIError>,
{
    fn spi_read(&mut self) -> Result<(), Error<BusError<SPIError>>> {
        self.spi
            .transfer(&mut self.rx_buff, &self.tx_buff)
            .map_err(|e| Error::Bus(BusError::Spi(e)))?;
        Ok(())
    }

    pub fn read_xy(&mut self) -> Result<Point, Error<BusError<SPIError>>> {
        self.spi_read()?;

        let x = (self.rx_buff[1] as i32) << 8 | self.rx_buff[2] as i32;
        let y = (self.rx_buff[3] as i32) << 8 | self.rx_buff[4] as i32;
        Ok(Point::new(x, y))
    }

    pub fn init<D: DelayNs>(&mut self, delay: &mut D) -> Result<(), Error<BusError<SPIError>>> {
        self.tx_buff[0] = 0x80;
        self.spi_read()?;
        delay.delay_us(1);

        /*
         * Load the tx_buffer with the channels config
         * for all subsequent reads
         * The byte shifting provides padding to align the read bytes with the
         * DCLK. XPT2046 datasheet figure 12
         */
        self.tx_buff = [
            CHANNEL_SETTING_X >> 3,
            CHANNEL_SETTING_X << 5,
            CHANNEL_SETTING_Y >> 3,
            CHANNEL_SETTING_Y << 5,
            0,
        ];
        Ok(())
    }
}
