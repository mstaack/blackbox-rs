//! Minimal audio bring-up test: clocks + CS42528 + SAI1, 440 Hz sine on the phones.
//!
//! `cargo run --release --example audio_tone`, then listen on the headphone jack.
//! This deliberately skips SDRAM/display/touch so that silence points straight at the
//! SAI/codec path — the smallest thing that proves audio works end to end.

#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::i2c::{self, I2c};
use embassy_stm32::sai::{self, Sai};
use embassy_stm32::time::Hertz;
use {defmt_rtt as _, panic_probe as _};

use blackbox_rs::{audio, clock, Irqs};

#[path = "common/tone.rs"]
mod tone;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_stm32::init(clock::config());

    // CS42528 control port: I2C1 (PB6/PB7), active-low reset on PG13. Configure before MCLK.
    let mut cfg = i2c::Config::default();
    cfg.frequency = Hertz(400_000);
    let mut i2c = I2c::new_blocking(p.I2C1, p.PB6, p.PB7, cfg);
    let mut reset = Output::new(p.PG13, Level::High, Speed::Low);
    let ok = audio::init_codec(&mut i2c, &mut reset).await;
    defmt::info!("audio_tone: codec acked = {}", ok);

    // SAI1_A as I2S stereo master: SCK=PE5, SD=PB2, FS=PE4, MCLK=PE2; DMA1_CH0.
    let (sai_a, _sai_b) = sai::split_subblocks(p.SAI1);
    let tx = Sai::new_asynchronous_with_mclk(
        sai_a, p.PE5, p.PB2, p.PE4, p.PE2, p.DMA1_CH0, audio::tx_buffer(), Irqs, audio::tx_config(),
    );

    defmt::info!("audio_tone: streaming 440 Hz — listen on the phones");
    spawner.spawn(tone::play_440(tx).unwrap()); // runs on after main returns
}
