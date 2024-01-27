## silpkg

`silpkg` is a library for interacting with [SIL](https://achurch.org/SIL/) PKG files.

Documentation for the library can be generated through `rustdoc` (`cargo doc`).
Although incomplete it should cover some basic usages.

## CLI

A CLI that allows performing basic operations on archives is also included and can be easily installed with `cargo install --git https://github.com/afishhh/silpkg cli`.

## MSRV

Nightly rust is required to use this library.
This is because I experimented with using Rust's [Coroutines](https://doc.rust-lang.org/beta/unstable-book/language-features/coroutines.html) for IO agnostic parsing logic.

## no_std

While the crate will build without `std`, it still depends on `alloc` and since `std::io` is absent the only way to actually use the library is through the `silpkg::base` module with the `unstable_base` feature.
In the future a slice-backed `Pkg` may be implemented to allow for easy no_std use, although currently I have too little time to do this myself.
For implementing your own IO frontend for the `silpkg::base` module, look at how `silpkg::sync::Pkg` is currently implemented.
