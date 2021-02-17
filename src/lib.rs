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
use std::io::Read;
use std::io::Seek;

use anyhow::anyhow;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Error;
use bitflags::bitflags;
use byteorder::{LittleEndian, ReadBytesExt};
use derivative::Derivative;
use positioned_io::{ReadAt, WriteAt};

mod block_groups;
mod extents;

/// Raw object parsing API. Not versioned / supported.
pub mod parse;

use crate::extents::TreeReader;

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
#[derive(Derivative)]
#[derivative(Debug)]
pub struct SuperBlock<R> {
    #[derivative(Debug = "ignore")]
    inner: R,
    load_xattrs: bool,
    /// All* checksums are computed after concatenation with the UUID, so we keep that.
    uuid_checksum: Option<u32>,
    #[derivative(Debug = "ignore")]
    groups: block_groups::BlockGroups,

    /*00*/ s_inodes_count: u32,         /* Inodes count */
    s_blocks_count_lo: u32,      /* Blocks count */
    s_r_blocks_count_lo: u32,    /* Reserved blocks count */
    s_free_blocks_count_lo: u32, /* Free blocks count */
    /*10*/ s_free_inodes_count: u32, /* Free inodes count */
    s_first_data_block: u32,  /* First Data Block */
    s_log_block_size: u32,    /* Block size */
    s_log_cluster_size: u32,  /* Allocation cluster size */
    /*20*/ s_blocks_per_group: u32,   /* # Blocks per group */
    s_clusters_per_group: u32, /* # Clusters per group */
    s_inodes_per_group: u32,   /* # Inodes per group */
    s_mtime: u32,              /* Mount time */
    /*30*/ s_wtime: u32,           /* Write time */
    s_mnt_count: u16,       /* Mount count */
    s_max_mnt_count: u16,   /* Maximal mount count */
    s_magic: u16,           /* Magic signature */
    s_state: u16,           /* File system state */
    s_errors: u16,          /* Behaviour when detecting errors */
    s_minor_rev_level: u16, /* minor revision level */
    /*40*/ s_lastcheck: u32,     /* time of last check */
    s_checkinterval: u32, /* max. time between checks */
    s_creator_os: u32,    /* OS */
    s_rev_level: u32,     /* Revision level */
    /*50*/ s_def_resuid: u16, /* Default uid for reserved blocks */
    s_def_resgid: u16, /* Default gid for reserved blocks */
    /*
     * These fields are for EXT4_DYNAMIC_REV superblocks only.
     *
     * Note: the difference between the compatible feature set and
     * the incompatible feature set is that if there is a bit set
     * in the incompatible feature set that the kernel doesn't
     * know about, it should refuse to mount the filesystem.
     *
     * e2fsck's requirements are more strict, if it doesn't know
     * about a feature in either the compatible or incompatible
     * feature set, it must abort and not try to meddle with
     * things it doesn't understand...
     */
    s_first_ino: u32,      /* First non-reserved inode */
    s_inode_size: u16,     /* size of inode structure */
    s_block_group_nr: u16, /* block group # of this superblock */
    s_feature_compat: u32, /* compatible feature set */
    /*60*/ s_feature_incompat: u32,  /* incompatible feature set */
    s_feature_ro_compat: u32, /* readonly-compatible feature set */
    /*68*/ s_uuid: [u8; 16], /* 128-bit uuid for volume */
    // /*78*/ s_volume_name: [char; 16], /* volume name */
    // /*88*/ s_last_mounted: [char; 64], //__nonstring,	/* directory where last mounted */
    // char is very different in rust
    /*78*/
    s_volume_name: [u8; 16], /* volume name */
    /*88*/ s_last_mounted: [u8; 64], //__nonstring,	/* directory where last mounted */
    /*C8*/ s_algorithm_usage_bitmap: u32, /* For compression */
    /*
     * Performance hints.  Directory preallocation should only
     * happen if the EXT4_FEATURE_COMPAT_DIR_PREALLOC flag is on.
     */
    s_prealloc_blocks: u8,      /* Nr of blocks to try to preallocate*/
    s_prealloc_dir_blocks: u8,  /* Nr to preallocate for dirs */
    s_reserved_gdt_blocks: u16, /* Per group desc for online growth */
    /*
     * Journaling support valid if EXT4_FEATURE_COMPAT_HAS_JOURNAL set.
     */
    /*D0*/
    s_journal_uuid: [u8; 16], /* uuid of journal superblock */
    /*E0*/ s_journal_inum: u32,    /* inode number of journal file */
    s_journal_dev: u32,     /* device number of journal file */
    s_last_orphan: u32,     /* start of list of inodes to delete */
    s_hash_seed: [u32; 4],  /* HTREE hash seed */
    s_def_hash_version: u8, /* Default hash version to use */
    s_jnl_backup_type: u8,
    s_desc_size: u16, /* size of group descriptor */
    /*100*/ s_default_mount_opts: u32,
    s_first_meta_bg: u32,    /* First metablock block group */
    s_mkfs_time: u32,        /* When the filesystem was created */
    s_jnl_blocks: [u32; 17], /* Backup of the journal inode */
    /* 64bit support valid if EXT4_FEATURE_COMPAT_64BIT */
    /*150*/
    s_blocks_count_hi: u32,      /* Blocks count */
    s_r_blocks_count_hi: u32,    /* Reserved blocks count */
    s_free_blocks_count_hi: u32, /* Free blocks count */
    s_min_extra_isize: u16,      /* All inodes have at least # bytes */
    s_want_extra_isize: u16,     /* New inodes should reserve # bytes */
    s_flags: u32,                /* Miscellaneous flags */
    s_raid_stride: u16,          /* RAID stride */
    s_mmp_update_interval: u16,  /* # seconds to wait in MMP checking */
    s_mmp_block: u64,            /* Block for multi-mount protection */
    s_raid_stripe_width: u32,    /* blocks on all data disks (N*stride)*/
    s_log_groups_per_flex: u8,   /* FLEX_BG group size */
    s_checksum_type: u8,         /* metadata checksum algorithm used */
    s_encryption_level: u8,      /* versioning level for encryption */
    s_reserved_pad: u8,          /* Padding to next 32bits */
    s_kbytes_written: u64,       /* nr of lifetime kilobytes written */
    s_snapshot_inum: u32,        /* Inode number of active snapshot */
    s_snapshot_id: u32,          /* sequential ID of active snapshot */
    s_snapshot_r_blocks_count: u64, /* reserved blocks for active
                                 snapshot's future use */
    s_snapshot_list: u32, /* inode number of the head of the
                          on-disk snapshot list */
    //#define EXT4_S_ERR_START offsetof(struct ext4_super_block, s_error_count)
    s_error_count: u32,           /* number of fs errors */
    s_first_error_time: u32,      /* first time an error happened */
    s_first_error_ino: u32,       /* inode involved in first error */
    s_first_error_block: u64,     /* block involved of first error */
    s_first_error_func: [u8; 32], // __nonstring,	/* function where the error happened */
    s_first_error_line: u32,      /* line number where error happened */
    s_last_error_time: u32,       /* most recent time of an error */
    s_last_error_ino: u32,        /* inode involved in last error */
    s_last_error_line: u32,       /* line number where error happened */
    s_last_error_block: u64,      /* block involved of last error */
    s_last_error_func: [u8; 32],  //__nonstring,	/* function where the error happened */
    //#define EXT4_S_ERR_END offsetof(struct ext4_super_block, s_mount_opts)
    s_mount_opts: [u8; 64],
    s_usr_quota_inum: u32,       /* inode for tracking user quota */
    s_grp_quota_inum: u32,       /* inode for tracking group quota */
    s_overhead_clusters: u32,    /* overhead blocks/clusters in fs */
    s_backup_bgs: [u32; 2],      /* groups with sparse_super2 SBs */
    s_encrypt_algos: [u8; 4],    /* Encryption algorithms in use  */
    s_encrypt_pw_salt: [u8; 16], /* Salt used for string2key algorithm */
    s_lpf_ino: u32,              /* Location of the lost+found inode */
    s_prj_quota_inum: u32,       /* inode for tracking project quota */
    s_checksum_seed: u32,        /* crc32c(uuid) if csum_seed set */
    s_wtime_hi: u8,
    s_mtime_hi: u8,
    s_mkfs_time_hi: u8,
    s_lastcheck_hi: u8,
    s_first_error_time_hi: u8,
    s_last_error_time_hi: u8,
    s_first_error_errcode: u8,
    s_last_error_errcode: u8,
    s_encoding: u16,       /* Filename charset encoding */
    s_encoding_flags: u16, /* Filename charset encoding flags */
    s_reserved: [u32; 95], /* Padding to the end of the block */
    s_checksum: u32,       /* crc32c(superblock) */
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

impl<R> SuperBlock<R>
where
    R: ReadAt,
{
    /// Open a filesystem, and load its superblock.
    pub fn new(inner: R) -> Result<SuperBlock<R>, Error> {
        SuperBlock::new_with_options(inner, &Options::default())
    }

    pub fn new_with_options(inner: R, options: &Options) -> Result<SuperBlock<R>, Error> {
        Ok(parse::superblock(inner, options)
            .with_context(|| anyhow!("failed to parse superblock"))?)
    }

    /// Load a filesystem entry by inode number.
    pub fn load_inode(&self, inode: u32) -> Result<Inode, Error> {
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

    fn load_inode_bytes(&self, inode: u32) -> Result<Vec<u8>, Error> {
        let offset = self.groups.index_of(inode)?;
        let mut data = vec![0u8; usize::try_from(self.groups.inode_size)?];
        self.inner.read_exact_at(offset, &mut data)?;
        Ok(data)
    }

    fn load_disc_bytes(&self, block: u64) -> Result<Vec<u8>, Error> {
        load_disc_bytes(&self.inner, self.groups.block_size, block)
    }

    /// Load the root node of the filesystem (typically `/`).
    pub fn root(&self) -> Result<Inode, Error> {
        Ok(self
            .load_inode(2)
            .with_context(|| anyhow!("failed to load root inode"))?)
    }

    /// Visit every entry in the filesystem in an arbitrary order.
    /// The closure should return `true` if it wants walking to continue.
    /// The method returns `true` if the closure always returned true.
    pub fn walk<F>(&self, inode: &Inode, path: &str, visit: &mut F) -> Result<bool, Error>
    where
        F: FnMut(&Self, &str, &Inode, &Enhanced) -> Result<bool, Error>,
    {
        let enhanced = inode.enhance(&self.inner)?;

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
                if !self
                    .walk(&child_node, &format!("{}/{}", path, entry.name), visit)
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
    pub fn resolve_path(&self, path: &str) -> Result<DirEntry, Error> {
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

    fn dir_entry_named(&self, inode: &Inode, name: &str) -> Result<DirEntry, Error> {
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
    pub fn open(&self, inode: &Inode) -> Result<TreeReader<&R>, Error> {
        inode.reader(&self.inner)
    }

    /// Load extra metadata about some types of entries.
    pub fn enhance(&self, inode: &Inode) -> Result<Enhanced, Error> {
        inode.enhance(&self.inner)
    }
}

impl<R> SuperBlock<R>
where
    R: WriteAt,
{
    pub fn write_superblock(&mut self) -> Result<usize, Error> {
        let bytes_count = parse::superblock_write(self)
            .with_context(|| anyhow!("write_superblock() Oh god no"))?;
        println!(
            "AA DEBUG: write_superblock() bytes_count: {:?}",
            bytes_count
        );
        Ok(bytes_count)
    }
}

fn load_disc_bytes<R>(inner: R, block_size: u32, block: u64) -> Result<Vec<u8>, Error>
where
    R: ReadAt,
{
    let offset = block * u64::from(block_size);
    let mut data = vec![0u8; usize::try_from(block_size)?];
    inner.read_exact_at(offset, &mut data)?;
    Ok(data)
}

impl Inode {
    fn reader<R>(&self, inner: R) -> Result<TreeReader<R>, Error>
    where
        R: ReadAt,
    {
        Ok(TreeReader::new(
            inner,
            self.block_size,
            self.stat.size,
            self.core,
            self.checksum_prefix,
        )
        .with_context(|| anyhow!("opening inode <{}>", self.number))?)
    }

    fn enhance<R>(&self, inner: R) -> Result<Enhanced, Error>
    where
        R: ReadAt,
    {
        Ok(match self.stat.extracted_type {
            FileType::RegularFile => Enhanced::RegularFile,
            FileType::Socket => Enhanced::Socket,
            FileType::Fifo => Enhanced::Fifo,

            FileType::Directory => Enhanced::Directory(self.read_directory(inner)?),
            FileType::SymbolicLink => {
                Enhanced::SymbolicLink(if self.stat.size < u64::try_from(INODE_CORE_SIZE)? {
                    ensure!(
                        self.flags.is_empty(),
                        unsupported_feature(format!(
                            "symbolic links may not have flags: {:?}",
                            self.flags
                        ))
                    );
                    std::str::from_utf8(&self.core[0..usize::try_from(self.stat.size)?])
                        .with_context(|| anyhow!("short symlink is invalid utf-8"))?
                        .to_string()
                } else {
                    ensure!(
                        self.only_relevant_flag_is_extents(),
                        unsupported_feature(format!(
                            "symbolic links may not have non-extent flags: {:?}",
                            self.flags
                        ))
                    );
                    std::str::from_utf8(&self.load_all(inner)?)
                        .with_context(|| anyhow!("long symlink is invalid utf-8"))?
                        .to_string()
                })
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

    fn load_all<R>(&self, inner: R) -> Result<Vec<u8>, Error>
    where
        R: ReadAt,
    {
        let size = usize::try_from(self.stat.size)?;
        let mut ret = vec![0u8; size];

        self.reader(inner)?.read_exact(&mut ret)?;

        Ok(ret)
    }

    fn read_directory<R>(&self, inner: R) -> Result<Vec<DirEntry>, Error>
    where
        R: ReadAt,
    {
        let mut dirs = Vec::with_capacity(40);

        let data = {
            // if the flags, minus irrelevant flags, isn't just EXTENTS...
            ensure!(
                self.only_relevant_flag_is_extents(),
                unsupported_feature(format!(
                    "inode with unsupported flags: {0:x} {0:b}",
                    self.flags
                ))
            );

            self.load_all(inner)?
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
                let name = std::str::from_utf8(&name)
                    .map_err(|e| parse_error(format!("invalid utf-8 in file name: {}", e)))?;

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

    fn only_relevant_flag_is_extents(&self) -> bool {
        self.flags
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
