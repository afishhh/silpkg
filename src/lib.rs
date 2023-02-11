//! `silpkg` is a library for interacting with [SIL](https://achurch.org/SIL/) PKG files
//!
//! # Features
//! This library separates parsing/modification logic from IO by using [generators](https://doc.rust-lang.org/beta/unstable-book/language-features/generators.html)
//! and aims to support many ways of interfacing with the base logic module.
//! Currently only a synchronous interface is implemented in [`sync`].
//!
//! - [X] Sync
//!     - [X] reading PKG files
//!     - [X] reading uncompressed entries
//!     - [X] adding uncompressed entries
//!     - [X] reading deflate compressed entries
//!     - [X] adding deflate compressed entries
//!     - [X] creating new PKG files
//! - [ ] Slice
//! - [ ] Async
//!
//! # Quick start
//! To open an existing archive use [`Pkg::parse`](sync::Pkg::parse).
//! To create a new archive use [`Pkg::create`](sync::Pkg::create).
//! For information on how to interact with archives look around in [`Pkg`](sync::Pkg)'s documentation.

#![feature(seek_stream_len)]
#![feature(iterator_try_collect)]
#![feature(map_try_insert)]
#![feature(drain_filter)]
#![feature(generators, generator_trait)]
#![feature(read_buf)]
#![feature(map_many_mut)]

// thiserror_core
#![feature(error_in_core)]

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod errors;

mod util;
pub use errors::*;

mod base;
#[cfg(feature = "std")]
pub mod sync;

pub use base::{Compression, EntryCompression, Flags};
#[cfg(feature = "std")]
pub use sync::Truncate;
