//! Plain data types shared by every crate of the daemon: the error type with its stable codes,
//! the hardware model table and time helpers. Nothing here performs I/O.

pub mod error;
pub mod hardware;
pub mod rtttl;
pub mod time;

pub use error::{Error, Result};
