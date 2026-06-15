//! 11 active-high indicator LEDs (README §LEDs).

use embassy_stm32::gpio::{AnyPin, Level, Output, Speed};
use embassy_stm32::Peri;

/// Number of LEDs on the board.
pub const COUNT: usize = 11;

/// The board's LED bank. Construct via [`crate::init`].
pub struct Leds {
    out: [Output<'static>; COUNT],
}

impl Leds {
    /// Pins in board order; pass already-degraded [`AnyPin`]s.
    pub fn new(pins: [Peri<'static, AnyPin>; COUNT]) -> Self {
        Self {
            out: pins.map(|p| Output::new(p, Level::Low, Speed::Low)),
        }
    }

    /// Drive LED `i` on or off.
    pub fn set(&mut self, i: usize, on: bool) {
        if on {
            self.out[i].set_high();
        } else {
            self.out[i].set_low();
        }
    }

    /// Light exactly LED `i`, clearing the rest (handy for a heartbeat/chase).
    pub fn only(&mut self, i: usize) {
        for (n, led) in self.out.iter_mut().enumerate() {
            if n == i {
                led.set_high();
            } else {
                led.set_low();
            }
        }
    }

    pub const fn count(&self) -> usize {
        COUNT
    }
}
