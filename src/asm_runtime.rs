//! This module holds the assembly runtime that supports your Rust program.
//!
//! Most importantly, you can set the [`RUST_IRQ_HANDLER`] variable to assign
//! which function should be run during a hardware interrupt.
//! * When a hardware interrupt occurs, control first goes to the BIOS, which
//!   will then call the assembly runtime's handler.
//! * The assembly runtime handler will properly acknowledge the interrupt
//!   within the system on its own without you having to do anything.
//! * If a function is set in the `RUST_IRQ_HANDLER` variable then that function
//!   will be called and passed the bits for which interrupt(s) occurred.

use crate::{
  dma::DmaControl,
  gba_cell::GbaCell,
  interrupts::IrqFn,
  mgba::MGBA_LOGGING_ENABLE_REQUEST,
  mmio::{DMA3_SRC, IME, MGBA_LOG_ENABLE, WAITCNT},
};

const DMA_32_BIT_MEMCPY: DmaControl =
  DmaControl::new().with_transfer_32bit(true).with_enabled(true);

const DMA3_OFFSET: usize = DMA3_SRC.as_usize() - 0x0400_0000;
const WAITCNT_OFFSET: usize = WAITCNT.as_usize() - 0x0400_0000;

// Proc-macros can't see the target being built for, so we use this declarative
// macro to determine if we're on a thumb target (and need to force our asm into
// a32 mode) or if we're not on thumb (and our asm can pass through untouched).
#[cfg(target_feature = "thumb-mode")]
macro_rules! force_a32 {
  ($($asm_line:expr),+ $(,)?) => {
    bracer::t32_with_a32_scope! {
      $( concat!($asm_line, "\n") ),+ ,
    }
  }
}
#[cfg(not(target_feature = "thumb-mode"))]
macro_rules! force_a32 {
  ($($asm_line:expr),+ $(,)?) => {
    concat!(
      $( concat!($asm_line, "\n") ),+ ,
    )
  }
}

// This handler DOES NOT allow nested interrupts at this time.
core::arch::global_asm! {
  bracer::put_fn_in_section!(".iwram.__runtime_irq_handler"),
  ".global __runtime_irq_handler",
  // On Entry: r0 = 0x0400_0000 (mmio_base)
  // We're allowed to use the usual C ABI registers.
  "__runtime_irq_handler:",

  force_a32!{
    /* A fox wizard told me how to do this one */
    // handle MMIO interrupt system
    "mov  r12, 0x04000000",     // load r12 with a 1 cycle value
    "ldr  r0, [r12, #0x200]!",  // load IE_IF with r12 writeback
    "and  r0, r0, r0, LSR #16", // bits = IE & IF
    "strh r0, [r12, #2]",       // write16 to just IF
    // handle BIOS IntrWait system
    "ldr  r1, [r12, #-0x208]!", // load BIOS_IF_?? with r12 writeback
    "orr  r1, r1, r0",          // mark `bits` as `has_occurred`
    "strh r1, [r12]",           // write16 to just BIOS_IF

    // Get the rust code handler fn pointer, call it if non-null.
    "ldr r12, ={RUST_IRQ_HANDLER}",
    "ldr r12, [r12]",
    bracer::when!(("r12" != "#0")[1] {
      bracer::a32_read_spsr_to!("r3"),
      bracer::a32_set_cpu_control!(System, irq_masked = true, fiq_masked = true),
      "push {{r3, lr}}",
      bracer::a32_fake_blx!("r12"),
      "pop {{r3, lr}}",
      bracer::a32_set_cpu_control!(IRQ, irq_masked = true, fiq_masked = true),
      bracer::a32_write_spsr_from!("r3"),
    }),

    // return to the BIOS
    "bx lr",
  },

  // Define Our Constants
  RUST_IRQ_HANDLER = sym crate::RUST_IRQ_HANDLER,
}
