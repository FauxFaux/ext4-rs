/*!
This crate can load ext4 filesystems, letting you read metadata
and files from them.

# Example

```rust,no_run
let mut block_device = std::io::BufReader::new(std::fs::File::open("/dev/sda1").unwrap());
let mut superblock = ext4::SuperBlock::new(&mut block_device).unwrap();
let target_inode_number = superblock.resolve_path("/etc/passwd").unwrap().inode;
let inode = superblock.load_inode(target_inode_number).unwrap();
let passwd_reader = superblock.open(&inode).unwrap();
```

Note: normal users can't read /dev/sda by default, as it would allow them to read any
file on the filesystem. You can grant yourself temporary access with
`sudo setfacl -m u:${USER}:r /dev/sda1`, if you so fancy. This will be lost at reboot.
*/

#[macro_use] extern crate bitflags;
extern crate byteorder;
extern crate crc;
#[macro_use] extern crate error_chain;

use std::io;

use byteorder::{ReadBytesExt, LittleEndian};

use std::collections::HashMap;
use std::io::Read;
use std::io::Seek;

mod block_groups;
mod extents;
mod parse;

pub mod mbr;

use extents::TreeReader;

mod errors {
    error_chain! {
        errors {
            /// The filesystem doesn't meet the code's expectations;
            /// maybe the code is wrong, maybe the filesystem is corrupt.
            AssumptionFailed(t: String) {
                description("programming error")
                display("assumption failed: {}", t)
            }
            // The filesystem is valid, but requests a feature the code doesn't support.
            UnsupportedFeature(t: String) {
                description("filesystem uses an unsupported feature")
                display("unsupported feature: {}", t)
            }
            // The request is for something which we are sure is not there.
            NotFound(t: String) {
                description("asked for something that we are sure does not exist")
                display("not found: {}", t)
            }
        }

        foreign_links {
            Io(::std::io::Error);
        }
    }
}

pub use errors::*;
use errors::ErrorKind::*;

