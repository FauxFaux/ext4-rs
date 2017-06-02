#[macro_use] extern crate bitflags;
extern crate byteorder;

use std::io;

use byteorder::{ReadBytesExt, LittleEndian};

use std::io::Read;
use std::io::Seek;

mod block_groups;
mod extents;
mod parse;

pub mod mbr;

use extents::TreeReader;


bitflags! {
    struct InodeFlags: u32 {
        const INODE_SECRM        = 0x00000001; /* Secure deletion */
        const INODE_UNRM         = 0x00000002; /* Undelete */
        const INODE_COMPR        = 0x00000004; /* Compress file */
        const INODE_SYNC         = 0x00000008; /* Synchronous updates */
        const INODE_IMMUTABLE    = 0x00000010; /* Immutable file */
        const INODE_APPEND       = 0x00000020; /* writes to file may only append */
        const INODE_NODUMP       = 0x00000040; /* do not dump file */
        const INODE_NOATIME      = 0x00000080; /* do not update atime */
        const INODE_DIRTY        = 0x00000100; /* reserved for compression */
        const INODE_COMPRBLK     = 0x00000200; /* One or more compressed clusters */
        const INODE_NOCOMPR      = 0x00000400; /* Don't compress */
        const INODE_ENCRYPT      = 0x00000800; /* encrypted file */
        const INODE_INDEX        = 0x00001000; /* hash-indexed directory */
        const INODE_IMAGIC       = 0x00002000; /* AFS directory */
        const INODE_JOURNAL_DATA = 0x00004000; /* file data should be journaled */
        const INODE_NOTAIL       = 0x00008000; /* file tail should not be merged */
        const INODE_DIRSYNC      = 0x00010000; /* dirsync behaviour (directories only) */
        const INODE_TOPDIR       = 0x00020000; /* Top of directory hierarchies*/
        const INODE_HUGE_FILE    = 0x00040000; /* Set to each huge file */
        const INODE_EXTENTS      = 0x00080000; /* Inode uses extents */
        const INODE_EA_INODE     = 0x00200000; /* Inode used for large EA */
        const INODE_EOFBLOCKS    = 0x00400000; /* Blocks allocated beyond EOF */
        const INODE_INLINE_DATA  = 0x10000000; /* Inode has inline data. */
        const INODE_PROJINHERIT  = 0x20000000; /* Create with parents projid */
        const INODE_RESERVED     = 0x80000000; /* reserved for ext4 lib */
    }
}

#[derive(Debug, PartialEq)]
pub enum FileType {
    RegularFile,     // S_IFREG (Regular file)
    SymbolicLink,    // S_IFLNK (Symbolic link)
    CharacterDevice, // S_IFCHR (Character device)
    BlockDevice,     // S_IFBLK (Block device)
    Directory,       // S_IFDIR (Directory)
    Fifo,            // S_IFIFO (FIFO)
    Socket,          // S_IFSOCK (Socket)
}

#[derive(Debug)]
pub enum Enhanced {
    RegularFile,
    SymbolicLink(String),
    CharacterDevice(u16, u32),
    BlockDevice(u16, u32),
    Directory(Vec<DirEntry>),
    Fifo,
    Socket,
}

impl FileType {
    fn from_mode(mode: u16) -> Option<FileType> {
        match mode >> 12 {
            0x1 => Some(FileType::Fifo),
            0x2 => Some(FileType::CharacterDevice),
            0x4 => Some(FileType::Directory),
            0x6 => Some(FileType::BlockDevice),
            0x8 => Some(FileType::RegularFile),
            0xA => Some(FileType::SymbolicLink),
            0xC => Some(FileType::Socket),
            _ => None,
        }
    }

