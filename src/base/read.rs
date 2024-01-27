use flate2::Decompress;
use macros::generator;

use crate::base::BUFFER_SIZE;

use super::{OpenError, PkgState, RawFlags, ReadSeekRequest, Response, SeekError, SeekFrom};

pub trait GeneratorRead {
    #[generator(static, yield ReadSeekRequest -> Response)]
    fn read(&mut self, buffer: &mut [u8]) -> usize;
}

pub trait GeneratorSeek {
    #[generator(static, yield ReadSeekRequest -> Response)]
    fn seek(&mut self, seekfrom: SeekFrom) -> Result<u64, SeekError>;
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

#[generator(static, yield ReadSeekRequest -> Response)]
pub fn open(state: &PkgState, path: &str) -> Result<ReadHandle, OpenError> {
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

impl GeneratorRead for RawReadWriteHandle {
    #[generator(static, yield ReadSeekRequest -> Response)]
    fn read(&mut self, buffer: &mut [u8]) -> usize {
        let end = (self.cursor + buffer.len() as u64).min(self.size);
        let count = end - self.cursor;
        let value = request!(read count);
        buffer[..value.len()].copy_from_slice(&value);
        self.cursor += count;
        value.len()
    }
}

impl GeneratorSeek for RawReadWriteHandle {
    #[generator(static, yield ReadSeekRequest -> Response)]
    fn seek(&mut self, seekfrom: SeekFrom) -> Result<u64, SeekError> {
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

impl GeneratorRead for DeflateReadHandle {
    #[generator(static, yield ReadSeekRequest -> Response)]
    fn read(&mut self, mut buffer: &mut [u8]) -> usize {
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

impl GeneratorRead for ReadHandle {
    #[generator(static, yield ReadSeekRequest -> Response)]
    fn read(&mut self, buffer: &mut [u8]) -> usize {
        match self {
            ReadHandle::Raw(h) => h.read(buffer).await,
            ReadHandle::Deflate(h) => h.read(buffer).await,
        }
    }
}

impl GeneratorSeek for ReadHandle {
    #[generator(static, yield ReadSeekRequest -> Response)]
    fn seek(&mut self, seekfrom: SeekFrom) -> Result<u64, SeekError> {
        match self {
            ReadHandle::Raw(h) => h.seek(seekfrom).await,
            ReadHandle::Deflate(_) => Err(SeekError::NotSeekable),
        }
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
