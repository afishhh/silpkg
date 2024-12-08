use alloc::{string::String, vec, vec::Vec};
use core::{cmp::Ordering, marker::PhantomData};

use flate2::Compress;
use hashbrown::HashMap;
use macros::generator;

use crate::{
    base::{
        pkg_path_hash, PkgState, RawFlags, ReadSeekWriteRequest, Response, SeekFrom, BUFFER_SIZE,
        ENTRY_SIZE, HEADER_SIZE, MAGIC,
    },
    EntryCompression, Flags,
};

use super::{
    CreateError, Entry, InsertError, RawReadWriteHandle, ReadSeekWriteTruncateRequest, RemoveError,
    RenameError, RepackError, ReplaceError,
};

const PREALLOCATED_PATH_LEN: u64 = 30;
const PREALLOCATED_ENTRY_COUNT: u64 = 64;

impl PkgState {
    #[generator(static, yield ReadSeekWriteRequest -> Response)]
    pub fn create() -> Result<PkgState, CreateError> {
        request!(rewind);

        request!(write all MAGIC);
        request!(write u16 be HEADER_SIZE as u16);
        request!(write u16 be ENTRY_SIZE as u16);

        let initial_entry_count = PREALLOCATED_ENTRY_COUNT;
        let initial_path_region_size = initial_entry_count * PREALLOCATED_PATH_LEN;
        request!(write u32 be initial_entry_count as u32);
        request!(write u32 be initial_path_region_size as u32);

        request!(write repeated 0, initial_path_region_size + initial_entry_count * ENTRY_SIZE);

        Ok(PkgState {
            path_region_size: initial_path_region_size as u32,
            path_region_empty_offset: 0,
            entries: vec![None; initial_entry_count as usize],
            path_to_entry_index_map: HashMap::default(),
        })
    }

