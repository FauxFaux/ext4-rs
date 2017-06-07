extern crate clap;
#[macro_use] extern crate error_chain;
extern crate ext4;

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
    let root = fs.root()?;
    fs.walk(&root, "".to_string(), &mut |path, number, stat, enhanced| {
        println!("<{}> {}: {:?} {:?}", number, path, enhanced, stat);
        Ok(true)
    }).map(|_|())?; // we don't care about the returned "true"
    Ok(())
}

fn run() -> Result<()> {
    let matches = App::new("ext4tool")
        .setting(clap::AppSettings::SubcommandRequiredElseHelp)
        .subcommand(SubCommand::with_name("dump-ls")
        )
        .arg(Arg::with_name("file")
            .required(true)
        ).get_matches();

    let file = matches.value_of("file").unwrap();
    let mut reader = io::BufReader::new(fs::File::open(file)?);
    let superblock = ext4::SuperBlock::new(&mut reader)?;

    match matches.subcommand() {
        ("dump-ls", Some(_)) => {
            dump_ls(superblock).chain_err(|| format!("while processing '{}'", file))
        },
        (_, _) => unreachable!(),
    }
}

quick_main!(run);
