use std::io::{Read, Seek, Write};

use crate::base::BUFFER_SIZE;

macro_rules! define_read_le_methods {
    { $($name:ident -> $ret:ty;)* } => {
        $(fn $name(&mut self) -> ::std::io::Result<$ret> {
            let mut buf = [0u8; ::std::mem::size_of::<$ret>()];
            self.read_exact(&mut buf)?;
            Ok(<$ret>::from_le_bytes(buf))
        })*
    };
}

macro_rules! define_read_be_methods {
    { $($name:ident -> $ret:ty;)* } => {
        $(fn $name(&mut self) -> ::std::io::Result<$ret> {
            let mut buf = [0u8; ::std::mem::size_of::<$ret>()];
            self.read_exact(&mut buf)?;
            Ok(<$ret>::from_be_bytes(buf))
        })*
    };
}

pub trait ReadExt: Read {
    fn read_u8(&mut self) -> std::io::Result<u8> {
        let mut buf = [0u8; 1];
        self.read_exact(&mut buf)?;
        Ok(buf[0])
    }

    fn read_i8(&mut self) -> std::io::Result<i8> {
        let mut buf = [0u8; 1];
        self.read_exact(&mut buf)?;
        Ok(i8::from_ne_bytes(buf))
    }

    define_read_le_methods! {
        read_u16_le -> u16;
        read_u32_le -> u32;
        read_u64_le -> u64;

        read_i16_le -> i16;
        read_i32_le -> i32;
        read_i64_le -> i64;
    }

    define_read_be_methods! {
        read_u16_be -> u16;
        read_u32_be -> u32;
        read_u64_be -> u64;

        read_i16_be -> i16;
        read_i32_be -> i32;
        read_i64_be -> i64;
    }

    fn read_n_exact<const N: usize>(&mut self) -> std::io::Result<[u8; N]> {
        let mut buf = [0u8; N];
        self.read_exact(&mut buf)?;
        Ok(buf)
    }
}

macro_rules! define_write_le_methods {
    { $($name:ident($type:ty);)* } => {
        $(fn $name(&mut self, value: $type) -> ::std::io::Result<()> {
            self.write_all(&value.to_le_bytes())
        })*
    };
}

macro_rules! define_write_be_methods {
    { $($name:ident($type:ty);)* } => {
        $(fn $name(&mut self, value: $type) -> ::std::io::Result<()> {
            self.write_all(&value.to_be_bytes())
        })*
    };
}

pub trait WriteExt: Write {
    fn write_u8(&mut self, value: u8) -> std::io::Result<()> {
        self.write_all(&[value])
    }

    fn write_i8(&mut self, value: i8) -> std::io::Result<()> {
        self.write_all(&value.to_ne_bytes())
    }

    define_write_le_methods! {
        write_u16_le(u16);
        write_u32_le(u32);
        write_u64_le(u64);

        write_i16_le(i16);
        write_i32_le(i32);
        write_i64_le(i64);
    }

    define_write_be_methods! {
        write_u16_be(u16);
        write_u32_be(u32);
        write_u64_be(u64);

        write_i16_be(i16);
        write_i32_be(i32);
        write_i64_be(i64);
    }

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

impl<W: Write> WriteExt for W {}

impl<R: Read> ReadExt for R {}

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

impl<R: Read + Seek + Write> ReadSeekWriteExt for R {}

macro_rules! declare_as_methods {
    { $($name:ident -> $type:ty;)* } => {
        $(fn $name(&self) -> $type;)*
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
        fn $name(&self) -> $type {
            assert!(self.len() == std::mem::size_of::<$type>());

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
