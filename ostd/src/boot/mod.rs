#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::console::early_println;

#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::cpu::wait_forever;
