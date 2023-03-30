use core::{convert::Infallible, error::Error};

use alloc::string::String;
use thiserror::Error;

use crate::base::{ENTRY_SIZE, HEADER_SIZE};

/// An error triggered while parsing an existing archive.
#[derive(Debug, Error)]
pub enum ParseError<Io: Error = Infallible> {
    #[error("File does not start the correct magic number")]
    /// The input did not start with the correct magic number.
    MismatchedMagic,

    #[error("File uses unsupported header size {size} (expected {HEADER_SIZE})")]
    /// The input archive indicated an unsupported header size.
    MismatchedHeaderSize {
        /// The header size provided by the input archive
        size: u16,
    },
    #[error("File uses unsupported entry size {size} (expected {ENTRY_SIZE})")]
    /// The input archive indicated an unsupported entry size.
    MismatchedEntrySize {
        /// The entry size provided by the input archive
        size: u16,
    },
    #[error("File claims header section extends beyond EOF")]
    /// The input archive indicated its header section extends beyond EOF.
    EntryOverflow,
    #[error("File claims path region extends beyond EOF")]
    /// The input archive indicated its path region extends beyond EOF.
    PathOverflow,

    #[error("Entry contains unrecognised entry flags {0:#04X}")]
    /// The input archive contained unrecognised entry flags.
    UnrecognisedEntryFlags(u32),

    #[error("Entry has a non-ascii path")]
    /// The input archive contained a non-ascii path.
    NonAsciiPath,
    #[error("Archive contains two entries with the same path {0}")]
    /// The input archive contained two entries with the same path.
    SamePath(String),

    #[cfg(feature = "std")]
    #[error(transparent)]
    /// An IO error occurred.
    Io(#[from] Io),
}

/// An error triggered while creating a new archive.
#[derive(Debug, Error)]
pub enum CreateError<Io: Error = Infallible> {
    #[cfg(feature = "std")]
    #[error(transparent)]
    /// An IO error occurred.
    Io(#[from] Io),
}

/// An error triggered while removing an entry.
#[derive(Debug, Error)]
pub enum RemoveError<Io: Error = Infallible> {
    #[error("Entry does not exist")]
    /// The target entry was not found.
    NotFound,

    #[cfg(feature = "std")]
    #[error(transparent)]
    /// An IO error occurred.
    Io(#[from] Io),
}

/// An error triggered while renaming an entry.
#[derive(Debug, Error)]
pub enum RenameError<Io: Error = Infallible> {
    #[error("Source entry does not exist")]
    /// The source entry was not found.
    NotFound,
    #[error("Desination entry already exists")]
    /// An entry with the destination path was already present.
    AlreadyExists,

    #[cfg(feature = "std")]
    #[error(transparent)]
    /// An IO error occurred.
    Io(#[from] Io),
}

/// An error triggered while replacing one entry with another.
#[derive(Debug, Error)]
pub enum ReplaceError<Io: Error = Infallible> {
    #[error("Entry does not exist")]
    /// The source entry was not found.
    NotFound,

    #[cfg(feature = "std")]
    #[error(transparent)]
    /// An IO error occurred.
    Io(#[from] Io),
}

/// An error triggered while inserting a new entry into an archive.
#[derive(Debug, Error)]
pub enum InsertError<Io: Error = Infallible> {
    #[error("An entry with the same path already exists")]
    /// An entry with that name already existed.
    AlreadyExists,

    #[cfg(feature = "std")]
    #[error(transparent)]
    /// An IO error occurred.
    Io(#[from] Io),
}

/// An error triggered while opening an entry for reading.
#[derive(Debug, Error)]
pub enum OpenError<Io: Error = Infallible> {
    #[error("Entry does not exist")]
    /// An entry with that name was not found.
    NotFound,

    #[cfg(feature = "std")]
    #[error(transparent)]
    /// An IO error occurred.
    Io(#[from] Io),
}

/// An error triggered while repacking.
#[derive(Debug, Error)]
pub enum RepackError<Io: Error = Infallible> {
    #[error("Repacking PKGs with overlapping entries is not supported (yet)")]
    /// The archive contained overlapping entries.
    ///
    /// This cannot be triggered by creating your own archive and can only happen if you parse an
    /// archive that contains such overlapping entries and try to repack it.
    OverlappingEntries,

