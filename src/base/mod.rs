use std::collections::HashMap;

pub enum ReadSeekRequest {
    Read(u64),
    ReadExact(u64),
    Seek(std::io::SeekFrom),
}

pub enum WriteRequest {
    // TODO: Transient borrow, maybe this can be worked around using pointers?
    WriteAll(Vec<u8>),
    Write(Vec<u8>),
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
    (write all $buffer: expr) => {
        (yield $crate::base::WriteRequest::WriteAll($buffer.into()).into()).assert_none()
    };
    (write $buffer: expr) => {
        (yield $crate::base::WriteRequest::Write($buffer.into()).into()).assert_into_written()
    };
    (@uninit_buf $size: expr) => {};
    // (write from $reader: expr) => {{
    //     let mut reader = $reader;
    //     let mut total_read = 0;
    //
    //     let mut buf = [0; $crate::base::BUFFER_SIZE as usize];
    //     loop {
    //         let read = ::std::io::Read::read(&mut reader, &mut buf[..])?;
    //         if read == 0 {
    //             break;
    //         }
    //         total_read += read;
    //
    //         (yield $crate::base::WriteRequest::Write(bbuf.filled().to_vec()).into()).assert_none();
    //
    //         bbuf.clear();
    //     }
    //
    //     total_read
    // }};
    (write repeated $value: expr, $count: expr) => {
        (yield ($crate::base::WriteRequest::WriteRepeated {
            value: $value,
            count: $count,
        })
        .into())
        .assert_none()
    };
    (write $int: ident be $value: expr) => {
        (yield $crate::base::WriteRequest::WriteAll($int::to_be_bytes($value).to_vec()).into())
            .assert_none()
    };
    (write $int: ident le $value: expr) => {
        (yield $crate::base::WriteRequest::WriteAll($int::to_be_bytes($value).to_vec()).into())
            .assert_none()
    };
    (write u8 $value: expr) => {
        (yield $crate::base::WriteRequest::WriteAll(vec![$value]).into()).assert_none()
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
        (yield $crate::base::ReadSeekRequest::Seek(::std::io::SeekFrom::Start(0)).into())
            .assert_into_seek()
    };
    (stream len) => {{
        let prev = (yield $crate::base::ReadSeekRequest::Seek(::std::io::SeekFrom::Current(0))
            .into())
        .assert_into_seek();
        let len = (yield $crate::base::ReadSeekRequest::Seek(::std::io::SeekFrom::End(0)).into())
            .assert_into_seek();
        (yield $crate::base::ReadSeekRequest::Seek(::std::io::SeekFrom::Start(prev)).into())
            .assert_into_seek();
        len
    }};
    (stream pos) => {{
        (yield $crate::base::ReadSeekRequest::Seek(::std::io::SeekFrom::Current(0)).into())
            .assert_into_seek()
    }};
    (truncate $size: expr) => {
        (yield $crate::base::ReadSeekWriteTruncateRequest::Truncate($size).into()).assert_none()
    };
}

pub trait Truncate {
    // FIXME: Should this be i64 instead?
    fn truncate(&mut self, len: u64) -> std::io::Result<()>;
}

impl Truncate for Vec<u8> {
    fn truncate(&mut self, len: u64) -> std::io::Result<()> {
        self.resize(len as usize, 0);
        Ok(())
    }
}

impl Truncate for std::fs::File {
    fn truncate(&mut self, len: u64) -> std::io::Result<()> {
        self.set_len(len)
    }
}

impl<T: Truncate> Truncate for std::io::Cursor<T> {
    fn truncate(&mut self, len: u64) -> std::io::Result<()> {
        self.get_mut().truncate(len)
    }
}

impl<T: Truncate> Truncate for &mut T {
    fn truncate(&mut self, len: u64) -> std::io::Result<()> {
        (*self).truncate(len)
    }
}

impl<T: Truncate> Truncate for Box<T> {
    fn truncate(&mut self, len: u64) -> std::io::Result<()> {
        self.as_mut().truncate(len)
    }
}

bitflags::bitflags! {
    struct RawFlags: u32 {
        const DEFLATED = 1 << 24;
    }
}

#[derive(Debug, Default, Clone)]
pub struct Flags {
    pub compression: EntryCompression,
}

pub use flate2::Compression;

#[derive(Debug, Default, Clone)]
pub enum EntryCompression {
    Deflate(flate2::Compression),
    #[default]
    None,
}

impl EntryCompression {
    pub fn is_none(&self) -> bool {
        matches!(self, EntryCompression::None)
    }
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
        let path_offset_and_flags: u32 = self.relative_path_offset | self.flags.bits;

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

mod common;
pub use common::*;
mod read;
use macros::generator;
pub use read::*;
mod write;
pub use write::*;
