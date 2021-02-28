use std::convert::TryFrom;
use std::io;

use anyhow::ensure;
use anyhow::Error;
use byteorder::{LittleEndian, ReadBytesExt};

use crate::assumption_failed;
use crate::not_found;

const EXT4_BLOCK_GROUP_INODES_UNUSED: u16 = 0b1;
const EXT4_BLOCK_GROUP_BLOCKS_UNUSED: u16 = 0b10;

#[derive(Debug)]
struct Entry {
    inode_table_block: u64,
    max_inode_number: u32,
}

#[derive(Debug)]
pub struct BlockGroup {
    bg_block_bitmap_lo: u32,      /* Blocks bitmap block */
    bg_inode_bitmap_lo: u32,      /* Inodes bitmap block */
    bg_inode_table_lo: u32,       /* Inodes table block */
    bg_free_blocks_count_lo: u16, /* Free blocks count */
    bg_free_inodes_count_lo: u16, /* Free inodes count */
    bg_used_dirs_count_lo: u16,   /* Directories count */
    bg_flags: u16,                /* EXT4_BG_flags (INODE_UNINIT, etc) */
    bg_exclude_bitmap_lo: u32,    /* Exclude bitmap for snapshots */
    bg_block_bitmap_csum_lo: u16, /* crc32c(s_uuid+grp_num+bbitmap) LE */
    bg_inode_bitmap_csum_lo: u16, /* crc32c(s_uuid+grp_num+ibitmap) LE */
    bg_itable_unused_lo: u16,     /* Unused inodes count */
    bg_checksum: u16,             /* crc16(sb_uuid+group+desc) */
    bg_block_bitmap_hi: u32,      /* Blocks bitmap block MSB */
    bg_inode_bitmap_hi: u32,      /* Inodes bitmap block MSB */
    bg_inode_table_hi: u32,       /* Inodes table block MSB */
    bg_free_blocks_count_hi: u16, /* Free blocks count MSB */
    bg_free_inodes_count_hi: u16, /* Free inodes count MSB */
    bg_used_dirs_count_hi: u16,   /* Directories count MSB */
    bg_itable_unused_hi: u16,     /* Unused inodes count MSB */
    bg_exclude_bitmap_hi: u32,    /* Exclude bitmap block MSB */
    bg_block_bitmap_csum_hi: u16, /* crc32c(s_uuid+grp_num+bbitmap) BE */
    bg_inode_bitmap_csum_hi: u16, /* crc32c(s_uuid+grp_num+ibitmap) BE */
    bg_reserved: u32,             /* Padding to 64 bytes */
}

impl BlockGroup {
    pub fn new(buffer: Vec<u8>, s_desc_size: u16) -> Result<BlockGroup, Error> {
        let mut inner = io::Cursor::new(buffer);

        let bg_block_bitmap_lo = inner.read_u32::<LittleEndian>()?; /* Blocks bitmap block */
        let bg_inode_bitmap_lo = inner.read_u32::<LittleEndian>()?; /* Inodes bitmap block */
        let bg_inode_table_lo = inner.read_u32::<LittleEndian>()?; /* Inodes table block */
        let bg_free_blocks_count_lo = inner.read_u16::<LittleEndian>()?; /* Free blocks count */
        let bg_free_inodes_count_lo = inner.read_u16::<LittleEndian>()?; /* Free inodes count */
        let bg_used_dirs_count_lo = inner.read_u16::<LittleEndian>()?; /* Directories count */
        let bg_flags = inner.read_u16::<LittleEndian>()?; /* EXT4_BG_flags (INODE_UNINIT, etc) */
        let bg_exclude_bitmap_lo = inner.read_u32::<LittleEndian>()?; /* Exclude bitmap for snapshots */
        let bg_block_bitmap_csum_lo = inner.read_u16::<LittleEndian>()?; /* crc32c(s_uuid+grp_num+bbitmap) LE */
        let bg_inode_bitmap_csum_lo = inner.read_u16::<LittleEndian>()?; /* crc32c(s_uuid+grp_num+ibitmap) LE */
        let bg_itable_unused_lo = inner.read_u16::<LittleEndian>()?; /* Unused inodes count */
        let bg_checksum = inner.read_u16::<LittleEndian>()?; /* crc16(sb_uuid+group+desc) */

        // In ext2, ext3, and ext4 (when the 64bit feature is not enabled),
        // the block group descriptor was only 32 bytes long and therefore ends at bg_checksum.
        // On an ext4 filesystem with the 64bit feature enabled, the block group descriptor expands
        // to at least the 64 bytes described below; the size is stored in the superblock.

        let bg_block_bitmap_hi = if s_desc_size > 32 {
            inner.read_u32::<LittleEndian>()?
        } else {
            0
        };
        let bg_inode_bitmap_hi = if s_desc_size > 32 {
            inner.read_u32::<LittleEndian>()?
        } else {
            0
        };
        let bg_inode_table_hi = if s_desc_size > 32 {
            inner.read_u32::<LittleEndian>()?
        } else {
            0
        };

        let bg_free_blocks_count_hi = if s_desc_size > 32 {
            inner.read_u16::<LittleEndian>()?
        } else {
            0
        };

        let bg_free_inodes_count_hi = if s_desc_size > 32 {
            inner.read_u16::<LittleEndian>()?
        } else {
            0
        };

        /* Directories count MSB */
        let bg_used_dirs_count_hi = if s_desc_size > 32 {
            inner.read_u16::<LittleEndian>()?
        } else {
            0
        };
        /* Unused inodes count MSB */
        let bg_itable_unused_hi = if s_desc_size > 32 {
            inner.read_u16::<LittleEndian>()?
        } else {
            0
        };
        /* Exclude bitmap block MSB */
        let bg_exclude_bitmap_hi = if s_desc_size > 32 {
            inner.read_u32::<LittleEndian>()?
        } else {
            0
        };
        /* crc32c(s_uuid+grp_num+bbitmap) BE */
        let bg_block_bitmap_csum_hi = if s_desc_size > 32 {
            inner.read_u16::<LittleEndian>()?
        } else {
            0
        };
        /* crc32c(s_uuid+grp_num+ibitmap) BE */
        let bg_inode_bitmap_csum_hi = if s_desc_size > 32 {
            inner.read_u16::<LittleEndian>()?
        } else {
            0
        };

        /* Padding to 64 bytes */
        let bg_reserved = if s_desc_size > 32 {
            inner.read_u32::<LittleEndian>()?
        } else {
            0
        };

        Ok(BlockGroup {
            bg_block_bitmap_lo,
            bg_inode_bitmap_lo,
            bg_inode_table_lo,
            bg_free_blocks_count_lo,
            bg_free_inodes_count_lo,
            bg_used_dirs_count_lo,
            bg_flags,
            bg_exclude_bitmap_lo,
            bg_block_bitmap_csum_lo,
            bg_inode_bitmap_csum_lo,
            bg_itable_unused_lo,
            bg_checksum,
            bg_block_bitmap_hi,
            bg_inode_bitmap_hi,
            bg_inode_table_hi,
            bg_free_blocks_count_hi,
            bg_free_inodes_count_hi,
            bg_used_dirs_count_hi,
            bg_itable_unused_hi,
            bg_exclude_bitmap_hi,
            bg_block_bitmap_csum_hi,
            bg_inode_bitmap_csum_hi,
            bg_reserved,
        })
    }
}

