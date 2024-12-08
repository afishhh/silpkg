pub const MAGIC: &[u8] = b"PKG\n";
pub const HEADER_SIZE: u64 = 16;
pub const ENTRY_SIZE: u64 = 20;

pub const BUFFER_SIZE: u64 = 8192;

pub fn pkg_path_hash(path: &str) -> u32 {
    let mut hash: u32 = 0;
    for mut c in path.chars() {
        assert!(c.is_ascii(), "non-ascii string passed to pkg_path_hash()");

        // TODO: Why is this case insensitive
        c.make_ascii_lowercase();
        hash = hash.overflowing_shl(27).0 | hash.overflowing_shr(5).0;
        hash ^= c as u32;
        hash &= 0x00000000FFFFFFFF;
    }
    hash
}
