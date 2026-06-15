//! Demo app: a live debug screen for the Blackbox board — every control and its state
//! rendered to the panel, a 440 Hz tone on the phones, LED heartbeat.

#![no_std]
#![no_main]

use core::fmt::Write as _;
use defmt::info;
use embassy_executor::Spawner;
use embassy_time::Instant;
use embedded_graphics::mono_font::ascii::{FONT_6X10, FONT_8X13};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Baseline, Text};
use heapless::String;
use {defmt_rtt as _, panic_probe as _};

use blackbox_rs::buttons::{self, Button};
use blackbox_rs::display::{FrameBuf, MAX_BACKLIGHT_PCT};
use blackbox_rs::knobs::{Knob, Reading};
use blackbox_rs::touch::TouchPoint;

#[path = "common/tone.rs"]
mod tone;

// dmesg-style timestamp: "[ssssssss.uuuuuu] ..."
defmt::timestamp!("[{=u64:08}.{=u64:06}]",
    Instant::now().as_micros() / 1_000_000,
    Instant::now().as_micros() % 1_000_000
);

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut board = blackbox_rs::init().await;
    spawner.spawn(tone::play_440(board.audio).unwrap()); // 440 Hz sine on the phones (DAC1/2)

    let mut prev_released = [true; buttons::COUNT];
    loop {
        let pressed = board.buttons.pressed();
        for b in Button::ALL {
            if pressed[b.index()] && prev_released[b.index()] {
                info!("button: {} pressed", b.label());
            }
            prev_released[b.index()] = !pressed[b.index()];
        }
        // Light each LED while its button is held (13 buttons, 11 LEDs — extras have no LED).
        for (i, &on) in pressed.iter().take(board.leds.count()).enumerate() {
            board.leds.set(i, on);
        }

        let knobs = board.knobs.read_all();
        let touch = board.touch.poll(&mut board.i2c);

        render_debug(&mut board.display.target(), &pressed, &knobs, touch, board.codec_ok);
        board.display.swap().await; // blocks ~one frame — paces the loop at the refresh rate
    }
}

/// Paint the whole control surface: button cells, knob angles, touch crosshair, LED row.
fn render_debug(
    t: &mut FrameBuf,
    pressed: &[bool; buttons::COUNT],
    knobs: &[Reading; 4],
    touch: Option<TouchPoint>,
    codec_ok: bool,
) {
    let white = MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE);
    let black = MonoTextStyle::new(&FONT_6X10, Rgb565::BLACK);
    let title = MonoTextStyle::new(&FONT_8X13, Rgb565::YELLOW);
    let cyan = MonoTextStyle::new(&FONT_6X10, Rgb565::CYAN);
    let green = Rgb565::GREEN;
    let gray = Rgb565::new(8, 16, 8);

    let _ = t.clear(Rgb565::BLACK);
    let _ = Text::with_baseline("blackbox debug - controls", Point::new(4, 2), title, Baseline::Top).draw(t);

    // Button cells: 7 per row, green = pressed.
    for b in Button::ALL {
        let i = b.index();
        let x = 4 + (i % 7) as i32 * 45;
        let y = 22 + (i / 7) as i32 * 18;
        let fill = if pressed[i] { green } else { gray };
        let _ = Rectangle::new(Point::new(x, y), Size::new(42, 15))
            .into_styled(PrimitiveStyle::with_fill(fill))
            .draw(t);
        let style = if pressed[i] { black } else { white };
        let _ = Text::with_baseline(b.label(), Point::new(x + 3, y + 3), style, Baseline::Top).draw(t);
    }

    // Knob angles + raw wipers.
    let mut y = 64;
    for k in Knob::ALL {
        let r = &knobs[k.index()];
        let mut s: String<48> = String::new();
        let _ = write!(s, "knob {}: {:>4}deg  A={:5} B={:5}", k.label(), r.angle_deg(), r.a, r.b);
        let _ = Text::with_baseline(&s, Point::new(4, y), white, Baseline::Top).draw(t);
        y += 12;
    }

    // Touch state + crosshair.
    let mut s: String<32> = String::new();
    match touch {
        Some(tp) => {
            let _ = write!(s, "touch: {} pt @ {},{}", tp.count, tp.x, tp.y);
            let cx = (tp.x as i32).clamp(0, 319);
            let cy = (tp.y as i32).clamp(0, 239);
            let _ = Rectangle::new(Point::new(cx - 5, cy), Size::new(11, 1))
                .into_styled(PrimitiveStyle::with_fill(Rgb565::RED))
                .draw(t);
            let _ = Rectangle::new(Point::new(cx, cy - 5), Size::new(1, 11))
                .into_styled(PrimitiveStyle::with_fill(Rgb565::RED))
                .draw(t);
        }
        None => {
            let _ = write!(s, "touch: none");
        }
    }
    let _ = Text::with_baseline(&s, Point::new(4, 116), cyan, Baseline::Top).draw(t);

    let mut s2: String<48> = String::new();
    let _ = write!(s2, "backlight: {}%", MAX_BACKLIGHT_PCT);
    let _ = Text::with_baseline(&s2, Point::new(4, 132), white, Baseline::Top).draw(t);

    let mut s3: String<48> = String::new();
    let _ = write!(s3, "audio: 440Hz sine L/R  codec:{}", if codec_ok { "ok" } else { "FAIL" });
    let _ = Text::with_baseline(&s3, Point::new(4, 148), cyan, Baseline::Top).draw(t);
}
