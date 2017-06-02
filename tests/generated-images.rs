extern crate ext4;

use std::fs;
use std::io;
use std::path;

#[test]
fn all_types() {
    for file in path::Path::new("tests/generated").read_dir().unwrap() {
        let file = file.unwrap();
        let image_name = file.file_name().into_string().unwrap();
        if !image_name.starts_with("all-types") {
            continue;
        }

        let mut img = io::BufReader::new(fs::File::open(file.path()).unwrap());
        for part in ext4::mbr::read_partition_table(&mut img).unwrap() {
            if 0x83 != part.type_code {
                continue;
            }

            let mut part_reader = ext4::mbr::read_partition(&mut img, &part).unwrap();
            let superblock = ext4::SuperBlock::load(&mut part_reader).unwrap();
            let root = superblock.root(&mut part_reader).unwrap();
            superblock.walk(&mut part_reader, &root, image_name.to_string()).unwrap();
        }
    }
}
