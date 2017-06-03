extern crate ext4;

use std::fs;
use std::io;
use std::path;

use std::io::Read;

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
            let mut superblock = ext4::SuperBlock::new(&mut part_reader).unwrap();
            let root = superblock.root().unwrap();
            superblock.walk(&root, image_name.to_string(), &|path, number, stat, enhanced| {
                println!("<{}> {}: {:?} {:?}", number, path, enhanced, stat);
                Ok(true)
            }).unwrap();

            let path = superblock.resolve_path("/home/faux/hello.txt").unwrap().inode;
            let nice_node = superblock.load_inode(path).unwrap();
            let mut s = String::new();
            superblock.open(&nice_node).unwrap().read_to_string(&mut s).unwrap();
            println!("{}", s);
        }
    }
}
