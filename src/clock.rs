//! Board clock tree (README §RCC).
//!
//! HSE is a 6.144 MHz crystal (chosen for clean audio divisors, not the usual 8/25 MHz):
//!   * PLL1 P  → 399.36 MHz sysclk
//!   * PLL2 P  → 12.288 MHz (256 × 48 kHz) SAI kernel clock
//!   * PLL3 R  → 6.4 MHz LTDC pixel clock (~59.4 Hz refresh)
//!   * AHB ÷2 → HCLK 199.68 MHz → SDCLK 100 MHz

use embassy_stm32::pac::rcc::vals::{Adcsel, Persel, Saisel};
use embassy_stm32::rcc::{
    AHBPrescaler, APBPrescaler, Hse, HseMode, Pll, PllDiv, PllMul, PllPreDiv, PllSource, Sysclk,
    VoltageScale,
};
use embassy_stm32::time::Hertz;
use embassy_stm32::Config;

/// The embassy [`Config`] that brings the board up at its design clocks.
pub fn config() -> Config {
    let mut config = Config::default();
    let rcc = &mut config.rcc;

    rcc.hse = Some(Hse {
        freq: Hertz(6_144_000),
        mode: HseMode::Oscillator,
    });
    rcc.pll1 = Some(Pll {
        source: PllSource::HSE,
        prediv: PllPreDiv::DIV2, // 3.072 MHz
        mul: PllMul::MUL260,     // 798.72 MHz VCO
        divp: Some(PllDiv::DIV2), // 399.36 MHz sysclk
        divq: Some(PllDiv::DIV18),
        divr: None,
        fracn: None,
    });
    rcc.pll2 = Some(Pll {
        source: PllSource::HSE,
        prediv: PllPreDiv::DIV4,   // 1.536 MHz
        mul: PllMul::MUL272,       // 417.792 MHz VCO
        divp: Some(PllDiv::DIV34), // 12.288 MHz → SAI
        divq: None,
        divr: None,
        fracn: None,
    });
    rcc.pll3 = Some(Pll {
        source: PllSource::HSE,
        prediv: PllPreDiv::DIV4,   // 1.536 MHz
        mul: PllMul::MUL125,       // 192 MHz VCO
        divp: None,
        divq: None,
        divr: Some(PllDiv::DIV30), // 6.4 MHz pixel clock
        fracn: None,
    });
    rcc.sys = Sysclk::PLL1_P;
    rcc.d1c_pre = AHBPrescaler::DIV1;
    rcc.ahb_pre = AHBPrescaler::DIV2;
    rcc.apb1_pre = APBPrescaler::DIV2;
    rcc.apb2_pre = APBPrescaler::DIV2;
    rcc.apb3_pre = APBPrescaler::DIV2;
    rcc.apb4_pre = APBPrescaler::DIV2;
    rcc.voltage_scale = VoltageScale::Scale1;
    rcc.mux.adcsel = Adcsel::PER; // ADC kernel clock = PER = HSI
    rcc.mux.persel = Persel::HSI;
    rcc.mux.sai1sel = Saisel::PLL2_P; // SAI1/2 kernel clock = 12.288 MHz

    config
}