bitflags! {
    pub struct InodeFlags: u32 {
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

/// Flag indicating the type of file stored in this inode.
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

/// Extended, type-specific information read from an inode.
#[derive(Debug)]
pub enum Enhanced {
    RegularFile,
    /// A symlink, with its decoded destination.
    SymbolicLink(String),
    /// A 'c' device, with its major and minor numbers.
    CharacterDevice(u16, u32),
    /// A 'b' device, with its major and minor numbers.
    BlockDevice(u16, u32),
    /// A directory, with its listing.
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

/// An entry in a directory, without its extra metadata.
#[derive(Debug)]
pub struct DirEntry {
    pub inode: u32,
    pub file_type: FileType,
    pub name: String,
}

/// Full information about a disc entry.
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
    pub xattrs: HashMap<String, Vec<u8>>,
}

const INODE_CORE_SIZE: usize = 4 * 15;

/// An actual disc metadata entry.
pub struct Inode {
    pub stat: Stat,
    pub number: u32,
    flags: InodeFlags,

    /// The other implementations call this the inode's "block", which is so unbelievably overloaded.
    /// I made up a new name.
    core: [u8; INODE_CORE_SIZE],
    block_size: u32,
}

/// The critical core of the filesystem.
#[derive(Debug)]
pub struct SuperBlock<R> {
    inner: R,
    load_xattrs: bool,
    /// All* checksums are computed after concatenation with the UUID, so we keep that.
    uuid_checksum: Option<u32>,
    groups: block_groups::BlockGroups,
}

/// A raw filesystem time.
#[derive(Debug)]
pub struct Time {
    pub epoch_secs: u32,
    pub nanos: Option<u32>,
}

impl<R> SuperBlock<R>
where R: io::Read + io::Seek {

    /// Open a filesystem, and load its superblock.
    pub fn new(mut inner: R) -> Result<SuperBlock<R>> {
        inner.seek(io::SeekFrom::Start(1024))?;
        parse::superblock(inner).chain_err(|| "failed to parse superblock")
    }

    /// Load a filesystem entry by inode number.
    pub fn load_inode(&mut self, inode: u32) -> Result<Inode> {
        let data = self.load_inode_bytes(inode)
            .chain_err(|| format!("failed to find inode <{}> on disc", inode))?;

        let parsed = parse::inode(&data, |block| self.load_disc_bytes(block))
            .chain_err(|| format!("failed to parse inode <{}>", inode))?;

        Ok(Inode {
            number: inode,
            stat: parsed.stat,
            flags: parsed.flags,
            core: parsed.core,
            block_size: self.groups.block_size,
        })
    }

    fn load_inode_bytes(&mut self, inode: u32) -> Result<Vec<u8>> {
        self.inner.seek(io::SeekFrom::Start(self.groups.index_of(inode)?))?;
        let mut data = vec![0u8; self.groups.inode_size as usize];
        self.inner.read_exact(&mut data)?;
        Ok(data)
    }

    fn load_disc_bytes(&mut self, block: u32) -> Result<Vec<u8>> {
        self.inner.seek(io::SeekFrom::Start(block as u64 * self.groups.block_size as u64))?;
        let mut data = vec![0u8; self.groups.block_size as usize];
        self.inner.read_exact(&mut data)?;
        Ok(data)
    }

    /// Load the root node of the filesystem (typically `/`).
    pub fn root(&mut self) -> Result<Inode> {
        self.load_inode(2).chain_err(|| "failed to load root inode")
    }

    /// Visit every entry in the filesystem in an arbitrary order.
    /// The closure should return `true` if it wants walking to continue.
    /// The method returns `true` if the closure always returned true.
    pub fn walk<F>(&mut self, inode: &Inode, path: String, visit: &mut F) -> Result<bool>
    where F: FnMut(&mut Self, &str, &Inode, &Enhanced) -> Result<bool> {
        let enhanced = inode.enhance(&mut self.inner)?;

        if !visit(self, path.as_str(), inode, &enhanced)
                .chain_err(|| "user closure failed")? {
            return Ok(false);
        }

        if let Enhanced::Directory(entries) = enhanced {
            for entry in entries {
                if "." == entry.name || ".." == entry.name {
                    continue;
                }

                let child_node = self.load_inode(entry.inode)
                    .chain_err(|| format!("loading {} ({:?})", entry.name, entry.file_type))?;
                if !self.walk(&child_node, format!("{}/{}", path, entry.name), visit)
                        .chain_err(|| format!("processing '{}'", entry.name))? {
                    return Ok(false)
                }
            }
        }

//    self.walk(inner, &i, format!("{}/{}", path, entry.name)).map_err(|e|
//    parse_error(format!("while processing {}: {}", path, e)))?;

        Ok(true)
    }

    /// Parse a path, and find the directory entry it represents.
    /// Note that "/foo/../bar" will be treated literally, not resolved to "/bar" then looked up.
    pub fn resolve_path(&mut self, path: &str) -> Result<DirEntry> {
        let path = path.trim_right_matches('/');
        if path.is_empty() {
            // this is a bit of a lie, but it works..?
            return Ok(DirEntry {
                inode: 2,
                file_type: FileType::Directory,
                name: "/".to_string(),
            });
        }

        let mut curr = self.root()?;

        let mut parts = path.split('/').collect::<Vec<&str>>();
        let last = parts.pop().unwrap();
        for part in parts {
            if part.is_empty() {
                continue;
            }

            let child_inode = self.dir_entry_named(&curr, part)?.inode;
            curr = self.load_inode(child_inode)?;
        }

        self.dir_entry_named(&curr, last)
    }

    fn dir_entry_named(&mut self, inode: &Inode, name: &str) -> Result<DirEntry> {
        if let Enhanced::Directory(entries) = self.enhance(inode)? {
            if let Some(en) = entries.into_iter().find(|entry| entry.name == name) {
                Ok(en)
            } else {
                Err(NotFound(format!("component {} isn't there", name)).into())
            }
        } else {
            Err(NotFound(format!("component {} isn't a directory", name)).into())
        }
    }

    /// Read the data from an inode. You might not want to call this on thigns that aren't regular files.
    pub fn open(&mut self, inode: &Inode) -> Result<TreeReader<&mut R>> {
        inode.reader(&mut self.inner)
    }

    /// Load extra metadata about some types of entries.
    pub fn enhance(&mut self, inode: &Inode) -> Result<Enhanced> {
        inode.enhance(&mut self.inner)
    }
}

impl Inode {

    fn reader<R>(&self, inner: R) -> Result<TreeReader<R>>
    where R: io::Read + io::Seek {
        TreeReader::new(inner, self.block_size, self.stat.size, self.core)
            .chain_err(|| format!("opening inode <{}>", self.number))
    }

    fn enhance<R>(&self, inner: R) -> Result<Enhanced>
    where R: io::Read + io::Seek {
        Ok(match self.stat.extracted_type {
            FileType::RegularFile => Enhanced::RegularFile,
            FileType::Socket => Enhanced::Socket,
            FileType::Fifo => Enhanced::Fifo,

            FileType::Directory => Enhanced::Directory(self.read_directory(inner)?),
            FileType::SymbolicLink =>
                Enhanced::SymbolicLink(if self.stat.size < INODE_CORE_SIZE as u64 {
                    ensure!(self.flags.is_empty(),
                        UnsupportedFeature(format!("symbolic links may not have flags: {:?}", self.flags)));
                    std::str::from_utf8(&self.core[0..self.stat.size as usize]).expect("utf-8").to_string()
                } else {
                    ensure!(self.only_relevant_flag_is_extents(),
                        UnsupportedFeature(format!("symbolic links may not have non-extent flags: {:?}", self.flags)));
                    std::str::from_utf8(&self.load_all(inner)?).expect("utf-8").to_string()
                }),
            FileType::CharacterDevice => {
                let (maj, min) = load_maj_min(self.core);
                Enhanced::CharacterDevice(maj, min)
            }
            FileType::BlockDevice => {
                let (maj, min) = load_maj_min(self.core);
                Enhanced::BlockDevice(maj, min)
            }
        })
    }

    fn load_all<R>(&self, inner: R) -> Result<Vec<u8>>
    where R: io::Read + io::Seek {
        let size = usize_check(self.stat.size)?;
        let mut ret = vec![0u8; size];

        self.reader(inner)?.read_exact(&mut ret)?;

        Ok(ret)
    }

    fn read_directory<R>(&self, inner: R) -> Result<Vec<DirEntry>>
    where R: io::Read + io::Seek {

        let mut dirs = Vec::with_capacity(40);

        let data = {
            // if the flags, minus irrelevant flags, isn't just EXTENTS...
            ensure!(self.only_relevant_flag_is_extents(),
                UnsupportedFeature(format!("inode with unsupported flags: {0:x} {0:b}", self.flags)));

            self.load_all(inner)?
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
                        .ok_or_else(|| UnsupportedFeature(format!("unexpected file type in directory: {}", file_type)))?,
                });
            }

            read += rec_len as usize;
            if read >= total_len {
                ensure!(read == total_len,
                    AssumptionFailed(format!("short read, {} != {}", read, total_len)));
                break;
            }
        }

        Ok(dirs)
    }

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

fn load_maj_min(core: [u8; INODE_CORE_SIZE]) -> (u16, u32) {
    if 0 != core[0] || 0 != core[1] {
        (core[1] as u16, core[0] as u32)
    } else {
        // if you think reading this is bad, I had to write it
        (core[5] as u16
             | (((core[6] & 0b0000_1111) as u16) << 8),
         core[4] as u32
             | ((core[7] as u32) << 12)
             | (((core[6] & 0b1111_0000) as u32) >> 4) << 8)
    }
}

fn as_u16(buf: &[u8]) -> u16 {
    buf[0] as u16 + buf[1] as u16 * 0x100
}

fn as_u32(buf: &[u8]) -> u32 {
    as_u16(buf) as u32 + as_u16(&buf[2..]) as u32 * 0x10000
}

fn parse_error(msg: String) -> Error {
    AssumptionFailed(msg).into()
}

#[allow(unknown_lints, absurd_extreme_comparisons)]
pub fn usize_check(val: u64) -> Result<usize> {
    // this check only makes sense on non-64-bit platforms; on 64-bit usize == u64.
    ensure!(val <= std::usize::MAX as u64,
        AssumptionFailed(format!("value is too big for memory on this platform: {}", val)));

    Ok(val as usize)
}
