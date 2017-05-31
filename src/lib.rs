#![feature(associated_consts)] // for enum only

extern crate byteorder;
extern crate enum_traits;
#[macro_use] extern crate enum_traits_macros;

use std::fmt;
use std::io;

use byteorder::{ReadBytesExt, LittleEndian};

use enum_traits::Discriminant;
use enum_traits::FromIndex;
use enum_traits::Index;
use enum_traits::Iterable;
use enum_traits::ToIndex;

const EXT4_SUPER_MAGIC: u16 = 0xEF53;

const EXT4_BLOCK_GROUP_INODES_UNUSED: u16 = 0b1;
const EXT4_BLOCK_GROUP_BLOCKS_UNUSED: u16 = 0b10;

#[derive(Debug, PartialEq, EnumIndex, EnumToIndex, EnumFromIndex, EnumLen, EnumIter)]
enum IncompatibleFeature {
    Compression,
    FileType,
    RecoveryNeeded,
    JournalDevice,
    MetaBG,
    Reserved1,
    Extents,
    SixtyFourBit,
    MMP,
    FlexBg,
    EaInode,
    Reserved2,
    DirData,
    CsumSeed,
    LargeDir,
    InlineData,
    Encryption,
    Unknown,
}

impl IncompatibleFeature {
    fn lookup(id: u8) -> IncompatibleFeature {
        IncompatibleFeature::from_index(id)
            .unwrap_or(IncompatibleFeature::Unknown)
    }

    fn from_bitset(bits: u32) -> Vec<IncompatibleFeature> {
        let len = IncompatibleFeature::Unknown.into_index();
        let mut features = Vec::with_capacity(len as usize);

        for val in IncompatibleFeature::variants() {
            if 0 != bits & (1 << val.index()) {
                features.push(val);
            }
        }
        features
    }
}

#[derive(Debug, PartialEq, EnumIndex, EnumLen, EnumIter)]
enum FileModes {
    OX,
    OW,
    OR,
    GX,
    GW,
    GR,
    UX,
    UW,
    UR,
    Sticky,
    SetGid,
    SetUid,
}

#[derive(Debug, PartialEq, EnumDiscriminant)]
enum FileType {
    Fifo            = 0x1000, // S_IFIFO (FIFO)
    CharacterDevice = 0x2000, // S_IFCHR (Character device)
    Directory       = 0x4000, // S_IFDIR (Directory)
    BlockDevice     = 0x6000, // S_IFBLK (Block device)
    RegularFile     = 0x8000, // S_IFREG (Regular file)
    SymbolicLink    = 0xA000, // S_IFLNK (Symbolic link)
    Socket          = 0xC000, // S_IFSOCK (Socket)
}

impl FileType {
    fn from_mode(mode: u16) -> Option<FileType> {
        FileType::from_discriminant((mode & 0xF000) as usize)
    }
}

#[derive(Debug)]
struct BlockGroup {
    block_bitmap_block: u64,
    inode_bitmap_block: u64,
    inode_table_block: u64,
}

#[derive(Debug)]
struct Extent {
    start: u64,
    len: u16,
}

#[derive(Debug)]
struct Fs {
    block_size: u16,
}

