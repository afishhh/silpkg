#![feature(file_create_new)]

use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::{bail, Context};
use clap::Parser;
use silpkg::{sync::Pkg, Flags};

mod progress;
use progress::{ProgressBar, ProgressBarStyle};
mod spinner;
use spinner::{Spinner, SpinnerStyle};

const PROGRESS_BAR_STYLE: ProgressBarStyle = ProgressBarStyle { width: 60 };
const SPINNER_STYLE: SpinnerStyle = SpinnerStyle::const_default();
const COMPRESS_TMP_PATH: &str = "____silpkg_cli_compress_temporary_4729875987234";

fn pkg_open_ro(path: &Path) -> Result<Pkg<File>, anyhow::Error> {
    Pkg::parse(File::open(path).context("Could not open pkg file")?)
        .context("Could not parse pkg file")
}

fn pkg_open_rw(path: &Path) -> Result<Pkg<File>, anyhow::Error> {
    Pkg::parse(
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .context("Could not open pkg file")?,
    )
    .context("Could not parse pkg file")
}

#[derive(clap::Args)]
/// Lists paths contained in an archive
struct List {
    pkg: PathBuf,
}

#[derive(clap::Args)]
/// Extracts paths from an archive
struct Extract {
    pkg: PathBuf,
    output: PathBuf,

    /// If an extracted file conflicts with an existing file, overwrite it.
    #[arg(short, long)]
    overwrite: bool,
}

#[derive(clap::Args)]
/// Prints a single file from an archive
struct Cat {
    pkg: PathBuf,
    file: String,
}

#[derive(clap::Args)]
/// Adds files to an archive or creates a new one if it doesn't exist
struct Add {
    pkg: PathBuf,
    files: Vec<PathBuf>,

    #[arg(short, long)]
    /// Repack the archive after adding all the files.
    ///
    /// Repacking will make the archive smaller by defragmenting the data region.
    /// This operation makes further modifications slightly slower and is slow itself.
    repack: bool,

    #[arg(short, long)]
    /// If the archive already contains an entry with the same name, overwrite it.
    overwrite: bool,

    #[arg(
        short,
        long = "compress",
        value_parser = clap::value_parser!(u32).range(0..=9),
    )]
    /// The compression level to use when inserting files, if not specified files are not compressed.
    compression_level: Option<u32>,
}

#[derive(clap::Args)]
/// Compresses all the files in an archive and then repacks it
struct Compress {
    pkg: PathBuf,
    #[arg(
        value_parser = clap::value_parser!(u32).range(0..=9),
    )]
    /// The compression level to use when inserting files, if not specified files are not compressed
    compression_level: u32,
}

#[derive(clap::Args)]
/// Renames a file in an archive
struct Rename {
    pkg: PathBuf,
    source: String,
    destination: String,
}

#[derive(clap::Subcommand)]
enum Command {
    List(List),
    Extract(Extract),
    Cat(Cat),
    Add(Add),
    Compress(Compress),
    Rename(Rename),
}

#[derive(Parser)]
#[command(author, version, about)]
struct Opts {
    #[command(subcommand)]
    command: Command,
}

