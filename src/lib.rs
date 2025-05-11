//! `silpkg` is a library for interacting with [SIL](https://achurch.org/SIL/) PKG files
//!
//! # Features
//! This library separates parsing/modification logic from IO by using [coroutines](https://doc.rust-lang.org/beta/unstable-book/language-features/coroutines.html)
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
//! For information on how to interact with archives look around in [`Pkg`](sync::Pkg).

#![warn(missing_docs)]
#![feature(doc_cfg)]
#![feature(seek_stream_len)]
#![feature(iterator_try_collect)]
#![feature(map_try_insert)]
#![feature(coroutines, coroutine_trait)]
#![feature(read_buf)]
#![allow(dead_code)] // TODO: remove
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

/// Defines all the errors used by this library.
pub mod errors;

/// The low-level generator-based API that can be used when the high-level variants are not enough.
#[cfg(feature = "unstable_base")]
pub mod base;
#[cfg(not(feature = "unstable_base"))]
mod base;

mod util;

/// A synchronous interface for reading and writing PKG archives.
#[cfg(feature = "std")]
#[doc(cfg(feature = "std"))]
pub mod sync;

pub use base::{Compression, EntryCompression, EntryInfo, Flags};

#[cfg(feature = "std")]
#[doc(cfg(feature = "std"))]
pub use sync::Truncate;
