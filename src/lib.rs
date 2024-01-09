#![doc = include_str!("../README.md")]
#![cfg_attr(not(feature = "std"), no_std)]

use core::fmt;
use embedded_hal::{
    delay::DelayNs,
    digital::{InputPin, OutputPin, PinState},
};

/// A sensor reading
#[derive(Debug, Clone, Copy)]
pub struct Reading {
    humidity: f32,
    temperature: f32,
}

impl Reading {
    /// Returns the ambient humidity, as a percentage value from 0.0 to 100.0
    pub fn humidity(&self) -> f32 {
        self.humidity
    }

    /// Returns the ambient temperature, in degrees Celsius
    pub fn temperature(&self) -> f32 {
        self.temperature
    }
}

/// A type detailing various errors the DHT sensor can return
#[derive(Debug, Clone)]
pub enum DhtError<HE> {
    /// The DHT sensor was not found on the specified GPIO
    NotPresent,
    /// The checksum provided in the DHT sensor data did not match the checksum of the data itself (expected, calculated)
    ChecksumMismatch(u8, u8),
    /// The seemingly-valid data has impossible values (e.g. a humidity value less than 0 or greater than 100)
    InvalidData,
    /// The read timed out
    Timeout,
    /// Received a low-level error from the HAL while reading or writing to pins
    PinError(HE),
}

impl<HE> From<HE> for DhtError<HE> {
    fn from(error: HE) -> Self {
        DhtError::PinError(error)
    }
}

impl<HE: fmt::Debug> fmt::Display for DhtError<HE> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use DhtError::*;
        match self {
            NotPresent => write!(f, "DHT device not found"),
            ChecksumMismatch(expected, calculated) => write!(
                f,
                "Data read was corrupt (expected checksum {:x}, calculated {:x})",
                expected, calculated
            ),
            InvalidData => f.write_str("Received data is out of range"),
            Timeout => f.write_str("Timed out waiting for a read"),
            PinError(err) => write!(f, "HAL pin error: {:?}", err),
        }
    }
}

#[cfg(feature = "std")]
impl<HE: fmt::Debug> std::error::Error for DhtError<HE> {}

/// Trait that allows us to disable interrupts when reading from the sensor
pub trait InterruptControl {
    fn enable_interrupts(&mut self);
    fn disable_interrupts(&mut self);
}

/// A dummy implementation of InterruptControl that does nothing
pub struct NoopInterruptControl;

impl InterruptControl for NoopInterruptControl {
    fn enable_interrupts(&mut self) {}
    fn disable_interrupts(&mut self) {}
}

/// A trait for reading data from the sensor
///
/// This level of indirection is useful so you can write generic code that
/// does not assume whether a DHT11 or DHT22 sensor is being used.
pub trait DhtSensor<HE> {
    /// Reads data from the sensor and returns a `Reading`
    fn read(&mut self) -> Result<Reading, DhtError<HE>>;
}

#[doc(hidden)]
pub struct Dht<
    HE,
    ID: InterruptControl,
    D: DelayNs,
    P: InputPin<Error = HE> + OutputPin<Error = HE>,
> {
    interrupt_disabler: ID,
    delay: D,
    pin: P,
}

