extern crate bootsector;
extern crate ext4;

use std::convert::TryFrom;
use std::fs;
use std::io;
use std::io::{Read, Seek, SeekFrom};
use std::path;

use anyhow::Result;
use tar::Archive;
use tempfile::NamedTempFile;
use positioned_io::ReadAt;

fn open_assets() -> Result<Archive<Box<dyn Read>>> {
    let tar = flate2::read::GzDecoder::new(io::Cursor::new(&include_bytes!("../scripts/generate-images/images.tgz")[..]));
    Ok(tar::Archive::new(Box::new(tar) as Box<dyn Read>))
}

#[test]
fn all_types() -> Result<()> {
    let mut files_successfully_processed = 0u64;

    for file in open_assets()?.entries()? {
        let mut entry = file?;
        let image_name = entry.header().path()?.to_string_lossy().to_string();
        if !image_name.contains("all-types") {
            continue;
        }

        let mut img = NamedTempFile::new()?;
        entry.unpack(&mut img)?;
        // io::copy(&mut entry, &mut img)?;
        img.seek(SeekFrom::Start(0))?;
        println!("{:?}", img.path());
        std::thread::sleep(std::time::Duration::new(100, 0));

        let partitions = bootsector::list_partitions(&mut img, &bootsector::Options::default()).unwrap();
        let mut img = ReadAtTempFile { inner: img };

        for part in partitions {
            match part.attributes {
                bootsector::Attributes::MBR { type_code, .. } => {
                    if 0x83 != type_code {
                        continue;
                    }
                }
                _ => panic!("unexpected partition table"),
            }

            let part_reader = positioned_io::Slice::new(&mut img, part.first_byte, Some(part.len));
            let superblock = ext4::SuperBlock::new(part_reader).unwrap();
            let root = superblock.root().unwrap();
            superblock
                .walk(&root, &image_name, &mut |fs, path, inode, enhanced| {
                    println!(
                        "<{}> {}: {:?} {:?}",
                        inode.number, path, enhanced, inode.stat
                    );
                    if ext4::FileType::RegularFile == inode.stat.extracted_type {
                        let expected_size = usize::try_from(inode.stat.size).unwrap();
                        let mut buf = Vec::with_capacity(expected_size);
                        fs.open(inode)?.read_to_end(&mut buf)?;
                        assert_eq!(expected_size, buf.len());
                    }

                    files_successfully_processed += 1;
                    Ok(true)
                })
                .unwrap();

            let path = superblock
                .resolve_path("/home/faux/hello.txt")
                .unwrap()
                .inode;
            let nice_node = superblock.load_inode(path).unwrap();
            let mut s = String::new();
            superblock
                .open(&nice_node)
                .unwrap()
                .read_to_string(&mut s)
                .unwrap();
            assert_eq!("Hello, world!\n", s);

            assert_eq!(
                11847456550,
                superblock
                    .load_inode(superblock.resolve_path("future-file").unwrap().inode)
                    .unwrap()
                    .stat
                    .mtime
                    .epoch_secs
            );
        }
    }

    assert_eq!(28 * 5, files_successfully_processed);

    Ok(())
}

struct ReadAtTempFile {
    inner: NamedTempFile,
}

impl ReadAt for ReadAtTempFile {
    fn read_at(&self, pos: u64, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.as_file().read_at(pos, buf)
    }
}
