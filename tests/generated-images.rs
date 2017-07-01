extern crate bootsector;
extern crate ext4;

use std::fs;
use std::io;
use std::path;

use std::io::Read;

#[test]
fn all_types() {
    let mut files_successfully_processed = 0u64;
    for file in path::Path::new("tests/generated").read_dir().unwrap() {
        let file = file.unwrap();
        let image_name = file.file_name().into_string().unwrap();
        if !image_name.starts_with("all-types") {
            continue;
        }

        let mut img = io::BufReader::new(fs::File::open(file.path()).unwrap());
        for part in bootsector::list_partitions(&mut img, &bootsector::Options::default()).unwrap() {
            match part.attributes {
                bootsector::Attributes::MBR {
                    type_code,
                    ..
                } => {
                    if 0x83 != type_code {
                        continue;
                    }
                }
                _ => panic!("unexpected partition table"),
            }

            let mut part_reader = bootsector::open_partition(&mut img, &part).unwrap();
            let mut superblock = ext4::SuperBlock::new(&mut part_reader).unwrap();
            let root = superblock.root().unwrap();
            superblock.walk(&root, image_name.to_string(), &mut |fs, path, inode, enhanced| {
                println!("<{}> {}: {:?} {:?}", inode.number, path, enhanced, inode.stat);
                if ext4::FileType::RegularFile == inode.stat.extracted_type {
                    let expected_size = ext4::usize_check(inode.stat.size).unwrap();
                    let mut buf = Vec::with_capacity(expected_size);
                    fs.open(inode)?.read_to_end(&mut buf)?;
                    assert_eq!(expected_size, buf.len());
                }

                files_successfully_processed += 1;
                Ok(true)
            }).unwrap();

            let path = superblock.resolve_path("/home/faux/hello.txt").unwrap().inode;
            let nice_node = superblock.load_inode(path).unwrap();
            let mut s = String::new();
            superblock.open(&nice_node).unwrap().read_to_string(&mut s).unwrap();
            assert_eq!("Hello, world!\n", s);

        }
    }

    assert_eq!(25 * 5, files_successfully_processed);
}
