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

use std::{
    cmp::Ordering,
    collections::HashMap,
    fs::File,
    io::{self, Read, Seek, Write},
};

mod ioext;
use flate2::{read::ZlibDecoder, write::ZlibEncoder};
use ioext::*;

pub use flate2::Compression;

bitflags::bitflags! {
    struct RawFlags: u32 {
        const DEFLATED = 1 << 24;
    }
}

#[derive(Debug, Default, Clone)]
pub struct Flags {
    pub compression: EntryCompression,
}

#[derive(Debug, Clone)]
pub enum EntryCompression {
    Deflate(flate2::Compression),
    None,
}

impl Default for EntryCompression {
    fn default() -> Self {
        EntryCompression::None
    }
}

const MAGIC: &[u8] = b"PKG\n";
const HEADER_SIZE: u64 = 16;
const ENTRY_SIZE: u64 = 20;
const PREALLOCATED_PATH_LEN: u64 = 30;

// FIXME: Should this be i64 instead?
fn pkg_path_hash(path: &str) -> u32 {
    let mut hash: u32 = 0;
    for mut c in path.chars() {
        assert!(c.is_ascii(), "non-ascii string passed to pkg_path_hash()");

        // TODO: Why is this case insensitive
        c.make_ascii_lowercase();
        // FIXME: Slipstream uses an unsigned shr
        //        (does that make a difference if we use an unsigned type here?)
        hash = hash.overflowing_shl(27).0 | hash.overflowing_shr(5).0;
        hash ^= c as u32;
        hash &= 0x00000000FFFFFFFF;
    }
    hash
}

pub trait Truncate {
    fn truncate(&mut self, len: u64) -> io::Result<()>;
}

impl Truncate for File {
    fn truncate(&mut self, len: u64) -> io::Result<()> {
        self.set_len(len)
    }
}

impl Truncate for Vec<u8> {
    fn truncate(&mut self, len: u64) -> io::Result<()> {
        self.resize(len as usize, 0);
        Ok(())
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
    fn write_to(&self, writer: &mut impl Write) -> io::Result<()> {
        let path_offset_and_flags: u32 = self.relative_path_offset | self.flags.bits;

        writer.write_u32_be(self.path_hash)?;
        writer.write_u32_be(path_offset_and_flags)?;
        writer.write_u32_be(self.data_offset)?;
        writer.write_u32_be(self.data_size)?;
        writer.write_u32_be(self.unpacked_size)?;

        Ok(())
    }
}

pub struct Pkg<S: Read + Seek> {
    storage: S,

    path_region_size: u32,
    path_region_empty_offset: u32,

    entries: Vec<Option<Entry>>,
    path_to_entry_index_map: HashMap<String, usize>,
}

impl<S: Read + Seek + Write> Pkg<S> {
    pub fn flush(&mut self) -> io::Result<()> {
        self.storage.flush()
    }

    pub fn contains(&self, path: &str) -> bool {
        self.path_to_entry_index_map.contains_key(path)
    }

    pub fn remove(&mut self, path: &str) -> io::Result<bool> {
        if let Some(entry_idx) = self.path_to_entry_index_map.remove(path) {
            self.entries[entry_idx] = None;

            self.storage.seek(io::SeekFrom::Start(
                self.entry_list_offset() + entry_idx as u64 * ENTRY_SIZE,
            ))?;
            self.storage.write_all(&[0x00; ENTRY_SIZE as usize])?;

            // TODO: Slipstream does a nice optimisation here and truncates if the data was at the end
            //       but we can't do that until specialisation comes around. (if we want to support non
            //       Truncate writers)

            Ok(true)
        } else {
            Ok(false)
        }
    }

