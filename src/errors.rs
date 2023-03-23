use alloc::string::String;
use thiserror::Error;

use crate::base::{ENTRY_SIZE, HEADER_SIZE};

/// An error triggered while parsing an existing archive.
#[derive(Debug, Error)]
pub enum ParseError {
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
    Io(#[from] std::io::Error),
}

/// An error triggered while creating a new archive.
#[derive(Debug, Error)]
pub enum CreateError {
    #[cfg(feature = "std")]
    #[error(transparent)]
    /// An IO error occurred.
    Io(#[from] std::io::Error),
}

/// An error triggered while removing an entry.
#[derive(Debug, Error)]
pub enum RemoveError {
    #[error("Entry does not exist")]
    /// The target entry was not found.
    NotFound,

    #[cfg(feature = "std")]
    #[error(transparent)]
    /// An IO error occurred.
    Io(#[from] std::io::Error),
}

/// An error triggered while renaming an entry.
#[derive(Debug, Error)]
pub enum RenameError {
    #[error("Source entry does not exist")]
    /// The source entry was not found.
    NotFound,
    #[error("Desination entry already exists")]
    /// An entry with the destination path was already present.
    AlreadyExists,

    #[cfg(feature = "std")]
    #[error(transparent)]
    /// An IO error occurred.
    Io(#[from] std::io::Error),
}

/// An error triggered while replacing one entry with another.
#[derive(Debug, Error)]
pub enum ReplaceError {
    #[error("Entry does not exist")]
    /// The source entry was not found.
    NotFound,

    #[cfg(feature = "std")]
    #[error(transparent)]
    /// An IO error occurred.
    Io(#[from] std::io::Error),
}

/// An error triggered while inserting a new entry into an archive.
#[derive(Debug, Error)]
pub enum InsertError {
    #[error("An entry with the same path already exists")]
    /// An entry with that name already existed.
    AlreadyExists,

    #[cfg(feature = "std")]
    #[error(transparent)]
    /// An IO error occurred.
    Io(#[from] std::io::Error),
}

/// An error triggered while opening an entry for reading.
#[derive(Debug, Error)]
pub enum OpenError {
    #[error("Entry does not exist")]
    /// An entry with that name was not found.
    NotFound,

    #[cfg(feature = "std")]
    #[error(transparent)]
    /// An IO error occurred.
    Io(#[from] std::io::Error),
}

/// An error triggered while repacking.
#[derive(Debug, Error)]
pub enum RepackError {
    #[error("Repacking PKGs with overlapping entries is not supported (yet)")]
    /// The archive contained overlapping entries.
    ///
    /// This cannot be triggered by creating your own archive and can only happen if you parse an
    /// archive that contains such overlapping entries and try to repack it.
    OverlappingEntries,

    #[cfg(feature = "std")]
    #[error(transparent)]
    /// An IO error occurred.
    Io(#[from] std::io::Error),
}

#[cfg(feature = "std")]
impl From<CreateError> for std::io::Error {
    fn from(val: CreateError) -> Self {
        match val {
            CreateError::Io(err) => err,
        }
    }
}

#[cfg(feature = "std")]
impl From<RemoveError> for std::io::Error {
    fn from(value: RemoveError) -> Self {
        match value {
            RemoveError::NotFound => {
                std::io::Error::new(std::io::ErrorKind::NotFound, value.to_string())
            }
            RemoveError::Io(err) => err,
        }
    }
}

#[cfg(feature = "std")]
impl From<RenameError> for std::io::Error {
    fn from(val: RenameError) -> Self {
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
impl From<InsertError> for std::io::Error {
    fn from(val: InsertError) -> Self {
        match val {
            InsertError::AlreadyExists => {
                std::io::Error::new(std::io::ErrorKind::AlreadyExists, val.to_string())
            }
            InsertError::Io(err) => err,
        }
    }
}

#[cfg(feature = "std")]
impl From<OpenError> for std::io::Error {
    fn from(val: OpenError) -> Self {
        match val {
            OpenError::NotFound => {
                std::io::Error::new(std::io::ErrorKind::NotFound, val.to_string())
            }
            OpenError::Io(err) => err,
        }
    }
}
