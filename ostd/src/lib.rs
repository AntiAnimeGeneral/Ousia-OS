#![no_std]
#![cfg_attr(target_os = "none", feature(alloc_error_handler))]

pub mod arch;
pub mod boot;
pub mod console;
pub mod mm;
