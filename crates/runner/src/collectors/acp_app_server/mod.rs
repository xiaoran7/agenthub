use std::{
    collections::{HashMap, VecDeque},
    io::{BufReader, BufWriter, Read, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use super::{
    BoundedLine, ChildJob, CollectorError, ProtocolRead, configure_process_group,
    read_bounded_line, sanitize_protocol_message,
};

pub mod types;
pub use types::*;

pub mod server;
pub use server::*;

pub mod protocol;
pub use protocol::*;

#[cfg(test)]
mod tests;
