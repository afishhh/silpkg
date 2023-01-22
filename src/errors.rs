use thiserror::Error;

use crate::base::{ENTRY_SIZE, HEADER_SIZE};

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("File does not start the correct magic number")]
    MismatchedMagic,

    #[error("File uses unsupported header size {size} (expected {HEADER_SIZE})")]
    MismatchedHeaderSize { size: u16 },
    #[error("File uses unsupported entry size {size} (expected {ENTRY_SIZE})")]
    MismatchedEntrySize { size: u16 },
    #[error("File claims header section extends beyond EOF")]
    EntryOverflow,
    #[error("File claims path region extends beyond EOF")]
    PathOverflow,

    #[error("Entry contains unrecognised entry flags {0:#04X}")]
    UnrecognisedEntryFlags(u32),

    #[error("Entry has a non-ascii path")]
    NonAsciiPath,
    #[error("Archive contains two entries with the same path {0}")]
    SamePath(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum CreateError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum RenameError {
    #[error("Source entry does not exist")]
    NotFound,
    #[error("Desination entry already exists")]
    AlreadyExists,

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum ReplaceError {
    #[error("Entry does not exist")]
    NotFound,

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum InsertError {
    #[error("An entry with the same path already exists")]
    AlreadyExists,

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum ExtractError {
    #[error("Entry does not exist")]
    NotFound,

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum RepackError {
    #[error("Repacking PKGs with overlapping entries is not supported (yet)")]
    OverlappingEntries,

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<CreateError> for std::io::Error {
    fn from(val: CreateError) -> Self {
        match val {
            CreateError::Io(err) => err,
        }
    }
}

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

impl From<ExtractError> for std::io::Error {
    fn from(val: ExtractError) -> Self {
        match val {
            ExtractError::NotFound => {
                std::io::Error::new(std::io::ErrorKind::NotFound, val.to_string())
            }
            ExtractError::Io(err) => err,
        }
    }
}
