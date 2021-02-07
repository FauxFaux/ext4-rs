use std::collections::HashMap;
use std::convert::TryFrom;
use std::io;
use std::io::Read;
use std::io::Seek;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Error;
use bitflags::bitflags;
use byteorder::{ByteOrder, LittleEndian, ReadBytesExt};
use positioned_io::Cursor;
use positioned_io::ReadAt;

use crate::not_found;
use crate::parse_error;
use crate::read_le16;
use crate::read_le32;
use crate::unsupported_feature;
use crate::Time;
use crate::{assumption_failed, read_lei32};

const EXT4_SUPER_MAGIC: u16 = 0xEF53;
const INODE_BASE_LEN: usize = 128;
const XATTR_MAGIC: u32 = 0xEA02_0000;

bitflags! {
    struct CompatibleFeature: u32 {
        const DIR_PREALLOC  = 0x0001;
        const IMAGIC_INODES = 0x0002;
        const HAS_JOURNAL   = 0x0004;
        const EXT_ATTR      = 0x0008;
        const RESIZE_INODE  = 0x0010;
        const DIR_INDEX     = 0x0020;
        const SPARSE_SUPER2 = 0x0200;
    }
}

bitflags! {
    struct CompatibleFeatureReadOnly: u32 {
        const SPARSE_SUPER  = 0x0001;
        const LARGE_FILE    = 0x0002;
        const BTREE_DIR     = 0x0004;
        const HUGE_FILE     = 0x0008;
        const GDT_CSUM      = 0x0010;
        const DIR_NLINK     = 0x0020;
        const EXTRA_ISIZE   = 0x0040;
        const QUOTA         = 0x0100;
        const BIGALLOC      = 0x0200;
        const METADATA_CSUM = 0x0400;
        const READONLY      = 0x1000;
        const PROJECT       = 0x2000;

    }
}

bitflags! {
    struct IncompatibleFeature: u32 {
       const COMPRESSION    = 0x0001;
       const FILETYPE       = 0x0002;
       const RECOVER        = 0x0004; /* Needs recovery */
       const JOURNAL_DEV    = 0x0008; /* Journal device */
       const META_BG        = 0x0010;
       const EXTENTS        = 0x0040; /* extents support */
       const SIXTY_FOUR_BIT = 0x0080;
       const MMP            = 0x0100;
       const FLEX_BG        = 0x0200;
       const EA_INODE       = 0x0400; /* EA in inode */
       const DIRDATA        = 0x1000; /* data in dirent */
       const CSUM_SEED      = 0x2000;
       const LARGEDIR       = 0x4000; /* >2GB or 3-lvl htree */
       const INLINE_DATA    = 0x8000; /* data in inode */
       const ENCRYPT        = 0x10000;
    }
}