    #[generator(static, yield ReadSeekWriteRequest -> Response)]
    pub fn push_back_data_region(&mut self, offset: u64) {
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
            let new_offset = request!(seek SeekFrom::End(0));
            // TODO: Do not panic on conversion to u32
            let old_offset = core::mem::replace(&mut entry.data_offset, new_offset as u32);

            request!(copy old_offset.into(), entry.data_size.into(), new_offset);
            request!(seek SeekFrom::Start(
                PkgState::entry_list_offset() + i as u64 * ENTRY_SIZE,
            ));
            entry.write().await;

            self.entries[i] = Some(entry);
        }
    }

    #[generator(static, yield ReadSeekWriteRequest -> Response)]
    pub fn push_back_and_resize_path_region(&mut self, offset: u64, new_size: u64) {
        log::trace!(target: "silpkg", "Moving path region to {} with a new size of {}", offset, new_size);
        self.push_back_data_region(offset + new_size).await;

        request!(copy self.path_region_offset(), self.path_region_size as u64, offset);

        self.path_region_size = new_size as u32;
        request!(seek SeekFrom::Start(MAGIC.len() as u64 + 8));
        request!(write u32 be self.path_region_size);
    }

    #[generator(static, yield ReadSeekWriteRequest -> Response)]
    pub fn reserve_path_space(&mut self, amount: u32) {
        log::trace!(target: "silpkg", "Resizing path region");
        let new_path_region_size = self.path_region_size + amount;
        let new_path_region_start = self.path_region_offset();
        let new_path_region_end = new_path_region_start as u32 + new_path_region_size;

        self.push_back_data_region(new_path_region_end as u64).await;

        request!(seek SeekFrom::Start(
            new_path_region_start + self.path_region_empty_offset as u64,
        ));

        request!(write repeated 0, (new_path_region_size - self.path_region_empty_offset).into());

        self.path_region_size = new_path_region_size;

        request!(seek SeekFrom::Start(MAGIC.len() as u64 + 8));
        request!(write u32 be self.path_region_size);
    }

    #[generator(static, yield ReadSeekWriteRequest -> Response)]
    pub fn reserve_entries(&mut self, amount: u64) {
        log::trace!(target: "silpkg", "Resizing entry list");
        let required_extra_entry_space = (amount * ENTRY_SIZE) as u32;
        let required_extra_path_space = (amount * PREALLOCATED_PATH_LEN) as u32;

        let entry_list_grow_start =
            PkgState::entry_list_offset() + self.entries.len() as u64 * ENTRY_SIZE;

        let new_path_region_offset = entry_list_grow_start as u32 + required_extra_entry_space;

        self.push_back_and_resize_path_region(
            new_path_region_offset as u64,
            self.path_region_size as u64 + required_extra_path_space as u64,
        )
        .await;

        request!(seek SeekFrom::Start(entry_list_grow_start));
        request!(write repeated 0, required_extra_entry_space.into());

        self.entries.reserve_exact(amount as usize);
        for _ in 0..amount {
            self.entries.push(None);
        }

        request!(seek SeekFrom::Start(MAGIC.len() as u64 + 4));
        request!(write u32 be self.entries.len() as u32);
    }

    #[generator(static, yield ReadSeekWriteRequest -> Response)]
    pub fn insert_path_into_path_region(&mut self, path: &str) -> u32 {
        log::trace!(target: "silpkg",
            "Inserting path {path} at {}/{}",
            self.path_region_empty_offset, self.path_region_size
        );
        if self.path_region_empty_offset + path.len() as u32 + 1 >= self.path_region_size {
            self.reserve_path_space(path.len() as u32 + 1 + PREALLOCATED_PATH_LEN as u32 * 32)
                .await;
        }
        let offset = self.path_region_empty_offset;

        request!(seek SeekFrom::Start(
            self.path_region_offset() + self.path_region_empty_offset as u64,
        ));

        request!(write all path);
        request!(write u8 0);

        self.path_region_empty_offset += path.len() as u32 + 1;

        offset
    }

    #[generator(static, yield ReadSeekWriteRequest -> Response)]
    pub fn remove(&mut self, path: &str) -> Result<(), RemoveError> {
        if let Some(entry_idx) = self.path_to_entry_index_map.remove(path) {
            self.entries[entry_idx] = None;

            request!(seek SeekFrom::Start(
                Self::entry_list_offset() + entry_idx as u64 * ENTRY_SIZE,
            ));
            request!(write all [0x00; ENTRY_SIZE as usize]);

            // TODO: Slipstream does a nice optimisation here and truncates if the data was at the end
            //       but we can't do that until specialisation comes around. (if we want to support non
            //       Truncate writers)

            Ok(())
        } else {
            Err(RemoveError::NotFound)
        }
    }

    #[generator(static, yield ReadSeekWriteRequest -> Response)]
    pub fn rename(&mut self, src: &str, dst: String) -> Result<(), RenameError> {
        if !self.path_to_entry_index_map.contains_key(src) {
            return Err(RenameError::NotFound);
        }

        if self.path_to_entry_index_map.contains_key(&dst) {
            return Err(RenameError::AlreadyExists);
        }

        let entry_idx = self.path_to_entry_index_map.remove(src).unwrap();
        let mut entry = self.entries[entry_idx].as_mut().unwrap();

        debug_assert_eq!(src, entry.path);
        entry.path = dst.clone();

        // If this is true then the previous path was at the end of the path region and we can just
        // extend the path region and overwrite it.
        if entry.relative_path_offset + src.len() as u32 == self.path_region_empty_offset {
            let relative_path_offset = entry.relative_path_offset;
            self.reserve_path_space((dst.len() - src.len()) as u32)
                .await;

            request!(seek SeekFrom::Start(relative_path_offset.into()));
            request!(write all dst.clone().into_bytes());
            entry = self.entries[entry_idx].as_mut().unwrap();
        // If the last path is not at the end the new path has to be inserted at the end and the
        // entry's path offset updated, the previous path will be removed during a repack.
        } else {
            let new_relative_path_offset = self.insert_path_into_path_region(&dst).await;
            entry = self.entries[entry_idx].as_mut().unwrap();
            entry.relative_path_offset = new_relative_path_offset;
        }

        self.path_to_entry_index_map.insert(dst, entry_idx);

        request!(seek SeekFrom::Start(Self::entry_list_offset() + entry_idx as u64 * ENTRY_SIZE));
        entry.write().await;

        Ok(())
    }

    #[generator(static, yield ReadSeekWriteRequest -> Response)]
    pub fn replace(&mut self, src: &str, dst: String) -> Result<(), ReplaceError> {
        let res = (
            self.path_to_entry_index_map.get(src).copied(),
            self.path_to_entry_index_map.get(&dst).copied(),
        );
        match res {
            (Some(one_idx), Some(two_idx)) => {
                let one = self.entries[one_idx].take().unwrap();
                self.path_to_entry_index_map.remove(src);

                let two = self.entries[two_idx].as_mut().unwrap();
                two.data_offset = one.data_offset;
                two.data_size = one.data_size;
                two.unpacked_size = one.unpacked_size;
                two.flags = one.flags;

                request!(seek SeekFrom::Start(Self::entry_list_offset() + one_idx as u64 * ENTRY_SIZE));
                Entry::write_empty().await;

                request!(seek SeekFrom::Start(Self::entry_list_offset() + two_idx as u64 * ENTRY_SIZE));
                two.write().await;

                Ok(())
            }
            (Some(_), None) => {
                self.rename(src, dst).await.map_err(|x| match x {
                    RenameError::NotFound | RenameError::AlreadyExists => unreachable!(),
                    RenameError::Io(err) => ReplaceError::Io(err),
                })?;

                self.path_to_entry_index_map.remove(src);

                Ok(())
            }
            (None, _) => Err(ReplaceError::NotFound),
        }
    }

    #[generator(static, yield ReadSeekWriteRequest -> Response)]
    fn write_packed_path_region_at(&mut self, offset: u64) -> u64 {
        request!(seek SeekFrom::Start(offset));

        let mut size = 0;
        for entry in self.entries.iter_mut().map(|e| e.as_mut().unwrap()) {
            entry.relative_path_offset = (request!(stream pos) - offset) as u32;
            // FIXME: borrow path
            request!(write all entry.path.clone());
            request!(write u8 0);
            size += entry.path.as_bytes().len() + 1;
        }

        size as u64
    }

    #[generator(static, yield ReadSeekWriteTruncateRequest -> Response)]
    pub fn repack(&mut self) -> Result<(), RepackError> {
        // Remove empty entries
        for entry in core::mem::take(&mut self.entries) {
            if entry.is_some() {
                self.entries.push(entry);
            }
        }

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
                    return Err(RepackError::OverlappingEntries);
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
        self.path_region_size = self.write_packed_path_region_at(path_region_offset).await as u32;
        assert_eq!(self.path_region_size, path_region_size as u32);

        // Defragment? the data region

        let mut current_data_offset = data_region_start as u32;
        // TODO: The unwraps are getting really annoying and possibly degrading performance
        //       Maybe something should be done about this? (I would love to avoid unwrap unchecked too)
        log::trace!(target: "silpkg", "Defragmenting data region");
        for entry in self.entries.iter_mut().map(|e| e.as_mut().unwrap()) {
            if current_data_offset != entry.data_offset {
                request!(copy entry.data_offset.into(), entry.data_size.into(), current_data_offset.into());

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
        request!(seek SeekFrom::Start(MAGIC.len() as u64 + 4));
        request!(write u32 be self.entries.len() as u32);
        request!(write u32 be path_region_size as u32);

        for maybe_entry in self.entries.iter() {
            match maybe_entry {
                Some(entry) => entry.write().await,
                None => request!(write repeated 0, ENTRY_SIZE),
            }
        }

        request!(truncate current_data_offset.into());

        Ok(())
    }

    #[generator(static, yield ReadSeekWriteRequest -> Response, lifetime 'coro)]
    pub fn insert<'coro>(
        &mut self,
        path: String,
        flags: Flags,
    ) -> Result<WriteHandle<'coro>, InsertError> {
        if self.path_to_entry_index_map.contains_key(&path) {
            return Err(InsertError::AlreadyExists);
        }

        let entry_slot = match self.entries.iter().enumerate().find(|(_i, o)| o.is_none()) {
            Some((i, _o)) => i,
            None => {
                let i = self.entries.len();
                self.reserve_entries(PREALLOCATED_ENTRY_COUNT).await;
                i
            }
        };

        assert!(self
            .path_to_entry_index_map
            .insert(path.clone(), entry_slot)
            .is_none());

        let relative_path_offset = self.insert_path_into_path_region(&path).await;
        let data_offset = request!(seek SeekFrom::End(0));

        Ok(WriteHandle {
            inner: match flags.compression {
                EntryCompression::Deflate(level) => DataWriteHandle::Deflate(DeflateWriteHandle {
                    offset: data_offset,
                    size: 0,
                    unpacked_size: 0,
                    compress: Compress::new(level, true),
                }),
                EntryCompression::None => DataWriteHandle::Raw(RawReadWriteHandle {
                    cursor: 0,
                    offset: data_offset,
                    size: 0,
                }),
            },

            state: self,
            path,
            relative_path_offset,
            entry_slot,
            flags,
        })
    }
}

