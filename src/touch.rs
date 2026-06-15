//! Goodix GT9147 capacitive touch over I2C1 (README §touch).
//!
//! The I2C address latches at chip POR from the INT level (low→0x5D, high→0x14). PG12 is
//! high-Z until firmware drives it, so we bias it low briefly then probe both addresses.
//! The chip is portrait-native, so its axes are swapped vs. the landscape panel.

use embassy_stm32::gpio::{AnyPin, Flex, Pull, Speed};
use embassy_stm32::i2c::{I2c, Master};
use embassy_stm32::mode::Blocking;
use embassy_stm32::Peri;
use embassy_time::Timer;

/// Drive PG12 (INT) low push-pull. Call as early as possible in board bring-up — the GT9147
/// samples this line across its own power-on reset, so holding it low through bring-up biases
/// the address toward 0x5D and lets the chip settle against a defined level. Hand the result
/// to [`Touch::new`], which floats it once bring-up is done.
pub fn bias_int_low(int_pin: Peri<'static, AnyPin>) -> Flex<'static> {
    let mut int = Flex::new(int_pin);
    int.set_as_output(Speed::Low);
    int.set_low();
    int
}

/// A touch report: number of active points and the first point in panel coordinates.
#[derive(Clone, Copy)]
pub struct TouchPoint {
    pub count: u8,
    pub x: u16,
    pub y: u16,
}

/// GT9147 driver. Holds the detected address; the shared I2C1 bus (codec shares it) is passed
/// in per call.
pub struct Touch {
    addr: Option<u8>,
}

impl Touch {
    /// Take the early-biased PG12 (still driven low — see [`bias_int_low`]), hold it low a
    /// touch longer to cover the chip's POR window, then float it as an input so the chip can
    /// drive INT itself. Then probe both addresses and start normal scan.
    pub async fn new(i2c: &mut I2c<'_, Blocking, Master>, mut int: Flex<'static>) -> Self {
        Timer::after_millis(50).await;
        int.set_as_input(Pull::None);
        Timer::after_millis(50).await;
        core::mem::forget(int); // leave PG12 floating for the run

        let mut addr = None;
        for a in [0x5Du8, 0x14] {
            let mut id = [0u8; 4]; // product id ASCII at 0x8140, e.g. "9147"
            if i2c.blocking_write_read(a, &[0x81, 0x40], &mut id).is_ok() {
                defmt::info!("touch: GT9147 @ {=u8:#04x} id {=[u8]:a}", a, id);
                addr = Some(a);
                break;
            }
        }
        if let Some(a) = addr {
            let _ = i2c.blocking_write(a, &[0x80, 0x40, 0x00]); // normal scan mode
            Timer::after_millis(25).await;
            let _ = i2c.blocking_write(a, &[0x81, 0x4E, 0x00]); // clear status latch
        } else {
            defmt::info!("touch: GT9147 not responding on 0x5d/0x14");
        }
        Self { addr }
    }

    pub fn detected(&self) -> bool {
        self.addr.is_some()
    }

    /// Poll the first touch point, or `None` if no fresh data. Always clears the status
    /// latch (the chip stops scanning otherwise). Axes are swapped to panel orientation.
    pub fn poll(&self, i2c: &mut I2c<'_, Blocking, Master>) -> Option<TouchPoint> {
        let addr = self.addr?;
        let mut st = [0u8; 1];
        i2c.blocking_write_read(addr, &[0x81, 0x4E], &mut st).ok()?;
        if st[0] & 0x80 == 0 {
            return None; // no new data; leave the latch alone
        }
        let count = st[0] & 0x0F;
        let mut pt = [0u8; 8]; // [id, x_lo, x_hi, y_lo, y_hi, area_lo, area_hi, _]
        let read = if count > 0 {
            i2c.blocking_write_read(addr, &[0x81, 0x4F], &mut pt)
        } else {
            Ok(())
        };
        let _ = i2c.blocking_write(addr, &[0x81, 0x4E, 0x00]); // ALWAYS clear or scanning stops
        read.ok()?;
        if count == 0 {
            return None;
        }
        let raw_x = u16::from_le_bytes([pt[1], pt[2]]);
        let raw_y = u16::from_le_bytes([pt[3], pt[4]]);
        Some(TouchPoint {
            count,
            x: raw_y, // chip raw_y → panel X
            y: raw_x, // chip raw_x → panel Y
        })
    }
}