pub fn superblock<R>(mut reader: R, options: &crate::Options) -> Result<crate::SuperBlock<R>, Error>
where
    R: ReadAt,
{
    let mut entire_superblock = [0u8; 1024];
    reader.read_exact_at(1024, &mut entire_superblock)?;

    let mut inner = io::Cursor::new(&mut entire_superblock[..]);

    // <a cut -c 9- | fgrep ' s_' | fgrep -v ERR_ | while read ty nam comment; do printf "let %s =\n  inner.read_%s::<LittleEndian>()?; %s\n" $(echo $nam | tr -d ';') $(echo $ty | sed 's/__le/u/; s/__//') $comment; done
    //    let s_inodes_count =
    inner.read_u32::<LittleEndian>()?; /* Inodes count */
    let s_blocks_count_lo = inner.read_u32::<LittleEndian>()?; /* Blocks count */
    //    let s_r_blocks_count_lo =
    inner.read_u32::<LittleEndian>()?; /* Reserved blocks count */
    //    let s_free_blocks_count_lo =
    inner.read_u32::<LittleEndian>()?; /* Free blocks count */
    //    let s_free_inodes_count =
    inner.read_u32::<LittleEndian>()?; /* Free inodes count */
    let s_first_data_block = inner.read_u32::<LittleEndian>()?; /* First Data Block */
    let s_log_block_size = inner.read_u32::<LittleEndian>()?; /* Block size */
    //    let s_log_cluster_size =
    inner.read_u32::<LittleEndian>()?; /* Allocation cluster size */
    let s_blocks_per_group = inner.read_u32::<LittleEndian>()?; /* # Blocks per group */
    //    let s_clusters_per_group =
    inner.read_u32::<LittleEndian>()?; /* # Clusters per group */
    let s_inodes_per_group = inner.read_u32::<LittleEndian>()?; /* # Inodes per group */
    //    let s_mtime =
    inner.read_u32::<LittleEndian>()?; /* Mount time */
    //    let s_wtime =
    inner.read_u32::<LittleEndian>()?; /* Write time */
    //    let s_mnt_count =
    inner.read_u16::<LittleEndian>()?; /* Mount count */
    //    let s_max_mnt_count =
    inner.read_u16::<LittleEndian>()?; /* Maximal mount count */
    let s_magic = inner.read_u16::<LittleEndian>()?; /* Magic signature */

    ensure!(
        EXT4_SUPER_MAGIC == s_magic,
        not_found(format!("invalid magic number: {:x}", s_magic))
    );

    let s_state = inner.read_u16::<LittleEndian>()?; /* File system state */
    //    let s_errors =
    inner.read_u16::<LittleEndian>()?; /* Behaviour when detecting errors */
    //    let s_minor_rev_level =
    inner.read_u16::<LittleEndian>()?; /* minor revision level */
    //    let s_lastcheck =
    inner.read_u32::<LittleEndian>()?; /* time of last check */
    //    let s_checkinterval =
    inner.read_u32::<LittleEndian>()?; /* max. time between checks */
    let s_creator_os = inner.read_u32::<LittleEndian>()?; /* OS */

    ensure!(
        0 == s_creator_os,
        unsupported_feature(format!(
            "only support filesystems created on linux, not '{}'",
            s_creator_os
        ))
    );

    let s_rev_level = inner.read_u32::<LittleEndian>()?; /* Revision level */
    //    let s_def_resuid =
    inner.read_u16::<LittleEndian>()?; /* Default uid for reserved blocks */
    //    let s_def_resgid =
    inner.read_u16::<LittleEndian>()?; /* Default gid for reserved blocks */
    //    let s_first_ino =
    inner.read_u32::<LittleEndian>()?; /* First non-reserved inode */
    let s_inode_size = inner.read_u16::<LittleEndian>()?; /* size of inode structure */
    //    let s_block_group_nr =
    inner.read_u16::<LittleEndian>()?; /* block group # of this superblock */
    let s_feature_compat = inner.read_u32::<LittleEndian>()?; /* compatible feature set */

    let compatible_features = CompatibleFeature::from_bits_truncate(s_feature_compat);

    let load_xattrs = compatible_features.contains(CompatibleFeature::EXT_ATTR);

    let s_feature_incompat = inner.read_u32::<LittleEndian>()?; /* incompatible feature set */

    let incompatible_features =
        IncompatibleFeature::from_bits(s_feature_incompat).ok_or_else(|| {
            parse_error(format!(
                "completely unsupported incompatible feature flag: {:b}",
                s_feature_incompat
            ))
        })?;

    let supported_incompatible_features = IncompatibleFeature::FILETYPE
        | IncompatibleFeature::EXTENTS
        | IncompatibleFeature::FLEX_BG
        | IncompatibleFeature::RECOVER
        | IncompatibleFeature::SIXTY_FOUR_BIT;

    if incompatible_features.intersects(!supported_incompatible_features) {
        return Err(parse_error(format!(
            "some unsupported incompatible feature flags: {:?}",
            incompatible_features & !supported_incompatible_features
        )));
    }

    let long_structs = incompatible_features.contains(IncompatibleFeature::SIXTY_FOUR_BIT);

    let s_feature_ro_compat = inner.read_u32::<LittleEndian>()?; /* readonly-compatible feature set */

    let compatible_features_read_only =
        CompatibleFeatureReadOnly::from_bits_truncate(s_feature_ro_compat);

    let has_checksums =
        compatible_features_read_only.contains(CompatibleFeatureReadOnly::METADATA_CSUM);

    ensure!(
        !(has_checksums
            && compatible_features_read_only.contains(CompatibleFeatureReadOnly::GDT_CSUM)),
        assumption_failed("metadata checksums are incompatible with the GDT checksum feature")
    );

    ensure!(
        has_checksums || crate::Checksums::Required != options.checksums,
        not_found("checksums are disabled, but required by options")
    );

    let mut s_uuid = [0; 16];
    inner.read_exact(&mut s_uuid)?; /* 128-bit uuid for volume */
    let mut s_volume_name = [0u8; 16];
    inner.read_exact(&mut s_volume_name)?; /* volume name */
    let mut s_last_mounted = [0u8; 64];
    inner.read_exact(&mut s_last_mounted)?; /* directory where last mounted */
    //    let s_algorithm_usage_bitmap =
    inner.read_u32::<LittleEndian>()?; /* For compression */
    //    let s_prealloc_blocks =
    inner.read_u8()?; /* Nr of blocks to try to preallocate*/
    //    let s_prealloc_dir_blocks =
    inner.read_u8()?; /* Nr to preallocate for dirs */
    //    let s_reserved_gdt_blocks =
    inner.read_u16::<LittleEndian>()?; /* Per group desc for online growth */
    let mut s_journal_uuid = [0u8; 16];
    inner.read_exact(&mut s_journal_uuid)?; /* uuid of journal superblock */
    //    let s_journal_inum =
    inner.read_u32::<LittleEndian>()?; /* inode number of journal file */
    //    let s_journal_dev =
    inner.read_u32::<LittleEndian>()?; /* device number of journal file */
    //    let s_last_orphan =
    inner.read_u32::<LittleEndian>()?; /* start of list of inodes to delete */
    let mut s_hash_seed = [0u8; 4 * 4];
    inner.read_exact(&mut s_hash_seed)?; /* HTREE hash seed */
    //    let s_def_hash_version =
    inner.read_u8()?; /* Default hash version to use */
    //    let s_jnl_backup_type =
    inner.read_u8()?;
    let s_desc_size = inner.read_u16::<LittleEndian>()?; /* size of group descriptor */
    //    let s_default_mount_opts =
    inner.read_u32::<LittleEndian>()?;
    //    let s_first_meta_bg =
    inner.read_u32::<LittleEndian>()?; /* First metablock block group */
    //    let s_mkfs_time =
    inner.read_u32::<LittleEndian>()?; /* When the filesystem was created */
    let mut s_jnl_blocks = [0; 17 * 4];
    inner.read_exact(&mut s_jnl_blocks)?; /* Backup of the journal inode */

    let s_blocks_count_hi = if !long_structs {
        None
    } else {
        Some(inner.read_u32::<LittleEndian>()?) /* Blocks count */
    };
    ////    let s_r_blocks_count_hi =
    //        if !long_structs { None } else {
    //            Some(inner.read_u32::<LittleEndian>()?) /* Reserved blocks count */
    //        };
    ////    let s_free_blocks_count_hi =
    //        if !long_structs { None } else {
    //            Some(inner.read_u32::<LittleEndian>()?) /* Free blocks count */
    //        };
    ////    let s_min_extra_isize =
    //        if !long_structs { None } else {
    //            Some(inner.read_u16::<LittleEndian>()?) /* All inodes have at least # bytes */
    //        };
    ////    let s_want_extra_isize =
    //        if !long_structs { None } else {
    //            Some(inner.read_u16::<LittleEndian>()?) /* New inodes should reserve # bytes */
    //        };
    ////    let s_flags =
    //        if !long_structs { None } else {
    //            Some(inner.read_u32::<LittleEndian>()?) /* Miscellaneous flags */
    //        };

    // TODO: check s_checksum_type == 1 (crc32c)

    if has_checksums {
        inner.seek(io::SeekFrom::End(-4))?;
        let s_checksum = inner.read_u32::<LittleEndian>()?;
        let expected = ext4_style_crc32c_le(!0, &inner.into_inner()[..(1024 - 4)]);
        ensure!(
            s_checksum == expected,
            assumption_failed(format!(
                "superblock reports checksums supported, but didn't match: {:x} != {:x}",
                s_checksum, expected
            ))
        );
    }

    {
        const S_STATE_UNMOUNTED_CLEANLY: u16 = 0b01;
        const S_STATE_ERRORS_DETECTED: u16 = 0b10;

        if s_state & S_STATE_UNMOUNTED_CLEANLY == 0 || s_state & S_STATE_ERRORS_DETECTED != 0 {
            return Err(parse_error(format!(
                "filesystem is not in a clean state: {:b}",
                s_state
            )));
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
            return Err(parse_error(format!(
                "unexpected block size: 2^{}",
                s_log_block_size + 10
            )));
        }
    };

    if !long_structs {
        ensure!(
            0 == s_desc_size,
            assumption_failed(format!(
                "outside long mode, block group desc size must be zero, not {}",
                s_desc_size
            ))
        );
    }

    ensure!(
        1 == s_rev_level,
        unsupported_feature(format!("rev level {}", s_rev_level))
    );

    let group_table_pos = if 1024 == block_size {
        // for 1k blocks, the table is in the third block, after:
        1024   // boot sector
        + 1024 // superblock
    } else {
        // for other blocks, the boot sector is in the first 1k of the first block,
        // followed by the superblock (also in first block), and the group table is afterwards
        block_size
    };

    let mut grouper = Cursor::new(&mut reader);
    grouper.seek(io::SeekFrom::Start(u64::from(group_table_pos)))?;
    let blocks_count = (u64::from(s_blocks_count_lo)
        + (u64::from(s_blocks_count_hi.unwrap_or(0)) << 32)
        - u64::from(s_first_data_block)
        + u64::from(s_blocks_per_group)
        - 1)
        / u64::from(s_blocks_per_group);

    let groups = crate::block_groups::BlockGroups::new(
        &mut grouper,
        blocks_count,
        s_desc_size,
        s_inodes_per_group,
        block_size,
        s_inode_size,
    )?;

    let uuid_checksum = if has_checksums {
        // TODO: check s_checksum_seed
        Some(ext4_style_crc32c_le(!0, &s_uuid))
    } else {
        None
    };

    Ok(crate::SuperBlock {
        inner: reader,
        load_xattrs,
        uuid_checksum,
        groups,
    })
}

