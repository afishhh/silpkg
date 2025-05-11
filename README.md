## silpkg

`silpkg` is a library for interacting with [SIL](https://achurch.org/SIL/) PKG files.

Documentation for the library can be generated through `rustdoc` (`cargo doc`).
The main entry point to the library is `silpkg::sync::Pkg` which allows for reading and writing archives backed by a `Read + Seek (+ Write (+ silpkg::sync::Truncate))` type.

## CLI

A CLI that allows performing basic operations on archives is also included and can be easily installed with `cargo install --git https://github.com/afishhh/silpkg cli`.

## MSRV

Nightly rust is required to use this library.
This is because I experimented with using Rust's [Coroutines](https://doc.rust-lang.org/beta/unstable-book/language-features/coroutines.html) for IO agnostic parsing logic.

## no_std

While the crate will build without `std`, it still depends on `alloc` and since `std::io` is absent the only way to actually use the library is through the `silpkg::base` module with the `unstable_base` feature.
Theoretically a slice-backed `Pkg` could be implemented to allow for easy no_std use, although currently I have too little time (and see little reason) to do this myself.
For implementing your own IO frontend for the `silpkg::base` module, look at how `silpkg::sync::Pkg` is currently implemented.

## License

`silpkg` itself is licensed under the GNU General Public License 2.0, `silpkg-macros` in the `macros/` directory is licensed under MIT OR Apache-2.0.
