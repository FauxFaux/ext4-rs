extern crate clap;
#[macro_use] extern crate error_chain;
extern crate ext4;
extern crate hexdump;

use std::fs;
use std::io;

use std::io::{Read, Seek};

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
where R: Read + Seek {
    let root = &fs.root()?;
    fs.walk(root, "".to_string(), &mut |_, path, inode, enhanced| {
        println!("<{}> {}: {:?} {:?}", inode.number, path, enhanced, inode.stat);
        Ok(true)
    }).map(|_|())?; // we don't care about the returned "true"
    Ok(())
}

fn head_all<R>(mut fs: SuperBlock<R>, bytes: usize) -> Result<()>
where R: Read + Seek {
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
    }).map(|_|())?; // we don't care about the returned "true"
    Ok(())
}


fn for_each_input<F>(matches: &clap::ArgMatches, work: F) -> Result<()>
where F: Fn(&clap::ArgMatches, SuperBlock<&mut std::io::BufReader<std::fs::File>>) -> Result<()> {
    let file = matches.value_of("file").unwrap();
    let mut reader = io::BufReader::new(fs::File::open(file)?);
    let superblock = ext4::SuperBlock::new(&mut reader)?;
    work(matches, superblock).chain_err(|| format!("while processing '{}'", file))
}

fn run() -> Result<()> {
    let paths_arg = Arg::with_name("file")
        .required(true);

    let matches = App::new("ext4tool")
        .setting(clap::AppSettings::SubcommandRequiredElseHelp)
        .subcommand(SubCommand::with_name("dump-ls")
            .arg(&paths_arg)
        )
        .subcommand(SubCommand::with_name("head-all")
            .arg(Arg::with_name("bytes")
                .short("c")
                .long("bytes")
                .default_value("32")
                .validator(|s| s.parse::<usize>()
                    .map(|_|())
                    .map_err(|e| format!("invalid positive integer '{}': {}", s, e)))
            )
            .arg(&paths_arg)
        ).get_matches();

    match matches.subcommand() {
        ("dump-ls", Some(matches)) => {
            for_each_input(matches, |_, fs| dump_ls(fs))
        },
        ("head-all", Some(matches)) => {
            for_each_input(matches, |matches, fs| {
                let bytes = matches.value_of("bytes").unwrap().parse::<usize>().unwrap();
                head_all(fs, bytes)
            })
        }
        (_, _) => unreachable!(),
    }
}

quick_main!(run);
