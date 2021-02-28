use std::convert::TryInto;

use crate::read_be16;
use crate::read_le16;
use crate::read_le32;
use crate::read_le64;
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
    pub i_extra_isize: Option<u16>,
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
        assert!(data.len() >= 0x80);
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
            i_extra_isize: if data.len() >= 0x82 {
                Some(read_le16(&data[0x80..]))
            } else {
                None
            },
            i_checksum_hi: if data.len() >= 0x84 {
                Some(read_le16(&data[0x82..]))
            } else {
                None
            },
            i_ctime_extra: if data.len() >= 0x88 {
                Some(read_le32(&data[0x84..]))
            } else {
                None
            },
            i_mtime_extra: if data.len() >= 0x8c {
                Some(read_le32(&data[0x88..]))
            } else {
                None
            },
            i_atime_extra: if data.len() >= 0x90 {
                Some(read_le32(&data[0x8c..]))
            } else {
                None
            },
            i_crtime: if data.len() >= 0x94 {
                Some(read_lei32(&data[0x90..]))
            } else {
                None
            },
            i_crtime_extra: if data.len() >= 0x98 {
                Some(read_le32(&data[0x94..]))
            } else {
                None
            },
            i_version_hi: if data.len() >= 0x9c {
                Some(read_le32(&data[0x98..]))
            } else {
                None
            },
            i_projid: if data.len() >= 0xa0 {
                Some(read_le32(&data[0x9c..]))
            } else {
                None
            },
        }
    }

    pub fn peek_i_extra_isize(data: &[u8]) -> Option<u16> {
        if data.len() >= 0x82 {
            Some(read_le16(&data[0x80..]))
        } else {
            None
        }
    }
}

pub struct RawBlockGroup {
    /* Blocks bitmap block */
    pub bg_block_bitmap_lo: u32,
    /* Inodes bitmap block */
    pub bg_inode_bitmap_lo: u32,
    /* Inodes table block */
    pub bg_inode_table_lo: u32,
    /* Free blocks count */
    pub bg_free_blocks_count_lo: u16,
    /* Free inodes count */
    pub bg_free_inodes_count_lo: u16,
    /* Directories count */
    pub bg_used_dirs_count_lo: u16,
    /* EXT4_BG_flags (INODE_UNINIT, etc) */
    pub bg_flags: u16,
    /* Exclude bitmap for snapshots */
    pub bg_exclude_bitmap_lo: u32,
    /* crc32c(s_uuid+grp_num+bbitmap) LE */
    pub bg_block_bitmap_csum_lo: u16,
    /* crc32c(s_uuid+grp_num+ibitmap) LE */
    pub bg_inode_bitmap_csum_lo: u16,
    /* Unused inodes count */
    pub bg_itable_unused_lo: u16,
    /* crc16(sb_uuid+group+desc) */
    pub bg_checksum: u16,
    /* Blocks bitmap block MSB */
    pub bg_block_bitmap_hi: Option<u32>,
    /* Inodes bitmap block MSB */
    pub bg_inode_bitmap_hi: Option<u32>,
    /* Inodes table block MSB */
    pub bg_inode_table_hi: Option<u32>,
    /* Free blocks count MSB */
    pub bg_free_blocks_count_hi: Option<u16>,
    /* Free inodes count MSB */
    pub bg_free_inodes_count_hi: Option<u16>,
    /* Directories count MSB */
    pub bg_used_dirs_count_hi: Option<u16>,
    /* Unused inodes count MSB */
    pub bg_itable_unused_hi: Option<u16>,
    /* Exclude bitmap block MSB */
    pub bg_exclude_bitmap_hi: Option<u32>,
    /* crc32c(s_uuid+grp_num+bbitmap) BE */
    pub bg_block_bitmap_csum_hi: Option<u16>,
    /* crc32c(s_uuid+grp_num+ibitmap) BE */
    pub bg_inode_bitmap_csum_hi: Option<u16>,
    pub bg_reserved: Option<u32>,
}

