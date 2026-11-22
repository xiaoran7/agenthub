use crate::daemon_protocol::{
    DaemonEventEnvelope, DaemonRequest, DaemonRequestEnvelope, DaemonResponse,
    DaemonResponseEnvelope, MAX_DAEMON_FRAME_BYTES, validate_request,
};
pub use anyhow::{Context, Result, bail};
pub use serde::{Serialize, de::DeserializeOwned};
pub use sha2::{Digest, Sha256};
pub use std::{
    io::{self, Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    thread,
    time::Duration,
};

pub mod types;
pub use types::*;

pub mod server;
pub use server::*;

pub mod client;
pub use client::*;

pub(crate) mod platform;

#[cfg(test)]
mod tests;
