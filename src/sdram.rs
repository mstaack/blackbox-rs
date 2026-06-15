//! External SDRAM: 2× ISSI IS42S16160J (32-bit bus) on FMC bank1, 64 MiB @ 0xC000_0000.
//!
//! The chip definition + a busy-wait delay for `Sdram::init` (README: don't depend on the
//! time driver during SDRAM init). The big pin-list constructor lives in [`crate::init`]
//! since it consumes ~50 GPIO singletons; everything reusable is here.

/// Base address of the SDRAM aperture.
pub const BASE: usize = 0xC000_0000;
const WORDS: usize = 16 * 1024 * 1024; // 64 MiB / 4

/// IS42S16160J: col 9, row 13, 4 banks, 32-bit, CL2, 8192 rows (README §SDRAM).
pub struct Is42s16160j;

impl stm32_fmc::SdramChip for Is42s16160j {
    // burst length 1, sequential, CAS latency 2, standard op, single-location writes
    const MODE_REGISTER: u16 = 0x0020 | 0x0200;

    const TIMING: stm32_fmc::SdramTiming = stm32_fmc::SdramTiming {
        startup_delay_ns: 100_000,    // 100 µs power-up
        max_sd_clock_hz: 100_000_000, // SDCLK = HCLK3/2 = 100 MHz
        refresh_period_ns: 7_812,     // 64 ms / 8192 rows
        mode_register_to_active: 2,
        exit_self_refresh: 7,
        active_to_precharge: 4,
        row_cycle: 7,
        row_precharge: 2,
        row_to_column: 2,
    };

    const CONFIG: stm32_fmc::SdramConfiguration = stm32_fmc::SdramConfiguration {
        column_bits: 9,
        row_bits: 13,
        memory_data_width: 32,
        internal_banks: 4,
        cas_latency: 2,
        write_protection: false,
        read_burst: true,
        read_pipe_delay_cycles: 0,
    };
}

/// Busy-wait `DelayNs` for `Sdram::init`. `cycles_per_us` is padded ~2× so the 100 µs
/// floor holds even with M7 dual-issue.
pub struct BusyDelay(pub u32);

impl embedded_hal::delay::DelayNs for BusyDelay {
    fn delay_ns(&mut self, ns: u32) {
        // Round up to whole µs, then spin cycles/µs. init only calls delay_us(100), which
        // the default delay_us forwards here as delay_ns(100_000) → identical cycle count.
        cortex_m::asm::delay(self.0 * ns.div_ceil(1000));
    }
}

/// Pattern + aliasing smoke test — run before trusting the region (README §SDRAM).
pub fn smoke_test(base: usize) {
    let p = base as *mut u32;
    unsafe {
        for i in 0..4096usize {
            p.add(i).write_volatile(0xA5A5_0000 ^ i as u32);
        }
        cortex_m::asm::dsb();
        for i in 0..4096usize {
            defmt::assert_eq!(p.add(i).read_volatile(), 0xA5A5_0000 ^ i as u32);
        }
        // Lowest and highest words must be independent (no address aliasing).
        p.write_volatile(0xDEAD_BEEF);
        p.add(WORDS - 1).write_volatile(0x1234_5678);
        cortex_m::asm::dsb();
        defmt::assert!(p.read_volatile() == 0xDEAD_BEEF);
        defmt::assert!(p.add(WORDS - 1).read_volatile() == 0x1234_5678);
    }
    defmt::info!("sdram: 64 MiB @ {=usize:#010x}, pattern+aliasing OK", base);
}
