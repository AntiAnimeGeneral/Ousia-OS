//! Stable Ousia-native kernel error boundary.
//!
//! Subsystems may keep richer debug context internally, but syscall-facing and
//! behavior-test-facing paths collapse failures to these semantic categories.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelError {
    InvalidHandle,
    WrongObjectType,
    MissingRights,
    StaleHandle,
    DeadObject,
    InvalidArgument,
    WouldBlock,
    NoCapacity,
    NoMemory,
    QuotaExceeded,
    PeerClosed,
    Canceled,
    TimedOut,
    Unsupported,
}

pub type KernelResult<T> = Result<T, KernelError>;