    // PERF: This is clearly not optimal but using raw_entry_mut is impossible because the
    //       borrow checker gets mad
    pub fn rename(&mut self, src: &str, dst: String) -> io::Result<()> {
        if !self.path_to_entry_index_map.contains_key(src) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "path not found in archive",
            ));
        }
        if self.path_to_entry_index_map.contains_key(&dst) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "destination path already exists",
            ));
        }

        let entry = self.path_to_entry_index_map.remove(src).unwrap();
        self.path_to_entry_index_map.insert(dst, entry);
        Ok(())
    }

    fn write_packed_path_region_at(&mut self, offset: u64) -> io::Result<u64> {
        self.storage.seek(io::SeekFrom::Start(offset))?;

        let mut size = 0;
        for entry in self.entries.iter_mut().map(|e| e.as_mut().unwrap()) {
            entry.relative_path_offset = (self.storage.stream_position()? - offset) as u32;
            self.storage.write_all(entry.path.as_bytes())?;
            self.storage.write_all(&[0x00])?;
            size += entry.path.as_bytes().len() + 1;
        }

        Ok(size as u64)
    }

    fn insert_path_into_path_region(&mut self, path: &str) -> io::Result<u32> {
        log::trace!(target: "silpkg",
            "Inserting path {path} at {}/{}",
            self.path_region_empty_offset, self.path_region_size
        );
        if self.path_region_empty_offset + path.len() as u32 + 1 >= self.path_region_size {
            self.reserve_path_space(path.len() as u32 + 1 + PREALLOCATED_PATH_LEN as u32 * 32)?;
        }
        let offset = self.path_region_empty_offset;
        self.storage.seek(io::SeekFrom::Start(
            self.path_region_offset() + self.path_region_empty_offset as u64,
        ))?;

        self.storage.write_all(path.as_bytes())?;
        self.storage.write_all(&[0x00])?;

        self.path_region_empty_offset += path.len() as u32 + 1;

        Ok(offset)
    }

    fn push_back_data_region(&mut self, offset: u64) -> io::Result<()> {
        log::trace!(target: "silpkg", "Moving data region to {offset}");
        let entries_to_move = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, opt)| opt.as_ref().map(|entry| (i, entry)))
            .filter_map(|(i, entry)| {
                if (entry.data_offset as u64) < offset {
                    Some(i)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        log::trace!("Moving {} entries", entries_to_move.len());
        for i in entries_to_move {
            let mut entry = self.entries[i].take().unwrap();
            let new_offset = self.storage.seek(io::SeekFrom::End(0))?;
            // TODO: Do not panic on conversion to u32
            let old_offset = std::mem::replace(&mut entry.data_offset, new_offset as u32);

            self.storage
                .copy_within(old_offset.into(), entry.data_size.into(), new_offset)?;

            self.storage.seek(io::SeekFrom::Start(
                self.entry_list_offset() + i as u64 * ENTRY_SIZE,
            ))?;
            entry.write_to(&mut self.storage)?;
            self.entries[i] = Some(entry);
        }

        Ok(())
    }

    fn push_back_and_resize_path_region(&mut self, offset: u64, new_size: u64) -> io::Result<()> {
        log::trace!(target: "silpkg", "Moving path region to {} with a new size of {}", offset, new_size);
        self.push_back_data_region(offset + new_size)?;

        self.storage.copy_within(
            self.path_region_offset(),
            self.path_region_size as u64,
            offset,
        )?;

        self.path_region_size = new_size as u32;
        self.storage
            .seek(io::SeekFrom::Start(MAGIC.len() as u64 + 8))?;
        self.storage.write_u32_be(self.path_region_size)?;

        Ok(())
    }

    // fn push_back_path_region(&mut self, offset: u64) -> io::Result<()> {
    //     self.push_back_and_resize_path_region(offset, self.path_region_size as u64)
    // }

    fn reserve_path_space(&mut self, amount: u32) -> io::Result<()> {
        log::trace!(target: "silpkg", "Resizing path region");
        let new_path_region_size = self.path_region_size + amount;
        let new_path_region_start = self.path_region_offset();
        let new_path_region_end = new_path_region_start as u32 + new_path_region_size;

        self.push_back_data_region(new_path_region_end as u64)?;

        self.storage.seek(io::SeekFrom::Start(
            new_path_region_start + self.path_region_empty_offset as u64,
        ))?;
        self.storage.fill(
            0,
            (new_path_region_size - self.path_region_empty_offset).into(),
        )?;

        self.path_region_size = new_path_region_size;

        self.storage
            .seek(io::SeekFrom::Start(MAGIC.len() as u64 + 8))?;
        self.storage.write_u32_be(self.path_region_size)?;

        Ok(())
    }

    pub fn reserve_entries(&mut self, amount: u32) -> io::Result<()> {
        log::trace!(target: "silpkg", "Resizing entry list");
        let required_extra_entry_space = amount * ENTRY_SIZE as u32;
        let required_extra_path_space = amount * PREALLOCATED_PATH_LEN as u32;

        let entry_list_grow_start =
            self.entry_list_offset() + self.entries.len() as u64 * ENTRY_SIZE;

        let new_path_region_offset = entry_list_grow_start as u32 + required_extra_entry_space;

        self.push_back_and_resize_path_region(
            new_path_region_offset as u64,
            self.path_region_size as u64 + required_extra_path_space as u64,
        )?;

        self.storage
            .seek(io::SeekFrom::Start(entry_list_grow_start))?;

        self.storage.fill(0, required_extra_entry_space.into())?;

        self.entries.reserve_exact(amount as usize);
        for _ in 0..amount {
            self.entries.push(None);
        }

        self.storage
            .seek(io::SeekFrom::Start(MAGIC.len() as u64 + 4))?;
        self.storage.write_u32_be(self.entries.len() as u32)?;

        Ok(())
    }

    pub fn create(mut storage: S) -> io::Result<Self> {
        storage.rewind()?;

        storage.write_all(MAGIC)?;
        storage.write_u16_be(HEADER_SIZE as u16)?;
        storage.write_u16_be(ENTRY_SIZE as u16)?;

        let starting_entry_count = 16;
        let starting_path_region_size = starting_entry_count * PREALLOCATED_PATH_LEN;
        storage.write_u32_be(starting_entry_count as u32)?;
        storage.write_u32_be(starting_path_region_size as u32)?;

        storage.fill(
            0,
            starting_path_region_size + starting_entry_count * ENTRY_SIZE,
        )?;

        Ok(Self {
            storage,

            path_region_size: starting_path_region_size as u32,
            path_region_empty_offset: 0,
            entries: vec![None; starting_entry_count as usize],
            path_to_entry_index_map: HashMap::default(),
        })
    }

    pub fn insert(
        &mut self,
        path: String,
        flags: Flags,
        mut reader: impl Read,
    ) -> io::Result<()> {
        if self.path_to_entry_index_map.contains_key(&path) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "path already exists in archive",
            ));
        }

        let entry_slot = match self.entries.iter().enumerate().find(|(_i, o)| o.is_none()) {
            Some((i, _o)) => i,
            None => {
                let i = self.entries.len();
                self.reserve_entries(64)?;
                i
            }
        };

        assert!(self
            .path_to_entry_index_map
            .insert(path.clone(), entry_slot)
            .is_none());

        let relative_path_offset = self.insert_path_into_path_region(&path)?;
        let data_offset = self.storage.seek(io::SeekFrom::End(0))?;
        let mut entry = Entry {
            data_offset: data_offset as u32,
            path_hash: pkg_path_hash(&path),
            relative_path_offset,
            path,
            data_size: 0,
            flags: match flags.compression {
                EntryCompression::Deflate(_) => RawFlags::DEFLATED,
                EntryCompression::None => RawFlags::empty(),
            },
            unpacked_size: 0,
        };

        if let EntryCompression::Deflate(level) = flags.compression {
            log::trace!(target: "silpkg", "Writing compressed entry data to {}", data_offset);
            let mut writer = ZlibEncoder::new(&mut self.storage, level);
            std::io::copy(&mut reader, &mut writer)?;
            writer.try_finish()?;
            log::trace!(target: "silpkg",
                "Data size: {}B ({}B compressed)",
                writer.total_in(),
                writer.total_out(),
            );
            entry.data_size = writer.total_out() as u32;
            entry.unpacked_size = writer.total_in() as u32;
        } else {
            log::trace!(target: "silpkg", "Writing entry data to {}", data_offset);
            entry.data_size = std::io::copy(&mut reader, &mut self.storage)? as u32;
            entry.unpacked_size = entry.data_size;
        }

        log::trace!(target: "silpkg", "Updating entry at {} with written data", entry_slot);
        self.storage.seek(io::SeekFrom::Start(
            self.entry_list_offset() + entry_slot as u64 * ENTRY_SIZE,
        ))?;

        entry.write_to(&mut self.storage)?;
        self.entries[entry_slot] = Some(entry);

        Ok(())
    }
}

