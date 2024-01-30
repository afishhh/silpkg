use std::{
    io::{Read, Seek, Write},
    mem::ManuallyDrop,
    ops::Coroutine,
    pin::Pin,
};

use base::FlattenResult;

use crate::{
    base::{
        self, DataWriteHandle, Flags, GeneratorRead, GeneratorSeek, GeneratorWrite, PkgState,
        ReadSeekRequest, ReadSeekWriteRequest, ReadSeekWriteTruncateRequest, Response,
        WriteRequest,
    },
    errors,
    util::{ReadSeekWriteExt, WriteExt},
};

pub type CreateError = errors::CreateError<std::io::Error>;
pub type ParseError = errors::ParseError<std::io::Error>;
pub type InsertError = errors::InsertError<std::io::Error>;
pub type OpenError = errors::OpenError<std::io::Error>;
pub type RemoveError = errors::RemoveError<std::io::Error>;
pub type RenameError = errors::RenameError<std::io::Error>;
pub type RepackError = errors::RepackError<std::io::Error>;
pub type ReplaceError = errors::ReplaceError<std::io::Error>;

/// A trait for objects that can be truncated.
pub trait Truncate {
    /// Truncates this object to the given length.
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
            ReadSeekRequest::Seek(offset) => Response::Seek(self.storage.seek(offset.into())?),
        })
    }

    pub fn drive_read<R>(
        &mut self,
        mut coroutine: impl Coroutine<Response, Return = R, Yield = ReadSeekRequest>,
    ) -> std::io::Result<R> {
        let mut response = Response::None;

        loop {
            use std::ops::CoroutineState;

            match unsafe { Pin::new_unchecked(&mut coroutine) }.resume(response) {
                CoroutineState::Yielded(request) => response = self.handle_readseek(request)?,
                CoroutineState::Complete(result) => break Ok(result),
            }
        }
    }
}

impl<S: Read + Seek + Write> SyncDriver<S> {
    fn handle_write(&mut self, request: WriteRequest) -> std::io::Result<Response> {
        Ok(match request {
            WriteRequest::WriteAll(ptr, count) => {
                self.storage
                    .write_all(unsafe { core::slice::from_raw_parts(ptr, count) })?;
                Response::None
            }
            WriteRequest::Write(ptr, count) => Response::Written(
                self.storage
                    .write(unsafe { core::slice::from_raw_parts(ptr, count) })?,
            ),
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
        mut coroutine: impl Coroutine<Response, Return = R, Yield = ReadSeekWriteRequest>,
    ) -> std::io::Result<R> {
        let mut response = Response::None;

        loop {
            use std::ops::CoroutineState;

            match unsafe { Pin::new_unchecked(&mut coroutine) }.resume(response) {
                CoroutineState::Yielded(ReadSeekWriteRequest::ReadSeek(request)) => {
                    response = self.handle_readseek(request)?
                }
                CoroutineState::Yielded(ReadSeekWriteRequest::Write(request)) => {
                    response = self.handle_write(request)?
                }
                CoroutineState::Complete(result) => break Ok(result),
            }
        }
    }
}

impl<S: Read + Seek + Write + Truncate> SyncDriver<S> {
    pub fn drive_truncate<R>(
        &mut self,
        mut coroutine: impl Coroutine<Response, Return = R, Yield = ReadSeekWriteTruncateRequest>,
    ) -> std::io::Result<R> {
        let mut response = Response::None;

        loop {
            use std::ops::CoroutineState;

            match unsafe { Pin::new_unchecked(&mut coroutine) }.resume(response) {
                CoroutineState::Yielded(ReadSeekWriteTruncateRequest::ReadSeek(request)) => {
                    response = self.handle_readseek(request)?
                }
                CoroutineState::Yielded(ReadSeekWriteTruncateRequest::Write(request)) => {
                    response = self.handle_write(request)?
                }
                CoroutineState::Yielded(ReadSeekWriteTruncateRequest::Truncate(size)) => {
                    self.storage.truncate(size)?;
                    response = Response::None;
                }
                CoroutineState::Complete(result) => break Ok(result),
            }
        }
    }
}

/// A synchronous PKG archive reader/writer.
pub struct Pkg<S: Read + Seek> {
    driver: SyncDriver<S>,
    state: PkgState,
}

/// A reader that allows reading a single entry from a [`Pkg`]
pub struct EntryReader<'a, S: Read + Seek> {
    driver: &'a mut SyncDriver<S>,
    handle: base::ReadHandle,
}

impl<'a, S: Read + Seek> Read for EntryReader<'a, S> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.driver.drive_read(self.handle.read(buf))
    }
}

