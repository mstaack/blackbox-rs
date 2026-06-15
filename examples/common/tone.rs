//! Shared 440 Hz sine tone task for the examples (kept out of the BSP lib, which is sample-
//! source agnostic). Include with `#[path = "common/tone.rs"] mod tone;`.

use blackbox_rs::audio::{ToneTx, HALF_BUFFER_LEN, SAMPLE_RATE};

/// Stream a 440 Hz sine to both I2S channels forever via the SAI DMA ring buffer.
#[embassy_executor::task]
pub async fn play_440(mut tx: ToneTx) {
    const FULL_SCALE_24BIT: f32 = 0x007F_FFFF as f32;

    // 256-point half-scale sine LUT; 32.0 fixed-point phase accumulator.
    let mut lut = [0i32; 256];
    for (i, s) in lut.iter_mut().enumerate() {
        let ph = i as f32 / 256.0 * core::f32::consts::TAU;
        *s = (libm::sinf(ph) * 0.5 * FULL_SCALE_24BIT) as i32;
    }
    let inc = ((440u64 << 32) / SAMPLE_RATE as u64) as u32;
    let mut phase = 0u32;

    let mut buf = [0u32; HALF_BUFFER_LEN];
    loop {
        for frame in buf.chunks_exact_mut(2) {
            let s = lut[(phase >> 24) as usize & 0xFF] as u32;
            phase = phase.wrapping_add(inc);
            frame[0] = s; // left
            frame[1] = s; // right
        }
        let _ = tx.write(&buf).await; // ignore transient underruns
    }
}
