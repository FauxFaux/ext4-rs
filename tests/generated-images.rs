extern crate bootsector;
extern crate ext4;

use std::convert::TryFrom;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::PathBuf;
use std::process::Stdio;

use anyhow::Result;
use tempfile::TempDir;

fn calculate_position_after_seek(
    position: SeekFrom,
    current_offset: u64,
    total_size: u64,
) -> io::Result<u64> {
    let new_offset = match position {
        SeekFrom::Current(offset) => current_offset
            .checked_add_signed(offset)
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Numeric overflow"))?,
        SeekFrom::End(offset) => total_size
            .checked_add_signed(offset)
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Numeric overflow"))?,
        SeekFrom::Start(offset) => offset,
    };

    if new_offset > total_size {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "Out of sub-stream bounds",
        ));
    }

    Ok(new_offset)
}

pub struct StreamSlice<T> {
    base_stream: T,
    start_offset: u64,
    size: u64,
    current_offset: u64,
}

impl<T> StreamSlice<T>
where
    T: Seek,
{
    pub fn new(base_stream: T, start_offset: u64, size: u64) -> Result<StreamSlice<T>> {
        let mut slice = StreamSlice {
            base_stream,
            start_offset,
            size,
            current_offset: 0,
        };
        slice.seek(SeekFrom::Start(0))?;

        Ok(slice)
    }
}

impl<T> Seek for StreamSlice<T>
where
    T: Seek,
{
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.current_offset = calculate_position_after_seek(pos, self.current_offset, self.size)?;
        self.base_stream
            .seek(SeekFrom::Start(self.start_offset + self.current_offset))?;
        Ok(self.current_offset)
    }
}

impl<T> Read for StreamSlice<T>
where
    T: Seek + Read,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.current_offset > self.size {
            return Err(io::Error::new(io::ErrorKind::Other, "End of stream"));
        }
        let size_to_read = std::cmp::min((self.size - self.current_offset) as usize, buf.len());

        let size = self.base_stream.read(&mut buf[..size_to_read])?;
        self.current_offset += size as u64;
        Ok(size)
    }
}

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

            let part_reader = StreamSlice::new(&mut img, part.first_byte, part.len)?;
            let mut superblock = ext4::SuperBlock::new(part_reader).unwrap();
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

            let future_file_inode = superblock.resolve_path("future-file").unwrap().inode;
            assert_eq!(
                11847456550,
                superblock
                    .load_inode(future_file_inode)
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
