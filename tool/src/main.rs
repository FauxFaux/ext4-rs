extern crate bootsector;
extern crate cast;
extern crate clap;
extern crate ext4;
#[macro_use]
extern crate failure;
extern crate hexdump;

use std::fs;
use std::io;
use std::io::Read;
use std::io::Seek;

use cast::u64;
use cast::usize;
use clap::{App, Arg, SubCommand};
use ext4::SuperBlock;
use failure::Error;
use failure::ResultExt;

fn dump_ls<R>(mut fs: SuperBlock<R>) -> Result<(), Error>
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

fn head_all<R>(mut fs: SuperBlock<R>, bytes: usize) -> Result<(), Error>
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
        let to_read = usize(std::cmp::min(inode.stat.size, u64(bytes)));
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

fn on_fs(file: &str, work: Command) -> Result<(), Error> {
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

fn for_each_input(matches: &clap::ArgMatches, work: Command) -> Result<(), Error> {
    let file = matches.value_of("file").unwrap();
    Ok(on_fs(file, work).with_context(|_| format_err!("while processing '{}'", file))?)
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Command {
    DumpLs,
    HeadAll { bytes: usize },
}

impl Command {
    fn exec<R: Read + Seek>(self, fs: SuperBlock<R>) -> Result<(), Error> {
        match self {
            Command::DumpLs => dump_ls(fs),
            Command::HeadAll { bytes } => head_all(fs, bytes),
        }
    }
}

fn main() -> Result<(), Error> {
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