    #[cfg(feature = "std")]
    #[error(transparent)]
    /// An IO error occurred.
    Io(#[from] Io),
}

#[cfg(feature = "std")]
impl From<CreateError<std::io::Error>> for std::io::Error {
    fn from(val: CreateError<std::io::Error>) -> Self {
        match val {
            CreateError::Io(err) => err,
        }
    }
}

#[cfg(feature = "std")]
impl From<RemoveError<std::io::Error>> for std::io::Error {
    fn from(value: RemoveError<std::io::Error>) -> Self {
        match value {
            RemoveError::NotFound => {
                std::io::Error::new(std::io::ErrorKind::NotFound, value.to_string())
            }
            RemoveError::Io(err) => err,
        }
    }
}

#[cfg(feature = "std")]
impl From<RenameError<std::io::Error>> for std::io::Error {
    fn from(val: RenameError<std::io::Error>) -> Self {
        match val {
            RenameError::NotFound => {
                std::io::Error::new(std::io::ErrorKind::NotFound, val.to_string())
            }
            RenameError::AlreadyExists => {
                std::io::Error::new(std::io::ErrorKind::AlreadyExists, val.to_string())
            }
            RenameError::Io(err) => err,
        }
    }
}

#[cfg(feature = "std")]
impl From<InsertError<std::io::Error>> for std::io::Error {
    fn from(val: InsertError<std::io::Error>) -> Self {
        match val {
            InsertError::AlreadyExists => {
                std::io::Error::new(std::io::ErrorKind::AlreadyExists, val.to_string())
            }
            InsertError::Io(err) => err,
        }
    }
}

#[cfg(feature = "std")]
impl From<OpenError<std::io::Error>> for std::io::Error {
    fn from(val: OpenError<std::io::Error>) -> Self {
        match val {
            OpenError::NotFound => {
                std::io::Error::new(std::io::ErrorKind::NotFound, val.to_string())
            }
            OpenError::Io(err) => err,
        }
    }
}

// PERF FIXME: This is a hacky solution, and probably does not optimise very well!!
pub(crate) trait FlattenResult<T, E>: Sized {
    fn flatten(self) -> Result<T, E>;
}

impl<T, E: Error> FlattenResult<T, ParseError<E>> for Result<Result<T, ParseError<Infallible>>, E> {
    fn flatten(self) -> Result<T, ParseError<E>> {
        match self {
            Ok(o) => match o {
                Ok(o) => Ok(o),
                Err(e) => Err(match e {
                    ParseError::MismatchedMagic => ParseError::MismatchedMagic,
                    ParseError::MismatchedHeaderSize { size } => {
                        ParseError::MismatchedHeaderSize { size }
                    }
                    ParseError::MismatchedEntrySize { size } => {
                        ParseError::MismatchedEntrySize { size }
                    }
                    ParseError::EntryOverflow => ParseError::EntryOverflow,
                    ParseError::PathOverflow => ParseError::PathOverflow,
                    ParseError::UnrecognisedEntryFlags(flags) => {
                        ParseError::UnrecognisedEntryFlags(flags)
                    }
                    ParseError::NonAsciiPath => ParseError::NonAsciiPath,
                    ParseError::SamePath(path) => ParseError::SamePath(path),
                    ParseError::Io(_) => unreachable!(),
                }),
            },
            Err(e) => Err(ParseError::Io(e)),
        }
    }
}

impl<T, E: Error> FlattenResult<T, CreateError<E>>
    for Result<Result<T, CreateError<Infallible>>, E>
{
    fn flatten(self) -> Result<T, CreateError<E>> {
        match self {
            Ok(o) => match o {
                Ok(o) => Ok(o),
                #[allow(unreachable_code)] // This is a lot cleaner as a match
                Err(e) => Err(match e {
                    CreateError::Io(_) => unreachable!(),
                }),
            },
            Err(e) => Err(CreateError::Io(e)),
        }
    }
}

impl<T, E: Error> FlattenResult<T, RemoveError<E>>
    for Result<Result<T, RemoveError<Infallible>>, E>
{
    fn flatten(self) -> Result<T, RemoveError<E>> {
        match self {
            Ok(o) => match o {
                Ok(o) => Ok(o),
                Err(e) => Err(match e {
                    RemoveError::NotFound => RemoveError::NotFound,
                    RemoveError::Io(_) => unreachable!(),
                }),
            },
            Err(e) => Err(RemoveError::Io(e)),
        }
    }
}

impl<T, E: Error> FlattenResult<T, RenameError<E>>
    for Result<Result<T, RenameError<Infallible>>, E>
{
    fn flatten(self) -> Result<T, RenameError<E>> {
        match self {
            Ok(o) => match o {
                Ok(o) => Ok(o),
                Err(e) => Err(match e {
                    RenameError::NotFound => RenameError::NotFound,
                    RenameError::AlreadyExists => RenameError::AlreadyExists,
                    RenameError::Io(_) => unreachable!(),
                }),
            },
            Err(e) => Err(RenameError::Io(e)),
        }
    }
}

impl<T, E: Error> FlattenResult<T, ReplaceError<E>>
    for Result<Result<T, ReplaceError<Infallible>>, E>
{
    fn flatten(self) -> Result<T, ReplaceError<E>> {
        match self {
            Ok(o) => match o {
                Ok(o) => Ok(o),
                Err(e) => Err(match e {
                    ReplaceError::NotFound => ReplaceError::NotFound,
                    ReplaceError::Io(_) => unreachable!(),
                }),
            },
            Err(e) => Err(ReplaceError::Io(e)),
        }
    }
}

impl<T, E: Error> FlattenResult<T, InsertError<E>>
    for Result<Result<T, InsertError<Infallible>>, E>
{
    fn flatten(self) -> Result<T, InsertError<E>> {
        match self {
            Ok(o) => match o {
                Ok(o) => Ok(o),
                Err(e) => Err(match e {
                    InsertError::AlreadyExists => InsertError::AlreadyExists,
                    InsertError::Io(_) => unreachable!(),
                }),
            },
            Err(e) => Err(InsertError::Io(e)),
        }
    }
}

impl<T, E: Error> FlattenResult<T, OpenError<E>> for Result<Result<T, OpenError<Infallible>>, E> {
    fn flatten(self) -> Result<T, OpenError<E>> {
        match self {
            Ok(o) => match o {
                Ok(o) => Ok(o),
                Err(e) => Err(match e {
                    OpenError::NotFound => OpenError::NotFound,
                    OpenError::Io(_) => unreachable!(),
                }),
            },
            Err(e) => Err(OpenError::Io(e)),
        }
    }
}

impl<T, E: Error> FlattenResult<T, RepackError<E>>
    for Result<Result<T, RepackError<Infallible>>, E>
{
    fn flatten(self) -> Result<T, RepackError<E>> {
        match self {
            Ok(o) => match o {
                Ok(o) => Ok(o),
                #[allow(unreachable_code)] // This is a lot cleaner as a match
                Err(e) => Err(match e {
                    RepackError::OverlappingEntries => RepackError::OverlappingEntries,
                    RepackError::Io(_) => unreachable!(),
                }),
            },
            Err(e) => Err(RepackError::Io(e)),
        }
    }
}
