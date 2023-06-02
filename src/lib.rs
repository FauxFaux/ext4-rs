/*!
This crate can load ext4 filesystems, letting you read metadata
and files from them.

# Example

```rust,no_run
let mut block_device = std::fs::File::open("/dev/sda1").unwrap();
let mut superblock = ext4::SuperBlock::new(&mut block_device).unwrap();
let target_inode_number = superblock.resolve_path("/etc/passwd").unwrap().inode;
let inode = superblock.load_inode(target_inode_number).unwrap();
let passwd_reader = superblock.open(&inode).unwrap();
```

Note: normal users can't read `/dev/sda1` by default, as it would allow them to read any
file on the filesystem. You can grant yourself temporary access with
`sudo setfacl -m u:${USER}:r /dev/sda1`, if you so fancy. This will be lost at reboot.
 */

use std::collections::HashMap;
use std::convert::TryFrom;
use std::io;
use std::io::{ErrorKind, Read};
use std::io::{Seek, SeekFrom};

use anyhow::anyhow;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Error;
use bitflags::bitflags;
use byteorder::{LittleEndian, ReadBytesExt};

mod block_groups;
mod extents;

mod inner_reader;
mod none_crypto;
/// Raw object parsing API. Not versioned / supported.
pub mod parse;

use crate::extents::TreeReader;
pub use crate::none_crypto::NoneCrypto;
pub use inner_reader::{InnerReader, MetadataCrypto};

pub trait ReadAt {
    /// Read bytes from an offset in this source into a buffer, returning how many bytes were read.
    ///
    /// This function may yield fewer bytes than the size of `buf`, if it was interrupted or hit
    /// end-of-file.
    ///
    /// See [`Read::read()`](https://doc.rust-lang.org/std/io/trait.Read.html#tymethod.read).
    fn read_at(&mut self, pos: u64, buf: &mut [u8]) -> io::Result<usize>;

