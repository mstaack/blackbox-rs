//! 13 active-low buttons with board pull-ups (README §buttons).

use embassy_stm32::gpio::{AnyPin, Input, Pull};
use embassy_stm32::Peri;

/// Number of buttons.
pub const COUNT: usize = 13;

/// A named button. Discriminant = position in the pin order passed to [`Buttons::new`].
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum Button {
    Pads,
    Keys,
    Seqs,
    Song,
    Fx,
    Mix,
    Pset,
    Tools,
    Rec,
    Stop,
    Play,
    Back,
    Info,
}

impl Button {
    /// Every button, in pin order.
    pub const ALL: [Button; COUNT] = [
        Button::Pads,
        Button::Keys,
        Button::Seqs,
        Button::Song,
        Button::Fx,
        Button::Mix,
        Button::Pset,
        Button::Tools,
        Button::Rec,
        Button::Stop,
        Button::Play,
        Button::Back,
        Button::Info,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn label(self) -> &'static str {
        match self {
            Button::Pads => "PADS",
            Button::Keys => "KEYS",
            Button::Seqs => "SEQS",
            Button::Song => "SONG",
            Button::Fx => "FX",
            Button::Mix => "MIX",
            Button::Pset => "PSET",
            Button::Tools => "TOOLS",
            Button::Rec => "REC",
            Button::Stop => "STOP",
            Button::Play => "PLAY",
            Button::Back => "BACK",
            Button::Info => "INFO",
        }
    }
}

/// The board's button bank. Construct via [`crate::init`].
pub struct Buttons {
    inp: [Input<'static>; COUNT],
}

impl Buttons {
    /// Pins in [`Button::ALL`] order; pass already-degraded [`AnyPin`]s. Board has pull-ups.
    pub fn new(pins: [Peri<'static, AnyPin>; COUNT]) -> Self {
        Self {
            inp: pins.map(|p| Input::new(p, Pull::None)),
        }
    }

    /// Is this button currently pressed (active-low → pin low)?
    pub fn is_pressed(&self, b: Button) -> bool {
        self.inp[b.index()].is_low()
    }

    /// Pressed state of every button, indexed by [`Button::index`].
    pub fn pressed(&self) -> [bool; COUNT] {
        core::array::from_fn(|i| self.inp[i].is_low())
    }
}