pub trait GeneratorWrite {
    #[generator(static, yield ReadSeekWriteRequest -> Response)]
    fn write(&mut self, buf: &[u8]) -> usize;
}

pub struct DeflateWriteHandle {
    // Used during data IO
    offset: u64,
    size: u64,
    unpacked_size: u64,
    compress: flate2::Compress,
}

pub enum DataWriteHandle {
    Raw(RawReadWriteHandle),
    Deflate(DeflateWriteHandle),
}

pub struct WriteHandle<'a> {
    inner: DataWriteHandle,

    // Used during flush
    state: &'a mut PkgState,
    path: String,
    relative_path_offset: u32,
    entry_slot: usize,
    flags: Flags,
}

impl<'a, 'b: 'a> WriteHandle<'b> {
    pub fn inner_mut(&mut self) -> &mut DataWriteHandle {
        &mut self.inner
    }

    // FIXME: The PhantomData is a workaround for, possibly, a rustc bug.
    #[generator(static, yield ReadSeekWriteRequest -> Response, lifetime 'a)]
    fn flush_internal(&mut self) -> PhantomData<&'b ()> {
        match &mut self.inner {
            DataWriteHandle::Deflate(deflate) => deflate.flush().await,
            _ => (),
        }

        log::trace!("Updating entry {} with written data", self.entry_slot);