    /// Read the exact number of bytes required to fill `buf`, from an offset.
    ///
    /// If only a lesser number of bytes can be read, will yield an error.
    fn read_exact_at(&mut self, mut pos: u64, mut buf: &mut [u8]) -> io::Result<()> {
        while !buf.is_empty() {
            match self.read_at(pos, buf) {
                Ok(0) => break,
                Ok(n) => {
                    let tmp = buf;
                    buf = &mut tmp[n..];
                    pos += n as u64;
                }
                Err(ref e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        if !buf.is_empty() {
            Err(io::Error::new(
                ErrorKind::UnexpectedEof,
                "failed to fill whole buffer",
            ))
        } else {
            Ok(())
        }
    }
}

impl<T> ReadAt for T
where
    T: Read + Seek,
{
    fn read_at(&mut self, pos: u64, buf: &mut [u8]) -> io::Result<usize> {
        self.seek(SeekFrom::Start(pos))?;
        self.read(buf)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// The filesystem doesn't meet the code's expectations;
    /// maybe the code is wrong, maybe the filesystem is corrupt.
    #[error("assumption failed: {reason:?}")]
    AssumptionFailed { reason: String },

    /// The filesystem is valid, but requests a feature the code doesn't support.
    #[error("filesystem uses an unsupported feature: {reason:?}")]
    UnsupportedFeature { reason: String },

    /// The request is for something which we are sure is not there.
    #[error("filesystem uses an unsupported feature: {reason:?}")]
    NotFound { reason: String },
}

pub fn map_lib_error_to_io<E: ToString>(error: E) -> io::Error {
    io::Error::new(
        ErrorKind::Other,
        format!("Ext4 error: {}", error.to_string()),
    )
}

fn assumption_failed<S: ToString>(reason: S) -> ParseError {
    ParseError::AssumptionFailed {
        reason: reason.to_string(),
    }
}

fn unsupported_feature<S: ToString>(reason: S) -> ParseError {
    ParseError::UnsupportedFeature {
        reason: reason.to_string(),
    }
}

fn not_found<S: ToString>(reason: S) -> ParseError {
    ParseError::NotFound {
        reason: reason.to_string(),
    }
}

bitflags! {
    pub struct InodeFlags: u32 {
        const SECRM        = 0x0000_0001; /* Secure deletion */
        const UNRM         = 0x0000_0002; /* Undelete */
        const COMPR        = 0x0000_0004; /* Compress file */
        const SYNC         = 0x0000_0008; /* Synchronous updates */
        const IMMUTABLE    = 0x0000_0010; /* Immutable file */
        const APPEND       = 0x0000_0020; /* writes to file may only append */
        const NODUMP       = 0x0000_0040; /* do not dump file */
        const NOATIME      = 0x0000_0080; /* do not update atime */
        const DIRTY        = 0x0000_0100; /* reserved for compression */
        const COMPRBLK     = 0x0000_0200; /* One or more compressed clusters */
        const NOCOMPR      = 0x0000_0400; /* Don't compress */
        const ENCRYPT      = 0x0000_0800; /* encrypted file */
        const INDEX        = 0x0000_1000; /* hash-indexed directory */
        const IMAGIC       = 0x0000_2000; /* AFS directory */
        const JOURNAL_DATA = 0x0000_4000; /* file data should be journaled */
        const NOTAIL       = 0x0000_8000; /* file tail should not be merged */
        const DIRSYNC      = 0x0001_0000; /* dirsync behaviour (directories only) */
        const TOPDIR       = 0x0002_0000; /* Top of directory hierarchies*/
        const HUGE_FILE    = 0x0004_0000; /* Set to each huge file */
        const EXTENTS      = 0x0008_0000; /* Inode uses extents */
        const EA_INODE     = 0x0020_0000; /* Inode used for large EA */
        const EOFBLOCKS    = 0x0040_0000; /* Blocks allocated beyond EOF */
        const INLINE_DATA  = 0x1000_0000; /* Inode has inline data. */
        const PROJINHERIT  = 0x2000_0000; /* Create with parents projid */
        const RESERVED     = 0x8000_0000; /* reserved for ext4 lib */
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

pub trait Crypto {
    fn decrypt_filename(&self, context: &[u8], encrypted_name: &[u8]) -> Result<Vec<u8>, Error>;
    fn decrypt_page(&self, context: &[u8], page: &mut [u8], page_addr: u64) -> Result<(), Error>;
}

/// An actual disc metadata entry.
pub struct Inode {
    pub stat: Stat,
    pub number: u32,
    flags: InodeFlags,

    checksum_prefix: Option<u32>,

    /// The other implementations call this the inode's "block", which is so unbelievably overloaded.
    /// I made up a new name.
    core: [u8; INODE_CORE_SIZE],
    block_size: u32,
}

/// The critical core of the filesystem.
#[derive(Debug)]
pub struct SuperBlock<R: ReadAt, C: Crypto, M: MetadataCrypto> {
    inner: InnerReader<R, M>,
    load_xattrs: bool,
    /// All* checksums are computed after concatenation with the UUID, so we keep that.
    uuid_checksum: Option<u32>,
    uuid: [u8; 16],
    groups: block_groups::BlockGroups,
    crypto: C,
}

/// A raw filesystem time.
#[derive(Debug)]
pub struct Time {
    pub epoch_secs: i64,
    pub nanos: Option<u32>,
}

impl Time {
    // c.f. ext4_decode_extra_time
    // "We use an encoding that preserves the times for extra epoch"
    // the lower two bits of the extra field are added to the top of the sec field,
    // the remainder are the nsec
    pub fn from_extra(epoch_secs: i32, extra: Option<u32>) -> Time {
        let mut epoch_secs = i64::from(epoch_secs);
        match extra {
            None => Time {
                epoch_secs,
                nanos: None,
            },
            Some(extra) => {
                let epoch_bits = 2;

                // 0b1100_00..0000
                let epoch_mask = (1 << epoch_bits) - 1;

                // 0b00..00_0011
                let nsec_mask = !0u32 << epoch_bits;

                epoch_secs += i64::from(extra & epoch_mask) << 32;

                let nanos = (extra & nsec_mask) >> epoch_bits;
                Time {
                    epoch_secs,
                    nanos: Some(nanos.clamp(0, 999_999_999)),
                }
            }
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Checksums {
    Required,
    Enabled,
}

impl Default for Checksums {
    fn default() -> Self {
        Checksums::Required
    }
}

#[derive(Debug, Default)]
pub struct Options {
    pub checksums: Checksums,
}

impl<R: ReadAt> SuperBlock<R, NoneCrypto, NoneCrypto> {
    /// Open a filesystem, and load its superblock.
    pub fn new(inner: R) -> Result<Self, Error> {
        Self::new_with_options(inner, &Options::default())
    }

    pub fn new_with_options(inner: R, options: &Options) -> Result<Self, Error> {
        Self::new_with_options_and_crypto(inner, options, NoneCrypto {}, NoneCrypto {})
    }
}

impl<R: ReadAt, C: Crypto, M: MetadataCrypto> SuperBlock<R, C, M> {
    pub fn new_with_crypto(
        inner: R,
        crypto: C,
        metadata_crypto: M,
    ) -> Result<SuperBlock<R, C, M>, Error> {
        Self::new_with_options_and_crypto(inner, &Options::default(), crypto, metadata_crypto)
    }

    pub fn get_uuid(&self) -> &[u8; 16] {
        &self.uuid
    }

    pub fn get_crypto_mut(&mut self) -> &mut C {
        &mut self.crypto
    }

    pub fn get_crypto(&self) -> &C {
        &self.crypto
    }

    pub fn set_crypto(&mut self, crypto: C) {
        self.crypto = crypto;
    }

    pub fn get_metadata_crypto_mut(&mut self) -> &mut M {
        &mut self.inner.metadata_crypto
    }

    pub fn get_metadata_crypto(&self) -> &M {
        &self.inner.metadata_crypto
    }

    pub fn set_metadata_crypto(&mut self, crypto: M) {
        self.inner.metadata_crypto = crypto;
    }

    /// Returns inner R, consuming self
    pub fn into_inner(self) -> R {
        self.inner.inner
    }

    pub fn ref_inner(&self) -> &R {
        &self.inner.inner
    }

    pub fn new_with_options_and_crypto(
        inner: R,
        options: &Options,
        crypto: C,
        metadata_crypto: M,
    ) -> Result<SuperBlock<R, C, M>, Error> {
        Ok(parse::superblock(inner, options, crypto, metadata_crypto)
            .with_context(|| anyhow!("failed to parse superblock"))?)
    }

    /// Load a filesystem entry by inode number.
    pub fn load_inode(&mut self, inode: u32) -> Result<Inode, Error> {
        let data = self
            .load_inode_bytes(inode)
            .with_context(|| anyhow!("failed to find inode <{}> on disc", inode))?;

        let uuid_checksum = self.uuid_checksum;
        let parsed = parse::inode(
            data,
            |block| self.load_disc_bytes(block),
            uuid_checksum,
            inode,
        )
        .with_context(|| anyhow!("failed to parse inode <{}>", inode))?;

        Ok(Inode {
            number: inode,
            stat: parsed.stat,
            flags: parsed.flags,
            core: parsed.core,
            checksum_prefix: parsed.checksum_prefix,
            block_size: self.groups.block_size,
        })
    }

    fn load_inode_bytes(&mut self, inode: u32) -> Result<Vec<u8>, Error> {
        let offset = self.groups.index_of(inode)?;
        let mut data = vec![0u8; usize::try_from(self.groups.inode_size)?];
        self.inner.read_exact_at(offset, &mut data)?;
        Ok(data)
    }

    fn load_disc_bytes(&mut self, block: u64) -> Result<Vec<u8>, Error> {
        load_disc_bytes(&mut self.inner, self.groups.block_size, block)
    }

    /// Load the root node of the filesystem (typically `/`).
    pub fn root(&mut self) -> Result<Inode, Error> {
        Ok(self
            .load_inode(2)
            .with_context(|| anyhow!("failed to load root inode"))?)
    }

    /// Visit every entry in the filesystem in an arbitrary order.
    /// The closure should return `true` if it wants walking to continue.
    /// The method returns `true` if the closure always returned true.
    pub fn walk<F>(&mut self, inode: &Inode, path: &str, visit: &mut F) -> Result<bool, Error>
    where
        F: FnMut(&mut Self, &str, &Inode, &Enhanced) -> Result<bool, Error>,
    {
        let enhanced = inode.enhance(&mut self.inner, &self.crypto)?;

        if !visit(self, path, inode, &enhanced).with_context(|| anyhow!("user closure failed"))? {
            return Ok(false);
        }

        if let Enhanced::Directory(entries) = enhanced {
            for entry in entries {
                if "." == entry.name || ".." == entry.name {
                    continue;
                }

                let child_node = self
                    .load_inode(entry.inode)
                    .with_context(|| anyhow!("loading {} ({:?})", entry.name, entry.file_type))?;

                let path = std::path::Path::new(path).join(&entry.name);

                if !self
                    .walk(&child_node, &path.to_string_lossy(), visit)
                    .with_context(|| anyhow!("processing '{}'", entry.name))?
                {
                    return Ok(false);
                }
            }
        }

        //    self.walk(inner, &i, format!("{}/{}", path, entry.name)).map_err(|e|
        //    parse_error(format!("while processing {}: {}", path, e)))?;

        Ok(true)
    }

    /// Parse a path, and find the directory entry it represents.
    /// Note that "/foo/../bar" will be treated literally, not resolved to "/bar" then looked up.
    pub fn resolve_path(&mut self, path: &str) -> Result<DirEntry, Error> {
        let path = path.replace('\\', "/");
        let path = path.trim_end_matches('/');

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
        let last = parts
            .pop()
            .with_context(|| parse_error(format!("path separate failed")))?;
        for part in parts {
            if part.is_empty() {
                continue;
            }

            let child_inode = self.dir_entry_named(&curr, part)?.inode;
            curr = self.load_inode(child_inode)?;
        }

        self.dir_entry_named(&curr, last)
    }

    fn dir_entry_named(&mut self, inode: &Inode, name: &str) -> Result<DirEntry, Error> {
        if let Enhanced::Directory(entries) = self.enhance(inode)? {
            if let Some(en) = entries.into_iter().find(|entry| entry.name == name) {
                Ok(en)
            } else {
                Err(not_found(format!("component {} isn't there", name)).into())
            }
        } else {
            Err(not_found(format!("component {} isn't a directory", name)).into())
        }
    }

    /// Read the data from an inode. You might not want to call this on thigns that aren't regular files.
    pub fn open<'a>(&'a mut self, inode: &'a Inode) -> Result<TreeReader<'a, R, C, M>, Error> {
        inode.reader(&mut self.inner, &self.crypto)
    }

    /// Load extra metadata about some types of entries.
    pub fn enhance(&mut self, inode: &Inode) -> Result<Enhanced, Error> {
        inode.enhance(&mut self.inner, &self.crypto)
    }
}

fn load_disc_bytes<R: ReadAt, M: MetadataCrypto>(
    inner: &mut InnerReader<R, M>,
    block_size: u32,
    block: u64,
) -> Result<Vec<u8>, Error> {
    let offset = block * u64::from(block_size);
    let mut data = vec![0u8; usize::try_from(block_size)?];
    inner.read_exact_at(offset, &mut data)?;
    Ok(data)
}

impl Inode {
    fn reader<'a, R: ReadAt, C: Crypto, M: MetadataCrypto>(
        &'a self,
        inner: &'a mut InnerReader<R, M>,
        crypto: &'a C,
    ) -> Result<TreeReader<R, C, M>, Error> {
        let context = if matches!(self.stat.extracted_type, FileType::RegularFile) {
            self.get_encryption_context()
        } else {
            None
        };

        Ok(TreeReader::new(
            inner,
            self.block_size,
            self.stat.size,
            self.core,
            self.checksum_prefix,
            context,
            crypto,
        )
        .with_context(|| anyhow!("opening inode <{}>", self.number))?)
    }

    fn enhance<R: ReadAt, C: Crypto, M: MetadataCrypto>(
        &self,
        inner: &mut InnerReader<R, M>,
        crypto: &C,
    ) -> Result<Enhanced, Error> {
        Ok(match self.stat.extracted_type {
            FileType::RegularFile => Enhanced::RegularFile,
            FileType::Socket => Enhanced::Socket,
            FileType::Fifo => Enhanced::Fifo,

            FileType::Directory => Enhanced::Directory(self.read_directory(inner, crypto)?),
            FileType::SymbolicLink => {
                let mut points_to = if self.stat.size < u64::try_from(INODE_CORE_SIZE)? {
                    ensure!(
                        (self.flags & !InodeFlags::ENCRYPT).is_empty(),
                        unsupported_feature(format!(
                            "symbolic links may not have flags: {:?}",
                            self.flags
                        ))
                    );

                    self.core[0..usize::try_from(self.stat.size)?].to_vec()
                } else {
                    ensure!(
                        Self::only_relevant_flag_is_extents(self.flags & !InodeFlags::ENCRYPT),
                        unsupported_feature(format!(
                            "symbolic links may not have non-extent flags: {:?}",
                            self.flags
                        ))
                    );

                    self.load_all(inner, crypto)?
                };

                if self.flags & InodeFlags::ENCRYPT == InodeFlags::ENCRYPT {
                    let mut cursor = io::Cursor::new(points_to.as_slice());
                    let name_size = cursor.read_u16::<LittleEndian>()?;

                    let mut encrypted_filename = vec![0u8; name_size as usize];
                    cursor.read_exact(&mut encrypted_filename)?;

                    let context = self.get_encryption_context().with_context(|| {
                        anyhow!("encrypted short symlink has no encryption context")
                    })?;

                    points_to = crypto.decrypt_filename(context, &encrypted_filename)?;
                }

                let points_to = std::str::from_utf8(&points_to)
                    .with_context(|| anyhow!("symlink is invalid utf-8"))?
                    .to_string();

                Enhanced::SymbolicLink(points_to.trim_end_matches('\0').to_string())
            }
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

    fn load_all<R: ReadAt, C: Crypto, M: MetadataCrypto>(
        &self,
        inner: &mut InnerReader<R, M>,
        crypto: &C,
    ) -> Result<Vec<u8>, Error> {
        let size = usize::try_from(self.stat.size)?;
        let mut ret = vec![0u8; size];

        self.reader(inner, crypto)?.read_exact(&mut ret)?;

        Ok(ret)
    }

    fn get_encryption_context(&self) -> Option<&Vec<u8>> {
        self.stat.xattrs.get("encryption.c")
    }

    fn read_directory<R: ReadAt, C: Crypto, M: MetadataCrypto>(
        &self,
        inner: &mut InnerReader<R, M>,
        crypto: &C,
    ) -> Result<Vec<DirEntry>, Error> {
        let mut dirs = Vec::with_capacity(40);

        let data = {
            // if the flags, minus irrelevant flags, isn't just EXTENTS...
            ensure!(
                self.get_encryption_context().is_some()
                    || Self::only_relevant_flag_is_extents(self.flags),
                unsupported_feature(format!(
                    "inode with unsupported flags: {0:x} {0:b}",
                    self.flags
                ))
            );

            self.load_all(inner, crypto)?
        };

        let total_len = data.len();

        let mut cursor = io::Cursor::new(data);
        let mut read = 0usize;
        loop {
            let child_inode = cursor.read_u32::<LittleEndian>()?;
            let rec_len = cursor.read_u16::<LittleEndian>()?;

            ensure!(
                rec_len > 8,
                unsupported_feature(format!(
                    "directory record length is too short, {} must be > 8",
                    rec_len
                ))
            );

            let name_len = cursor.read_u8()?;
            let file_type = cursor.read_u8()?;
            let mut name = vec![0u8; usize::try_from(name_len)?];
            cursor.read_exact(&mut name)?;

            if 0 != child_inode {
                let name = if let (Some(context), false) = (
                    self.get_encryption_context(),
                    [b".".as_slice(), b"..".as_slice()].contains(&name.as_slice()),
                ) {
                    crypto.decrypt_filename(context, &name)?
                } else {
                    name
                };

                let forbidden_chars: &[_] = &['\0'];
                let name = std::str::from_utf8(&name)
                    .map_err(|e| parse_error(format!("invalid utf-8 in file name: {}", e)))?
                    .trim_end_matches(forbidden_chars);

                dirs.push(DirEntry {
                    inode: child_inode,
                    name: name.to_string(),
                    file_type: FileType::from_dir_hint(file_type).ok_or_else(|| {
                        unsupported_feature(format!(
                            "unexpected file type in directory: {}",
                            file_type
                        ))
                    })?,
                });
            } else if 12 == rec_len && 0 == name_len && 0xDE == file_type {
                // Magic entry representing the end of the list

                if let Some(checksum_prefix) = self.checksum_prefix {
                    let expected = cursor.read_u32::<LittleEndian>()?;
                    let computed =
                        parse::ext4_style_crc32c_le(checksum_prefix, &cursor.into_inner()[0..read]);
                    ensure!(
                        expected == computed,
                        assumption_failed(format!(
                            "directory checksum mismatch: on-disk: {:08x}, computed: {:08x}",
                            expected, computed
                        ))
                    );
                }

                break;
            }

            cursor.seek(io::SeekFrom::Current(
                i64::from(rec_len) - i64::from(name_len) - 4 - 2 - 1 - 1,
            ))?;

            read += usize::try_from(rec_len)?;
            if read >= total_len {
                ensure!(
                    read == total_len,
                    assumption_failed(format!("short read, {} != {}", read, total_len))
                );

                ensure!(
                    self.checksum_prefix.is_none(),
                    assumption_failed(
                        "directory checksums are enabled but checksum record not found"
                    )
                );

                break;
            }
        }

        Ok(dirs)
    }

    fn only_relevant_flag_is_extents(flags: InodeFlags) -> bool {
        flags
            & (InodeFlags::COMPR
                | InodeFlags::DIRTY
                | InodeFlags::COMPRBLK
                | InodeFlags::ENCRYPT
                | InodeFlags::IMAGIC
                | InodeFlags::NOTAIL
                | InodeFlags::TOPDIR
                | InodeFlags::HUGE_FILE
                | InodeFlags::EXTENTS
                | InodeFlags::EA_INODE
                | InodeFlags::EOFBLOCKS
                | InodeFlags::INLINE_DATA)
            == InodeFlags::EXTENTS
    }
}

fn load_maj_min(core: [u8; INODE_CORE_SIZE]) -> (u16, u32) {
    if 0 != core[0] || 0 != core[1] {
        (u16::from(core[1]), u32::from(core[0]))
    } else {
        // if you think reading this is bad, I had to write it
        (
            u16::from(core[5]) | (u16::from(core[6] & 0b0000_1111) << 8),
            u32::from(core[4])
                | (u32::from(core[7]) << 12)
                | (u32::from(core[6] & 0b1111_0000) >> 4) << 8,
        )
    }
}

#[inline]
fn read_le16(from: &[u8]) -> u16 {
    use byteorder::ByteOrder;
    LittleEndian::read_u16(from)
}

#[inline]
fn read_le32(from: &[u8]) -> u32 {
    use byteorder::ByteOrder;
    LittleEndian::read_u32(from)
}

#[inline]
fn read_lei32(from: &[u8]) -> i32 {
    use byteorder::ByteOrder;
    LittleEndian::read_i32(from)
}

fn parse_error(msg: String) -> Error {
    assumption_failed(msg).into()
}
