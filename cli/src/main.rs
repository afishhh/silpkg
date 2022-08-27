#![feature(file_create_new)]

use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::{bail, Context};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use silpkg::{Flags, Pkg};

fn pkg_open_ro(path: &Path) -> Result<Pkg<File>, anyhow::Error> {
    Pkg::parse(File::open(path).context("Could not open pkg file")?, true)
        .context("Could not parse pkg file")
}

fn pkg_open_rw(path: &Path) -> Result<Pkg<File>, anyhow::Error> {
    Pkg::parse(
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .context("Could not open pkg file")?,
        true,
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

#[derive(clap::Subcommand)]
enum Command {
    List(List),
    Extract(Extract),
    Add(Add),
    Compress(Compress),
}

#[derive(Parser)]
#[command(author, version, about)]
struct Opts {
    #[command(subcommand)]
    command: Command,
}

fn real_main() -> Result<ExitCode, anyhow::Error> {
    let opts = Opts::parse();

    let spinner_style =
        ProgressStyle::with_template("{prefix:.bold} {spinner} {msg}")?.tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ");

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

            for path in pkg.paths().cloned().collect::<Vec<String>>() {
                let out = extract_opts.output.join(path.clone());
                std::fs::create_dir_all(out.parent().unwrap())?;
                pkg.extract_to(
                    &path,
                    std::fs::File::create_new(out.clone()).with_context(|| {
                        format!("Could not write output file {}", out.display())
                    })?,
                )?;
            }
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

            let bar = ProgressBar::new_spinner()
                .with_style(spinner_style.clone())
                .with_prefix("Adding files");

            let mut add_one = |path: &Path| -> Result<(), anyhow::Error> {
                let path_str = path.to_str().filter(|x| x.is_ascii()).with_context(|| {
                    format!("Input file path {} is not valid ASCII", path.display())
                })?;

                bar.set_message({
                    let mut msg = format!("{}", path.display());
                    if msg.len() > 50 {
                        msg.truncate(50);
                        msg += "...";
                    }
                    msg
                });

                if add_opts.overwrite && pkg.contains(path_str) {
                    pkg.remove(path_str)?;
                }

                pkg.insert(
                    path_str.to_string(),
                    if add_opts.compression_level.is_some() {
                        silpkg::Flags::DEFLATED
                    } else {
                        silpkg::Flags::empty()
                    },
                    add_opts.compression_level.map(silpkg::Compression::new),
                    std::fs::File::open(path)
                        .with_context(|| "Could not open input file {file}")?,
                )
                .with_context(|| format!("Could not add {path_str} to archive"))?;

                bar.inc(1);
                bar.suspend(|| {
                    println!("{path_str}");
                });

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

            bar.finish_with_message("done");

            let bar = ProgressBar::new_spinner()
                .with_style(spinner_style)
                .with_prefix("Adding files");

            bar.set_prefix("Repacking");
            bar.set_message("");
            if add_opts.repack {
                // TODO: spinner here
                pkg.repack()?;
            }

            bar.finish_with_message("done");
        }
        Command::Compress(compress_opts) => {
            let mut pkg = pkg_open_rw(&compress_opts.pkg)?;

            let bar = ProgressBar::new_spinner()
                .with_style(spinner_style)
                .with_prefix("Compressing files");

            let mut tmp = vec![];
            for path in pkg.paths().cloned().collect::<Vec<_>>() {
                bar.set_message(path.clone());

                pkg.extract_to(&path, &mut tmp)?;
                pkg.remove(&path)?;
                pkg.insert(
                    path.clone(),
                    Flags::DEFLATED,
                    Some(silpkg::Compression::new(compress_opts.compression_level)),
                    std::io::Cursor::new(&tmp),
                )?;
                tmp.clear();

                bar.inc(1);
                bar.suspend(|| {
                    println!("{path}");
                })
            }

            pkg.repack()?;
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
                        log::Level::Error => console::style("error").red(),
                        log::Level::Warn => console::style(" warn").yellow(),
                        log::Level::Info => console::style(" info").blue(),
                        log::Level::Debug => console::style("debug").magenta(),
                        log::Level::Trace => console::style("trace").white(),
                    }
                    .bold()
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
            eprint!("\x1b[31;1merror\x1b[0m: {err}");

            let chain = err.chain();
            if chain.len() > 1 {
                eprintln!(": {}", err.chain().nth(1).unwrap());
            } else if chain.len() > 2 {
                eprintln!();
                chain.enumerate().skip(1).for_each(|(i, err)| {
                    eprintln!("         #{i}: {err}");
                });
            }

            ExitCode::FAILURE
        }
    }
}
