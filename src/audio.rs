//! Audio: SAI1 as an I2S stereo master, streamed to the CS42528 over DMA.
//!
//! Uses embassy's [`Sai`] driver (mirrors the upstream `stm32h7/sai.rs` example) instead of
//! the former raw-register One-Line-Mode topology. One SAI sub-block carries a standard I2S
//! stereo pair to the codec — the headphone DACs (DAC1/2). The other six OLM DACs are not
//! driven by this path.
//!
//! SAI1 kernel clock = PLL2_P = 12.288 MHz (256 × 48 kHz, set in `clock::config`), so the
//! master-clock divider is 1. The DMA ring buffer lives in D2 SRAM1 (0x3000_0000): reachable
//! from DMA1's domain and coherent without cache maintenance (D-cache is off).
//!
//! Construction lives in [`crate::init`] (it owns the peripherals + interrupt bindings); this
//! module provides the codec bring-up and the board's SAI [`Config`]/buffer.

use embassy_stm32::dma::word;
use embassy_stm32::gpio::Output as GpioOut;
use embassy_stm32::i2c::{I2c, Master};
use embassy_stm32::mode::Blocking;
use embassy_stm32::pac;
use embassy_stm32::peripherals::SAI1;
use embassy_stm32::sai::{
    BitOrder, ClockStrobe, Config, DataSize, FifoThreshold, FrameSyncOffset, FrameSyncPolarity,
    MasterClockDivider, Mode, Sai, StereoMono, TxRx,
};
use embassy_time::Timer;

const CODEC_ADDR: u8 = 0x4C;

/// Audio sample rate — matches the 12.288 MHz SAI kernel clock (256 × 48 kHz).
pub const SAMPLE_RATE: u32 = 48_000;

/// Configured SAI1 I2S stereo transmitter (24-bit samples). Drive it with [`Sai::write`].
pub type ToneTx = Sai<'static, SAI1, u32>;

/// Interleaved L/R samples per [`Sai::write`] call (one half of the DMA ring buffer).
pub const HALF_BUFFER_LEN: usize = 64;
const DMA_BUFFER_LEN: usize = HALF_BUFFER_LEN * 2;
const TX_BUFFER_ADDR: *mut u32 = 0x3000_0000 as *mut u32;

/// CS42528 bring-up over I2C1. The control port acks without MCLK; the I²S/clock regs take
/// effect once SAI feeds 12.288 MHz MCLK — so call this before the transmitter is built.
pub async fn init_codec(i2c: &mut I2c<'_, Blocking, Master>, reset: &mut GpioOut<'_>) -> bool {
    reset.set_low(); // PG13 active-low reset
    Timer::after_millis(10).await;
    reset.set_high();
    Timer::after_millis(20).await;

    let seq: [[u8; 2]; 6] = [
        [0x02, 0x80], // Power Control
        [0x03, 0x08], // Functional Mode
        [0x04, 0x04], // Interface Format: I²S, 24-bit, standard (OLM off)
        [0x05, 0x04], // Misc Control
        [0x06, 0x02], // Clock Control: MCLK from OMCK, 12.288 MHz
        [0x0E, 0x00], // unmute all DAC channels
    ];
    let mut ok = true;
    for reg in seq {
        ok &= i2c.blocking_write(CODEC_ADDR, &reg).is_ok();
    }
    ok
}

/// Board SAI config: I2S, master TX, 24-bit stereo, MCLK = kernel/1 = 256 × 48 kHz.
/// Frame layout mirrors the upstream embassy `stm32h7/sai.rs` example.
pub fn tx_config() -> Config {
    let mut c = Config::default();
    c.mode = Mode::Master;
    c.tx_rx = TxRx::Transmitter;
    c.sync_output = true;
    c.clock_strobe = ClockStrobe::Falling;
    c.master_clock_divider = MasterClockDivider::DIV1;
    c.stereo_mono = StereoMono::Stereo;
    c.data_size = DataSize::Data24;
    c.bit_order = BitOrder::MsbFirst;
    c.frame_sync_polarity = FrameSyncPolarity::ActiveHigh;
    c.frame_sync_offset = FrameSyncOffset::OnFirstBit;
    c.frame_length = 64;
    c.frame_sync_active_level_length = word::U7(32);
    c.fifo_threshold = FifoThreshold::Quarter;
    c
}

/// The zeroed DMA TX buffer in D2 SRAM1. Enables the SRAM1 clock (gated at reset) first.
/// Call once — the returned slice aliases a fixed region.
pub fn tx_buffer() -> &'static mut [u32] {
    pac::RCC.ahb2enr().modify(|w| w.set_sram1en(true)); // D2 SRAM1 — gated off at reset
    let _ = pac::RCC.ahb2enr().read(); // order the enable before first access
    // SAFETY: 0x3000_0000 is the reserved non-cacheable D2 SRAM1 audio region; called once.
    let buf = unsafe { core::slice::from_raw_parts_mut(TX_BUFFER_ADDR, DMA_BUFFER_LEN) };
    buf.fill(0);
    buf
}
