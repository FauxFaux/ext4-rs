use std::convert::TryInto;

use crate::read_le16;
use crate::read_le32;
use crate::read_lei32;

pub struct RawInode {
    /* File mode */
    pub i_mode: u16,
    /* Low 16 bits of Owner Uid */
    pub i_uid: u16,
    /* Size in bytes */
    pub i_size_lo: u32,
    /* Access time */
    pub i_atime: i32,
    /* Inode Change time */
    pub i_ctime: i32,
    /* Modification time */
    pub i_mtime: i32,
    /* Deletion Time */
    pub i_dtime: i32,
    /* Low 16 bits of Group Id */
    pub i_gid: u16,
    /* Links count */
    pub i_links_count: u16,
    /* Blocks count */
    pub i_blocks_lo: u32,
    /* File flags */
    pub i_flags: u32,
    pub l_i_version: u32,
    /* Pointers to blocks */
    pub i_block: [u8; 60],
    /* File version (for NFS) */
    pub i_generation: u32,
    /* File ACL */
    pub i_file_acl_lo: u32,
    pub i_size_high: u32,
    /* Obsoleted fragment address */
    pub i_obso_faddr: u32,
    /* were l_i_reserved1 */
    pub l_i_blocks_high: u16,
    pub l_i_file_acl_high: u16,
    /* these 2 fields */
    pub l_i_uid_high: u16,
    /* were reserved2[0] */
    pub l_i_gid_high: u16,
    /* crc32c(uuid+inum+inode) LE */
    pub l_i_checksum_lo: u16,
    pub l_i_reserved: u16,
    pub i_extra_isize: u16,
    /* crc32c(uuid+inum+inode) BE */
    pub i_checksum_hi: Option<u16>,
    /* extra Change time      (nsec << 2 | epoch) */
    pub i_ctime_extra: Option<u32>,
    /* extra Modification time(nsec << 2 | epoch) */
    pub i_mtime_extra: Option<u32>,
    /* extra Access time      (nsec << 2 | epoch) */
    pub i_atime_extra: Option<u32>,
    /* File Creation time */
    pub i_crtime: Option<i32>,
    /* extra FileCreationtime (nsec << 2 | epoch) */
    pub i_crtime_extra: Option<u32>,
    /* high 32 bits for 64-bit version */
    pub i_version_hi: Option<u32>,
    /* Project ID */
    pub i_projid: Option<u32>,
}

impl RawInode {
    pub fn from_slice(data: &[u8]) -> Self {
        let i_extra_isize = read_le16(&data[0x80..]);

        Self {
            i_mode: read_le16(&data[0x00..]),
            i_uid: read_le16(&data[0x02..]),
            i_size_lo: read_le32(&data[0x04..]),
            i_atime: read_lei32(&data[0x08..]),
            i_ctime: read_lei32(&data[0x0c..]),
            i_mtime: read_lei32(&data[0x10..]),
            i_dtime: read_lei32(&data[0x14..]),
            i_gid: read_le16(&data[0x18..]),
            i_links_count: read_le16(&data[0x1a..]),
            i_blocks_lo: read_le32(&data[0x1c..]),
            i_flags: read_le32(&data[0x20..]),
            l_i_version: read_le32(&data[0x24..]),
            i_block: data[0x28..0x64].try_into().expect("sliced"),
            i_generation: read_le32(&data[0x64..]),
            i_file_acl_lo: read_le32(&data[0x68..]),
            i_size_high: read_le32(&data[0x6c..]),
            i_obso_faddr: read_le32(&data[0x70..]),
            l_i_blocks_high: read_le16(&data[0x74..]),
            l_i_file_acl_high: read_le16(&data[0x76..]),
            l_i_uid_high: read_le16(&data[0x78..]),
            l_i_gid_high: read_le16(&data[0x7a..]),
            l_i_checksum_lo: read_le16(&data[0x7c..]),
            l_i_reserved: read_le16(&data[0x7e..]),
            i_extra_isize,
            i_checksum_hi: if i_extra_isize >= 2 {
                Some(read_le16(&data[0x82..]))
            } else {
                None
            },
            i_ctime_extra: if i_extra_isize >= 6 {
                Some(read_le32(&data[0x84..]))
            } else {
                None
            },
            i_mtime_extra: if i_extra_isize >= 10 {
                Some(read_le32(&data[0x88..]))
            } else {
                None
            },
            i_atime_extra: if i_extra_isize >= 14 {
                Some(read_le32(&data[0x8c..]))
            } else {
                None
            },
            i_crtime: if i_extra_isize >= 18 {
                Some(read_lei32(&data[0x90..]))
            } else {
                None
            },
            i_crtime_extra: if i_extra_isize >= 22 {
                Some(read_le32(&data[0x94..]))
            } else {
                None
            },
            i_version_hi: if i_extra_isize >= 26 {
                Some(read_le32(&data[0x98..]))
            } else {
                None
            },
            i_projid: if i_extra_isize >= 30 {
                Some(read_le32(&data[0x9c..]))
            } else {
                None
            },
        }
    }
}
