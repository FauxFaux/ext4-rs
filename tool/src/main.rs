extern crate bootsector;
extern crate clap;
#[macro_use]
extern crate error_chain;
extern crate ext4;
extern crate hexdump;

use std::fs;
use std::io;
use std::io::Read;
use std::io::Seek;

use clap::{App, Arg, SubCommand};

use ext4::SuperBlock;

mod errors {
    error_chain! {
        links {
            Ext4(::ext4::Error, ::ext4::ErrorKind);
        }

        foreign_links {
            Io(::std::io::Error);
        }
    }
}

use errors::*;

fn dump_ls<R>(mut fs: SuperBlock<R>) -> Result<()>
where
    R: Read + Seek,
{
    let root = &fs.root()?;
    fs.walk(root, "".to_string(), &mut |_, path, inode, enhanced| {
        println!(
            "<{}> {}: {:?} {:?}",
            inode.number, path, enhanced, inode.stat
        );
        Ok(true)
    }).map(|_| ())?; // we don't care about the returned "true"
    Ok(())
}

fn head_all<R>(mut fs: SuperBlock<R>, bytes: usize) -> Result<()>
where
    R: Read + Seek,
{
    let root = fs.root()?;
    fs.walk(&root, "".to_string(), &mut |fs, path, inode, _| {
        if ext4::FileType::RegularFile != inode.stat.extracted_type {
            return Ok(true);
        }

        if 0 == inode.stat.size {
            println!("==> (empty) {}  <==", path);
            return Ok(true);
        }

        println!("==> {} <==", path);
        let to_read = std::cmp::min(inode.stat.size, bytes as u64) as usize;
        let mut buf = vec![0u8; to_read];

        fs.open(inode)?.read_exact(&mut buf)?;

        match String::from_utf8(buf) {
            Ok(str) => println!("{}", str),
            Err(e) => hexdump::hexdump(&e.into_bytes()),
        };

        Ok(true)
    }).map(|_| ())?; // we don't care about the returned "true"
    Ok(())
}

fn on_fs(file: &str, work: Command) -> Result<()> {
    let mut reader = io::BufReader::new(fs::File::open(file)?);
    match bootsector::list_partitions(&mut reader, &bootsector::Options::default()) {
        Ok(partitions) => for part in partitions {
            work.exec(ext4::SuperBlock::new(bootsector::open_partition(
                &mut reader,
                &part,
            )?)?)?;
        },
        Err(_) => work.exec(ext4::SuperBlock::new(reader)?)?,
    }
    Ok(())
}

fn for_each_input(matches: &clap::ArgMatches, work: Command) -> Result<()> {
    let file = matches.value_of("file").unwrap();
    on_fs(file, work).chain_err(|| format!("while processing '{}'", file))
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Command {
    DumpLs,
    HeadAll { bytes: usize },
}

impl Command {
    fn exec<R: Read + Seek>(self, fs: SuperBlock<R>) -> Result<()> {
        match self {
            Command::DumpLs => dump_ls(fs),
            Command::HeadAll { bytes } => head_all(fs, bytes),
        }
    }
}

fn run() -> Result<()> {
    let paths_arg = Arg::with_name("file").required(true);

    let matches = App::new("ext4tool")
        .setting(clap::AppSettings::SubcommandRequiredElseHelp)
        .subcommand(SubCommand::with_name("dump-ls").arg(&paths_arg))
        .subcommand(
            SubCommand::with_name("head-all")
                .arg(
                    Arg::with_name("bytes")
                        .short("c")
                        .long("bytes")
                        .default_value("32")
                        .validator(|s| {
                            s.parse::<usize>()
                                .map(|_| ())
                                .map_err(|e| format!("invalid positive integer '{}': {}", s, e))
                        }),
                )
                .arg(&paths_arg),
        )
        .get_matches();

    match matches.subcommand() {
        ("dump-ls", Some(matches)) => for_each_input(matches, Command::DumpLs),
        ("head-all", Some(matches)) => for_each_input(
            matches,
            Command::HeadAll {
                bytes: matches.value_of("bytes").unwrap().parse::<usize>().unwrap(),
            },
        ),
        (_, _) => unreachable!(),
    }
}

quick_main!(run);
