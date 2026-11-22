//! Dedicated content journal. Never mixed with the v1 metadata-only Outbox.
//! Intent commits precede native side effects; receipts and snapshots commit together.

pub use crate::remote_host::{
    CommandReceipt, ExecutionState, NativeSessionHost, NativeTransport, PendingApprovalSnapshot,
    RemoteCommand, SessionBinding,
};
pub use rusqlite::{Connection, OptionalExtension, params};
pub use serde::{Deserialize, Serialize};
pub use serde_json::{Value, json};
pub use std::{path::Path, time::Duration};

pub type Result<T> = std::result::Result<T, JournalError>;

pub mod types;
pub use types::*;

pub mod projection;
pub(crate) use projection::*;

pub mod journal;
pub use journal::*;

pub mod commands;

pub mod durable_host;
pub use durable_host::*;

#[cfg(test)]
mod tests;
