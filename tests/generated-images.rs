extern crate bootsector;
extern crate ext4;

use std::convert::TryFrom;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::io::Read;
use std::path::PathBuf;
use std::process::Stdio;

use anyhow::Result;
use positioned_io2::ReadAt;
use tempfile::NamedTempFile;
use tempfile::TempDir;

#[test]
fn all_types() -> Result<()> {
    let mut files_successfully_processed = 0u64;

    for image_name in open_assets()?.entries()? {
        let mut img = fs::File::open(image_name)?;

        let partitions =
            bootsector::list_partitions(&mut img, &bootsector::Options::default()).unwrap();

        for part in partitions {
            match part.attributes {
                bootsector::Attributes::MBR { type_code, .. } => {
                    if 0x83 != type_code {
                        continue;
                    }
                }
                _ => panic!("unexpected partition table"),
            }

            let part_reader = positioned_io2::Slice::new(&mut img, part.first_byte, Some(part.len));
            let superblock = ext4::SuperBlock::new(part_reader).unwrap();
            let root = superblock.root().unwrap();
            superblock
                .walk(&root, "", &mut |fs, path, inode, enhanced| {
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

struct Assets {
    tempdir: TempDir,
}

fn open_assets() -> Result<Assets> {
    let tempdir = TempDir::new()?;
    let mut tar = std::process::Command::new("tar")
        .args(&[
            OsStr::new("-C"),
            tempdir.path().as_os_str(),
            OsStr::new("-xz"),
        ])
        .stdin(Stdio::piped())
        .spawn()?;

    io::copy(
        &mut io::Cursor::new(&include_bytes!("../scripts/generate-images/images.tgz")[..]),
        &mut tar.stdin.as_mut().expect("configured above"),
    )?;

    assert!(tar.wait()?.success());

    Ok(Assets { tempdir })
}

impl Assets {
    fn entries(&self) -> Result<Vec<PathBuf>> {
        fs::read_dir(self.tempdir.path())?
            .map(|e| -> Result<PathBuf> { Ok(e?.path()) })
            .collect()
    }
}
