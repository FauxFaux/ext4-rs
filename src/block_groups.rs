use std::io;

use byteorder::{ReadBytesExt, LittleEndian};

const EXT4_BLOCK_GROUP_INODES_UNUSED: u16 = 0b1;
const EXT4_BLOCK_GROUP_BLOCKS_UNUSED: u16 = 0b10;

#[derive(Debug)]
struct Entry {
    inode_table_block: u64,
    inodes: u32,
}

#[derive(Debug)]
pub struct BlockGroups{
    groups: Vec<Entry>,
    inodes_per_group: u32,
    pub block_size: u32,
    inode_size: u16,
}

impl BlockGroups {
    pub fn new<R>(
        mut inner: R,
        blocks_count: u64,
        s_desc_size: u16,
        s_inodes_per_group: u32,
        block_size: u32,
        inode_size: u16) -> io::Result<BlockGroups>
    where R: io::Read + io::Seek {
        let blocks_count = ::usize_check(blocks_count)?;

        let mut groups = Vec::with_capacity(blocks_count);

        for block in 0..blocks_count {
//            let bg_block_bitmap_lo =
                inner.read_u32::<LittleEndian>()?; /* Blocks bitmap block */
//            let bg_inode_bitmap_lo =
                inner.read_u32::<LittleEndian>()?; /* Inodes bitmap block */
            let bg_inode_table_lo =
                inner.read_u32::<LittleEndian>()?; /* Inodes table block */
//            let bg_free_blocks_count_lo =
                inner.read_u16::<LittleEndian>()?; /* Free blocks count */
            let bg_free_inodes_count_lo =
                inner.read_u16::<LittleEndian>()?; /* Free inodes count */
//            let bg_used_dirs_count_lo =
                inner.read_u16::<LittleEndian>()?; /* Directories count */
            let bg_flags =
                inner.read_u16::<LittleEndian>()?; /* EXT4_BG_flags (INODE_UNINIT, etc) */
//            let bg_exclude_bitmap_lo =
                inner.read_u32::<LittleEndian>()?; /* Exclude bitmap for snapshots */
//            let bg_block_bitmap_csum_lo =
                inner.read_u16::<LittleEndian>()?; /* crc32c(s_uuid+grp_num+bbitmap) LE */
//            let bg_inode_bitmap_csum_lo =
                inner.read_u16::<LittleEndian>()?; /* crc32c(s_uuid+grp_num+ibitmap) LE */
//            let bg_itable_unused_lo =
                inner.read_u16::<LittleEndian>()?; /* Unused inodes count */
//            let bg_checksum =
                inner.read_u16::<LittleEndian>()?; /* crc16(sb_uuid+group+desc) */

//            let bg_block_bitmap_hi =
                if s_desc_size < 4 { None } else {
                    Some(inner.read_u32::<LittleEndian>()?) /* Blocks bitmap block MSB */
                };
//            let bg_inode_bitmap_hi =
                if s_desc_size < 4 + 4 { None } else {
                    Some(inner.read_u32::<LittleEndian>()?) /* Inodes bitmap block MSB */
                };
            let bg_inode_table_hi =
                if s_desc_size < 4 + 4 + 4 { None } else {
                    Some(inner.read_u32::<LittleEndian>()?) /* Inodes table block MSB */
                };
//            let bg_free_blocks_count_hi =
                if s_desc_size < 4 + 4 + 4 + 2 { None } else {
                    Some(inner.read_u16::<LittleEndian>()?) /* Free blocks count MSB */
                };
            let bg_free_inodes_count_hi =
                if s_desc_size < 4 + 4 + 4 + 2 + 2 { None } else {
                    Some(inner.read_u16::<LittleEndian>()?) /* Free inodes count MSB */
                };

//          let bg_used_dirs_count_hi =
//              inner.read_u16::<LittleEndian>()?; /* Directories count MSB */
//          let bg_itable_unused_hi =
//              inner.read_u16::<LittleEndian>()?; /* Unused inodes count MSB */
//          let bg_exclude_bitmap_hi =
//              inner.read_u32::<LittleEndian>()?; /* Exclude bitmap block MSB */
//          let bg_block_bitmap_csum_hi =
//              inner.read_u16::<LittleEndian>()?; /* crc32c(s_uuid+grp_num+bbitmap) BE */
//          let bg_inode_bitmap_csum_hi =
//              inner.read_u16::<LittleEndian>()?; /* crc32c(s_uuid+grp_num+ibitmap) BE */

            if s_desc_size > 16 {
                inner.seek(io::SeekFrom::Current((s_desc_size - 16) as i64))?;
            }

            let inode_table_block = bg_inode_table_lo as u64
                | ((bg_inode_table_hi.unwrap_or(0) as u64) << 32);
            let free_inodes_count = bg_free_inodes_count_lo as u32
                | ((bg_free_inodes_count_hi.unwrap_or(0) as u32) << 16);

            let unallocated = bg_flags & EXT4_BLOCK_GROUP_INODES_UNUSED != 0 || bg_flags & EXT4_BLOCK_GROUP_BLOCKS_UNUSED != 0;

            if free_inodes_count > s_inodes_per_group {
                return Err(::parse_error(format!("too many free inodes in group {}: {} > {}",
                                               block, free_inodes_count, s_inodes_per_group)));
            }

            let inodes = if unallocated {
                0
            } else {
                s_inodes_per_group - free_inodes_count
            };

            groups.push(Entry {
                inode_table_block,
                inodes,
            });
        }

        Ok(BlockGroups {
            groups,
            inodes_per_group: s_inodes_per_group,
            block_size,
            inode_size
        })
    }

    pub fn index_of(&self, inode: u32) -> u64 {
        assert_ne!(0, inode);

        let inode = inode - 1;
        let group_number = inode / self.inodes_per_group;
        let group = &self.groups[group_number as usize];
        let inode_index_in_group = inode % self.inodes_per_group;
        assert!(inode_index_in_group < group.inodes,
                "inode <{}> number must fit in group: {} is greater than {} for group {}",
                inode + 1,
                inode_index_in_group, group.inodes, group_number);
        let block = group.inode_table_block;
        block * self.block_size as u64 + inode_index_in_group as u64 * self.inode_size as u64
    }
}