impl RawBlockGroup {
    pub fn from_slice(data: &[u8]) -> Self {
        assert!(data.len() >= 0x20);
        Self {
            bg_block_bitmap_lo: read_le32(&data[0x00..]),
            bg_inode_bitmap_lo: read_le32(&data[0x04..]),
            bg_inode_table_lo: read_le32(&data[0x08..]),
            bg_free_blocks_count_lo: read_le16(&data[0x0c..]),
            bg_free_inodes_count_lo: read_le16(&data[0x0e..]),
            bg_used_dirs_count_lo: read_le16(&data[0x10..]),
            bg_flags: read_le16(&data[0x12..]),
            bg_exclude_bitmap_lo: read_le32(&data[0x14..]),
            bg_block_bitmap_csum_lo: read_le16(&data[0x18..]),
            bg_inode_bitmap_csum_lo: read_le16(&data[0x1a..]),
            bg_itable_unused_lo: read_le16(&data[0x1c..]),
            bg_checksum: read_le16(&data[0x1e..]),
            bg_block_bitmap_hi: if data.len() >= 0x24 {
                Some(read_le32(&data[0x20..]))
            } else {
                None
            },
            bg_inode_bitmap_hi: if data.len() >= 0x28 {
                Some(read_le32(&data[0x24..]))
            } else {
                None
            },
            bg_inode_table_hi: if data.len() >= 0x2c {
                Some(read_le32(&data[0x28..]))
            } else {
                None
            },
            bg_free_blocks_count_hi: if data.len() >= 0x2e {
                Some(read_le16(&data[0x2c..]))
            } else {
                None
            },
            bg_free_inodes_count_hi: if data.len() >= 0x30 {
                Some(read_le16(&data[0x2e..]))
            } else {
                None
            },
            bg_used_dirs_count_hi: if data.len() >= 0x32 {
                Some(read_le16(&data[0x30..]))
            } else {
                None
            },
            bg_itable_unused_hi: if data.len() >= 0x34 {
                Some(read_le16(&data[0x32..]))
            } else {
                None
            },
            bg_exclude_bitmap_hi: if data.len() >= 0x38 {
                Some(read_le32(&data[0x34..]))
            } else {
                None
            },
            bg_block_bitmap_csum_hi: if data.len() >= 0x3a {
                Some(read_be16(&data[0x38..]))
            } else {
                None
            },
            bg_inode_bitmap_csum_hi: if data.len() >= 0x3c {
                Some(read_be16(&data[0x3a..]))
            } else {
                None
            },
            bg_reserved: if data.len() >= 0x40 {
                Some(read_le32(&data[0x3c..]))
            } else {
                None
            },
        }
    }
}

