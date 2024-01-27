use alloc::{string::String, vec::Vec};

use flate2::Decompress;
use hashbrown::HashMap;
use macros::generator;

use crate::{
    base::{BUFFER_SIZE, ENTRY_SIZE, HEADER_SIZE, MAGIC},
    util::ByteSliceExt,
};

use super::{
    Entry, OpenError, ParseError, PkgState, RawFlags, ReadSeekRequest, Response, SeekError,
    SeekFrom,
};

#[generator(static, yield ReadSeekRequest -> Response)]
pub fn check_magic() -> bool {
    for expected in MAGIC {
        let byte = match request!(read 1)[..] {
            [byte] => byte,
            [] => return false,
            _ => unreachable!(),
        };

        if byte != *expected {
            return false;
        }
    }

    true
}

#[generator(static, yield ReadSeekRequest -> Response)]
pub fn parse(expect_magic: bool) -> Result<PkgState, ParseError> {
    request!(rewind);

    if expect_magic && !check_magic().await {
        return Err(ParseError::MismatchedMagic);
    }

    let read = request!(read exact 4);

    {
        let header_size = read[0..2].as_u16_be();
        if header_size as u64 != HEADER_SIZE {
            return Err(ParseError::MismatchedHeaderSize { size: header_size });
        }
    }

    {
        let entry_size = read[2..4].as_u16_be();
        if entry_size as u64 != ENTRY_SIZE {
            return Err(ParseError::MismatchedEntrySize { size: entry_size });
        }
    }

    let storage_len = request!(stream len);
    let read = request!(read exact 8);
    let entry_count = read[0..4].as_u32_be();
    if (HEADER_SIZE + entry_count as u64 * ENTRY_SIZE) > storage_len {
        return Err(ParseError::EntryOverflow);
    }

    let path_region_size = read[4..8].as_u32_be();
    if (HEADER_SIZE + entry_count as u64 * ENTRY_SIZE) + path_region_size as u64 > storage_len {
        return Err(ParseError::PathOverflow);
    }

    let mut entries = Vec::with_capacity(entry_count as usize);
    let mut path_to_entry_index_map = HashMap::new();

    for _ in 0..entry_count {
        let read = request!(read exact 20);
        let path_hash = read[0..4].as_u32_be();

        let path_offset_and_flags = read[4..8].as_u32_be();
        let path_offset = path_offset_and_flags & 0x00FFFFFF;
        let flag_bits = path_offset_and_flags & 0xFF000000;
        let flags =
            RawFlags::from_bits(flag_bits).ok_or(ParseError::UnrecognisedEntryFlags(flag_bits))?;

        let data_offset = read[8..12].as_u32_be();
        let data_size = read[12..16].as_u32_be();
        let unpacked_size = read[16..20].as_u32_be();

        if data_offset == 0 {
            entries.push(None)
        } else {
            entries.push(Some(Entry {
                path_hash,
                relative_path_offset: path_offset,
                path: String::new(),
                data_offset,
                data_size,
                unpacked_size,
                flags,
            }))
        }
    }

    let read = request!(read exact path_region_size.into());
    for (i, maybe_entry) in entries.iter_mut().enumerate() {
        if let Some(entry) = maybe_entry {
            let path = read[entry.relative_path_offset as usize..]
                .iter()
                // TODO: Fail if null terminator is not present
                .take_while(|b| **b != 0)
                .map(|b| {
                    if !b.is_ascii() {
                        Err(ParseError::NonAsciiPath)
                    } else {
                        Ok(*b as char)
                    }
                })
                .try_collect::<String>()?;

            entry.path = path.clone();
            path_to_entry_index_map
                .try_insert(path, i)
                .map_err(|e| ParseError::SamePath(e.entry.key().clone()))?;
        }
    }

    Ok(PkgState {
        path_region_size,
        path_region_empty_offset: path_region_size
            .checked_add_signed(-((read.iter().rev().take_while(|b| **b == 0).count() as i32) - 1))
            .unwrap(),
        entries,
        path_to_entry_index_map,
    })
}

pub struct RawReadWriteHandle {
    pub(super) offset: u64,
    pub(super) cursor: u64,
    pub(super) size: u64,
}

pub struct DeflateReadHandle {
    offset: u64,
    cursor: u64,
    size: u64,

