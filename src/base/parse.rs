use alloc::{string::String, vec::Vec};

use hashbrown::HashMap;
use macros::generator;

use crate::{
    base::{ENTRY_SIZE, HEADER_SIZE, MAGIC},
    util::ByteSliceExt,
};

use super::{Entry, ParseError, PkgState, RawFlags, ReadSeekRequest, Response};

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