/// # Notes
/// Even though this type implements [`Seek`] [`seek`]ing will not always succeed, for example if the entry
/// happens to be compressed then [`seek`]ing will fail with [`NotSeekable`] if the `io_error_more`
/// feature is enabled or [`Other`] otherwise.
///
/// [`seek`]: EntryReader::seek
/// [`NotSeekable`]: std::io::ErrorKind::NotSeekable
/// [`Other`]: std::io::ErrorKind::Other
impl<'a, S: Read + Seek> Seek for EntryReader<'a, S> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        match &mut self.handle {
            base::ReadHandle::Raw(handle) => {
                Ok(self.driver.drive_read(handle.seek(pos.into())).flatten()?)
            }
            // FIXME: Should this really work like this?
            base::ReadHandle::Deflate(_) => Err(std::io::Error::new(
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
    /// Returns a reference to the underlying reader
    pub fn inner(&self) -> &S {
        &self.driver.storage
    }

    /// Checks whether the archive contains `path`.
    pub fn contains(&self, path: &str) -> bool {
        self.state.contains(path)
    }

    /// Returns an iterator over all the paths in the archive.
    pub fn paths(&self) -> impl Iterator<Item = &String> {
        self.state.paths()
    }

    /// Parses a [`Pkg`] from the supplied reader.
    pub fn parse(storage: S) -> Result<Self, ParseError> {
        let mut driver = SyncDriver::new(storage);
        let state = driver.drive_read(base::parse(true)).flatten()?;

        Ok(Self { driver, state })
    }

    /// Opens an entry for reading.
    pub fn open(&mut self, path: &str) -> Result<EntryReader<S>, OpenError> {
        let handle = self
            .driver
            .drive_read(base::open(&self.state, path))
            .flatten()?;

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

/// A writer that allows writing a single entry into a [`Pkg`].
pub struct EntryWriter<'a, S: Read + Seek + Write> {
    driver: &'a mut SyncDriver<S>,
    handle: ManuallyDrop<base::WriteHandle<'a>>,
}

impl<'a, S: Read + Seek + Write> EntryWriter<'a, S> {
    /// Writes entry metadata to the underlying writer.
    pub fn finish(mut self) -> std::io::Result<()> {
        let handle = unsafe { ManuallyDrop::take(&mut self.handle) };
        self.driver.drive_write(handle.finish())?;
        self.driver.get_mut().flush()?;
        std::mem::forget(self);

        Ok(())
    }
}

impl<'a, S: Read + Seek + Write> Write for EntryWriter<'a, S> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self.handle.inner_mut() {
            DataWriteHandle::Raw(handle) => Ok(self.driver.drive_write(handle.write(buf))?),
            DataWriteHandle::Deflate(handle) => Ok(self.driver.drive_write(handle.write(buf))?),
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
            DataWriteHandle::Raw(handle) => self.driver.drive_read(handle.read(buf)),
            DataWriteHandle::Deflate(_) => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Cannot read on compressed entry writer",
            )),
        }
    }
}