    decompress: Decompress,
    done: bool,
}

pub enum ReadHandle {
    Raw(RawReadWriteHandle),
    Deflate(DeflateReadHandle),
}

#[generator(static, yield ReadSeekRequest -> Response, lifetime 'coro)]
pub fn open<'coro>(state: &'coro PkgState, path: &'coro str) -> Result<ReadHandle, OpenError> {
    let entry = state.entries[match state.path_to_entry_index_map.get(path) {
        Some(index) => *index,
        None => return Err(OpenError::NotFound),
    }]
    .as_ref()
    .unwrap();

    request!(seek SeekFrom::Start(entry.data_offset as u64));

    Ok(if entry.flags.contains(RawFlags::DEFLATED) {
        ReadHandle::Deflate(DeflateReadHandle {
            offset: entry.data_offset.into(),
            cursor: 0,
            size: entry.data_size.into(),
            decompress: Decompress::new(true),
            done: false,
        })
    } else {
        ReadHandle::Raw(RawReadWriteHandle {
            offset: entry.data_offset.into(),
            cursor: 0,
            size: entry.data_size.into(),
        })
    })
}

impl RawReadWriteHandle {
    #[generator(static, yield ReadSeekRequest -> Response, lifetime 'coro)]
    pub fn read<'coro>(&'coro mut self, buffer: &'coro mut [u8]) -> usize {
        let end = (self.cursor + buffer.len() as u64).min(self.size);
        let count = end - self.cursor;
        let value = request!(read count);
        buffer[..value.len()].copy_from_slice(&value);
        self.cursor += count;
        value.len()
    }

    #[generator(static, yield ReadSeekRequest -> Response, lifetime 'coro)]
    pub fn seek<'coro>(&'coro mut self, seekfrom: SeekFrom) -> Result<u64, SeekError> {
        match seekfrom {
            SeekFrom::Start(start) => {
                request!(seek SeekFrom::Start(self.offset + start));
                Ok(start)
            }
            SeekFrom::End(end) => {
                match self.size.checked_add_signed(end).map(|x| x + self.offset) {
                    Some(off) => {
                        request!(seek SeekFrom::Start(off));
                        Ok(off - self.offset)
                    }
                    None => Err(SeekError::SeekOutOfBounds),
                }
            }
            SeekFrom::Current(off) => match self.cursor.checked_add_signed(off) {
                Some(off) => {
                    request!(seek SeekFrom::Start(self.offset + off));
                    Ok(off)
                }
                None => Err(SeekError::SeekOutOfBounds),
            },
        }
    }
}

impl DeflateReadHandle {
    #[generator(static, yield ReadSeekRequest -> Response)]
    pub fn read(&mut self, mut buffer: &mut [u8]) -> usize {
        if self.done {
            return 0;
        }

        log::trace!("Writing compressed entry data to {}", self.offset);

        let mut read = 0;

        while !buffer.is_empty() {
            let end = (self.cursor + BUFFER_SIZE / 2).min(self.size);
            let count = end - self.cursor;

            let prev_in = self.decompress.total_in();
            let prev_out = self.decompress.total_out();
            request!(seek SeekFrom::Start(self.offset + self.cursor));
            let input = request!(read count);
            let status = self
                .decompress
                .decompress(&input, buffer, flate2::FlushDecompress::None)
                .unwrap();

            let read_now = (self.decompress.total_out() - prev_out) as usize;
            let consumed_now = self.decompress.total_in() - prev_in;

            read += read_now;
            self.cursor += consumed_now;

            match status {
                flate2::Status::StreamEnd => {
                    self.done = true;
                    break;
                }
                flate2::Status::Ok | flate2::Status::BufError => {
                    buffer = &mut buffer[read_now..];
                }
            };

            if read_now == 0 && consumed_now == 0 {
                if self.size != 0 {
                    log::warn!(
                        "Deflate stream ended unexpectedly, resulting data may be truncated!"
                    );
                }
                self.done = true;
                break;
            }
        }

        read
    }
}

impl ReadHandle {
    pub fn is_compressed(&self) -> bool {
        match self {
            ReadHandle::Raw(_) => false,
            ReadHandle::Deflate(_) => true,
        }
    }

    pub fn is_seekable(&self) -> bool {
        !self.is_compressed()
    }
}
