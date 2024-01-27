use alloc::{string::String, vec::Vec};
#[cfg(feature = "std")]
use std::io::SeekFrom as StdSeekFrom;

use macros::generator;

use hashbrown::HashMap;

pub enum SeekFrom {
    Start(u64),
    End(i64),
    Current(i64),
}

#[cfg(feature = "std")]
impl From<StdSeekFrom> for SeekFrom {
    fn from(value: StdSeekFrom) -> Self {
        match value {
            StdSeekFrom::Start(s) => Self::Start(s),
            StdSeekFrom::End(e) => Self::End(e),
            StdSeekFrom::Current(c) => Self::Current(c),
        }
    }
}

#[cfg(feature = "std")]
impl From<SeekFrom> for StdSeekFrom {
    fn from(val: SeekFrom) -> Self {
        match val {
            SeekFrom::Start(s) => StdSeekFrom::Start(s),
            SeekFrom::End(e) => StdSeekFrom::End(e),
            SeekFrom::Current(c) => StdSeekFrom::Current(c),
        }
    }
}

pub enum ReadSeekRequest {
    Read(u64),
    ReadExact(u64),
    Seek(SeekFrom),
}

pub enum WriteRequest {
    // TODO: This should be replaced with a borrow when possible.
    WriteAll(*const u8, usize),
    Write(*const u8, usize),
    Copy { from: u64, count: u64, to: u64 },
    WriteRepeated { value: u8, count: u64 },
}

pub enum ReadSeekWriteRequest {
    ReadSeek(ReadSeekRequest),
    Write(WriteRequest),
}

impl From<ReadSeekRequest> for ReadSeekWriteRequest {
    fn from(rsr: ReadSeekRequest) -> Self {
        Self::ReadSeek(rsr)
    }
}

impl From<WriteRequest> for ReadSeekWriteRequest {
    fn from(wr: WriteRequest) -> Self {
        Self::Write(wr)
    }
}

pub enum ReadSeekWriteTruncateRequest {
    ReadSeek(ReadSeekRequest),
    Write(WriteRequest),
    Truncate(u64),
}

impl From<ReadSeekRequest> for ReadSeekWriteTruncateRequest {
    fn from(rsr: ReadSeekRequest) -> Self {
        Self::ReadSeek(rsr)
    }
}

impl From<WriteRequest> for ReadSeekWriteTruncateRequest {
    fn from(wr: WriteRequest) -> Self {
        Self::Write(wr)
    }
}

impl From<ReadSeekWriteRequest> for ReadSeekWriteTruncateRequest {
    fn from(wr: ReadSeekWriteRequest) -> Self {
        match wr {
            ReadSeekWriteRequest::ReadSeek(rs) => Self::ReadSeek(rs),
            ReadSeekWriteRequest::Write(w) => Self::Write(w),
        }
    }
}

#[derive(Default)]
pub enum Response {
    // TODO: Transient borrow, maybe this can be worked around using pointers?
    Read(Vec<u8>),
    Seek(u64),
    Written(usize),
    #[default]
    None,
}

impl Response {
    fn assert_into_sized_read(self, size: u64) -> Vec<u8> {
        let value = self.assert_into_read();
        assert_eq!(value.len() as u64, size);

        value
    }

    fn assert_into_read(self) -> Vec<u8> {
        match self {
            Response::Read(value) => value,
            _ => panic!("Response::assert_into_read on non Response::Read response"),
        }
    }

    fn assert_into_seek(self) -> u64 {
        match self {
            Response::Seek(len) => len,
            _ => panic!("Response::assert_into_storagelen on non Response::StorageLen response"),
        }
    }

    fn assert_into_written(self) -> usize {
        match self {
            Response::Written(value) => value,
            _ => panic!("Response::assert_into_written on non Response::Written response"),
        }
    }

    fn assert_none(self) {
        debug_assert!(
            matches!(self, Self::None),
            "Response::assert_none on non Response::None response"
        )
    }
}