        let entry = match self.inner {
            DataWriteHandle::Raw(RawReadWriteHandle {
                offset,
                size: unpacked_size @ size,
                ..
            })
            | DataWriteHandle::Deflate(DeflateWriteHandle {
                offset,
                size,
                unpacked_size,
                ..
            }) => Entry {
                data_offset: offset as u32,
                data_size: size as u32,
                unpacked_size: unpacked_size as u32,
                path_hash: pkg_path_hash(&self.path),
                relative_path_offset: self.relative_path_offset,
                path: self.path.clone(),
                flags: match self.flags.compression {
                    EntryCompression::Deflate(_) => RawFlags::DEFLATED,
                    EntryCompression::None => RawFlags::empty(),
                },
            },
        };

        request!(seek SeekFrom::Start(
            PkgState::entry_list_offset() + self.entry_slot as u64 * ENTRY_SIZE,
        ));

        entry.write().await;
        self.state.entries[self.entry_slot] = Some(entry);

        Default::default()
    }

    #[generator(static, yield ReadSeekWriteRequest -> Response, lifetime 'a)]
    pub fn flush(&mut self) -> PhantomData<&'b ()> {
        let (offset, cursor) = match self.inner {
            DataWriteHandle::Raw(RawReadWriteHandle { cursor, offset, .. })
            | DataWriteHandle::Deflate(DeflateWriteHandle {
                size: cursor,
                offset,
                ..
            }) => (offset, cursor),
        };

        self.flush_internal().await;
        request!(seek SeekFrom::Start(offset + cursor));

        Default::default()
    }

    #[generator(static, yield ReadSeekWriteRequest -> Response, lifetime 'a)]
    pub fn finish(mut self) {
        self.flush_internal().await;
    }
}

