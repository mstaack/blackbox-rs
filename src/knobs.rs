//! 4 endless rotary encoders, read through ADC1 (README §knobs).
//!
//! Each Alps endless encoder has two analog wipers 90° out of phase; `atan2` of the two
//! recovers an absolute angle. 16-bit + 8× hardware averaging + a long sample window settle
//! the resistive wipers against SAI crosstalk.

use embassy_stm32::adc::{Adc, AdcConfig, AnyAdcChannel, Averaging, Resolution, SampleTime};
use embassy_stm32::peripherals::ADC1;

/// Number of encoders.
pub const COUNT: usize = 4;

/// A named encoder. Discriminant = wiper-pair position in [`Knobs::new`].
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum Knob {
    /// Top-left
    Tl,
    /// Top-right
    Tr,
    /// Bottom-left
    Bl,
    /// Bottom-right
    Br,
}

impl Knob {
    pub const ALL: [Knob; COUNT] = [Knob::Tl, Knob::Tr, Knob::Bl, Knob::Br];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn label(self) -> &'static str {
        match self {
            Knob::Tl => "TL",
            Knob::Tr => "TR",
            Knob::Bl => "BL",
            Knob::Br => "BR",
        }
    }
}

/// One encoder reading: the two raw wipers and the recovered angle.
#[derive(Clone, Copy)]
pub struct Reading {
    pub a: u16,
    pub b: u16,
}

impl Reading {
    /// Absolute angle in degrees from the two 90°-out-of-phase wipers.
    pub fn angle_deg(&self) -> i32 {
        const MID: f32 = 32768.0; // 16-bit midscale
        let rad = libm::atan2f(self.a as f32 - MID, self.b as f32 - MID);
        (rad * 180.0 / core::f32::consts::PI) as i32
    }
}

/// Long sample window settling the resistive wipers against SAI crosstalk.
const SAMPLE_TIME: SampleTime = SampleTime::CYCLES810_5;

/// The board's 4 encoders on ADC1. Construct via [`crate::init`].
pub struct Knobs {
    adc: Adc<'static, ADC1>,
    /// Wiper channels, paired per encoder in [`Knob::ALL`] order: [a, b, a, b, ...].
    wipers: [AnyAdcChannel<'static, ADC1>; COUNT * 2],
}

impl Knobs {
    pub fn new(
        adc1: embassy_stm32::Peri<'static, ADC1>,
        wipers: [AnyAdcChannel<'static, ADC1>; COUNT * 2],
    ) -> Self {
        // 16-bit + 8× hardware averaging, applied through the v0.6 AdcConfig.
        let adc = Adc::new_with_config(
            adc1,
            AdcConfig {
                resolution: Some(Resolution::BITS16),
                averaging: Some(Averaging::Samples8),
            },
        );
        Self { adc, wipers }
    }

    /// Sample one encoder.
    pub fn read(&mut self, k: Knob) -> Reading {
        let i = k.index() * 2;
        let a = self.adc.blocking_read(&mut self.wipers[i], SAMPLE_TIME);
        let b = self.adc.blocking_read(&mut self.wipers[i + 1], SAMPLE_TIME);
        Reading { a, b }
    }

    /// Sample all 4 encoders, indexed by [`Knob::index`].
    pub fn read_all(&mut self) -> [Reading; COUNT] {
        core::array::from_fn(|i| {
            let a = self.adc.blocking_read(&mut self.wipers[i * 2], SAMPLE_TIME);
            let b = self.adc.blocking_read(&mut self.wipers[i * 2 + 1], SAMPLE_TIME);
            Reading { a, b }
        })
    }
}