pub struct ParsedInode {
    pub stat: crate::Stat,
    pub flags: crate::InodeFlags,
    pub core: [u8; crate::INODE_CORE_SIZE],
    pub checksum_prefix: Option<u32>,
}

pub fn inode<F>(
    mut data: Vec<u8>,
    load_block: F,
    uuid_checksum: Option<u32>,
    number: u32,
) -> Result<ParsedInode, Error>
where
    F: FnOnce(u64) -> Result<Vec<u8>, Error>,
{
    ensure!(
        data.len() >= INODE_BASE_LEN,
        assumption_failed("inode isn't bigger than the minimum length")
    );

    // generated from inode.spec by structs.py
    let i_mode = read_le16(&data[0x00..0x02]); /* File mode */
    let i_uid = read_le16(&data[0x02..0x04]); /* Low 16 bits of Owner Uid */
    let i_size_lo = read_le32(&data[0x04..0x08]); /* Size in bytes */
    let i_atime = read_lei32(&data[0x08..0x0C]); /* Access time */
    let i_ctime = read_lei32(&data[0x0C..0x10]); /* Inode Change time */
    let i_mtime = read_lei32(&data[0x10..0x14]); /* Modification time */
    //    let i_dtime           = read_le32(&data[0x14..0x18]); /* Deletion Time */
    let i_gid = read_le16(&data[0x18..0x1A]); /* Low 16 bits of Group Id */
    let i_links_count = read_le16(&data[0x1A..0x1C]); /* Links count */
    //    let i_blocks_lo       = read_le32(&data[0x1C..0x20]); /* Blocks count */
    let i_flags = read_le32(&data[0x20..0x24]); /* File flags */
    //    let l_i_version       = read_le32(&data[0x24..0x28]);

    let mut i_block = [0u8; crate::INODE_CORE_SIZE];
    i_block.clone_from_slice(&data[0x28..0x64]); /* Pointers to blocks */

    let i_generation = read_le32(&data[0x64..0x68]); /* File version (for NFS) */
    let i_file_acl_lo = read_le32(&data[0x68..0x6C]); /* File ACL */
    let i_size_high = read_le32(&data[0x6C..0x70]);
    //    let i_obso_faddr      = read_le32(&data[0x70..0x74]); /* Obsoleted fragment address */
    //    let l_i_blocks_high   = read_le16(&data[0x74..0x76]); /* were l_i_reserved1 */
    let l_i_file_acl_high = read_le16(&data[0x76..0x78]);
    let l_i_uid_high = read_le16(&data[0x78..0x7A]); /* these 2 fields */
    let l_i_gid_high = read_le16(&data[0x7A..0x7C]); /* were reserved2[0] */
    let l_i_checksum_lo = read_le16(&data[0x7C..0x7E]); /* crc32c(uuid+inum+inode) LE */
    //    let l_i_reserved      = read_le16(&data[0x7E..0x80]);

    let i_extra_isize = if data.len() < 0x82 {
        0
    } else {
        read_le16(&data[0x80..0x82])
    };
    let inode_end = INODE_BASE_LEN + usize::try_from(i_extra_isize)?;

    ensure!(
        inode_end <= data.len(),
        assumption_failed(format!(
            "more extra inode ({}) than inode ({})",
            inode_end,
            data.len()
        ))
    );

    let i_checksum_hi = if i_extra_isize < 2 + 2 {
        None
    } else {
        Some(read_le16(&data[0x82..0x84]))
    }; /* crc32c(uuid+inum+inode) BE */
    let i_ctime_extra = if i_extra_isize < 6 + 2 {
        None
    } else {
        Some(read_le32(&data[0x84..0x88]))
    }; /* extra Change time      (nsec << 2 | epoch) */
    let i_mtime_extra = if i_extra_isize < 10 + 2 {
        None
    } else {
        Some(read_le32(&data[0x88..0x8C]))
    }; /* extra Modification time(nsec << 2 | epoch) */
    let i_atime_extra = if i_extra_isize < 14 + 2 {
        None
    } else {
        Some(read_le32(&data[0x8C..0x90]))
    }; /* extra Access time      (nsec << 2 | epoch) */
    let i_crtime = if i_extra_isize < 18 + 2 {
        None
    } else {
        Some(read_lei32(&data[0x90..0x94]))
    }; /* File Creation time */
    let i_crtime_extra = if i_extra_isize < 22 + 2 {
        None
    } else {
        Some(read_le32(&data[0x94..0x98]))
    }; /* extra FileCreationtime (nsec << 2 | epoch) */
    //    let i_version_hi      = if i_extra_isize < 26 { None } else { Some(read_le32(&data[0x98..0x9C])) }; /* high 32 bits for 64-bit version */
    //    let i_projid          = if i_extra_isize < 30 { None } else { Some(read_le32(&data[0x9C..0xA0])) }; /* Project ID */
    let mut checksum_prefix = None;

    if let Some(uuid_checksum) = uuid_checksum {
        data[0x7C] = 0;
        data[0x7D] = 0;

        let mut bytes = [0u8; 8];
        LittleEndian::write_u32(&mut bytes[0..4], number);
        LittleEndian::write_u32(&mut bytes[4..8], i_generation);
        checksum_prefix = Some(ext4_style_crc32c_le(uuid_checksum, &bytes));

        if i_checksum_hi.is_some() {
            data[0x82] = 0;
            data[0x83] = 0;
        }

        let computed = ext4_style_crc32c_le(checksum_prefix.unwrap(), &data);

        if let Some(high) = i_checksum_hi {
            let expected = u32::from(l_i_checksum_lo) | (u32::from(high) << 16);
            ensure!(
                expected == computed,
                assumption_failed(format!(
                    "full checksum mismatch: on-disc: {:08x} computed: {:08x}",
                    expected, computed
                ))
            );
        } else {
            let short_computed = u16::try_from(computed & 0xFFFF).unwrap();
            ensure!(
                l_i_checksum_lo == short_computed,
                assumption_failed(format!(
                    "short checksum mismatch: on-disc: {:04x} computed: {:04x}",
                    l_i_checksum_lo, short_computed
                ))
            );
        }
    }

    // extended attributes after the inode
    let mut xattrs = HashMap::new();

    if inode_end + 4 <= data.len() && XATTR_MAGIC == read_le32(&data[inode_end..(inode_end + 4)]) {
        let table_start = &data[inode_end + 4..];
        read_xattrs(&mut xattrs, table_start, table_start)?;
    }

    if 0 != i_file_acl_lo || 0 != l_i_file_acl_high {
        let block = u64::from(i_file_acl_lo) | (u64::from(l_i_file_acl_high) << 32);

        xattr_block(&mut xattrs, load_block(block)?, uuid_checksum, block)
            .with_context(|| anyhow!("loading xattr block {}", block))?
    }

    let stat = crate::Stat {
        extracted_type: crate::FileType::from_mode(i_mode).ok_or_else(|| {
            unsupported_feature(format!("unexpected file type in mode: {:b}", i_mode))
        })?,
        file_mode: i_mode & 0b111_111_111_111,
        uid: u32::from(i_uid) | (u32::from(l_i_uid_high) << 16),
        gid: u32::from(i_gid) | (u32::from(l_i_gid_high) << 16),
        size: u64::from(i_size_lo) | (u64::from(i_size_high) << 32),
        atime: Time::from_extra(i_atime, i_atime_extra),
        ctime: Time::from_extra(i_ctime, i_ctime_extra),
        mtime: Time::from_extra(i_mtime, i_mtime_extra),
        btime: i_crtime.map(|i_crtime| Time::from_extra(i_crtime, i_crtime_extra)),
        link_count: i_links_count,
        xattrs,
    };

    Ok(ParsedInode {
        stat,
        flags: crate::InodeFlags::from_bits(i_flags).ok_or_else(|| {
            unsupported_feature(format!("unrecognised inode flags: {:b}", i_flags))
        })?,
        core: i_block,
        checksum_prefix,
    })
}

