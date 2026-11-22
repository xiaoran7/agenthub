//! Local authentication, quota and credential-cache integration.
//!
//! The runner is the only component that is allowed to touch provider
//! credentials.  `AuthSnapshot` is deliberately safe to serialize and sync;
//! `TokenSet` is kept private to this module and is only returned by the
//! explicit `auth token --reveal` CLI command.

pub use std::{
    collections::BTreeMap,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

pub use aes_gcm::{Aes256Gcm, Nonce, aead::Aead, aead::KeyInit};
pub use chrono::{DateTime, Utc};
pub use keyring::Entry;
pub use rand_core::{OsRng, RngCore};
pub use serde::{Deserialize, Serialize};
pub use serde_json::{Value, json};
pub use thiserror::Error;

pub use crate::{
    collectors::{CodexAppServer, CollectorError},
    device::DeviceIdentity,
};

pub mod types;
pub use types::*;

pub(crate) mod cache;
pub(crate) use cache::*;

pub mod manager;
pub use manager::*;

pub(crate) mod probes;
pub(crate) use probes::*;

#[cfg(test)]
mod tests;
