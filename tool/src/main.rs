extern crate clap;
extern crate ext4;

use std::fs;
use std::io;

use clap::{App, Arg, SubCommand};

fn dump_ls(file: &str) {
    let mut reader = io::BufReader::new(fs::File::open(file).expect("input file"));

    let mut superblock = ext4::SuperBlock::new(&mut reader).unwrap();
    let root = superblock.root().unwrap();
    superblock.walk(&root, file.to_string(), &mut |path, number, stat, enhanced| {
        println!("<{}> {}: {:?} {:?}", number, path, enhanced, stat);
        Ok(true)
    }).unwrap();
}

fn main() {
    match App::new("ext4tool")
        .setting(clap::AppSettings::SubcommandRequiredElseHelp)
        .subcommand(SubCommand::with_name("dump-ls")
            .arg(Arg::with_name("file")
                .required(true))
        ).get_matches().subcommand() {

        ("dump-ls", Some(matches)) => {
            let file = matches.value_of("file").unwrap();
            dump_ls(file);
        },
        (_, _) => unreachable!(),
    }
}
