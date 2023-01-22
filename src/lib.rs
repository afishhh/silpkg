//! `silpkg` is a library for interacting with [SIL](https://achurch.org/SIL/) PKG files
//!
//! # Features
//! - [X] reading PKG files
//! - [X] reading uncompressed entries
//! - [X] adding uncompressed entries
//! - [X] reading deflate compressed entries
//! - [X] adding deflate compressed entries
//! - [X] creating new PKG files
//!
//! # Quick start
//! To open an existing archive use [`Pkg::parse`], and
//! to create a new archive use [`Pkg::create`].
//! For information on how to interact with archives look around in [`Pkg`]'s documnetaiton.

#![feature(seek_stream_len)]
#![feature(iterator_try_collect)]
#![feature(map_try_insert)]
#![feature(drain_filter)]
#![feature(generators, generator_trait)]
#![feature(read_buf)]
#![feature(map_many_mut)]

pub mod errors;
mod util;
pub use errors::*;

mod base;
pub mod sync;

pub use base::{Compression, EntryCompression, Flags};