/// These functions require the [`Truncate`] trait to be implemented because they may shrink the
/// archive.
///
/// Currently there is no such trait in the standard library so this library provides this trait
/// and implements it for [`File`] and [`Vec<u8>`].
///
/// [`File`]: std::fs::File
impl<S: Read + Seek + Write + Truncate> Pkg<S> {
    /// Packs the archive to be the smallest possible size at the price of easy expansion.
    ///
    /// This function is pretty expensive and also makes proceeding [`insert`]s slower.
    ///
    /// [`insert`]: Pkg::insert
    pub fn repack(&mut self) -> io::Result<()> {
        // Remove empty entries
        self.entries.drain_filter(|entry| entry.is_none());
        self.entries.sort_by(|a, b| {
            let ea = a.as_ref().unwrap();
            let eb = b.as_ref().unwrap();

            match ea.data_offset.cmp(&eb.data_offset) {
                Ordering::Equal => ea.data_size.cmp(&eb.data_size),
                ord => ord,
            }
        });

        // Check for overlapping entries
        for window in self.entries.windows(2) {
            if let [Some(a), Some(b)] = window {
                if a.data_offset + a.data_size > b.data_offset {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "repacking PKGs with overlapping entries is unsupported",
                    ));
                }
            } else {
                unreachable!()
            };
        }

        let path_region_size: usize = self
            .entries
            .iter()
            .map(|entry| entry.as_ref().unwrap().path.len() + 1)
            .sum();

        let path_region_offset = self.path_region_offset();
        let data_region_start = path_region_offset + path_region_size as u64;

        assert!(data_region_start <= self.data_region_offset());

        // Update the path region
        log::trace!(target: "silpkg", "Packing path region");
        self.path_region_size = self.write_packed_path_region_at(path_region_offset)? as u32;
        assert_eq!(self.path_region_size, path_region_size as u32);

        // Defragment? the data region

        let mut current_data_offset = data_region_start as u32;
        // TODO: The unwraps are getting really annoying and possibly degrading performance
        //       Maybe something should be done about this? (I would love to avoid unwrap unchecked too)
        log::trace!(target: "silpkg", "Defragmenting data region");
        for entry in self.entries.iter_mut().map(|e| e.as_mut().unwrap()) {
            if current_data_offset != entry.data_offset {
                self.storage.copy_within(
                    entry.data_offset.into(),
                    entry.data_size.into(),
                    current_data_offset.into(),
                )?;

                entry.data_offset = current_data_offset;
            }

            current_data_offset += entry.data_size;
        }

        self.entries
            .sort_by_key(|entry| entry.as_ref().unwrap().path_hash);

        // Update path_to_entry_index_map
        for (i, entry) in self.entries.iter().enumerate() {
            *self
                .path_to_entry_index_map
                .get_mut(&entry.as_ref().unwrap().path)
                .unwrap() = i;
        }

        // And finally, update the header and write the entries!
        log::trace!(target: "silpkg", "Rewriting entry list");
        self.storage
            .seek(io::SeekFrom::Start(MAGIC.len() as u64 + 4))?;
        self.storage.write_u32_be(self.entries.len() as u32)?;
        self.storage.write_u32_be(path_region_size as u32)?;

        for maybe_entry in self.entries.iter() {
            match maybe_entry {
                Some(entry) => entry.write_to(&mut self.storage)?,
                None => self.storage.write_all(&[0x00; ENTRY_SIZE as usize])?,
            }
        }

        self.storage.truncate(current_data_offset.into())?;

        Ok(())
    }
} // Read + Seek + Write + Truncate

