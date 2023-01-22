use std::{
    io::{Read, Seek, Write},
    mem::ManuallyDrop,
    ops::Generator,
    pin::Pin,
};

use crate::{
    base::{
        self, Flags, InnerInsertHandle, PkgState, ReadSeekRequest, ReadSeekWriteRequest,
        ReadSeekWriteTruncateRequest, Response, Truncate, WriteRequest,
    },
    errors::ParseError,
    util::{ReadSeekWriteExt, WriteExt},
    CreateError, ExtractError, InsertError, RenameError, RepackError, ReplaceError,
};

struct SyncDriver<S> {
    storage: S,
}

impl<S: Read + Seek> SyncDriver<S> {
    pub fn new(storage: S) -> Self {
        Self { storage }
    }

    pub fn get_mut(&mut self) -> &mut S {
        &mut self.storage
    }

    fn handle_readseek(&mut self, request: ReadSeekRequest) -> std::io::Result<Response> {
        Ok(match request {
            ReadSeekRequest::Read(count) => {
                let mut buf = vec![0; count as usize];
                let read = self.storage.read(&mut buf)?;
                buf.truncate(read);
                Response::Read(buf)
            }
            ReadSeekRequest::ReadExact(count) => {
                let mut buf = vec![0; count as usize];
                self.storage.read_exact(&mut buf)?;
                Response::Read(buf)
            }
            ReadSeekRequest::Seek(offset) => Response::Seek(self.storage.seek(offset)?),
        })
    }

    pub fn drive_read<R>(
        &mut self,
        mut coroutine: impl Generator<Response, Return = R, Yield = ReadSeekRequest>,
    ) -> std::io::Result<R> {
        let mut response = Response::None;

        loop {
            use std::ops::GeneratorState;

            match unsafe { Pin::new_unchecked(&mut coroutine) }.resume(response) {
                GeneratorState::Yielded(request) => response = self.handle_readseek(request)?,
                GeneratorState::Complete(result) => break Ok(result),
            }
        }
    }
}

impl<S: Read + Seek + Write> SyncDriver<S> {
    fn handle_write(&mut self, request: WriteRequest) -> std::io::Result<Response> {
        Ok(match request {
            WriteRequest::WriteAll(buffer) => {
                self.storage.write_all(&buffer)?;
                Response::None
            }
            WriteRequest::Write(buffer) => Response::Written(self.storage.write(&buffer)?),
            WriteRequest::Copy { from, count, to } => {
                self.storage.copy_within(from, count, to)?;
                Response::None
            }
            WriteRequest::WriteRepeated { value, count } => {
                self.storage.fill(value, count)?;
                Response::None
            }
        })
    }

    pub fn drive_write<R>(
        &mut self,
        mut coroutine: impl Generator<Response, Return = R, Yield = ReadSeekWriteRequest>,
    ) -> std::io::Result<R> {
        let mut response = Response::None;

        loop {
            use std::ops::GeneratorState;

            match unsafe { Pin::new_unchecked(&mut coroutine) }.resume(response) {
                GeneratorState::Yielded(ReadSeekWriteRequest::ReadSeek(request)) => {
                    response = self.handle_readseek(request)?
                }
                GeneratorState::Yielded(ReadSeekWriteRequest::Write(request)) => {
                    response = self.handle_write(request)?
                }
                GeneratorState::Complete(result) => break Ok(result),
            }
        }
    }
}

impl<S: Read + Seek + Write + Truncate> SyncDriver<S> {
    pub fn drive_truncate<R>(
        &mut self,
        mut coroutine: impl Generator<Response, Return = R, Yield = ReadSeekWriteTruncateRequest>,
    ) -> std::io::Result<R> {
        let mut response = Response::None;

        loop {
            use std::ops::GeneratorState;

            match unsafe { Pin::new_unchecked(&mut coroutine) }.resume(response) {
                GeneratorState::Yielded(ReadSeekWriteTruncateRequest::ReadSeek(request)) => {
                    response = self.handle_readseek(request)?
                }
                GeneratorState::Yielded(ReadSeekWriteTruncateRequest::Write(request)) => {
                    response = self.handle_write(request)?
                }
                GeneratorState::Yielded(ReadSeekWriteTruncateRequest::Truncate(size)) => {
                    self.storage.truncate(size)?;
                    response = Response::None;
                }
                GeneratorState::Complete(result) => break Ok(result),
            }
        }
    }
}

pub struct Pkg<S: Read + Seek> {
    driver: SyncDriver<S>,
    state: PkgState,
}

pub struct EntryReader<'a, S: Read + Seek> {
    driver: &'a mut SyncDriver<S>,
    handle: base::ExtractHandle,
}

impl<'a, S: Read + Seek> Read for EntryReader<'a, S> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match &mut self.handle {
            base::ExtractHandle::Raw(handle) => self.driver.drive_read(handle.read(buf)),
            base::ExtractHandle::Deflate(handle) => self.driver.drive_read(handle.read(buf)),
        }
    }
}

