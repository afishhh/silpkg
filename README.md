## silpkg

`silpkg` is a library for interacting with [SIL](https://achurch.org/SIL/) PKG files.

Documentation for the library can be generated through `rustdoc` (`cargo doc`).
Although incomplete it should cover some basic usages.

## CLI

A CLI that allows performing basic operations on archives is also included and can be easily installed with `cargo install --git https://github.com/afishhh/silpkg cli`.

## MSRV

Nightly rust is required to use this library.
This is because I experimented with using Rust's [Coroutines](https://doc.rust-lang.org/beta/unstable-book/language-features/coroutines.html) for IO agnostic parsing logic.
