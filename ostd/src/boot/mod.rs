#[cfg(all(target_os = "none", target_arch = "aarch64"))]
mod aarch64;

#[cfg(all(target_os = "none", target_arch = "x86_64"))]
mod x86_64;

#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::console::early_println;

#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::cpu::wait_forever;

#[cfg(all(
    feature = "exception-smoke",
    target_os = "none",
    target_arch = "aarch64"
))]
pub fn trigger_diagnostic_exception_if_supported() {
    crate::arch::aarch64::exception::trigger_diagnostic_exception()
}

#[cfg(not(all(
    feature = "exception-smoke",
    target_os = "none",
    target_arch = "aarch64"
)))]
pub fn trigger_diagnostic_exception_if_supported() {}

#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::console::early_println;

#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::cpu::wait_forever;