impl<'a, S: Read + Seek> Seek for EntryReader<'a, S> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        match &mut self.handle {
            base::ExtractHandle::Raw(handle) => self.driver.drive_read(handle.seek(pos))?,
            // FIXME: Should this really work like this?
            base::ExtractHandle::Deflate(_) => Err(std::io::Error::new(
                #[cfg(feature = "io_error_more")]
                std::io::ErrorKind::NotSeekable,
                #[cfg(not(feature = "io_error_more"))]
                std::io::ErrorKind::Other,
                "Cannot seek on compressed entry reader",
            )),
        }
    }
}

impl<S: Read + Seek> Pkg<S> {
    pub fn contains(&self, path: &str) -> bool {
        self.state.contains(path)
    }

    pub fn paths(&self) -> impl Iterator<Item = &String> {
        self.state.paths()
    }

    pub fn parse(storage: S, expect_magic: bool) -> Result<Self, ParseError> {
        let mut driver = SyncDriver::new(storage);
        let state = driver.drive_read(base::parse(expect_magic))??;

        Ok(Self { driver, state })
    }

    pub fn open(&mut self, path: &str) -> Result<EntryReader<S>, ExtractError> {
        let handle = self.driver.drive_read(base::open(&self.state, path))??;

        Ok(EntryReader {
            driver: &mut self.driver,
            handle,
        })
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

pub struct EntryWriter<'a, S: Read + Seek + Write> {
    driver: &'a mut SyncDriver<S>,
    handle: ManuallyDrop<base::InsertHandle<'a>>,
}

impl<'a, S: Read + Seek + Write> Write for EntryWriter<'a, S> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self.handle.inner_mut() {
            InnerInsertHandle::Raw(handle) => self.driver.drive_write(handle.write(buf))?,
            InnerInsertHandle::Deflate(handle) => self.driver.drive_write(handle.write(buf))?,
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.driver.drive_write(self.handle.flush())?;
        self.driver.get_mut().flush()
    }
}

impl<'a, S: Read + Seek + Write> Read for EntryWriter<'a, S> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.handle.inner_mut() {
            InnerInsertHandle::Raw(handle) => self.driver.drive_read(handle.read(buf)),
            InnerInsertHandle::Deflate(_) => Err(std::io::Error::new(
                #[cfg(feature = "io_error_more")]
                std::io::ErrorKind::NotSeekable,
                #[cfg(not(feature = "io_error_more"))]
                std::io::ErrorKind::Other,
                "Cannot read on compressed entry writer",
            )),
        }
    }
}

impl<'a, S: Read + Seek + Write> Seek for EntryWriter<'a, S> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        match self.handle.inner_mut() {
            InnerInsertHandle::Raw(handle) => self.driver.drive_read(handle.seek(pos))?,
            InnerInsertHandle::Deflate(_) => Err(std::io::Error::new(
                #[cfg(feature = "io_error_more")]
                std::io::ErrorKind::NotSeekable,
                #[cfg(not(feature = "io_error_more"))]
                std::io::ErrorKind::Other,
                "Cannot seek on compressed entry writer",
            )),
        }
    }
}

impl<'a, S: Read + Seek + Write> Drop for EntryWriter<'a, S> {
    fn drop(&mut self) {
        let handle = unsafe { ManuallyDrop::take(&mut self.handle) };
        // TODO: Mention this ignoring errors in the description of EntryWriter
        _ = self.driver.drive_write(handle.finish());
    }
}

impl<S: Read + Seek + Write> Pkg<S> {
    pub fn create(storage: S) -> Result<Self, CreateError> {
        let mut driver = SyncDriver::new(storage);
        let state = driver.drive_write(PkgState::create())??;

        Ok(Self { driver, state })
    }

    pub fn remove(&mut self, path: &str) -> std::io::Result<bool> {
        self.driver.drive_write(self.state.remove(path))
    }

    pub fn rename(&mut self, src: &str, dst: String) -> Result<(), RenameError> {
        self.driver.drive_write(self.state.rename(src, dst))?
    }

    pub fn replace(&mut self, src: &str, dst: String) -> Result<(), ReplaceError> {
        self.driver.drive_write(self.state.replace(src, dst))?
    }

    pub fn insert(&mut self, path: String, flags: Flags) -> Result<EntryWriter<S>, InsertError> {
        let handle = self.driver.drive_write(self.state.insert(path, flags))??;

        Ok(EntryWriter {
            driver: &mut self.driver,
            handle: ManuallyDrop::new(handle),
        })
    }

    pub fn flush(&mut self) -> std::io::Result<()> {
        self.driver.get_mut().flush()
    }
}

/// These functions require the [`Truncate`] trait to be implemented because they may shrink the
/// archive.
///
/// [`File`]: std::fs::File
impl<S: Read + Seek + Write + Truncate> Pkg<S> {
    /// Packs the archive to be the smallest possible size at the price of easy expansion.
    ///
    /// This function is pretty expensive and also makes proceeding [`insert`]s slower.
    ///
    /// [`insert`]: Pkg::insert
    pub fn repack(&mut self) -> Result<(), RepackError> {
        self.driver.drive_truncate(self.state.repack())?
    }
} // Read + Seek + Write + Truncate
