__le16 i_mode;         /* File mode */
__le16 i_uid;          /* Low 16 bits of Owner Uid */
__le32 i_size_lo;      /* Size in bytes */
__lei32 i_atime;       /* Access time */
__lei32 i_ctime;       /* Inode Change time */
__lei32 i_mtime;       /* Modification time */
__lei32 i_dtime;       /* Deletion Time */
__le16 i_gid;          /* Low 16 bits of Group Id */
__le16 i_links_count;  /* Links count */
__le32 i_blocks_lo;    /* Blocks count */
__le32 i_flags;        /* File flags */
__le32 l_i_version;
__u8 i_block[60];      /* Pointers to blocks */
__le32 i_generation;   /* File version (for NFS) */
__le32 i_file_acl_lo;  /* File ACL */
__le32 i_size_high;
__le32 i_obso_faddr;   /* Obsoleted fragment address */
__le16 l_i_blocks_high; /* were l_i_reserved1 */
__le16 l_i_file_acl_high;
__le16 l_i_uid_high;   /* these 2 fields */
__le16 l_i_gid_high;   /* were reserved2[0] */
__le16 l_i_checksum_lo;/* crc32c(uuid+inum+inode) LE */
__le16 l_i_reserved;
__le16 i_extra_isize;
extra_size i_extra_isize;
__le16 i_checksum_hi;  /* crc32c(uuid+inum+inode) BE */
__le32 i_ctime_extra;  /* extra Change time      (nsec << 2 | epoch) */
__le32 i_mtime_extra;  /* extra Modification time(nsec << 2 | epoch) */
__le32 i_atime_extra;  /* extra Access time      (nsec << 2 | epoch) */
__lei32 i_crtime;      /* File Creation time */
__le32 i_crtime_extra; /* extra FileCreationtime (nsec << 2 | epoch) */
__le32 i_version_hi;   /* high 32 bits for 64-bit version */
__le32 i_projid;       /* Project ID */

