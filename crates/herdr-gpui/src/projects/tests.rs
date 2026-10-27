#![allow(clippy::unwrap_used)]
use super::*;

mod bindings;
mod persistence;
mod store;

fn device() -> &'static str {
    "ssh:box"
}

mod concurrent;
mod concurrent_names;
