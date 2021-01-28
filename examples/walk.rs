extern crate ext4;

use std::env;
use std::fs;

fn main() {
    let r = fs::File::open(env::args().nth(1).expect("one argument")).expect("openable file");
    let mut options = ext4::Options::default();
    options.checksums = ext4::Checksums::Enabled;
    let vol = ext4::SuperBlock::new_with_options(r, &options).expect("ext4 volume");
    let root = vol.root().expect("root");
    vol.walk(&root, "/", &mut |_, path, _, _| {
        println!("{}", path);
        Ok(true)
    })
    .expect("walk");
}
