use std::{
    collections::HashSet,
    io::{Read, Seek, Write},
};

use silpkg::{sync::Pkg, Compression, Flags};

use test_log::test;

mod data;

fn add<'a, S: Read + Seek + Write>(
    pkg: &mut Pkg<S>,
    flags: Flags,
    data: impl Iterator<Item = (String, &'a [u8])>,
) {
    for (name, data) in data {
        let mut writer = pkg.insert(name, flags.clone()).unwrap();
        writer.write_all(data).unwrap();
    }
}

fn rename<'a, S: Read + Seek + Write>(
    pkg: &mut Pkg<S>,
    paths: impl Iterator<Item = (&'a str, String)>,
) {
    for (src, dst) in paths {
        pkg.rename(src, dst).unwrap();
    }
}

fn extract<'a, S: Read + Seek + Write>(
    pkg: &mut Pkg<S>,
    data: impl Iterator<Item = (&'a str, &'a [u8])>,
) {
    for (name, data) in data {
        let mut reader = pkg.open(name).unwrap();
        let mut out = vec![];
        reader.read_to_end(&mut out).unwrap();
        assert_eq!(&out, &data);
    }
}

fn list<'a, S: Read + Seek + Write>(pkg: &mut Pkg<S>, paths: impl Iterator<Item = &'a str>) {
    let mut list_paths = pkg.paths().map(String::as_str).collect::<HashSet<_>>();

    for name in paths {
        assert!(list_paths.remove(name));
    }

    assert_eq!(list_paths.len(), 0);
}

// TODO: Tests for remove
// fn remove<'a, S: Read + Seek + Write>(pkg: &mut Pkg<S>, names: impl Iterator<Item = &'a str>) {
//     for name in names {
//         assert!(pkg.remove(name).unwrap());
//     }
// }

#[test]
fn add_extract() {
    let mut pkg = Pkg::create(std::io::Cursor::new(vec![])).unwrap();
    let data: Vec<(String, Vec<u8>)> = data::combined_data().collect();

    add(
        &mut pkg,
        Flags::default(),
        data.iter().map(|(n, d)| (n.to_string(), d.as_slice())),
    );

    extract(
        &mut pkg,
        data.iter().map(|(n, d)| (n.as_str(), d.as_slice())),
    );

    list(&mut pkg, data.iter().map(|(n, _)| n.as_str()));
}

#[test]
fn add_repack_extract() {
    let mut pkg = Pkg::create(std::io::Cursor::new(vec![])).unwrap();
    let data: Vec<(String, Vec<u8>)> = data::combined_data().collect();

    add(
        &mut pkg,
        Flags::default(),
        data.iter().map(|(n, d)| (n.to_string(), d.as_slice())),
    );

    pkg.repack().unwrap();

    extract(
        &mut pkg,
        data.iter().map(|(n, d)| (n.as_str(), d.as_slice())),
    );

    list(&mut pkg, data.iter().map(|(n, _)| n.as_str()));
}

#[test]
fn add_parse_extract() {
    let mut storage = std::io::Cursor::new(vec![]);
    let mut pkg = Pkg::create(&mut storage).unwrap();
    let data: Vec<(String, Vec<u8>)> = data::combined_data().collect();

    add(
        &mut pkg,
        Flags::default(),
        data.iter().map(|(n, d)| (n.to_string(), d.as_slice())),
    );

    let mut pkg = Pkg::parse(&mut storage).unwrap();

    extract(
        &mut pkg,
        data.iter().map(|(n, d)| (n.as_str(), d.as_slice())),
    );

    list(&mut pkg, data.iter().map(|(n, _)| n.as_str()));
}

#[test]
fn add_parse_repack_extract() {
    let mut storage = std::io::Cursor::new(vec![]);
    let mut pkg = Pkg::create(&mut storage).unwrap();
    let data: Vec<(String, Vec<u8>)> = data::combined_data().collect();

    add(
        &mut pkg,
        Flags::default(),
        data.iter().map(|(n, d)| (n.to_string(), d.as_slice())),
    );

    pkg.repack().unwrap();
    let mut pkg = Pkg::parse(&mut storage).unwrap();

    extract(
        &mut pkg,
        data.iter().map(|(n, d)| (n.as_str(), d.as_slice())),
    );
}

