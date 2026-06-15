//! Board support crate for the Blackbox board (STM32H743XI, rev.Y).
//!
//! [`init`] brings the whole board up at its design clocks and returns a [`Board`] holding
//! ready-to-use drivers. Each subsystem also stands alone in its own module if you want to
//! wire a custom subset.
//!
//! ```ignore
//! #[embassy_executor::main]
//! async fn main(_spawner: Spawner) -> ! {
//!     let mut board = blackbox_rs::init().await;
//!     loop {
//!         let touch = board.touch.poll(&mut board.i2c);
//!         board.display.target().clear(Rgb565::BLACK).ok();
//!         board.display.swap().await;
//!     }
//! }
//! ```

#![no_std]

pub mod audio;
pub mod buttons;
pub mod clock;
pub mod cpu;
pub mod display;
pub mod knobs;
pub mod leds;
pub mod sdram;
pub mod touch;

use embassy_stm32::adc::AdcChannel;
use embassy_stm32::fmc::Fmc;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::i2c::{self, I2c, Master};
use embassy_stm32::mode::Blocking;
use embassy_stm32::sai::{self, Sai};
use embassy_stm32::time::Hertz;
use embassy_stm32::{bind_interrupts, peripherals};

bind_interrupts!(pub struct Irqs {
    LTDC => embassy_stm32::ltdc::InterruptHandler<peripherals::LTDC>;
    DMA1_STREAM0 => embassy_stm32::dma::InterruptHandler<peripherals::DMA1_CH0>;
});

/// The fully initialized board. Touch owns its handle to the shared I2C1 bus.
pub struct Board {
    pub display: display::Display,
    pub leds: leds::Leds,
    pub buttons: buttons::Buttons,
    pub knobs: knobs::Knobs,
    pub touch: touch::Touch,
    /// Shared I2C1 bus — pass to `touch.poll(&mut board.i2c)` (the codec also lives here).
    pub i2c: I2c<'static, Blocking, Master>,
    /// Whether the CS42528 codec acked its init sequence.
    pub codec_ok: bool,
    /// SAI1 I2S stereo transmitter — feed it samples with `board.audio.write(&buf).await`.
    pub audio: audio::ToneTx,
}

/// Bring up clocks, CPU/MPU, SDRAM and every on-board peripheral, configure the codec, and
/// return a ready SAI I2S transmitter. Streaming samples is left to the caller — see
/// [`Board::audio`] and `examples/demo.rs`.
pub async fn init() -> Board {
    let p = embassy_stm32::init(clock::config());
    defmt::info!("blackbox: STM32H743XI, sysclk 399.36 MHz");

    cpu::init();
    cpu::dual_pad_fix();

    // SDRAM (FMC bank1). The constructor consumes ~50 GPIO singletons (all AF12).
    let mut sdram = Fmc::sdram_a13bits_d32bits_4banks_bank1(
        p.FMC, //
        p.PF0, p.PF1, p.PF2, p.PF3, p.PF4, p.PF5, p.PF12, p.PF13, p.PF14, p.PF15, p.PG0, p.PG1,
        p.PG2, // A0-A12
        p.PG4, p.PG5, // BA0-1
        p.PD14, p.PD15, p.PD0, p.PD1, p.PE7, p.PE8, p.PE9, p.PE10, p.PE11, p.PE12, p.PE13, p.PE14,
        p.PE15, p.PD8, p.PD9, p.PD10, p.PH8, p.PH9, p.PH10, p.PH11, p.PH12, p.PH13, p.PH14, p.PH15,
        p.PI0, p.PI1, p.PI2, p.PI3, p.PI6, p.PI7, p.PI9, p.PI10, // D0-D31
        p.PE0, p.PE1, p.PI4, p.PI5, // NBL0-3
        p.PH2, p.PG8, p.PG15, p.PH3, p.PF11, p.PH5, // ctrl
        sdram::Is42s16160j,
    );
    let mut delay = sdram::BusyDelay(800);
    let base = sdram.init(&mut delay) as usize;
    sdram::smoke_test(base);

    let leds = leds::Leds::new([
        p.PG9.into(),
        p.PJ8.into(),
        p.PB10.into(),
        p.PB8.into(),
        p.PB9.into(),
        p.PK2.into(),
        p.PA5.into(),
        p.PJ5.into(),
        p.PJ4.into(),
        p.PB11.into(),
        p.PA4.into(),
    ]);

    let buttons = buttons::Buttons::new([
        p.PI8.into(),
        p.PD4.into(),
        p.PD7.into(),
        p.PI12.into(),
        p.PI14.into(),
        p.PG3.into(),
        p.PH7.into(),
        p.PC13.into(),
        p.PB13.into(),
        p.PJ1.into(),
        p.PH4.into(),
        p.PC2.into(),
        p.PC3.into(),
    ]);

    let knobs = knobs::Knobs::new(
        p.ADC1,
        [
            p.PA0.degrade_adc(), // TL a
            p.PA1.degrade_adc(), // TL b
            p.PA6.degrade_adc(), // TR a
            p.PC4.degrade_adc(), // TR b
            p.PB1.degrade_adc(), // BL a
            p.PA7.degrade_adc(), // BL b
            p.PC5.degrade_adc(), // BR a
            p.PB0.degrade_adc(), // BR b
        ],
    );

    // Display first (panel power), matching the proven order — then the I2C devices.
    let display = display::Display::new(p.LTDC, p.PK7, p.TIM8, p.PJ6, base).await;

    // Shared I2C1 bus (touch + codec), accessed sequentially on this one executor.
    let mut i2c_config = i2c::Config::default();
    i2c_config.frequency = Hertz(400_000);
    let mut i2c = I2c::new_blocking(p.I2C1, p.PB6, p.PB7, i2c_config);

    let mut codec_rst = Output::new(p.PG13, Level::High, Speed::Low);
    let codec_ok = audio::init_codec(&mut i2c, &mut codec_rst).await;

    // Touch INT (PG12) reset/address strap: one clean drive-low, held through Touch::new,
    // which floats it — the low→float edge re-latches the operational 0x5D address.
    let touch_int = touch::bias_int_low(p.PG12.into());
    let touch = touch::Touch::new(&mut i2c, touch_int).await;

    // SAI1 block A as I2S stereo master TX: SCK=PE5, SD=PB2, FS=PE4, MCLK=PE2 (codec set up
    // above so it sees a configured control port before MCLK arrives). DMA1_CH0 ↔ DMA1_STREAM0.
    let (sai_a, _sai_b) = sai::split_subblocks(p.SAI1);
    let audio = Sai::new_asynchronous_with_mclk(
        sai_a, p.PE5, p.PB2, p.PE4, p.PE2, p.DMA1_CH0, audio::tx_buffer(), Irqs, audio::tx_config(),
    );

    defmt::info!("blackbox: board up — controls + display live, audio I2S ready");

    Board {
        display,
        leds,
        buttons,
        knobs,
        touch,
        i2c,
        codec_ok,
        audio,
    }
}
