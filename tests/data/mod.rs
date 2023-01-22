use rand::Rng;

pub const BASIC_EXAMPLE_FILES: &[(&str, &[u8])] = &[
    ("hello.txt", b"A very happy little file"),
    ("fox.txt", b"A brown fox jumps over the lazy dog."),
    ("lorem/lorem512.txt", include_bytes!("./lorem512.txt")),
    ("lorem/lorem1024.txt", include_bytes!("./lorem1024.txt")),
    ("lorem/lorem4096.txt", include_bytes!("./lorem4096.txt")),
    ("lorem/lorem16384.txt", include_bytes!("./lorem16384.txt")),
];

pub fn generate_big_data() -> impl Iterator<Item = (String, Vec<u8>)> {
    let mut rng = rand::thread_rng();

    // Goes up to exactly one MiB
    (0..20).map(move |i| {
        let size = 2usize.pow(i);
        let name = format!("random/{size}.bin");
        let mut data = vec![0; size];

        rng.fill(&mut data[..]);

        (name, data)
    })
}

pub fn combined_data() -> impl Iterator<Item = (String, Vec<u8>)> {
    BASIC_EXAMPLE_FILES
        .iter()
        .map(|(n, d)| (n.to_string(), d.to_vec()))
        .chain(generate_big_data())
}