#[test]
fn add_compressed_repack_extract() {
    let mut pkg = Pkg::create(std::io::Cursor::new(vec![])).unwrap();
    let data: Vec<(String, Vec<u8>)> = data::combined_data().collect();

    add(
        &mut pkg,
        silpkg::Flags {
            compression: silpkg::EntryCompression::Deflate(Compression::best()),
        },
        data.iter().map(|(n, d)| (n.to_string(), d.as_slice())),
    );

    pkg.repack().unwrap();

    extract(
        &mut pkg,
        data.iter().map(|(n, d)| (n.as_str(), d.as_slice())),
    );
}

#[test]
fn add_extract_rename_extract() {
    let mut pkg = Pkg::create(std::io::Cursor::new(vec![])).unwrap();
    let mut data: Vec<(String, Vec<u8>)> = data::combined_data().collect();

    add(
        &mut pkg,
        silpkg::Flags {
            compression: silpkg::EntryCompression::Deflate(Compression::best()),
        },
        data.iter().map(|(n, d)| (n.to_string(), d.as_slice())),
    );

    extract(
        &mut pkg,
        data.iter().map(|(n, d)| (n.as_str(), d.as_slice())),
    );

    rename(
        &mut pkg,
        data.iter()
            .map(|(n, _)| (n.as_str(), format!("{n}-renamed"))),
    );
    for (name, _) in data.iter_mut() {
        *name = format!("{name}-renamed")
    }

    extract(
        &mut pkg,
        data.iter().map(|(n, d)| (n.as_str(), d.as_slice())),
    );
}

#[test]
fn add_extract_rename_repack_extract() {
    let mut pkg = Pkg::create(std::io::Cursor::new(vec![])).unwrap();
    let mut data: Vec<(String, Vec<u8>)> = data::combined_data().collect();

    add(
        &mut pkg,
        silpkg::Flags {
            compression: silpkg::EntryCompression::Deflate(Compression::best()),
        },
        data.iter().map(|(n, d)| (n.to_string(), d.as_slice())),
    );

    extract(
        &mut pkg,
        data.iter().map(|(n, d)| (n.as_str(), d.as_slice())),
    );

    rename(
        &mut pkg,
        data.iter()
            .map(|(n, _)| (n.as_str(), format!("{n}-renamed"))),
    );

    for (name, _) in data.iter_mut() {
        *name = format!("{name}-renamed")
    }

    pkg.repack().unwrap();

    extract(
        &mut pkg,
        data.iter().map(|(n, d)| (n.as_str(), d.as_slice())),
    );
}

#[test]
fn everything() {
    let mut storage = std::io::Cursor::new(vec![]);
    let mut pkg = Pkg::create(&mut storage).unwrap();
    let mut data: Vec<(String, Vec<u8>)> = data::combined_data().collect();

    add(
        &mut pkg,
        silpkg::Flags {
            compression: silpkg::EntryCompression::Deflate(Compression::best()),
        },
        data.iter().map(|(n, d)| (n.to_string(), d.as_slice())),
    );

    extract(
        &mut pkg,
        data.iter().map(|(n, d)| (n.as_str(), d.as_slice())),
    );

    rename(
        &mut pkg,
        data.iter()
            .map(|(n, _)| (n.as_str(), format!("{n}-renamed"))),
    );

    for (name, _) in data.iter_mut() {
        *name = format!("{name}-renamed")
    }

    let mut pkg = Pkg::parse(&mut storage).unwrap();

    extract(
        &mut pkg,
        data.iter().map(|(n, d)| (n.as_str(), d.as_slice())),
    );

    pkg.repack().unwrap();
    let mut pkg = Pkg::parse(&mut storage).unwrap();

    extract(
        &mut pkg,
        data.iter().map(|(n, d)| (n.as_str(), d.as_slice())),
    );
}