pub struct RawSuperblock {
    /* Inodes count */
    pub s_inodes_count: u32,
    /* Blocks count */
    pub s_blocks_count_lo: u32,
    /* Reserved blocks count */
    pub s_r_blocks_count_lo: u32,
    /* Free blocks count */
    pub s_free_blocks_count_lo: u32,
    /* Free inodes count */
    pub s_free_inodes_count: u32,
    /* First Data Block */
    pub s_first_data_block: u32,
    /* Block size */
    pub s_log_block_size: u32,
    /* Allocation cluster size */
    pub s_log_cluster_size: u32,
    /* # Blocks per group */
    pub s_blocks_per_group: u32,
    /* # Clusters per group */
    pub s_clusters_per_group: u32,
    /* # Inodes per group */
    pub s_inodes_per_group: u32,
    /* Mount time */
    pub s_mtime: u32,
    /* Write time */
    pub s_wtime: u32,
    /* Mount count */
    pub s_mnt_count: u16,
    /* Maximal mount count */
    pub s_max_mnt_count: u16,
    /* Magic signature */
    pub s_magic: u16,
    /* File system state */
    pub s_state: u16,
    /* Behaviour when detecting errors */
    pub s_errors: u16,
    /* minor revision level */
    pub s_minor_rev_level: u16,
    /* time of last check */
    pub s_lastcheck: u32,
    /* max. time between checks */
    pub s_checkinterval: u32,
    /* OS */
    pub s_creator_os: u32,
    /* Revision level */
    pub s_rev_level: u32,
    /* Default uid for reserved blocks */
    pub s_def_resuid: u16,
    /* Default gid for reserved blocks */
    pub s_def_resgid: u16,
    /* First non-reserved inode */
    pub s_first_ino: u32,
    /* size of inode structure */
    pub s_inode_size: u16,
    /* block group # of this superblock */
    pub s_block_group_nr: u16,
    /* compatible feature set */
    pub s_feature_compat: u32,
    /* incompatible feature set */
    pub s_feature_incompat: u32,
    /* readonly-compatible feature set */
    pub s_feature_ro_compat: u32,
    /* 128-bit uuid for volume */
    pub s_uuid: [u8; 16],
    /* volume name */
    pub s_volume_name: [u8; 16],
    /* directory where last mounted */
    pub s_last_mounted: [u8; 64],
    /* For compression */
    pub s_algorithm_usage_bitmap: u32,
    /* Nr of blocks to try to preallocate*/
    pub s_prealloc_blocks: u8,
    /* Nr to preallocate for dirs */
    pub s_prealloc_dir_blocks: u8,
    /* Per group desc for online growth */
    pub s_reserved_gdt_blocks: u16,
    /* uuid of journal superblock */
    pub s_journal_uuid: [u8; 16],
    /* inode number of journal file */
    pub s_journal_inum: u32,
    /* device number of journal file */
    pub s_journal_dev: u32,
    /* start of list of inodes to delete */
    pub s_last_orphan: u32,
    /* (actually u32) HTREE hash seed */
    pub s_hash_seed: [u8; 16],
    /* Default hash version to use */
    pub s_def_hash_version: u8,
    pub s_jnl_backup_type: u8,
    /* size of group descriptor */
    pub s_desc_size: u16,
    pub s_default_mount_opts: u32,
    /* First metablock block group */
    pub s_first_meta_bg: u32,
    /* When the filesystem was created */
    pub s_mkfs_time: u32,
    /* (actually u32) Backup of the journal inode */
    pub s_jnl_blocks: [u8; 68],
    /* Blocks count */
    pub s_blocks_count_hi: u32,
    /* Reserved blocks count */
    pub s_r_blocks_count_hi: u32,
    /* Free blocks count */
    pub s_free_blocks_count_hi: u32,
    /* All inodes have at least # bytes */
    pub s_min_extra_isize: u16,
    /* New inodes should reserve # bytes */
    pub s_want_extra_isize: u16,
    /* Miscellaneous flags */
    pub s_flags: u32,
    /* RAID stride */
    pub s_raid_stride: u16,
    /* # seconds to wait in MMP checking */
    pub s_mmp_update_interval: u16,
    /* Block for multi-mount protection */
    pub s_mmp_block: u64,
    /* blocks on all data disks (N*stride)*/
    pub s_raid_stripe_width: u32,
    /* FLEX_BG group size */
    pub s_log_groups_per_flex: u8,
    /* metadata checksum algorithm used */
    pub s_checksum_type: u8,
    /* versioning level for encryption */
    pub s_encryption_level: u8,
    /* Padding to next 32bits */
    pub s_reserved_pad: u8,
    /* nr of lifetime kilobytes written */
    pub s_kbytes_written: u64,
    /* Inode number of active snapshot */
    pub s_snapshot_inum: u32,
    /* sequential ID of active snapshot */
    pub s_snapshot_id: u32,
    /* reserved blocks for active snapshot's future use */
    pub s_snapshot_r_blocks_count: u64,
    /* inode number of the head of the on-disk snapshot list */
    pub s_snapshot_list: u32,
    /* number of fs errors */
    pub s_error_count: u32,
    /* first time an error happened */
    pub s_first_error_time: u32,
    /* inode involved in first error */
    pub s_first_error_ino: u32,
    /* block involved of first error */
    pub s_first_error_block: u64,
    /* function where the error happened */
    pub s_first_error_func: [u8; 32],
    /* line number where error happened */
    pub s_first_error_line: u32,
    /* most recent time of an error */
    pub s_last_error_time: u32,
    /* inode involved in last error */
    pub s_last_error_ino: u32,
    /* line number where error happened */
    pub s_last_error_line: u32,
    /* block involved of last error */
    pub s_last_error_block: u64,
    /* function where the error happened */
    pub s_last_error_func: [u8; 32],
    pub s_mount_opts: [u8; 64],
    /* inode for tracking user quota */
    pub s_usr_quota_inum: u32,
    /* inode for tracking group quota */
    pub s_grp_quota_inum: u32,
    /* overhead blocks/clusters in fs */
    pub s_overhead_clusters: u32,
    /* groups with sparse_super2 SBs */
    pub s_backup_bgs: [u8; 8],
    /* Encryption algorithms in use  */
    pub s_encrypt_algos: [u8; 4],
    /* Salt used for string2key algorithm */
    pub s_encrypt_pw_salt: [u8; 16],
    /* Location of the lost+found inode */
    pub s_lpf_ino: u32,
    /* inode for tracking project quota */
    pub s_prj_quota_inum: u32,
    /* crc32c(uuid) if csum_seed set */
    pub s_checksum_seed: u32,
    pub s_wtime_hi: u8,
    pub s_mtime_hi: u8,
    pub s_mkfs_time_hi: u8,
    pub s_lastcheck_hi: u8,
    pub s_first_error_time_hi: u8,
    pub s_last_error_time_hi: u8,
    pub s_pad: [u8; 2],
    /* Filename __u8set encoding */
    pub s_encoding: u16,
    /* Filename __u8set encoding flags */
    pub s_encoding_flags: u16,
    /* (actually u32) Padding to the end of the block */
    pub s_reserved: [u8; 380],
    /* crc32c(superblock) */
    pub s_checksum: u32,
}

