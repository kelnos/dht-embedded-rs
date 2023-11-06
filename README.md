# dht-embedded

`dht-embedded` is a Rust crate that reads temperature and humidity data
from the DHT11 and DHT22 sensors.

## Prerequisites

* You've connected the DHT sensor to your device (such as a Raspberry
  Pi or ESP32) using a GPIO pin.

## Usage

Add the following to your `Cargo.toml`:

```toml
[dependencies]
dht-embedded = "=0.1.0-alpha.1"
```

Note that this crate currently tracks the 1.0.0 release candidates of
`embedded-hal`, so things can change & break (though since they're in
the RC phase, hopefully they won't), and your platform's `embedded-hal`
implementation may not have trait implementations for the 1.0.0 release
candidates at all, let alone the current one this crate supports.

You will need to use an `embedded-hal` implementation for your hardware.
Here's a simple one using `linux-embedded-hal` and `gpio-cdev, which
could be used on a Rasperry Pi.

```rust,no_run,ignore
use dht_embedded::{Dht22, DhtSensor, NoopInterruptControl};
use gpio_cdev::{Chip, LineRequestFlags};
use linux_embedded_hal::{CdevPin, Delay};
use std::{thread::sleep, time::Duration};

fn main() -> anyhow::Result<()> {
    let mut gpiochip = Chip::new("/dev/gpiochip0")?;
    let line = gpiochip.get_line(17)?;
    let handle = line.request(LineRequestFlags::INPUT | LineRequestFlags::OUTPUT, 1, "dht-sensor")?;
    let pin = CdevPin::new(handle)?;
    let mut sensor = Dht22::new(NoopInterruptControl, Delay, pin);

    loop {
        match sensor.read() {
            Ok(reading) => println!("{}°C, {}% RH", reading.temperature(), reading.humidity()),
            Err(e) => eprintln!("Error: {}", e),
        }

        sleep(Duration::from_millis(2100));
    }
}
```

Note that, if your hardware supports it, you should set the GPIO pin to
"open drain" mode.

(To be fair, the Linux kernel includes a driver for DHT sensors, and
honestly it's probably better to use that driver, since kernel space can
disable interrupts and get much more precise timing than we can.)

## Why

A search of crates.io might yield several different implementations of
this driver.  I wrote this because none of the others worked for me,
and, upon examination of their code, I found they used a completely
different protocols for reading from the sensor, protocols I couldn't
find documented anywhere as what's supposed to work.  This crate
implements one of the simpler protocols that doesn't require access to a
system clock, but still seems to work most of the time.
