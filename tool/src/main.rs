extern crate clap;
#[macro_use] extern crate error_chain;
extern crate ext4;

use std::fs;
use std::io;

use clap::{App, Arg, SubCommand};

mod errors {
    error_chain! {
        links {
            Ext4(::ext4::Error, ::ext4::ErrorKind);
        }
    }
}

use errors::*;

fn dump_ls(file: &str) -> Result<()> {
    let mut reader = io::BufReader::new(fs::File::open(file).expect("input file"));

    let mut superblock = ext4::SuperBlock::new(&mut reader)?;
    let root = superblock.root()?;
    superblock.walk(&root, file.to_string(), &mut |path, number, stat, enhanced| {
        println!("<{}> {}: {:?} {:?}", number, path, enhanced, stat);
        Ok(true)
    }).map(|_|())?; // we don't care about the returned "true"
    Ok(())
}

fn run() -> Result<()> {
    match App::new("ext4tool")
        .setting(clap::AppSettings::SubcommandRequiredElseHelp)
        .subcommand(SubCommand::with_name("dump-ls")
            .arg(Arg::with_name("file")
                .required(true))
        ).get_matches().subcommand() {

        ("dump-ls", Some(matches)) => {
            let file = matches.value_of("file").unwrap();
            dump_ls(file).chain_err(|| format!("while processing '{}'", file))
        },
        (_, _) => unreachable!(),
    }
}

quick_main!(run);