fn xattr_block(
    xattrs: &mut HashMap<String, Vec<u8>>,
    mut data: Vec<u8>,
    uuid_checksum: Option<u32>,
    block_number: u64,
) -> Result<(), Error> {
    ensure!(
        data.len() > 0x20,
        assumption_failed("xattr block is way too short")
    );

    ensure!(
        XATTR_MAGIC == read_le32(&data[0x00..0x04]),
        assumption_failed("xattr block contained invalid magic number")
    );

    //  let x_refcount    = read_le32(&data[0x04..0x08]);
    let x_blocks_used = read_le32(&data[0x08..0x0C]);
    //    let x_hash        = read_le32(&data[0x0C..0x10]);
    let x_checksum = read_le32(&data[0x10..0x14]);
    // [some reserved fields]

    if let Some(uuid_checksum) = uuid_checksum {
        data[0x10] = 0;
        data[0x11] = 0;
        data[0x12] = 0;
        data[0x13] = 0;

        let mut bytes = [0u8; 8];
        LittleEndian::write_u64(&mut bytes[0..8], block_number);

        let base = ext4_style_crc32c_le(uuid_checksum, &bytes);
        let computed = ext4_style_crc32c_le(base, &data);
        ensure!(
            x_checksum == computed,
            assumption_failed(format!(
                "xattr block checksum invalid: on-disk: {:08x}, computed: {:08x}",
                x_checksum, computed
            ))
        );
    }

    ensure!(
        1 == x_blocks_used,
        unsupported_feature(format!(
            "must have exactly one xattr block, not {}",
            x_blocks_used
        ))
    );

    read_xattrs(xattrs, &data[0x20..], &data[..])
}