#[derive(Debug)]
pub struct BlockGroups {
    groups: Vec<Entry>,
    inodes_per_group: u32,
    pub block_size: u32,
    pub inode_size: u16,
}

impl BlockGroups {
    pub fn new<R>(
        mut inner: R,
        blocks_count: u64,
        s_desc_size: u16,
        s_inodes_per_group: u32,
        block_size: u32,
        inode_size: u16,
    ) -> Result<BlockGroups, Error>
    where
        R: io::Read + io::Seek,
    {
        let blocks_count = usize::try_from(blocks_count)?;

        let mut groups = Vec::with_capacity(blocks_count);

        for block in 0..blocks_count {
            // let mut entire_blockgroup = [0u8; s_desc_size];
            let bufsize = if s_desc_size == 0 {
                32
            } else {
                s_desc_size.into()
            };
            let mut entire_blockgroup = vec![0u8; bufsize];
            inner.read_exact(&mut entire_blockgroup)?;
            let blockgroup = BlockGroup::new(entire_blockgroup, s_desc_size)?;

            // AA TODO What was this about?
            // if s_desc_size > 16 + 32 {
            //     inner.seek(io::SeekFrom::Current(i64::from(s_desc_size - 32 - 16)))?;
            // }

            let inode_table_block = u64::from(blockgroup.bg_inode_table_lo)
                | ((u64::from(blockgroup.bg_inode_table_hi)) << 32);
            let free_inodes_count = u32::from(blockgroup.bg_free_inodes_count_lo)
                | ((u32::from(blockgroup.bg_free_inodes_count_hi)) << 16);

            let unallocated = blockgroup.bg_flags & EXT4_BLOCK_GROUP_INODES_UNUSED != 0
                || blockgroup.bg_flags & EXT4_BLOCK_GROUP_BLOCKS_UNUSED != 0;

            if free_inodes_count > s_inodes_per_group {
                return Err(crate::parse_error(format!(
                    "too many free inodes in group {}: {} > {}",
                    block, free_inodes_count, s_inodes_per_group
                )));
            }

            let max_inode_number = if unallocated {
                0
            } else {
                // can't use free inodes here, as there can be unallocated ranges in the middle;
                // would have to parse the bitmap to work that out and it doesn't seem worth
                // the effort
                s_inodes_per_group
            };

            groups.push(Entry {
                inode_table_block,
                max_inode_number,
            });
        }

        Ok(BlockGroups {
            groups,
            inodes_per_group: s_inodes_per_group,
            block_size,
            inode_size,
        })
    }

    pub fn index_of(&self, inode: u32) -> Result<u64, Error> {
        ensure!(0 != inode, not_found("there is no inode zero"));

        let inode = inode - 1;
        let group_number = inode / self.inodes_per_group;
        let group = &self.groups[usize::try_from(group_number)?];
        let inode_index_in_group = inode % self.inodes_per_group;
        ensure!(
            inode_index_in_group < group.max_inode_number,
            assumption_failed(format!(
                "inode <{}> number must fit in group: {} is greater than {} for group {}",
                inode + 1,
                inode_index_in_group,
                group.max_inode_number,
                group_number
            ))
        );
        let block = group.inode_table_block;
        Ok(block * u64::from(self.block_size)
            + u64::from(inode_index_in_group) * u64::from(self.inode_size))
    }
}
