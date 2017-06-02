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

const EXT4_SUPER_MAGIC: u16 = 0xEF53;

bitflags! {
    struct IncompatibleFeature: u32 {
       const INCOMPAT_COMPRESSION = 0x0001;
       const INCOMPAT_FILETYPE    = 0x0002;
       const INCOMPAT_RECOVER     = 0x0004; /* Needs recovery */
       const INCOMPAT_JOURNAL_DEV = 0x0008; /* Journal device */
       const INCOMPAT_META_BG     = 0x0010;
       const INCOMPAT_EXTENTS     = 0x0040; /* extents support */
       const INCOMPAT_64BIT       = 0x0080;
       const INCOMPAT_MMP         = 0x0100;
       const INCOMPAT_FLEX_BG     = 0x0200;
       const INCOMPAT_EA_INODE    = 0x0400; /* EA in inode */
       const INCOMPAT_DIRDATA     = 0x1000; /* data in dirent */
       const INCOMPAT_CSUM_SEED   = 0x2000;
       const INCOMPAT_LARGEDIR    = 0x4000; /* >2GB or 3-lvl htree */
       const INCOMPAT_INLINE_DATA = 0x8000; /* data in inode */
       const INCOMPAT_ENCRYPT     = 0x10000;
    }
}

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
    where R: io::Read + io::Seek
    {
        inner.seek(io::SeekFrom::Start(1024))?;

        // <a cut -c 9- | fgrep ' s_' | fgrep -v ERR_ | while read ty nam comment; do printf "let %s =\n  inner.read_%s::<LittleEndian>()?; %s\n" $(echo $nam | tr -d ';') $(echo $ty | sed 's/__le/u/; s/__//') $comment; done
//        let s_inodes_count =
            inner.read_u32::<LittleEndian>()?; /* Inodes count */
        let s_blocks_count_lo =
            inner.read_u32::<LittleEndian>()?; /* Blocks count */
//        let s_r_blocks_count_lo =
            inner.read_u32::<LittleEndian>()?; /* Reserved blocks count */
//        let s_free_blocks_count_lo =
            inner.read_u32::<LittleEndian>()?; /* Free blocks count */
//        let s_free_inodes_count =
            inner.read_u32::<LittleEndian>()?; /* Free inodes count */
        let s_first_data_block =
            inner.read_u32::<LittleEndian>()?; /* First Data Block */
        let s_log_block_size =
            inner.read_u32::<LittleEndian>()?; /* Block size */
//        let s_log_cluster_size =
            inner.read_u32::<LittleEndian>()?; /* Allocation cluster size */
        let s_blocks_per_group =
            inner.read_u32::<LittleEndian>()?; /* # Blocks per group */
//        let s_clusters_per_group =
            inner.read_u32::<LittleEndian>()?; /* # Clusters per group */
        let s_inodes_per_group =
            inner.read_u32::<LittleEndian>()?; /* # Inodes per group */
//        let s_mtime =
            inner.read_u32::<LittleEndian>()?; /* Mount time */
//        let s_wtime =
            inner.read_u32::<LittleEndian>()?; /* Write time */
//        let s_mnt_count =
            inner.read_u16::<LittleEndian>()?; /* Mount count */
//        let s_max_mnt_count =
            inner.read_u16::<LittleEndian>()?; /* Maximal mount count */
        let s_magic =
            inner.read_u16::<LittleEndian>()?; /* Magic signature */
        let s_state =
            inner.read_u16::<LittleEndian>()?; /* File system state */
//        let s_errors =
            inner.read_u16::<LittleEndian>()?; /* Behaviour when detecting errors */
//        let s_minor_rev_level =
            inner.read_u16::<LittleEndian>()?; /* minor revision level */
//        let s_lastcheck =
            inner.read_u32::<LittleEndian>()?; /* time of last check */
//        let s_checkinterval =
            inner.read_u32::<LittleEndian>()?; /* max. time between checks */
        let s_creator_os =
            inner.read_u32::<LittleEndian>()?; /* OS */
        let s_rev_level =
            inner.read_u32::<LittleEndian>()?; /* Revision level */
//        let s_def_resuid =
            inner.read_u16::<LittleEndian>()?; /* Default uid for reserved blocks */
//        let s_def_resgid =
            inner.read_u16::<LittleEndian>()?; /* Default gid for reserved blocks */
//        let s_first_ino =
            inner.read_u32::<LittleEndian>()?; /* First non-reserved inode */
        let s_inode_size =
            inner.read_u16::<LittleEndian>()?; /* size of inode structure */
//        let s_block_group_nr =
            inner.read_u16::<LittleEndian>()?; /* block group # of this superblock */
//        let s_feature_compat =
            inner.read_u32::<LittleEndian>()?; /* compatible feature set */
        let s_feature_incompat =
            inner.read_u32::<LittleEndian>()?; /* incompatible feature set */

        let incompatible_features = IncompatibleFeature::from_bits(s_feature_incompat)
            .ok_or_else(|| parse_error(format!("completely unsupported feature flag: {:b}", s_feature_incompat)))?;

        let supported_incompatible_features =
            INCOMPAT_FILETYPE
                | INCOMPAT_EXTENTS
                | INCOMPAT_FLEX_BG
                | INCOMPAT_64BIT;

        if incompatible_features.intersects(!supported_incompatible_features) {
            return Err(parse_error(format!("some unsupported incompatible feature flags: {:?}",
                                           incompatible_features & !supported_incompatible_features)));
        }

        let long_structs = incompatible_features.contains(INCOMPAT_64BIT);

//        let s_feature_ro_compat =
            inner.read_u32::<LittleEndian>()?; /* readonly-compatible feature set */
        let mut s_uuid = [0; 16];
        inner.read_exact(&mut s_uuid)?; /* 128-bit uuid for volume */
        let mut s_volume_name = [0u8; 16];
        inner.read_exact(&mut s_volume_name)?; /* volume name */
        let mut s_last_mounted = [0u8; 64];
        inner.read_exact(&mut s_last_mounted)?; /* directory where last mounted */
//        let s_algorithm_usage_bitmap =
            inner.read_u32::<LittleEndian>()?; /* For compression */
//        let s_prealloc_blocks =
            inner.read_u8()?; /* Nr of blocks to try to preallocate*/
//        let s_prealloc_dir_blocks =
            inner.read_u8()?; /* Nr to preallocate for dirs */
//        let s_reserved_gdt_blocks =
            inner.read_u16::<LittleEndian>()?; /* Per group desc for online growth */
        let mut s_journal_uuid = [0u8; 16];
        inner.read_exact(&mut s_journal_uuid)?; /* uuid of journal superblock */
//        let s_journal_inum =
            inner.read_u32::<LittleEndian>()?; /* inode number of journal file */
//        let s_journal_dev =
            inner.read_u32::<LittleEndian>()?; /* device number of journal file */
//        let s_last_orphan =
            inner.read_u32::<LittleEndian>()?; /* start of list of inodes to delete */
        let mut s_hash_seed = [0u8; 4 * 4];
        inner.read_exact(&mut s_hash_seed)?; /* HTREE hash seed */
//        let s_def_hash_version =
            inner.read_u8()?; /* Default hash version to use */
//        let s_jnl_backup_type =
            inner.read_u8()?;
        let s_desc_size =
            inner.read_u16::<LittleEndian>()?; /* size of group descriptor */
//        let s_default_mount_opts =
            inner.read_u32::<LittleEndian>()?;
//        let s_first_meta_bg =
            inner.read_u32::<LittleEndian>()?; /* First metablock block group */
//        let s_mkfs_time =
            inner.read_u32::<LittleEndian>()?; /* When the filesystem was created */
        let mut s_jnl_blocks = [0; 17 * 4];
        inner.read_exact(&mut s_jnl_blocks)?; /* Backup of the journal inode */

        let s_blocks_count_hi =
            if !long_structs { None } else {
                Some(inner.read_u32::<LittleEndian>()?) /* Blocks count */
            };
////        let s_r_blocks_count_hi =
//            if !long_structs { None } else {
//                Some(inner.read_u32::<LittleEndian>()?) /* Reserved blocks count */
//            };
////        let s_free_blocks_count_hi =
//            if !long_structs { None } else {
//                Some(inner.read_u32::<LittleEndian>()?) /* Free blocks count */
//            };
////        let s_min_extra_isize =
//            if !long_structs { None } else {
//                Some(inner.read_u16::<LittleEndian>()?) /* All inodes have at least # bytes */
//            };
////        let s_want_extra_isize =
//            if !long_structs { None } else {
//                Some(inner.read_u16::<LittleEndian>()?) /* New inodes should reserve # bytes */
//            };
////        let s_flags =
//            if !long_structs { None } else {
//                Some(inner.read_u32::<LittleEndian>()?) /* Miscellaneous flags */
//            };

        if EXT4_SUPER_MAGIC != s_magic {
            return Err(parse_error(format!("invalid magic number: {:x} should be {:x}", s_magic, EXT4_SUPER_MAGIC)));
        }

        if 0 != s_creator_os {
            return Err(parse_error(format!("only support filesystems created on linux, not '{}'", s_creator_os)));
        }

        {
            const S_STATE_UNMOUNTED_CLEANLY: u16 = 0b01;
            const S_STATE_ERRORS_DETECTED: u16 = 0b10;

            if s_state & S_STATE_UNMOUNTED_CLEANLY == 0 || s_state & S_STATE_ERRORS_DETECTED != 0 {
                return Err(parse_error(format!("filesystem is not in a clean state: {:b}", s_state)));
            }
        }

        if 0 == s_inodes_per_group {
            return Err(parse_error("inodes per group cannot be zero".to_string()));
        }

        let block_size: u32 = match s_log_block_size {
            0 => 1024,
            1 => 2048,
            2 => 4096,
            6 => 65536,
            _ => {
                return Err(parse_error(format!("unexpected block size: 2^{}", s_log_block_size + 10)));
            }
        };

        if !long_structs {
            assert_eq!(0, s_desc_size);
        }

        if 1 != s_rev_level {
            return Err(parse_error(format!("unsupported rev_level {}", s_rev_level)));
        }

        let group_table_pos = if 1024 == block_size {
            // for 1k blocks, the table is in the third block, after:
            1024   // boot sector
            + 1024 // superblock
        } else {
            // for other blocks, the boot sector is in the first 1k of the first block,
            // followed by the superblock (also in first block), and the group table is afterwards
            block_size
        };

        inner.seek(io::SeekFrom::Start(group_table_pos as u64))?;
        let blocks_count = (
            s_blocks_count_lo as u64
            + ((s_blocks_count_hi.unwrap_or(0) as u64) << 32)
            - s_first_data_block as u64 + s_blocks_per_group as u64 - 1
        ) / s_blocks_per_group as u64;

        let groups = block_groups::BlockGroups::new(inner, blocks_count,
                                                    s_desc_size, s_inodes_per_group,
                                                    block_size, s_inode_size)?;

        Ok(SuperBlock {
            block_size,
            groups,
        })
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

        #[allow(unknown_lints, absurd_extreme_comparisons)] {
            // this check only makes sense on non-64-bit platforms; on 64-bit usize == u64.
            if inode.stat.size > std::usize::MAX as u64 {
                return Err(io::Error::new(io::ErrorKind::InvalidData,
                                          format!("file is too big for this platform to fit in memory: {}",
                                                  inode.stat.size)));
            }
        }

        let size = inode.stat.size as usize;

        let mut ret = Vec::with_capacity(size);

        assert_eq!(size, self.reader_for(inner, inode)?.read_to_end(&mut ret)?);

        Ok(ret)
    }


    fn reader_for<R>(&self, inner: R, inode: &Inode) -> io::Result<TreeReader<R>>
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
