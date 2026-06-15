//! LTDC display: 320×240 RGB565 panel, vblank-synced double buffer, plus panel power and
//! backlight. Framebuffers live in SDRAM (coherent with the LTDC DMA — D-cache is off).
//!
//! The `new_with_pins` constructor wants a pin for every channel including R2, which this
//! board doesn't route, so we configure the 25 routed pins by raw AF and program the
//! framebuffer address (CFBAR) ourselves. embassy's LTDC IRQ (bound in [`crate::Irqs`])
//! drives the vblank reload that `set_buffer` awaits.

use defmt::info;
use embassy_stm32::gpio::{Level, Output, OutputType, Speed};
use embassy_stm32::ltdc::{
    Ltdc, LtdcConfiguration, LtdcLayer, LtdcLayerConfig, PixelFormat, PolarityActive, PolarityEdge,
};
use embassy_stm32::pac;
use embassy_stm32::peripherals::{LTDC, PJ6, PK7, TIM8};
use embassy_stm32::time::Hertz;
use embassy_stm32::Peri;
use embassy_stm32::timer::low_level::CountingMode;
use embassy_stm32::timer::simple_pwm::{PwmPin, SimplePwm};
use embassy_time::Timer;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;

pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 240;
const PIXELS: usize = WIDTH * HEIGHT;
const FB_STRIDE: usize = 0x40000; // 256 KiB apart (frame is 150 KiB)

/// Backlight PWM hard ceiling — boost-regulator thermal limit (README §backlight).
/// Do NOT raise without a thermal basis; every brightness request is clamped to this.
pub const MAX_BACKLIGHT_PCT: u8 = 35;

// LTDC pin map (README §LTDC). All AF14 except R7/G6 = AF9. R2 not routed.
const PINS: &[(pac::gpio::Gpio, usize, u8)] = &[
    (pac::GPIOG, 7, 14),  // CLK
    (pac::GPIOC, 6, 14),  // HSYNC
    (pac::GPIOI, 13, 14), // VSYNC
    (pac::GPIOI, 15, 14), // R0
    (pac::GPIOA, 2, 14),  // R1
    (pac::GPIOJ, 2, 14),  // R3
    (pac::GPIOJ, 3, 14),  // R4
    (pac::GPIOA, 9, 14),  // R5
    (pac::GPIOA, 8, 14),  // R6
    (pac::GPIOJ, 0, 9),   // R7  (AF9)
    (pac::GPIOJ, 7, 14),  // G0
    (pac::GPIOE, 6, 14),  // G1
    (pac::GPIOJ, 9, 14),  // G2
    (pac::GPIOJ, 10, 14), // G3
    (pac::GPIOJ, 11, 14), // G4
    (pac::GPIOK, 0, 14),  // G5
    (pac::GPIOI, 11, 9),  // G6  (AF9)
    (pac::GPIOD, 3, 14),  // G7
    (pac::GPIOJ, 12, 14), // B0
    (pac::GPIOJ, 13, 14), // B1
    (pac::GPIOJ, 14, 14), // B2
    (pac::GPIOJ, 15, 14), // B3
    (pac::GPIOK, 3, 14),  // B4
    (pac::GPIOK, 4, 14),  // B5
    (pac::GPIOK, 5, 14),  // B6
    (pac::GPIOK, 6, 14),  // B7
];

fn set_af(g: pac::gpio::Gpio, pin: usize, afn: u8) {
    use pac::gpio::vals::{Moder, Ospeedr, Ot, Pupdr};
    g.moder().modify(|w| w.set_moder(pin, Moder::ALTERNATE));
    g.otyper().modify(|w| w.set_ot(pin, Ot::PUSH_PULL));
    g.ospeedr().modify(|w| w.set_ospeedr(pin, Ospeedr::VERY_HIGH_SPEED));
    g.pupdr().modify(|w| w.set_pupdr(pin, Pupdr::FLOATING));
    g.afr(pin / 8).modify(|w| w.set_afr(pin % 8, afn));
}

// 320×240 @ ~59.4 Hz, PLL3_R 6.4 MHz pixel clock. README timings, all-active-low, falling edge.
const LTDC_CFG: LtdcConfiguration = LtdcConfiguration {
    active_width: WIDTH as u16,
    active_height: HEIGHT as u16,
    h_back_porch: 42,
    h_front_porch: 45,
    v_back_porch: 12,
    v_front_porch: 10,
    h_sync: 1,
    v_sync: 1,
    h_sync_polarity: PolarityActive::ActiveLow,
    v_sync_polarity: PolarityActive::ActiveLow,
    data_enable_polarity: PolarityActive::ActiveLow,
    pixel_clock_polarity: PolarityEdge::FallingEdge,
};