impl GeneratorWrite for RawReadWriteHandle {
    #[generator(static, yield ReadSeekWriteRequest -> Response)]
    fn write(&mut self, buf: &[u8]) -> usize {
        log::trace!("Writing entry data to {}", self.offset);

        let written = request!(write buf);
        self.size += written as u64;
        self.cursor += written as u64;

        written
    }
}

impl GeneratorWrite for DeflateWriteHandle {
    #[generator(static, yield ReadSeekWriteRequest -> Response)]
    fn write(&mut self, mut buf: &[u8]) -> usize {
        log::trace!("Writing compressed entry data at {}", self.offset);

        let mut output = 0;
        let mut written = 0;

        let mut out = Vec::with_capacity(BUFFER_SIZE as usize);
        loop {
            let prev_in = self.compress.total_in();
            let prev_out = self.compress.total_out();

            // log::trace!("compressing buffer of size {}", buf.len());
            let status = self
                .compress
                .compress_vec(buf, &mut out, flate2::FlushCompress::None)
                .unwrap();

            // log::trace!(
            //     "writing compressed chunk of size {} {} -> {}",
            //     out.len(),
            //     self.compress.total_in() - prev_in,
            //     self.compress.total_out() - prev_out
            // );

            // FIXME: don't
            request!(write all out.clone());
            out.clear();

            let output_now = self.compress.total_out() - prev_out;
            let written_now = (self.compress.total_in() - prev_in) as usize;

            output += output_now;
            written += written_now;
            buf = &buf[written_now..];

            match status {
                flate2::Status::Ok if buf.is_empty() => break,
                flate2::Status::Ok | flate2::Status::BufError => {}
                flate2::Status::StreamEnd => unreachable!(),
            };
        }

        self.size += output;
        self.unpacked_size += written as u64;

        written
    }
}

impl DeflateWriteHandle {
    #[generator(static, yield ReadSeekWriteRequest -> Response)]
    pub fn flush(&mut self) {
        let mut out = Vec::with_capacity(BUFFER_SIZE as usize);

        loop {
            self.compress
                .compress_vec(&[], &mut out, flate2::FlushCompress::Finish)
                .unwrap();

            if out.is_empty() {
                break;
            } else {
                // FIXME: don't
                request!(write all out.clone());
                self.size += out.len() as u64;
                out.clear();
            }
        }
    }
}

impl GeneratorWrite for WriteHandle<'_> {
    #[generator(static, yield ReadSeekWriteRequest -> Response)]
    fn write(&mut self, buf: &[u8]) -> usize {
        match &mut self.inner {
            DataWriteHandle::Raw(h) => h.write(buf).await,
            DataWriteHandle::Deflate(h) => h.write(buf).await,
        }
    }
}

impl WriteHandle<'_> {
    pub fn is_compressed(&self) -> bool {
        match self.inner {
            DataWriteHandle::Raw(_) => false,
            DataWriteHandle::Deflate(_) => true,
        }
    }

    pub fn is_seekable(&self) -> bool {
        !self.is_compressed()
    }
}
