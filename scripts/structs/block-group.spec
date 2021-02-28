__le32 bg_block_bitmap_lo;      /* Blocks bitmap block */
__le32 bg_inode_bitmap_lo;      /* Inodes bitmap block */
__le32 bg_inode_table_lo;       /* Inodes table block */
__le16 bg_free_blocks_count_lo; /* Free blocks count */
__le16 bg_free_inodes_count_lo; /* Free inodes count */
__le16 bg_used_dirs_count_lo;   /* Directories count */
__le16 bg_flags;                /* EXT4_BG_flags (INODE_UNINIT, etc) */
__le32 bg_exclude_bitmap_lo;    /* Exclude bitmap for snapshots */
__le16 bg_block_bitmap_csum_lo; /* crc32c(s_uuid+grp_num+bbitmap) LE */
__le16 bg_inode_bitmap_csum_lo; /* crc32c(s_uuid+grp_num+ibitmap) LE */
__le16 bg_itable_unused_lo;     /* Unused inodes count */
__le16 bg_checksum;             /* crc16(sb_uuid+group+desc) */
__le32 bg_block_bitmap_hi;      /* Blocks bitmap block MSB */
__le32 bg_inode_bitmap_hi;      /* Inodes bitmap block MSB */
__le32 bg_inode_table_hi;       /* Inodes table block MSB */
__le16 bg_free_blocks_count_hi; /* Free blocks count MSB */
__le16 bg_free_inodes_count_hi; /* Free inodes count MSB */
__le16 bg_used_dirs_count_hi;   /* Directories count MSB */
__le16 bg_itable_unused_hi;     /* Unused inodes count MSB */
__le32 bg_exclude_bitmap_hi;    /* Exclude bitmap block MSB */
__be16 bg_block_bitmap_csum_hi; /* crc32c(s_uuid+grp_num+bbitmap) BE */
__be16 bg_inode_bitmap_csum_hi; /* crc32c(s_uuid+grp_num+ibitmap) BE */
__le32 bg_reserved;