impl<S: Read + Seek> Pkg<S> {
    #[inline]
    fn entry_list_offset(&self) -> u64 {
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

    pub fn is_pkg(mut storage: S) -> io::Result<bool> {
        storage.rewind()?;

        for b in MAGIC {
            if storage.read_u8()? != *b {
                return Ok(false);
            }
        }

        Ok(true)
    }

    pub fn parse(mut storage: S, expect_magic: bool) -> io::Result<Self> {
        storage.rewind()?;

        if expect_magic {
            for b in MAGIC {
                if storage.read_u8()? != *b {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "File does not seem to be a PKG file",
                    ));
                }
            }
        }

        {
            let header_size = storage.read_u16_be()?;
            if header_size as u64 != HEADER_SIZE {
                return Err(io::Error::new(io::ErrorKind::InvalidData, format!("File header claims header size is {header_size} while {HEADER_SIZE} was expected")));
            }
        }

        {
            let entry_size = storage.read_u16_be()?;
            if entry_size as u64 != ENTRY_SIZE {
                return Err(io::Error::new(io::ErrorKind::InvalidData, format!("File header claims entry size is {entry_size} while {ENTRY_SIZE} was expected")));
            }
        }

        let storage_len = storage.stream_len()?;
        let entry_count = storage.read_u32_be()? as u64;
        if (HEADER_SIZE + entry_count * ENTRY_SIZE) > storage_len {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "File header claims the file's entry list continues after EOF",
            ))?
        }

        let path_region_size = storage.read_u32_be()?;
        if (HEADER_SIZE + entry_count * ENTRY_SIZE) + path_region_size as u64 > storage_len {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "File header claims the file's string table continues after EOF",
            ))?
        }

        let mut entries = Vec::with_capacity(entry_count as usize);
        let mut path_to_entry_index_map = HashMap::new();

        for _ in 0..entry_count {
            let path_hash = storage.read_u32_be()?;

            let path_offset_and_flags = storage.read_u32_be()?;
            let path_offset = path_offset_and_flags & 0x00FFFFFF;
            let flag_bits = path_offset_and_flags & 0xFF000000;
            let flags = RawFlags::from_bits(flag_bits).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Entry contains unrecognised flags {flag_bits:x}"),
                )
            })?;

            let data_offset = storage.read_u32_be()?;
            let data_size = storage.read_u32_be()?;
            let unpacked_size = storage.read_u32_be()?;

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

        let mut path_region_buf = vec![0u8; path_region_size as usize];
        storage.read_exact(&mut path_region_buf)?;
        for (i, maybe_entry) in entries.iter_mut().enumerate() {
            if let Some(entry) = maybe_entry {
                let path = path_region_buf[entry.relative_path_offset as usize..]
                    .iter()
                    // TODO: Fail if null terminator is not present
                    .take_while(|b| **b != 0)
                    .map(|b| {
                        if !b.is_ascii() {
                            Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "Entry has a non-ASCII or invalid path",
                            ))
                        } else {
                            Ok(*b as char)
                        }
                    })
                    .try_collect::<String>()?;

                entry.path = path.clone();
                path_to_entry_index_map.try_insert(path, i).map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "File contains two entries with the same path ({:?})",
                            e.entry.key()
                        ),
                    )
                })?;
            }
        }

        Ok(Self {
            storage,

            path_region_size,
            path_region_empty_offset: path_region_size
                - (path_region_buf
                    .iter()
                    .rev()
                    .take_while(|b| **b == 0)
                    .count() as u32
                    - 1),
            entries,
            path_to_entry_index_map,
        })
    }

    pub fn paths(&self) -> impl Iterator<Item = &String> {
        self.path_to_entry_index_map.keys()
    }

    pub fn extract_to(&mut self, path: &str, mut writer: impl Write) -> io::Result<()> {
        let entry = self.entries[match self.path_to_entry_index_map.get(path) {
            Some(index) => *index,
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "Entry not found in archive",
                ))
            }
        }]
        .as_ref()
        .unwrap();

        self.storage
            .seek(io::SeekFrom::Start(entry.data_offset as u64))?;

        if entry.flags.contains(RawFlags::DEFLATED) {
            std::io::copy(
                &mut ZlibDecoder::new((&mut self.storage).take(entry.data_size.into())),
                &mut writer,
            )?;
        } else {
            std::io::copy(
                &mut (&mut self.storage).take(entry.data_size as u64),
                &mut writer,
            )?;
        }

        Ok(())
    }

    // TODO: Add a way to access this metadata
    // pub fn fixme_remove_this_print_size_info(&mut self) {
    //     {
    //         let occupied = self.entries.iter().filter(|x| x.is_some()).count();
    //         println!(
    //             "Entries: {} ({} occupied, {} empty)",
    //             self.entries.len(),
    //             occupied,
    //             self.entries.len() - occupied
    //         );
    //     }
    //     println!(
    //         "Path region size: {}B ({}B occupied, {}B unused)",
    //         self.path_region_size,
    //         self.path_region_empty_offset,
    //         self.path_region_size - self.path_region_empty_offset
    //     );
    //
    //     println!("Data region:");
    //     let data_region_size = self.storage.stream_len().unwrap() - self.data_region_offset();
    //     let occupied_data_region = self
    //         .entries
    //         .iter()
    //         .filter_map(|opt| opt.as_ref().map(|entry| entry.data_size as u64))
    //         .sum::<u64>();
    //     println!(
    //         "  Size: {data_region_size}B ({}B occupied, {}B unused)",
    //         occupied_data_region,
    //         data_region_size - occupied_data_region
    //     );
    //     println!(
    //         "  Fragmentation: {:.4}%",
    //         ((data_region_size - occupied_data_region) as f64) / data_region_size as f64
    //     )
    // }
} // Read + Seek