fn read_xattrs(
    xattrs: &mut HashMap<String, Vec<u8>>,
    mut reading: &[u8],
    block_offset_start: &[u8],
) -> Result<(), Error> {
    loop {
        ensure!(
            reading.len() > 0x10,
            assumption_failed("out of block while reading xattr header")
        );

        let e_name_len = reading[0x00];
        let e_name_prefix_magic = reading[0x01];
        let e_value_offset = read_le16(&reading[0x02..0x04]);
        let e_block = read_le32(&reading[0x04..0x08]);

        if 0 == e_name_len && 0 == e_name_prefix_magic && 0 == e_value_offset && 0 == e_block {
            break;
        }

        let e_value_size = read_le32(&reading[0x08..0x0C]);
        //        let e_hash              = read_le32(&reading[0x0C..0x10]);

        let end_of_name = 0x10 + usize::try_from(e_name_len)?;

        ensure!(
            reading.len() > end_of_name,
            assumption_failed("out of block while reading xattr name")
        );

        let name_suffix = &reading[0x10..end_of_name];

        let name = format!(
            "{}{}",
            match e_name_prefix_magic {
                0 => "",
                1 => "user.",
                2 => "system.posix_acl_access",
                3 => "system.posix_acl_default",
                4 => "trusted.",
                6 => "security.",
                7 => "system.",
                _ => bail!(unsupported_feature(format!(
                    "unsupported name prefix encoding: {}",
                    e_name_prefix_magic
                ))),
            },
            std::str::from_utf8(name_suffix).with_context(|| anyhow!("name is invalid utf-8"))?
        );

        let start = usize::try_from(e_value_offset)?;
        let end = start + usize::try_from(e_value_size)?;

        ensure!(
            start <= block_offset_start.len() && end <= block_offset_start.len(),
            assumption_failed(format!(
                "xattr value out of range: {}-{} > {}",
                start,
                end,
                block_offset_start.len()
            ))
        );

        xattrs.insert(name, block_offset_start[start..end].to_vec());

        let next_record = end_of_name + ((4 - (end_of_name % 4)) % 4);
        reading = &reading[next_record..];
    }

    Ok(())
}

