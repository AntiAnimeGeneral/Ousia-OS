mod endpoint;
pub mod message;

pub use endpoint::*;
pub use message::{IpcError, IpcPayload, MAX_IPC_WORDS};