impl Fs {
    fn new<R>(inner: &mut R) -> io::Result<Fs>
    where R: io::Read + io::Seek
    {
        {
            let mut boot_sector = [0; 1024];
            inner.read_exact(&mut boot_sector)?;
        }

        // <a cut -c 9- | fgrep ' s_' | fgrep -v ERR_ | while read ty nam comment; do printf "let %s =\n  inner.read_%s::<LittleEndian>()?; %s\n" $(echo $nam | tr -d ';') $(echo $ty | sed 's/__le/u/; s/__//') $comment; done
        let s_inodes_count =
            inner.read_u32::<LittleEndian>()?; /* Inodes count */
        let s_blocks_count_lo =
            inner.read_u32::<LittleEndian>()?; /* Blocks count */
        let s_r_blocks_count_lo =
            inner.read_u32::<LittleEndian>()?; /* Reserved blocks count */
        let s_free_blocks_count_lo =
            inner.read_u32::<LittleEndian>()?; /* Free blocks count */
        let s_free_inodes_count =
            inner.read_u32::<LittleEndian>()?; /* Free inodes count */
        let s_first_data_block =
            inner.read_u32::<LittleEndian>()?; /* First Data Block */
        let s_log_block_size =
            inner.read_u32::<LittleEndian>()?; /* Block size */
        let s_log_cluster_size =
            inner.read_u32::<LittleEndian>()?; /* Allocation cluster size */
        let s_blocks_per_group =
            inner.read_u32::<LittleEndian>()?; /* # Blocks per group */
        let s_clusters_per_group =
            inner.read_u32::<LittleEndian>()?; /* # Clusters per group */
        let s_inodes_per_group =
            inner.read_u32::<LittleEndian>()?; /* # Inodes per group */
        let s_mtime =
            inner.read_u32::<LittleEndian>()?; /* Mount time */
        let s_wtime =
            inner.read_u32::<LittleEndian>()?; /* Write time */
        let s_mnt_count =
            inner.read_u16::<LittleEndian>()?; /* Mount count */
        let s_max_mnt_count =
            inner.read_u16::<LittleEndian>()?; /* Maximal mount count */
        let s_magic =
            inner.read_u16::<LittleEndian>()?; /* Magic signature */
        let s_state =
            inner.read_u16::<LittleEndian>()?; /* File system state */
        let s_errors =
            inner.read_u16::<LittleEndian>()?; /* Behaviour when detecting errors */
        let s_minor_rev_level =
            inner.read_u16::<LittleEndian>()?; /* minor revision level */
        let s_lastcheck =
            inner.read_u32::<LittleEndian>()?; /* time of last check */
        let s_checkinterval =
            inner.read_u32::<LittleEndian>()?; /* max. time between checks */
        let s_creator_os =
            inner.read_u32::<LittleEndian>()?; /* OS */
        let s_rev_level =
            inner.read_u32::<LittleEndian>()?; /* Revision level */
        let s_def_resuid =
            inner.read_u16::<LittleEndian>()?; /* Default uid for reserved blocks */
        let s_def_resgid =
            inner.read_u16::<LittleEndian>()?; /* Default gid for reserved blocks */
        let s_first_ino =
            inner.read_u32::<LittleEndian>()?; /* First non-reserved inode */
        let s_inode_size =
            inner.read_u16::<LittleEndian>()?; /* size of inode structure */
        let s_block_group_nr =
            inner.read_u16::<LittleEndian>()?; /* block group # of this superblock */
        let s_feature_compat =
            inner.read_u32::<LittleEndian>()?; /* compatible feature set */
        let s_feature_incompat =
            inner.read_u32::<LittleEndian>()?; /* incompatible feature set */
        let s_feature_ro_compat =
            inner.read_u32::<LittleEndian>()?; /* readonly-compatible feature set */
        let mut s_uuid = [0; 16];
        inner.read_exact(&mut s_uuid)?; /* 128-bit uuid for volume */
        let mut s_volume_name = [0u8; 16];
        inner.read_exact(&mut s_volume_name)?; /* volume name */
        let mut s_last_mounted = [0u8; 64];
        inner.read_exact(&mut s_last_mounted)?; /* directory where last mounted */
        let s_algorithm_usage_bitmap =
            inner.read_u32::<LittleEndian>()?; /* For compression */
        let s_prealloc_blocks =
            inner.read_u8()?; /* Nr of blocks to try to preallocate*/
        let s_prealloc_dir_blocks =
            inner.read_u8()?; /* Nr to preallocate for dirs */
        let s_reserved_gdt_blocks =
            inner.read_u16::<LittleEndian>()?; /* Per group desc for online growth */
        let mut s_journal_uuid = [0u8; 16];
        inner.read_exact(&mut s_journal_uuid)?; /* uuid of journal superblock */
        let s_journal_inum =
            inner.read_u32::<LittleEndian>()?; /* inode number of journal file */
        let s_journal_dev =
            inner.read_u32::<LittleEndian>()?; /* device number of journal file */
        let s_last_orphan =
            inner.read_u32::<LittleEndian>()?; /* start of list of inodes to delete */
        let mut s_hash_seed = [0u8; 4 * 4];
        inner.read_exact(&mut s_hash_seed)?; /* HTREE hash seed */
        let s_def_hash_version =
            inner.read_u8()?; /* Default hash version to use */
        let s_jnl_backup_type =
            inner.read_u8()?;
        let s_desc_size =
            inner.read_u16::<LittleEndian>()?; /* size of group descriptor */
        let s_default_mount_opts =
            inner.read_u32::<LittleEndian>()?;
        let s_first_meta_bg =
            inner.read_u32::<LittleEndian>()?; /* First metablock block group */
        let s_mkfs_time =
            inner.read_u32::<LittleEndian>()?; /* When the filesystem was created */
        let mut s_jnl_blocks = [0; 17 * 4];
        inner.read_exact(&mut s_jnl_blocks)?; /* Backup of the journal inode */
        let s_blocks_count_hi =
            inner.read_u32::<LittleEndian>()?; /* Blocks count */
        let s_r_blocks_count_hi =
            inner.read_u32::<LittleEndian>()?; /* Reserved blocks count */
        let s_free_blocks_count_hi =
            inner.read_u32::<LittleEndian>()?; /* Free blocks count */
        let s_min_extra_isize =
            inner.read_u16::<LittleEndian>()?; /* All inodes have at least # bytes */
        let s_want_extra_isize =
            inner.read_u16::<LittleEndian>()?; /* New inodes should reserve # bytes */
        let s_flags =
            inner.read_u32::<LittleEndian>()?; /* Miscellaneous flags */

        if EXT4_SUPER_MAGIC != s_magic {
            return Err(parse_error(format!("invalid magic number: {:x} should be {:x}", EXT4_SUPER_MAGIC, s_magic)));
        }

        println!("{:?}", std::str::from_utf8(&mut s_last_mounted));

        let block_size: u16 = match s_log_block_size {
            1 => 2048,
            2 => 4096,
            6 => 65536,
            _ => {
                return Err(parse_error(format!("unexpected block size: 2^{}", s_log_block_size + 10)));
            }
        };

        let incompatible_features = IncompatibleFeature::from_bitset(s_feature_incompat);
        let supported_compatible_features = vec![
            IncompatibleFeature::FileType,
            IncompatibleFeature::Extents,
            IncompatibleFeature::FlexBg
        ];

        if supported_compatible_features != incompatible_features {
            return Err(parse_error(format!("some unsupported incompatible feature flags: {:?}", incompatible_features)));
        }

        // 64-bit mode isn't enabled (in the incompat features),
        // so this must be unset, and we'll assume short format.
        assert_eq!(0, s_desc_size);

        if 1 != s_rev_level {
            return Err(parse_error(format!("unsupported rev_level {}", s_rev_level)));
        }

        inner.seek(io::SeekFrom::Start(block_size as u64 * 1))?;
        let blocks_count = (s_blocks_count_lo - s_first_data_block + s_blocks_per_group - 1) / s_blocks_per_group;

        let mut groups = Vec::with_capacity(blocks_count as usize);

        for block in 0..blocks_count {
            let bg_block_bitmap_lo =
                inner.read_u32::<LittleEndian>()?; /* Blocks bitmap block */
            let bg_inode_bitmap_lo =
                inner.read_u32::<LittleEndian>()?; /* Inodes bitmap block */
            let bg_inode_table_lo =
                inner.read_u32::<LittleEndian>()?; /* Inodes table block */
            let bg_free_blocks_count_lo =
                inner.read_u16::<LittleEndian>()?; /* Free blocks count */
            let bg_free_inodes_count_lo =
                inner.read_u16::<LittleEndian>()?; /* Free inodes count */
            let bg_used_dirs_count_lo =
                inner.read_u16::<LittleEndian>()?; /* Directories count */
            let bg_flags =
                inner.read_u16::<LittleEndian>()?; /* EXT4_BG_flags (INODE_UNINIT, etc) */
            let bg_exclude_bitmap_lo =
                inner.read_u32::<LittleEndian>()?; /* Exclude bitmap for snapshots */
            let bg_block_bitmap_csum_lo =
                inner.read_u16::<LittleEndian>()?; /* crc32c(s_uuid+grp_num+bbitmap) LE */
            let bg_inode_bitmap_csum_lo =
                inner.read_u16::<LittleEndian>()?; /* crc32c(s_uuid+grp_num+ibitmap) LE */
            let bg_itable_unused_lo =
                inner.read_u16::<LittleEndian>()?; /* Unused inodes count */
            let bg_checksum =
                inner.read_u16::<LittleEndian>()?; /* crc16(sb_uuid+group+desc) */

            if false {
                let bg_block_bitmap_hi =
                    inner.read_u32::<LittleEndian>()?; /* Blocks bitmap block MSB */
                let bg_inode_bitmap_hi =
                    inner.read_u32::<LittleEndian>()?; /* Inodes bitmap block MSB */
                let bg_inode_table_hi =
                    inner.read_u32::<LittleEndian>()?; /* Inodes table block MSB */
                let bg_free_blocks_count_hi =
                    inner.read_u16::<LittleEndian>()?; /* Free blocks count MSB */
                let bg_free_inodes_count_hi =
                    inner.read_u16::<LittleEndian>()?; /* Free inodes count MSB */
                let bg_used_dirs_count_hi =
                    inner.read_u16::<LittleEndian>()?; /* Directories count MSB */
                let bg_itable_unused_hi =
                    inner.read_u16::<LittleEndian>()?; /* Unused inodes count MSB */
                let bg_exclude_bitmap_hi =
                    inner.read_u32::<LittleEndian>()?; /* Exclude bitmap block MSB */
                let bg_block_bitmap_csum_hi =
                    inner.read_u16::<LittleEndian>()?; /* crc32c(s_uuid+grp_num+bbitmap) BE */
                let bg_inode_bitmap_csum_hi =
                    inner.read_u16::<LittleEndian>()?; /* crc32c(s_uuid+grp_num+ibitmap) BE */
            }

            //
            if bg_flags & EXT4_BLOCK_GROUP_INODES_UNUSED != 0 || bg_flags & EXT4_BLOCK_GROUP_BLOCKS_UNUSED != 0 {
                continue;
            }

            let block_bitmap_block: u64 = bg_block_bitmap_lo as u64;
            let inode_bitmap_block: u64 = bg_inode_bitmap_lo as u64;
            let inode_table_block: u64 = bg_inode_table_lo as u64;

            groups.push(BlockGroup {
                block_bitmap_block,
                inode_bitmap_block,
                inode_table_block
            });
        }

        let mut dirs = Vec::with_capacity(4096);

        for group in groups {
            inner.seek(io::SeekFrom::Start(block_size as u64 * group.inode_bitmap_block))?;
            let mut bitmap = Vec::new();
            bitmap.resize((s_inodes_per_group / 8) as usize, 0);
            inner.read_exact(&mut bitmap);

            println!("{:b}", bitmap[0]);

            inner.seek(io::SeekFrom::Start(block_size as u64 * group.inode_table_block))?;
            for i in 0..s_inodes_per_group {
                let i_mode =
                    inner.read_u16::<LittleEndian>()?; /* File mode */
                let i_uid =
                    inner.read_u16::<LittleEndian>()?; /* Low 16 bits of Owner Uid */
                let i_size_lo =
                    inner.read_u32::<LittleEndian>()?; /* Size in bytes */
                let i_atime =
                    inner.read_u32::<LittleEndian>()?; /* Access time */
                let i_ctime =
                    inner.read_u32::<LittleEndian>()?; /* Inode Change time */
                let i_mtime =
                    inner.read_u32::<LittleEndian>()?; /* Modification time */
                let i_dtime =
                    inner.read_u32::<LittleEndian>()?; /* Deletion Time */
                let i_gid =
                    inner.read_u16::<LittleEndian>()?; /* Low 16 bits of Group Id */
                let i_links_count =
                    inner.read_u16::<LittleEndian>()?; /* Links count */
                let i_blocks_lo =
                    inner.read_u32::<LittleEndian>()?; /* Blocks count */
                let i_flags =
                    inner.read_u32::<LittleEndian>()?; /* File flags */
                let l_i_version =
                    inner.read_u32::<LittleEndian>()?;
                let mut i_block = [0u8; 15 * 4];
                inner.read_exact(&mut i_block)?; /* Pointers to blocks */
                let i_generation =
                    inner.read_u32::<LittleEndian>()?; /* File version (for NFS) */
                let i_file_acl_lo =
                    inner.read_u32::<LittleEndian>()?; /* File ACL */
                let i_size_high =
                    inner.read_u32::<LittleEndian>()?;
                let i_obso_faddr =
                    inner.read_u32::<LittleEndian>()?; /* Obsoleted fragment address */
                let l_i_blocks_high =
                    inner.read_u16::<LittleEndian>()?; /* were l_i_reserved1 */
                let l_i_file_acl_high =
                    inner.read_u16::<LittleEndian>()?;
                let l_i_uid_high =
                    inner.read_u16::<LittleEndian>()?; /* these 2 fields */
                let l_i_gid_high =
                    inner.read_u16::<LittleEndian>()?; /* were reserved2[0] */
                let l_i_checksum_lo =
                    inner.read_u16::<LittleEndian>()?; /* crc32c(uuid+inum+inode) LE */
                let l_i_reserved =
                    inner.read_u16::<LittleEndian>()?;
                let i_extra_isize =
                    inner.read_u16::<LittleEndian>()?;

                // rounding up to s_inode_size, don't get why we have to..
                inner.seek(io::SeekFrom::Current(128 - 2))?;

                if false {
                    let i_checksum_hi =
                        inner.read_u16::<LittleEndian>()?; /* crc32c(uuid+inum+inode) BE */
                    let i_ctime_extra =
                        inner.read_u32::<LittleEndian>()?; /* extra Change time      (nsec << 2 | epoch) */
                    let i_mtime_extra =
                        inner.read_u32::<LittleEndian>()?; /* extra Modification time(nsec << 2 | epoch) */
                    let i_atime_extra =
                        inner.read_u32::<LittleEndian>()?; /* extra Access time      (nsec << 2 | epoch) */
                    let i_crtime =
                        inner.read_u32::<LittleEndian>()?; /* File Creation time */
                    let i_crtime_extra =
                        inner.read_u32::<LittleEndian>()?; /* extra FileCreationtime (nsec << 2 | epoch) */
                    let i_version_hi =
                        inner.read_u32::<LittleEndian>()?; /* high 32 bits for 64-bit version */
                    let i_projid =
                        inner.read_u32::<LittleEndian>()?; /* Project ID */
                }

                if 0 == i_flags {
                    continue;
                }

                let extracted_type = FileType::from_mode(i_mode)
                    .ok_or_else(|| parse_error(format!("unexpected file type in mode: {:b}", i_mode)))?;

                if FileType::Directory != extracted_type {
                    continue;
                }

                println!("{} {:16b} {:?}", i_atime, i_extra_isize, i_mode, );
                // i_block.iter().map(|b| format!("{:02x} ", b)).collect::<String>()

                if i_flags & 0x00080000 == 0 {
                    return Err(parse_error("inode without extents".to_string()));
                }

                assert_eq!(0x0a, i_block[0]);
                assert_eq!(0xf3, i_block[1]);

                let extent_entries = as_u16(&i_block[2..]);
                let depth = as_u16(&i_block[6..]);

                assert_eq!(0, depth);

                for en in 0..extent_entries {
                    let extent = &i_block[12+en as usize*12 ..];
                    let ee_block = as_u32(extent);
                    let ee_len = as_u16(&extent[4..]);
                    let ee_start_hi = as_u16(&extent[6..]);
                    let ee_start_lo = as_u32(&extent[8..]);
                    let ee_start = ee_start_lo as u64 + 0x1000 * ee_start_hi as u64;
//                    assert_eq!(0, ee_block);
                    dirs.push(Extent {
                        start: ee_start,
                        len: ee_len,
                    });
                }
            }
        }

        for dir in dirs {
            inner.seek(io::SeekFrom::Start(block_size as u64 * dir.start))?;
            for i in 0..20 {
                let inode = inner.read_u32::<LittleEndian>()?;
                let rec_len = inner.read_u16::<LittleEndian>()?;
                let name_len = inner.read_u8()?;
                let file_type = inner.read_u8()?;
                let mut name = Vec::new();
                name.resize(name_len as usize, 0);
                inner.read(&mut name)?;
                inner.seek(io::SeekFrom::Current(rec_len as i64 - name_len as i64  - 4 - 2 - 2))?;
                println!("{} {:x} {} {} {:?}", dir.start, file_type, inode, rec_len, std::str::from_utf8(&name));
            }
        }

        Ok(Fs {
            block_size,
        })
    }
}

fn as_u16(buf: &[u8]) -> u16 {
    buf[0] as u16 + buf[1] as u16 * 0x100
}

fn as_u32(buf: &[u8]) -> u32 {
    as_u16(&buf) as u32 + as_u16(&buf[2..]) as u32 * 0x10000
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;

    #[test]
    fn it_works() {
        // s losetup -P -f --show ubuntu-16.04-preinstalled-server-armhf+raspi3.img
        // s chmod a+r /dev/loop0p2
        let file = fs::File::open("/dev/loop0p2").expect("device setup");
        ::Fs::new(&mut io::BufReader::new(file)).expect("success");
    }
}

fn parse_error(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg)
}