macro_rules! request {
    (read $count: expr) => {
        (yield $crate::base::ReadSeekRequest::Read($count).into()).assert_into_read()
    };
    (read exact $count: expr) => {{
        let __count = $count;
        (yield $crate::base::ReadSeekRequest::ReadExact(__count).into())
            .assert_into_sized_read(__count)
    }};
    (write all $buffer: expr) => {{
        let __buffer = $buffer;
        let __slice: &[u8] = __buffer.as_ref();
        (yield $crate::base::WriteRequest::WriteAll(__slice.as_ptr(), __slice.len()).into()).assert_none()
    }};
    (write $buffer: expr) => {{
        let __buffer = $buffer;
        let __slice: &[u8] = __buffer.as_ref();
        (yield $crate::base::WriteRequest::Write(__slice.as_ptr(), __slice.len()).into()).assert_into_written()
    }};
    (write repeated $value: expr, $count: expr) => {
        (yield ($crate::base::WriteRequest::WriteRepeated {
            value: $value,
            count: $count,
        })
        .into())
        .assert_none()
    };
    (write $int: ident be $value: expr) => {
        request!(write all $int::to_be_bytes($value))
    };
    (write $int: ident le $value: expr) => {
        request!(write all $int::to_le_bytes($value))
    };
    (write u8 $value: expr) => {
        request!(write all &[$value])
    };
    (copy $from: expr, $count: expr, $to: expr) => {
        (yield $crate::base::WriteRequest::Copy {
            from: $from,
            count: $count,
            to: $to,
        }
        .into())
        .assert_none()
    };
    (seek $seekfrom: expr) => {
        (yield $crate::base::ReadSeekRequest::Seek($seekfrom).into()).assert_into_seek()
    };
    (rewind) => {
        (yield $crate::base::ReadSeekRequest::Seek($crate::base::SeekFrom::Start(0)).into())
            .assert_into_seek()
    };
    (stream len) => {{
        let prev = (yield $crate::base::ReadSeekRequest::Seek($crate::base::SeekFrom::Current(0))
            .into())
        .assert_into_seek();
        let len = (yield $crate::base::ReadSeekRequest::Seek($crate::base::SeekFrom::End(0))
            .into())
        .assert_into_seek();
        (yield $crate::base::ReadSeekRequest::Seek($crate::base::SeekFrom::Start(prev)).into())
            .assert_into_seek();
        len
    }};
    (stream pos) => {{
        (yield $crate::base::ReadSeekRequest::Seek($crate::base::SeekFrom::Current(0)).into())
            .assert_into_seek()
    }};
    (truncate $size: expr) => {
        (yield $crate::base::ReadSeekWriteTruncateRequest::Truncate($size).into()).assert_none()
    };
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy)]
    struct RawFlags: u32 {
        const DEFLATED = 1 << 24;
    }
}

/// Flags that change the way entries are written.
#[derive(Debug, Default, Clone)]
pub struct Flags {
    /// The kind of compression in use.
    pub compression: EntryCompression,
}

pub use flate2::Compression;

/// An enum that specifies the ways entries can be compressed.
#[derive(Debug, Default, Clone)]
pub enum EntryCompression {
    /// Deflate compression with the specified level
    Deflate(flate2::Compression),
    #[default]
    /// No compression
    None,
}

impl EntryCompression {
    /// Returns whether `self` is [`None`](EntryCompression::None)
    pub fn is_none(&self) -> bool {
        matches!(self, EntryCompression::None)
    }
}

#[derive(Debug, Clone)]
pub struct EntryInfo {
    pub index: usize,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    pub compression: EntryCompression,
}

#[derive(Debug, Clone)]
struct Entry {
    path_hash: u32,
    relative_path_offset: u32,
    path: String,

    data_offset: u32,
    data_size: u32,
    unpacked_size: u32,
    flags: RawFlags,
}

impl Entry {
    #[generator(static, yield WriteRequest -> Response)]
    fn write(&self) -> () {
        let path_offset_and_flags: u32 = self.relative_path_offset | self.flags.bits();

        request!(write u32 be self.path_hash);
        request!(write u32 be path_offset_and_flags);
        request!(write u32 be self.data_offset);
        request!(write u32 be self.data_size);
        request!(write u32 be self.unpacked_size);
    }

    #[generator(static, yield WriteRequest -> Response)]
    fn write_empty() -> () {
        request!(write repeated 0, ENTRY_SIZE);
    }
}

pub struct PkgState {
    path_region_size: u32,
    path_region_empty_offset: u32,

    entries: Vec<Option<Entry>>,
    path_to_entry_index_map: HashMap<String, usize>,
}

impl PkgState {
    pub fn contains(&self, path: &str) -> bool {
        self.path_to_entry_index_map.contains_key(path)
    }

    pub fn paths(&self) -> impl Iterator<Item = &String> {
        self.path_to_entry_index_map.keys()
    }

    #[inline]
    fn entry_list_offset() -> u64 {
        HEADER_SIZE
    }

    #[inline]
    fn path_region_offset(&self) -> u64 {
        HEADER_SIZE + self.entries.len() as u64 * ENTRY_SIZE
    }

    #[inline]
    fn data_region_offset(&self) -> u64 {
        self.path_region_offset() + self.path_region_size as u64
    }
}

pub use crate::errors::*;

mod common;
pub use common::*;
mod parse;
pub use parse::*;
mod read;
pub use read::*;
mod write;
pub use write::*;