impl RawSuperblock {
    pub fn from_slice(data: &[u8]) -> Self {
        assert!(data.len() >= 0x400);
        Self {
            s_inodes_count: read_le32(&data[0x00..]),
            s_blocks_count_lo: read_le32(&data[0x04..]),
            s_r_blocks_count_lo: read_le32(&data[0x08..]),
            s_free_blocks_count_lo: read_le32(&data[0x0c..]),
            s_free_inodes_count: read_le32(&data[0x10..]),
            s_first_data_block: read_le32(&data[0x14..]),
            s_log_block_size: read_le32(&data[0x18..]),
            s_log_cluster_size: read_le32(&data[0x1c..]),
            s_blocks_per_group: read_le32(&data[0x20..]),
            s_clusters_per_group: read_le32(&data[0x24..]),
            s_inodes_per_group: read_le32(&data[0x28..]),
            s_mtime: read_le32(&data[0x2c..]),
            s_wtime: read_le32(&data[0x30..]),
            s_mnt_count: read_le16(&data[0x34..]),
            s_max_mnt_count: read_le16(&data[0x36..]),
            s_magic: read_le16(&data[0x38..]),
            s_state: read_le16(&data[0x3a..]),
            s_errors: read_le16(&data[0x3c..]),
            s_minor_rev_level: read_le16(&data[0x3e..]),
            s_lastcheck: read_le32(&data[0x40..]),
            s_checkinterval: read_le32(&data[0x44..]),
            s_creator_os: read_le32(&data[0x48..]),
            s_rev_level: read_le32(&data[0x4c..]),
            s_def_resuid: read_le16(&data[0x50..]),
            s_def_resgid: read_le16(&data[0x52..]),
            s_first_ino: read_le32(&data[0x54..]),
            s_inode_size: read_le16(&data[0x58..]),
            s_block_group_nr: read_le16(&data[0x5a..]),
            s_feature_compat: read_le32(&data[0x5c..]),
            s_feature_incompat: read_le32(&data[0x60..]),
            s_feature_ro_compat: read_le32(&data[0x64..]),
            s_uuid: data[0x68..0x78].try_into().expect("sliced"),
            s_volume_name: data[0x78..0x88].try_into().expect("sliced"),
            s_last_mounted: data[0x88..0xc8].try_into().expect("sliced"),
            s_algorithm_usage_bitmap: read_le32(&data[0xc8..]),
            s_prealloc_blocks: data[0xcc],
            s_prealloc_dir_blocks: data[0xcd],
            s_reserved_gdt_blocks: read_le16(&data[0xce..]),
            s_journal_uuid: data[0xd0..0xe0].try_into().expect("sliced"),
            s_journal_inum: read_le32(&data[0xe0..]),
            s_journal_dev: read_le32(&data[0xe4..]),
            s_last_orphan: read_le32(&data[0xe8..]),
            s_hash_seed: data[0xec..0xfc].try_into().expect("sliced"),
            s_def_hash_version: data[0xfc],
            s_jnl_backup_type: data[0xfd],
            s_desc_size: read_le16(&data[0xfe..]),
            s_default_mount_opts: read_le32(&data[0x100..]),
            s_first_meta_bg: read_le32(&data[0x104..]),
            s_mkfs_time: read_le32(&data[0x108..]),
            s_jnl_blocks: data[0x10c..0x150].try_into().expect("sliced"),
            s_blocks_count_hi: read_le32(&data[0x150..]),
            s_r_blocks_count_hi: read_le32(&data[0x154..]),
            s_free_blocks_count_hi: read_le32(&data[0x158..]),
            s_min_extra_isize: read_le16(&data[0x15c..]),
            s_want_extra_isize: read_le16(&data[0x15e..]),
            s_flags: read_le32(&data[0x160..]),
            s_raid_stride: read_le16(&data[0x164..]),
            s_mmp_update_interval: read_le16(&data[0x166..]),
            s_mmp_block: read_le64(&data[0x168..]),
            s_raid_stripe_width: read_le32(&data[0x170..]),
            s_log_groups_per_flex: data[0x174],
            s_checksum_type: data[0x175],
            s_encryption_level: data[0x176],
            s_reserved_pad: data[0x177],
            s_kbytes_written: read_le64(&data[0x178..]),
            s_snapshot_inum: read_le32(&data[0x180..]),
            s_snapshot_id: read_le32(&data[0x184..]),
            s_snapshot_r_blocks_count: read_le64(&data[0x188..]),
            s_snapshot_list: read_le32(&data[0x190..]),
            s_error_count: read_le32(&data[0x194..]),
            s_first_error_time: read_le32(&data[0x198..]),
            s_first_error_ino: read_le32(&data[0x19c..]),
            s_first_error_block: read_le64(&data[0x1a0..]),
            s_first_error_func: data[0x1a8..0x1c8].try_into().expect("sliced"),
            s_first_error_line: read_le32(&data[0x1c8..]),
            s_last_error_time: read_le32(&data[0x1cc..]),
            s_last_error_ino: read_le32(&data[0x1d0..]),
            s_last_error_line: read_le32(&data[0x1d4..]),
            s_last_error_block: read_le64(&data[0x1d8..]),
            s_last_error_func: data[0x1e0..0x200].try_into().expect("sliced"),
            s_mount_opts: data[0x200..0x240].try_into().expect("sliced"),
            s_usr_quota_inum: read_le32(&data[0x240..]),
            s_grp_quota_inum: read_le32(&data[0x244..]),
            s_overhead_clusters: read_le32(&data[0x248..]),
            s_backup_bgs: data[0x24c..0x254].try_into().expect("sliced"),
            s_encrypt_algos: data[0x254..0x258].try_into().expect("sliced"),
            s_encrypt_pw_salt: data[0x258..0x268].try_into().expect("sliced"),
            s_lpf_ino: read_le32(&data[0x268..]),
            s_prj_quota_inum: read_le32(&data[0x26c..]),
            s_checksum_seed: read_le32(&data[0x270..]),
            s_wtime_hi: data[0x274],
            s_mtime_hi: data[0x275],
            s_mkfs_time_hi: data[0x276],
            s_lastcheck_hi: data[0x277],
            s_first_error_time_hi: data[0x278],
            s_last_error_time_hi: data[0x279],
            s_pad: data[0x27a..0x27c].try_into().expect("sliced"),
            s_encoding: read_le16(&data[0x27c..]),
            s_encoding_flags: read_le16(&data[0x27e..]),
            s_reserved: data[0x280..0x3fc].try_into().expect("sliced"),
            s_checksum: read_le32(&data[0x3fc..]),
        }
    }
}