/// # Notes
/// Even though this type implements [`Seek`] [`seek`]ing will not always succeed, for example if the entry
/// happens to be compressed then [`seek`]ing will fail with [`NotSeekable`] if the `io_error_more`
/// feature is enabled or [`Other`] otherwise.
///
/// [`seek`]: EntryReader::seek
/// [`NotSeekable`]: std::io::ErrorKind::NotSeekable
/// [`Other`]: std::io::ErrorKind::Other
impl<'a, S: Read + Seek + Write> Seek for EntryWriter<'a, S> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        match self.handle.inner_mut() {
            DataWriteHandle::Raw(handle) => {
                Ok(self.driver.drive_read(handle.seek(pos.into())).flatten()?)
            }
            DataWriteHandle::Deflate(_) => Err(std::io::Error::new(
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
    /// Writes entry metadata to the underlying writer.
    ///
    /// # Errors
    /// This function will ignore IO errors!
    /// If you need to handle them use [`finish`](Self::finish).
    fn drop(&mut self) {
        let handle = unsafe { ManuallyDrop::take(&mut self.handle) };
        _ = self.driver.drive_write(handle.finish());
    }
}

impl<S: Read + Seek + Write> Pkg<S> {
    /// Create a new archive in `storage`.
    ///
    /// # Notes
    /// This function does not truncate the writer to allow for use with non-[`Truncate`]
    /// writers, this means that if the writer already contains data it may still remain there until
    /// it's overwritten by inserted data or the archive is [`repack`](Self::repack)ed.
    ///
    /// # Errors
    /// - [`CreateError::Io`] if an IO error occurs.
    pub fn create(storage: S) -> Result<Self, CreateError> {
        let mut driver = SyncDriver::new(storage);
        let state = driver.drive_write(PkgState::create()).flatten()?;

        Ok(Self { driver, state })
    }

    /// Removes an entry from the archive.
    pub fn remove(&mut self, path: &str) -> Result<(), RemoveError> {
        self.driver.drive_write(self.state.remove(path)).flatten()
    }

    /// Renames `src` to `dst`.
    ///
    /// # Errors
    /// - [`RenameError::NotFound`] if `src` does not exist.
    /// - [`RenameError::AlreadyExists`] if `dst` already exists.
    /// - [`RenameError::Io`] if an IO error occurs.
    pub fn rename(&mut self, src: &str, dst: String) -> Result<(), RenameError> {
        self.driver
            .drive_write(self.state.rename(src, dst))
            .flatten()
    }

    /// Replaces `dst` with `src` if it doesn't exist or renames `src` to `dst` otherwise.
    ///
    /// Unlike [`rename`](Self::rename) this function will not fail if `dst` already exists.
    pub fn replace(&mut self, src: &str, dst: String) -> Result<(), ReplaceError> {
        self.driver
            .drive_write(self.state.replace(src, dst))
            .flatten()
    }

    /// Inserts a new entry into the archive.
    ///
    /// # Examples
    /// ```
    /// # use std::io::{Read, Write};
    /// # use silpkg::{EntryCompression, Compression, Flags, sync::*};
    /// let mut storage = Vec::new();
    ///
    /// {
    ///     let mut pkg = Pkg::create(std::io::Cursor::new(&mut storage))?;
    ///     pkg.insert("hello".to_string(), Flags {
    ///         compression: EntryCompression::Deflate(Compression::new(5))
    ///     })?.write_all(b"A quick brown fox jumps over the lazy dog.")?;
    /// }
    ///
    /// {
    ///     let mut pkg = Pkg::parse(std::io::Cursor::new(&mut storage))?;
    ///     let mut buf = String::new();
    ///     pkg.open("hello")?.read_to_string(&mut buf)?;
    ///     assert_eq!(buf, "A quick brown fox jumps over the lazy dog.");
    /// }
    ///
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn insert(&mut self, path: String, flags: Flags) -> Result<EntryWriter<S>, InsertError> {
        let handle = self
            .driver
            .drive_write(self.state.insert(path, flags))
            .flatten()?;

        Ok(EntryWriter {
            driver: &mut self.driver,
            handle: ManuallyDrop::new(handle),
        })
    }

    /// Flushes the underlying writer
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
        self.driver.drive_truncate(self.state.repack()).flatten()
    }
} // Read + Seek + Write + Truncate
