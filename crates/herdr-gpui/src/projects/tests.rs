#![allow(clippy::unwrap_used)]
use super::*;

mod persistence;
mod store;

fn device() -> &'static str {
    "ssh:box"
}