impl<HE, ID: InterruptControl, D: DelayNs, P: InputPin<Error = HE> + OutputPin<Error = HE>>
    Dht<HE, ID, D, P>
{
    fn new(interrupt_disabler: ID, delay: D, pin: P) -> Self {
        Self {
            interrupt_disabler,
            delay,
            pin,
        }
    }

    fn read(&mut self, parse_data: fn(&[u8]) -> (f32, f32)) -> Result<Reading, DhtError<HE>> {
        self.interrupt_disabler.disable_interrupts();
        let res = self.read_uninterruptible(parse_data);
        self.interrupt_disabler.enable_interrupts();
        res
    }

    fn read_uninterruptible(
        &mut self,
        parse_data: fn(&[u8]) -> (f32, f32),
    ) -> Result<Reading, DhtError<HE>> {
        let mut buf: [u8; 5] = [0; 5];

        // Wake up the sensor
        self.pin.set_low()?;
        self.delay.delay_us(3000);

        // Ask for data
        self.pin.set_high()?;
        self.delay.delay_us(25);

        // Wait for DHT to signal data is ready (~80us low followed by ~80us high)
        self.wait_for_level(PinState::High, 85, DhtError::NotPresent)?;
        self.wait_for_level(PinState::Low, 85, DhtError::NotPresent)?;

        // Now read 40 data bits
        for bit in 0..40 {
            // Wait ~50us for high
            self.wait_for_level(PinState::High, 55, DhtError::Timeout)?;

            // See how long it takes to go low, with max of 70us
            let elapsed = self.wait_for_level(PinState::Low, 70, DhtError::Timeout)?;
            // If it took at least 30us to go low, it's a '1' bit
            if elapsed > 30 {
                let byte = bit / 8;
                let shift = 7 - bit % 8;
                buf[byte] |= 1 << shift;
            }
        }

        let checksum = (buf[0..=3]
            .iter()
            .fold(0u16, |accum, next| accum + *next as u16)
            & 0xff) as u8;
        if buf[4] == checksum {
            let (humidity, temperature) = parse_data(&buf);
            if !(0.0..=100.0).contains(&humidity) {
                Err(DhtError::InvalidData)
            } else {
                Ok(Reading {
                    humidity,
                    temperature,
                })
            }
        } else {
            Err(DhtError::ChecksumMismatch(buf[4], checksum))
        }
    }

    fn wait_for_level(
        &mut self,
        level: PinState,
        timeout_us: u32,
        on_timeout: DhtError<HE>,
    ) -> Result<u32, DhtError<HE>> {
        for elapsed in 0..=timeout_us {
            let is_ready = match level {
                PinState::High => self.pin.is_high(),
                PinState::Low => self.pin.is_low(),
            }?;

            if is_ready {
                return Ok(elapsed);
            }
            self.delay.delay_us(1);
        }
        Err(on_timeout)
    }
}

/// A DHT11 sensor
pub struct Dht11<
    HE,
    ID: InterruptControl,
    D: DelayNs,
    P: InputPin<Error = HE> + OutputPin<Error = HE>,
> {
    dht: Dht<HE, ID, D, P>,
}

impl<HE, ID: InterruptControl, D: DelayNs, P: InputPin<Error = HE> + OutputPin<Error = HE>>
    Dht11<HE, ID, D, P>
{
    pub fn new(interrupt_disabler: ID, delay: D, pin: P) -> Self {
        Self {
            dht: Dht::new(interrupt_disabler, delay, pin),
        }
    }

    fn parse_data(buf: &[u8]) -> (f32, f32) {
        (buf[0] as f32, buf[2] as f32)
    }
}

impl<HE, ID: InterruptControl, D: DelayNs, P: InputPin<Error = HE> + OutputPin<Error = HE>>
    DhtSensor<HE> for Dht11<HE, ID, D, P>
{
    fn read(&mut self) -> Result<Reading, DhtError<HE>> {
        self.dht.read(Dht11::<HE, ID, D, P>::parse_data)
    }
}

/// A DHT22 sensor
pub struct Dht22<
    HE,
    ID: InterruptControl,
    D: DelayNs,
    P: InputPin<Error = HE> + OutputPin<Error = HE>,
> {
    dht: Dht<HE, ID, D, P>,
}

impl<HE, ID: InterruptControl, D: DelayNs, P: InputPin<Error = HE> + OutputPin<Error = HE>>
    Dht22<HE, ID, D, P>
{
    pub fn new(interrupt_disabler: ID, delay: D, pin: P) -> Self {
        Self {
            dht: Dht::new(interrupt_disabler, delay, pin),
        }
    }

    fn parse_data(buf: &[u8]) -> (f32, f32) {
        let humidity = (((buf[0] as u16) << 8) | buf[1] as u16) as f32 / 10.0;
        let mut temperature = ((((buf[2] & 0x7f) as u16) << 8) | buf[3] as u16) as f32 / 10.0;
        if buf[2] & 0x80 != 0 {
            temperature = -temperature;
        }
        (humidity, temperature)
    }
}

impl<HE, ID: InterruptControl, D: DelayNs, P: InputPin<Error = HE> + OutputPin<Error = HE>>
    DhtSensor<HE> for Dht22<HE, ID, D, P>
{
    fn read(&mut self) -> Result<Reading, DhtError<HE>> {
        self.dht.read(Dht22::<HE, ID, D, P>::parse_data)
    }
}
