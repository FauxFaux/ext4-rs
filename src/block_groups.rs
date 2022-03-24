use std::convert::TryFrom;
use std::io;

use anyhow::ensure;
use anyhow::Error;

use crate::assumption_failed;
use crate::not_found;
use crate::raw::RawBlockGroup;

const EXT4_BLOCK_GROUP_INODES_UNUSED: u16 = 0b1;
const EXT4_BLOCK_GROUP_BLOCKS_UNUSED: u16 = 0b10;

#[derive(Debug)]
struct Entry {
    inode_table_block: u64,
    max_inode_number: u32,
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
            let mut data = vec![0u8; usize::from(s_desc_size.clamp(32, 4096))];
            inner.read_exact(&mut data)?;
            let raw = RawBlockGroup::from_slice(&data);

            let inode_table_block = u64::from(raw.bg_inode_table_lo)
                | ((u64::from(raw.bg_inode_table_hi.unwrap_or(0))) << 32);
            let free_inodes_count = u32::from(raw.bg_free_inodes_count_lo)
                | ((u32::from(raw.bg_free_inodes_count_hi.unwrap_or(0))) << 16);

            let unallocated = raw.bg_flags & EXT4_BLOCK_GROUP_INODES_UNUSED != 0
                || raw.bg_flags & EXT4_BLOCK_GROUP_BLOCKS_UNUSED != 0;

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