/// This is what the function in the ext4 code does, based on its results. I'm so sorry.
pub fn ext4_style_crc32c_le(seed: u32, buf: &[u8]) -> u32 {
    crc::crc32::update(seed ^ (!0), &crc::crc32::CASTAGNOLI_TABLE, buf) ^ (!0u32)
}

#[cfg(test)]
mod tests {
    use super::ext4_style_crc32c_le;

    #[test]
    fn crcs() {
        /*
        Comparing with:
        % gcc m.c crc32c.c -I .. -o m && ./m
        e2fsprogs-1.43.4/lib/ext2fs% cat m.c
            int main() {
                printf("%08x\n", ext2fs_crc32c_le(SEED, DATA, DATA.len()));
            }

            typedef unsigned int __u32;

            __u32 ext2fs_swab32(__u32 val)
            {
                return ((val>>24) | ((val>>8)&0xFF00) |
                    ((val<<8)&0xFF0000) | (val<<24));
            }
        */

        assert_eq!(0xffff_ffffu32, !0);
        // e3069283 is the "standard" test vector that you can Google up.
        assert_eq!(0x1cf96d7cu32, 0xe3069283u32 ^ !0);
        assert_crc(0x1cf96d7c, !0, b"123456789");
        assert_crc(0x58e3fa20, 0, b"123456789");
    }

    fn assert_crc(ex: u32, seed: u32, input: &[u8]) {
        let ac = ext4_style_crc32c_le(seed, input);
        if ex != ac {
            panic!(
                "CRC didn't match! ex: {:08x}, ac: {:08x}, len: {}",
                ex,
                ac,
                input.len()
            );
        }
    }
}