fn real_main() -> Result<ExitCode, anyhow::Error> {
    let opts = Opts::parse();

    match opts.command {
        Command::List(list_opts) => {
            let pkg = pkg_open_ro(list_opts.pkg.as_path())?;

            for path in pkg.paths() {
                println!("{path}")
            }
        }
        Command::Extract(extract_opts) => {
            let mut pkg = pkg_open_ro(extract_opts.pkg.as_path())?;

            if !extract_opts.output.exists() {
                std::fs::create_dir_all(extract_opts.output.clone())
                    .context("Could not create output directory")?;
            } else if !extract_opts.output.is_dir() {
                bail!("Output path already exists and is not a directory");
            }

            let mut paths = pkg.paths().cloned().collect::<Vec<String>>();
            paths.sort();
            let mut bar = ProgressBar::new(PROGRESS_BAR_STYLE, paths.len(), "".to_string());

            for path in paths {
                bar.paused(|| {
                    eprintln!("\x1b[1mExtracting\x1b[0m {path}...");
                });
                let out = extract_opts.output.join(path.clone());
                std::fs::create_dir_all(out.parent().unwrap())?;
                std::io::copy(&mut pkg.open(&path)?, &mut {
                    let mut opts = std::fs::OpenOptions::new();
                    opts.write(true);

                    if extract_opts.overwrite {
                        opts.create(true);
                    } else {
                        opts.create_new(true);
                    }

                    opts.open(&out)
                        .with_context(|| format!("Could not write output file {}", out.display()))?
                })?;
                bar.paused(|| {
                    eprint!("\x1b[1A\x1b[2K");
                    println!("{path}");
                });
                bar.inc();
            }
            bar.finish();
        }
        Command::Cat(Cat { pkg, file }) => {
            let mut pkg = pkg_open_ro(&pkg)?;

            std::io::copy(&mut pkg.open(&file)?, &mut std::io::stdout())?;
        }
        Command::Add(add_opts) => {
            let mut pkg = {
                if add_opts.pkg.exists() {
                    pkg_open_rw(&add_opts.pkg)?
                } else {
                    Pkg::create(
                        std::fs::File::create_new(add_opts.pkg)
                            .context("Could not create archive file")?,
                    )
                    .context("Could not write new archive to file")?
                }
            };

            let mut bar = Spinner::new(SPINNER_STYLE.clone(), "Adding files");

            let mut add_one = |path: &Path| -> Result<(), anyhow::Error> {
                let path_str = path.to_str().filter(|x| x.is_ascii()).with_context(|| {
                    format!("Input file path {} is not valid ASCII", path.display())
                })?;

                bar.paused(|| {
                    eprint!("\x1b[1mAdding\x1b[0m ",);
                    if path_str.len() > 50 {
                        eprint!("{}...", &path_str[..50]);
                    } else {
                        eprint!("{path_str}");
                    }
                    eprintln!()
                });

                if add_opts.overwrite && pkg.contains(path_str) {
                    pkg.remove(path_str)?;
                }

                let mut writer = pkg
                    .insert(
                        path_str.to_string(),
                        silpkg::Flags {
                            compression: add_opts
                                .compression_level
                                .map_or(silpkg::EntryCompression::None, |x| {
                                    silpkg::EntryCompression::Deflate(silpkg::Compression::new(x))
                                }),
                        },
                    )
                    .with_context(|| format!("Could not add {path_str} to archive"))?;

                std::io::copy(
                    &mut std::fs::File::open(path)
                        .with_context(|| "Could not open input file {file}")?,
                    &mut writer,
                )
                .with_context(|| format!("Could not write {path_str} to archive"))?;

                log::trace!("done with {path_str}");

                bar.paused(|| {
                    eprint!("\x1b[1F\x1b[2K");
                    println!("{path_str}");
                });
                bar.inc();

                Ok(())
            };

            for path in add_opts.files.into_iter() {
                let ft = path.metadata()?.file_type();
                if ft.is_file() {
                    add_one(&path)?;
                } else if ft.is_dir() {
                    for r in walkdir::WalkDir::new(path).into_iter() {
                        let entry = r?;
                        if entry.file_type().is_dir() {
                            continue;
                        } else if entry.file_type().is_file() {
                            add_one(entry.path())?;
                        } else {
                            log::warn!(
                                "Omitting {} as it is not a regular file or directory",
                                entry.path().display()
                            );
                        }
                    }
                }
            }

            bar.finish_with("done");

            if add_opts.repack {
                let bar = Spinner::new(SPINNER_STYLE.clone(), "Repacking");

                // TODO: spinner here
                pkg.repack()?;
                bar.finish_with("done");
            }
        }
        Command::Compress(compress_opts) => {
            let mut pkg = pkg_open_rw(&compress_opts.pkg)?;

            let mut paths = pkg.paths().cloned().collect::<Vec<_>>();
            paths.sort();
            let mut bar = ProgressBar::new(PROGRESS_BAR_STYLE, paths.len(), "".to_string());

            let mut tmp = vec![];
            for path in paths {
                bar.paused(|| {
                    eprintln!("\x1b[1mCompressing\x1b[0m {path}...");
                });

                pkg.open(&path)?.read_to_end(&mut tmp)?;
                pkg.insert(
                    COMPRESS_TMP_PATH.to_string(),
                    Flags {
                        compression: silpkg::EntryCompression::Deflate(silpkg::Compression::new(
                            compress_opts.compression_level,
                        )),
                    },
                )?
                .write_all(&tmp)
                .with_context(|| format!("Could not compress {path}"))?;
                pkg.replace(COMPRESS_TMP_PATH, path.clone())?;
                tmp.clear();

                bar.paused(|| {
                    eprint!("\x1b[1F\x1b[2K");
                    println!("{path}");
                });
                bar.inc();
            }
            bar.finish();

            pkg.repack()?;
        }
        Command::Rename(opts) => {
            let mut pkg = pkg_open_rw(&opts.pkg)?;

            pkg.rename(&opts.source, opts.destination)?;
        }
    }

    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    env_logger::builder()
        .format(|f, record| {
            for line in record.args().to_string().split('\n') {
                write!(
                    f,
                    "{}",
                    match record.level() {
                        log::Level::Error => "\x1b[1;31merror\x1b[0m",
                        log::Level::Warn => "\x1b[1;33m warn\x1b[0m",
                        log::Level::Info => "\x1b[1;34m info\x1b[0m",
                        log::Level::Debug => "\x1b[1;35mdebug\x1b[0m",
                        log::Level::Trace => "\x1b[1;37mtrace\x1b[0m",
                    }
                )?;
                write!(f, "({})", record.target())?;
                writeln!(f, ": {line}")?;
            }

            Ok(())
        })
        .filter_level({
            #[cfg(debug_assertions)]
            let v = log::LevelFilter::Debug;
            #[cfg(not(debug_assertions))]
            let v = log::LevelFilter::Info;
            v
        })
        .parse_env("SILPKG_LOG")
        .init();

    match real_main() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("\x1b[31;1merror\x1b[0m: {err}");

            for (i, err) in err.chain().enumerate().skip(1) {
                eprintln!("         #{i}: {err}");
            }

            ExitCode::FAILURE
        }
    }
}
