//! CPU caches, MPU regions and SYSCFG fixes that must run before the peripherals.
//!
//! D-cache stays OFF, so SDRAM / D2 SRAM are coherent with the LTDC and SAI DMA engines
//! without any cache maintenance. The MPU still marks those regions non-cacheable so the
//! layout stays correct the day D-cache is enabled (README §caches, rev.Y erratum).

use defmt::info;
use embassy_stm32::pac;

/// I-cache on, D-cache off; MPU non-cacheable regions: 0 = SDRAM (64 MiB), 1 = D2 SRAM (256 KiB).
pub fn init() {
    let mut cp = unsafe { cortex_m::Peripherals::steal() };
    cp.SCB.enable_icache();

    // RASR (Normal non-cacheable): ENABLE | SIZE | TEX=001,C=0,B=0 | AP=011 RW | XN
    const RASR_SDRAM: u32 = 0x1308_0033; // SIZE = 64 MiB (25)
    const RBAR_SDRAM: u32 = 0xC000_0000 | (1 << 4); // base + VALID + region 0
    const RASR_D2: u32 = 0x1308_0023; // SIZE = 256 KiB (17)
    const RBAR_D2: u32 = 0x3000_0000 | (1 << 4) | 1; // base + VALID + region 1
    unsafe {
        cp.MPU.ctrl.write(0);
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
        cp.MPU.rbar.write(RBAR_SDRAM);
        cp.MPU.rasr.write(RASR_SDRAM);
        cp.MPU.rbar.write(RBAR_D2);
        cp.MPU.rasr.write(RASR_D2);
        cp.MPU.ctrl.write((1 << 0) | (1 << 2)); // ENABLE | PRIVDEFENA
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
    }
    info!("cpu: I-cache on, D-cache off, MPU SDRAM + D2 SRAM non-cacheable");
}

/// Close the PA0/PA1/PC2/PC3 dual-pad analog switches (README §dual-pad, TFBGA240).
/// Run before the ADC (top-left knob) and the BACK/INFO buttons, or they read garbage.
pub fn dual_pad_fix() {
    pac::RCC.apb4enr().modify(|w| w.set_syscfgen(true));
    pac::SYSCFG.pmcr().modify(|w| {
        w.set_pa0so(false);
        w.set_pa1so(false);
        w.set_pc2so(false);
        w.set_pc3so(false);
    });
    info!("cpu: dual-pad analog switches closed (PA0/PA1/PC2/PC3)");
}
