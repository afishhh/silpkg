#[cfg(feature = "std")]
use std::io::{Read, Seek, Write};

use crate::base::BUFFER_SIZE;

#[cfg(feature = "std")]
pub trait WriteExt: Write {
    fn fill(&mut self, value: u8, count: u64) -> std::io::Result<()> {
        let zeros = [value; 1024];
        let mut remaining = count;
        while remaining > 0 {
            let chunk_size = zeros.len().min(remaining as usize);
            self.write_all(&zeros[..chunk_size])?;
            remaining -= chunk_size as u64;
        }

        Ok(())
    }
}

#[cfg(feature = "std")]
impl<W: Write> WriteExt for W {}

#[cfg(feature = "std")]
pub trait ReadSeekWriteExt: Read + Write + Seek {
    fn copy_within(
        &mut self,
        input_offset: u64,
        count: u64,
        output_offset: u64,
    ) -> std::io::Result<()> {
        if input_offset == output_offset {
        } else if (input_offset..count).contains(&output_offset) {
            assert!(output_offset != input_offset);

            let buf_size = (output_offset - input_offset).min(BUFFER_SIZE);

            // TODO: Optimise this case
            if count / buf_size > 64 {
                let mut buf = vec![0; count as usize];

                self.seek(std::io::SeekFrom::Start(input_offset))?;
                self.read_exact(&mut buf)?;

                self.seek(std::io::SeekFrom::Start(output_offset))?;
                self.write_all(&buf)?;

                return Ok(());
            }

            let mut buf = vec![0; buf_size as usize];
            let mut remaining = count;
            while remaining > 0 {
                let chunk_size = (buf.len()).min(remaining as usize);
                self.seek(std::io::SeekFrom::Start(input_offset + remaining))?;
                // TODO: read instead of read_exact
                self.read_exact(&mut buf[..chunk_size])?;
                self.seek(std::io::SeekFrom::Start(output_offset + remaining))?;
                self.write_all(&buf[..chunk_size])?;
                remaining -= chunk_size as u64;
            }
        } else {
            let mut buf = [0; BUFFER_SIZE as usize];
            let mut remaining = count;
            while remaining > 0 {
                let chunk_size = (buf.len()).min(remaining as usize);
                self.seek(std::io::SeekFrom::Start(input_offset + count - remaining))?;
                // TODO: read instead of read_exact
                self.read_exact(&mut buf[..chunk_size])?;
                self.seek(std::io::SeekFrom::Start(output_offset + count - remaining))?;
                self.write_all(&buf[..chunk_size])?;
                remaining -= chunk_size as u64;
            }
        }

        Ok(())
    }
}

#[cfg(feature = "std")]
impl<R: Read + Seek + Write> ReadSeekWriteExt for R {}

macro_rules! declare_as_methods {
    { $($name:ident -> $type:ty;)* } => {
        $(#[allow(dead_code)] fn $name(&self) -> $type;)*
    };
}

pub trait ByteSliceExt {
    declare_as_methods! {
        as_u8_le -> u8;
        as_u16_le -> u16;
        as_u32_le -> u32;
        as_u64_le -> u64;

        as_u8_be -> u8;
        as_u16_be -> u16;
        as_u32_be -> u32;
        as_u64_be -> u64;
    }
}

macro_rules! define_as_methods {
    (@internal_one $conversion:ident $name:ident $type:ty) => {
        #[allow(dead_code)]
        fn $name(&self) -> $type {
            assert!(self.len() == core::mem::size_of::<$type>());

            <$type>::$conversion(unsafe {(*self).try_into().unwrap_unchecked()})
        }
    };
    (@one le $name:ident $type:ty) => {
        define_as_methods!(@internal_one from_le_bytes $name $type);
    };
    (@one be $name:ident $type:ty) => {
        define_as_methods!(@internal_one from_be_bytes $name $type);
    };
    { $($endian:ident $name:ident -> $type:ty;)* } => {
        $(define_as_methods!(@one $endian $name $type);)*
    };
}

impl ByteSliceExt for [u8] {
    define_as_methods! {
        le as_u8_le -> u8;
        le as_u16_le -> u16;
        le as_u32_le -> u32;
        le as_u64_le -> u64;

        be as_u8_be -> u8;
        be as_u16_be -> u16;
        be as_u32_be -> u32;
        be as_u64_be -> u64;
    }
}