const LAYER_CFG: LtdcLayerConfig = LtdcLayerConfig {
    layer: LtdcLayer::Layer1,
    pixel_format: PixelFormat::RGB565,
    window_x0: 0,
    window_x1: WIDTH as u16,
    window_y0: 0,
    window_y1: HEIGHT as u16,
};

/// The panel: double-buffered LTDC layer + backlight.
pub struct Display {
    ltdc: Ltdc<'static, LTDC>,
    backlight: SimplePwm<'static, TIM8>,
    fb: [usize; 2],
    front: usize,
}

impl Display {
    /// Panel power on (PK7) → settle → drive pins → LTDC up → both buffers cleared → show
    /// buffer 0. Backlight starts at the hard-cap brightness.
    pub async fn new(
        ltdc_peri: Peri<'static, LTDC>,
        pk7: Peri<'static, PK7>,
        tim8: Peri<'static, TIM8>,
        pj6: Peri<'static, PJ6>,
        sdram_base: usize,
    ) -> Self {
        // Panel power enable PK7 active-high, ~50 ms settle before driving the pins.
        let pwr = Output::new(pk7, Level::High, Speed::Low);
        Timer::after_millis(50).await;
        core::mem::forget(pwr); // latch panel power for the run

        for &(g, pin, afn) in PINS {
            set_af(g, pin, afn);
        }

        let mut ltdc = Ltdc::new(ltdc_peri);
        ltdc.init(&LTDC_CFG);
        ltdc.init_layer(&LAYER_CFG, None);
        ltdc.enable();

        // Backlight: TIM8_CH2 PWM on PJ6, 50 kHz, clamped to the regulator ceiling.
        let bl_pin = PwmPin::new(pj6, OutputType::PushPull);
        let mut backlight = SimplePwm::new(
            tim8,
            None,
            Some(bl_pin),
            None,
            None,
            Hertz(50_000),
            CountingMode::EdgeAlignedUp,
        );
        backlight.ch2().set_duty_cycle_percent(MAX_BACKLIGHT_PCT);
        backlight.ch2().enable();

        let fb = [sdram_base, sdram_base + FB_STRIDE];
        for &addr in &fb {
            buffer(addr).fill(0);
        }
        cortex_m::asm::dsb();

        let mut d = Self {
            ltdc,
            backlight,
            fb,
            front: 1,
        };
        d.swap().await; // present buffer 0
        info!("display: 320x240 RGB565, double buffered @ ~59 Hz, backlight {}%", MAX_BACKLIGHT_PCT);
        d
    }

    /// An `embedded-graphics` draw target over the back buffer.
    pub fn target(&mut self) -> FrameBuf {
        FrameBuf {
            buf: buffer(self.fb[1 - self.front]),
        }
    }

    /// Present the back buffer at the next vblank (tear-free). Returns after the swap lands.
    pub async fn swap(&mut self) {
        cortex_m::asm::dsb(); // finish framebuffer writes before the controller reloads
        let back = 1 - self.front;
        let _ = self
            .ltdc
            .set_buffer(LtdcLayer::Layer1, self.fb[back] as *const ())
            .await;
        self.front = back;
    }

    /// Set backlight brightness in percent, hard-clamped to [`MAX_BACKLIGHT_PCT`].
    pub fn set_backlight(&mut self, pct: u8) {
        self.backlight
            .ch2()
            .set_duty_cycle_percent(pct.min(MAX_BACKLIGHT_PCT));
    }
}

fn buffer(addr: usize) -> &'static mut [u16] {
    unsafe { core::slice::from_raw_parts_mut(addr as *mut u16, PIXELS) }
}

/// `embedded-graphics` target backed by an RGB565 framebuffer in SDRAM.
pub struct FrameBuf {
    buf: &'static mut [u16],
}

impl OriginDimensions for FrameBuf {
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
}

impl DrawTarget for FrameBuf {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(p, color) in pixels {
            if p.x >= 0 && p.y >= 0 && (p.x as usize) < WIDTH && (p.y as usize) < HEIGHT {
                self.buf[p.y as usize * WIDTH + p.x as usize] = color.into_storage();
            }
        }
        Ok(())
    }
}
