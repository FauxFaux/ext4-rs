use std::collections::HashMap;
use std::convert::TryFrom;
use std::io;
use std::io::Seek;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Error;
use bitflags::bitflags;
use byteorder::{ByteOrder, LittleEndian};
use positioned_io::Cursor;
use positioned_io::ReadAt;

use crate::assumption_failed;
use crate::not_found;
use crate::parse_error;
use crate::raw::{RawInode, RawSuperblock};
use crate::read_le16;
use crate::read_le32;
use crate::unsupported_feature;
use crate::Time;

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

    let raw = RawSuperblock::from_slice(&entire_superblock);

    ensure!(
        EXT4_SUPER_MAGIC == raw.s_magic,
        not_found(format!("invalid magic number: {:x}", raw.s_magic))
    );

    ensure!(
        0 == raw.s_creator_os,
        unsupported_feature(format!(
            "only support filesystems created on linux, not '{}'",
            raw.s_creator_os
        ))
    );

    let compatible_features = CompatibleFeature::from_bits_truncate(raw.s_feature_compat);

    let load_xattrs = compatible_features.contains(CompatibleFeature::EXT_ATTR);

    let incompatible_features =
        IncompatibleFeature::from_bits(raw.s_feature_incompat).ok_or_else(|| {
            parse_error(format!(
                "completely unsupported incompatible feature flag: {:b}",
                raw.s_feature_incompat
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

    let compatible_features_read_only =
        CompatibleFeatureReadOnly::from_bits_truncate(raw.s_feature_ro_compat);

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

    // TODO: check s_checksum_type == 1 (crc32c)

    if has_checksums {
        let expected = ext4_style_crc32c_le(!0, &entire_superblock[..(1024 - 4)]);
        ensure!(
            raw.s_checksum == expected,
            assumption_failed(format!(
                "superblock reports checksums supported, but didn't match: {:x} != {:x}",
                raw.s_checksum, expected
            ))
        );
    }

    {
        const S_STATE_UNMOUNTED_CLEANLY: u16 = 0b01;
        const S_STATE_ERRORS_DETECTED: u16 = 0b10;

        if raw.s_state & S_STATE_UNMOUNTED_CLEANLY == 0
            || raw.s_state & S_STATE_ERRORS_DETECTED != 0
        {
            return Err(parse_error(format!(
                "filesystem is not in a clean state: {:b}",
                raw.s_state
            )));
        }
    }

    if 0 == raw.s_inodes_per_group {
        return Err(parse_error("inodes per group cannot be zero".to_string()));
    }

    let block_size: u32 = match raw.s_log_block_size {
        0 => 1024,
        1 => 2048,
        2 => 4096,
        6 => 65536,
        _ => {
            return Err(parse_error(format!(
                "unexpected block size: 2^{}",
                raw.s_log_block_size + 10
            )));
        }
    };

    if !long_structs {
        ensure!(
            0 == raw.s_desc_size,
            assumption_failed(format!(
                "outside long mode, block group desc size must be zero, not {}",
                raw.s_desc_size
            ))
        );
    }

    ensure!(
        1 == raw.s_rev_level,
        unsupported_feature(format!("rev level {}", raw.s_rev_level))
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
    let blocks_count = (u64::from(raw.s_blocks_count_lo)
        + (u64::from(raw.s_blocks_count_hi) << 32)
        - u64::from(raw.s_first_data_block)
        + u64::from(raw.s_blocks_per_group)
        - 1)
        / u64::from(raw.s_blocks_per_group);

    let groups = crate::block_groups::BlockGroups::new(
        &mut grouper,
        blocks_count,
        raw.s_desc_size,
        raw.s_inodes_per_group,
        block_size,
        raw.s_inode_size,
    )?;

    let uuid_checksum = if has_checksums {
        // TODO: check s_checksum_seed
        Some(ext4_style_crc32c_le(!0, &raw.s_uuid))
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

    let i_extra_isize = RawInode::peek_i_extra_isize(&data).unwrap_or(0);
    let inode_end = INODE_BASE_LEN + usize::try_from(i_extra_isize)?;

    ensure!(
        inode_end <= data.len(),
        assumption_failed(format!(
            "more extra inode ({}) than inode ({})",
            inode_end,
            data.len()
        ))
    );

    let raw = RawInode::from_slice(&data[..inode_end]);

    let mut checksum_prefix = None;

    if let Some(uuid_checksum) = uuid_checksum {
        data[0x7C] = 0;
        data[0x7D] = 0;

        let mut bytes = [0u8; 8];
        LittleEndian::write_u32(&mut bytes[0..4], number);
        LittleEndian::write_u32(&mut bytes[4..8], raw.i_generation);
        checksum_prefix = Some(ext4_style_crc32c_le(uuid_checksum, &bytes));

        if raw.i_checksum_hi.is_some() {
            data[0x82] = 0;
            data[0x83] = 0;
        }

        let computed = ext4_style_crc32c_le(checksum_prefix.unwrap(), &data);

        if let Some(high) = raw.i_checksum_hi {
            let expected = u32::from(raw.l_i_checksum_lo) | (u32::from(high) << 16);
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
                raw.l_i_checksum_lo == short_computed,
                assumption_failed(format!(
                    "short checksum mismatch: on-disc: {:04x} computed: {:04x}",
                    raw.l_i_checksum_lo, short_computed
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

    if 0 != raw.i_file_acl_lo || 0 != raw.l_i_file_acl_high {
        let block = u64::from(raw.i_file_acl_lo) | (u64::from(raw.l_i_file_acl_high) << 32);

        xattr_block(&mut xattrs, load_block(block)?, uuid_checksum, block)
            .with_context(|| anyhow!("loading xattr block {}", block))?
    }

    let stat = crate::Stat {
        extracted_type: crate::FileType::from_mode(raw.i_mode).ok_or_else(|| {
            unsupported_feature(format!("unexpected file type in mode: {:b}", raw.i_mode))
        })?,
        file_mode: raw.i_mode & 0b111_111_111_111,
        uid: u32::from(raw.i_uid) | (u32::from(raw.l_i_uid_high) << 16),
        gid: u32::from(raw.i_gid) | (u32::from(raw.l_i_gid_high) << 16),
        size: u64::from(raw.i_size_lo) | (u64::from(raw.i_size_high) << 32),
        atime: Time::from_extra(raw.i_atime, raw.i_atime_extra),
        ctime: Time::from_extra(raw.i_ctime, raw.i_ctime_extra),
        mtime: Time::from_extra(raw.i_mtime, raw.i_mtime_extra),
        btime: raw
            .i_crtime
            .map(|i_crtime| Time::from_extra(i_crtime, raw.i_crtime_extra)),
        link_count: raw.i_links_count,
        xattrs,
    };

    Ok(ParsedInode {
        stat,
        flags: crate::InodeFlags::from_bits(raw.i_flags).ok_or_else(|| {
            unsupported_feature(format!("unrecognised inode flags: {:b}", raw.i_flags))
        })?,
        core: raw.i_block,
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