    fn from_dir_hint(hint: u8) -> Option<FileType> {
        match hint {
            1 => Some(FileType::RegularFile),
            2 => Some(FileType::Directory),
            3 => Some(FileType::CharacterDevice),
            4 => Some(FileType::BlockDevice),
            5 => Some(FileType::Fifo),
            6 => Some(FileType::Socket),
            7 => Some(FileType::SymbolicLink),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct DirEntry {
    pub inode: u32,
    pub file_type: FileType,
    pub name: String,
}

#[derive(Debug)]
pub struct Stat {
    pub extracted_type: FileType,
    pub file_mode: u16,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub atime: Time,
    pub ctime: Time,
    pub mtime: Time,
    pub btime: Option<Time>,
    pub link_count: u16,
}

pub struct Inode {
    pub stat: Stat,
    pub number: u32,
    flags: InodeFlags,
    block: [u8; 4 * 15],
}

#[derive(Debug)]
pub struct SuperBlock {
    block_size: u32,
    groups: block_groups::BlockGroups,
}

#[derive(Debug)]
pub struct Time {
    pub epoch_secs: u32,
    pub nanos: Option<u32>,
}

impl SuperBlock {
    pub fn load<R>(inner: &mut R) -> io::Result<SuperBlock>
    where R: io::Read + io::Seek {
        inner.seek(io::SeekFrom::Start(1024))?;
        parse::superblock(inner)
    }

    fn load_inode<R>(&self, inner: &mut R, inode: u32) -> io::Result<Inode>
    where R: io::Read + io::Seek {
        inner.seek(io::SeekFrom::Start(self.groups.index_of(inode)))?;
        parse::inode(inner, inode)
    }

    fn read_directory<R>(&self, inner: &mut R, inode: &Inode) -> io::Result<Vec<DirEntry>>
    where R: io::Read + io::Seek {

        let mut dirs = Vec::with_capacity(40);

        let data = {
            // if the flags, minus irrelevant flags, isn't just EXTENTS...
            if !inode.only_relevant_flag_is_extents() {
                return Err(parse_error(format!("inode without unsupported flags: {0:x} {0:b}", inode.flags)));
            }

            self.load_all(inner, inode)?
        };

        let total_len = data.len();

        let mut cursor = io::Cursor::new(data);
        let mut read = 0usize;
        loop {
            let child_inode = cursor.read_u32::<LittleEndian>()?;
            let rec_len = cursor.read_u16::<LittleEndian>()?;
            let name_len = cursor.read_u8()?;
            let file_type = cursor.read_u8()?;
            let mut name = vec![0u8; name_len as usize];
            cursor.read_exact(&mut name)?;
            cursor.seek(io::SeekFrom::Current(rec_len as i64 - name_len as i64 - 4 - 2 - 2))?;
            if 0 != child_inode {
                let name = std::str::from_utf8(&name).map_err(|e|
                    parse_error(format!("invalid utf-8 in file name: {}", e)))?;

                dirs.push(DirEntry {
                    inode: child_inode,
                    name: name.to_string(),
                    file_type: FileType::from_dir_hint(file_type)
                        .expect("valid file type"),
                });
            }

            read += rec_len as usize;
            if read >= total_len {
                assert_eq!(read, total_len);
                break;
            }
        }

        Ok(dirs)
    }

    pub fn root<R>(&self, mut inner: &mut R) -> io::Result<Inode>
        where R: io::Read + io::Seek {
        self.load_inode(inner, 2)
    }

    pub fn walk<R>(&self, mut inner: &mut R, inode: &Inode, path: String) -> io::Result<()>
        where R: io::Read + io::Seek {
        let enhanced = self.enhance(inner, inode)?;

        println!("{}: {:?} {:?}", path, enhanced, inode.stat);

        if let Enhanced::Directory(entries) = enhanced {
            for entry in entries {
                if "." == entry.name || ".." == entry.name {
                    continue;
                }

                let child_node = self.load_inode(inner, entry.inode)?;
                self.walk(inner, &child_node, format!("{}/{}", path, entry.name))?;
            }
        }

//    self.walk(inner, &i, format!("{}/{}", path, entry.name)).map_err(|e|
//    parse_error(format!("while processing {}: {}", path, e)))?;

        Ok(())
    }

    pub fn enhance<R>(&self, mut inner: &mut R, inode: &Inode) -> io::Result<Enhanced>
        where R: io::Read + io::Seek {
        Ok(match inode.stat.extracted_type {
            FileType::RegularFile => Enhanced::RegularFile,
            FileType::Socket => Enhanced::Socket,
            FileType::Fifo => Enhanced::Fifo,

            FileType::Directory => Enhanced::Directory(self.read_directory(inner, inode)?),
            FileType::SymbolicLink =>
                Enhanced::SymbolicLink(if inode.stat.size < 60 {
                    assert!(inode.flags.is_empty());
                    std::str::from_utf8(&inode.block[0..inode.stat.size as usize]).expect("utf-8").to_string()
                } else {
                    assert!(inode.only_relevant_flag_is_extents());
                    std::str::from_utf8(&self.load_all(inner, inode)?).expect("utf-8").to_string()
                }),
            FileType::CharacterDevice => {
                let (maj, min) = load_maj_min(inode.block);
                Enhanced::CharacterDevice(maj, min)
            }
            FileType::BlockDevice => {
                let (maj, min) = load_maj_min(inode.block);
                Enhanced::BlockDevice(maj, min)
            }
        })
    }

    fn load_all<R>(&self, inner: &mut R, inode: &Inode) -> io::Result<Vec<u8>>
    where R: io::Read + io::Seek {
        let size = usize_check(inode.stat.size)?;
        let mut ret = Vec::with_capacity(size);

        assert_eq!(size, self.reader_for(inner, inode)?.read_to_end(&mut ret)?);

        Ok(ret)
    }


    pub fn reader_for<R>(&self, inner: R, inode: &Inode) -> io::Result<TreeReader<R>>
    where R: io::Read + io::Seek {
        TreeReader::new(inner, self.block_size, inode.block)
    }
}

fn load_maj_min(block: [u8; 4 * 15]) -> (u16, u32) {
    if 0 != block[0] || 0 != block[1] {
        (block[1] as u16, block[0] as u32)
    } else {
        // if you think reading this is bad, I had to write it
        (block[5] as u16
            | (((block[6] & 0b0000_1111) as u16) << 8),
        block[4] as u32
            | ((block[7] as u32) << 12)
            | (((block[6] & 0b1111_0000) as u32) >> 4) << 8)
    }
}

impl Inode {
    fn only_relevant_flag_is_extents(&self) -> bool {
        self.flags & (
            INODE_COMPR
            | INODE_DIRTY
            | INODE_COMPRBLK
            | INODE_ENCRYPT
            | INODE_IMAGIC
            | INODE_NOTAIL
            | INODE_TOPDIR
            | INODE_HUGE_FILE
            | INODE_EXTENTS
            | INODE_EA_INODE
            | INODE_EOFBLOCKS
            | INODE_INLINE_DATA
        ) == INODE_EXTENTS
    }
}

fn as_u16(buf: &[u8]) -> u16 {
    buf[0] as u16 + buf[1] as u16 * 0x100
}

fn as_u32(buf: &[u8]) -> u32 {
    as_u16(buf) as u32 + as_u16(&buf[2..]) as u32 * 0x10000
}

fn parse_error(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg)
}

#[allow(unknown_lints, absurd_extreme_comparisons)]
fn usize_check(val: u64) -> io::Result<usize> {
    // this check only makes sense on non-64-bit platforms; on 64-bit usize == u64.
    if val > std::usize::MAX as u64 {
        Err(io::Error::new(io::ErrorKind::InvalidData,
                                  format!("value is too big for memory on this platform: {}",
                                          val)))
    } else {
        Ok(val as usize)
    }
}
