use std::io;

use ::Time;
use ::parse_error;

use byteorder::{ReadBytesExt, LittleEndian, BigEndian};

pub fn inode<R>(mut inner: R, inode: u32) -> io::Result<::Inode>
where R: io::Read + io::Seek {
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
//  let i_dtime =
        inner.read_u32::<LittleEndian>()?; /* Deletion Time */
    let i_gid =
        inner.read_u16::<LittleEndian>()?; /* Low 16 bits of Group Id */
    let i_links_count =
        inner.read_u16::<LittleEndian>()?; /* Links count */
//  let i_blocks_lo =
        inner.read_u32::<LittleEndian>()?; /* Blocks count */
    let i_flags =
        inner.read_u32::<LittleEndian>()?; /* File flags */
//  let l_i_version =
    inner.read_u32::<LittleEndian>()?;

    let mut block = [0u8; 15 * 4];
        inner.read_exact(&mut block)?; /* Pointers to blocks */

//  let i_generation =
        inner.read_u32::<LittleEndian>()?; /* File version (for NFS) */
//  let i_file_acl_lo =
        inner.read_u32::<LittleEndian>()?; /* File ACL */
    let i_size_high =
        inner.read_u32::<LittleEndian>()?;
//  let i_obso_faddr =
        inner.read_u32::<LittleEndian>()?; /* Obsoleted fragment address */
//  let l_i_blocks_high =
        inner.read_u16::<LittleEndian>()?;
//  let l_i_file_acl_high =
        inner.read_u16::<LittleEndian>()?;
    let l_i_uid_high =
        inner.read_u16::<LittleEndian>()?;
    let l_i_gid_high =
        inner.read_u16::<LittleEndian>()?;
//  let l_i_checksum_lo =
        inner.read_u16::<LittleEndian>()?; /* crc32c(uuid+inum+inode) LE */
//  let l_i_reserved =
        inner.read_u16::<LittleEndian>()?;
    let i_extra_isize =
        inner.read_u16::<LittleEndian>()?;

//  let i_checksum_hi =
        if i_extra_isize < 2 { None } else {
            Some(inner.read_u16::<BigEndian>()?) /* crc32c(uuid+inum+inode) BE */
        };
    let i_ctime_extra =
        if i_extra_isize < 2 + 4 { None } else {
            Some(inner.read_u32::<LittleEndian>()?) /* extra Change time      (nsec << 2 | epoch) */
        };
    let i_mtime_extra =
        if i_extra_isize < 2 + 4 + 4 { None } else {
            Some(inner.read_u32::<LittleEndian>()?) /* extra Modification time(nsec << 2 | epoch) */
        };
    let i_atime_extra =
        if i_extra_isize < 2 + 4 + 4 + 4 { None } else {
            Some(inner.read_u32::<LittleEndian>()?) /* extra Access time      (nsec << 2 | epoch) */
        };
    let i_crtime =
        if i_extra_isize < 2 + 4 + 4 + 4 + 4 { None } else {
            Some(inner.read_u32::<LittleEndian>()?) /* File Creation time */
        };
    let i_crtime_extra =
        if i_extra_isize < 2 + 4 + 4 + 4 + 4 + 4 { None } else {
            Some(inner.read_u32::<LittleEndian>()?) /* extra FileCreationtime (nsec << 2 | epoch) */
        };
//  let i_version_hi =
        if i_extra_isize < 2 + 4 + 4 + 4 + 4 + 4 + 4 { None } else {
           Some(inner.read_u32::<LittleEndian>()?) /* high 32 bits for 64-bit version */
        };
//  let i_projid =
        if i_extra_isize < 2 + 4 + 4 + 4 + 4 + 4 + 4 + 4 { None } else {
            Some(inner.read_u32::<LittleEndian>()?) /* Project ID */
        };

    // TODO: there could be extended attributes to read here

    let stat = ::Stat {
        extracted_type: ::FileType::from_mode(i_mode)
            .ok_or_else(|| parse_error(format!("unexpected file type in mode: {:b}", i_mode)))?,
        file_mode: i_mode & 0b111_111_111_111,
        uid: i_uid as u32 | ((l_i_uid_high as u32) << 16),
        gid: i_gid as u32 | ((l_i_gid_high as u32) << 16),
        size: (i_size_lo as u64) | ((i_size_high as u64) << 32),
        atime: Time {
            epoch_secs: i_atime,
            nanos: i_atime_extra,
        },
        ctime: Time {
            epoch_secs: i_ctime,
            nanos: i_ctime_extra,
        },
        mtime: Time {
            epoch_secs: i_mtime,
            nanos: i_mtime_extra,
        },
        btime: i_crtime.map(|epoch_secs| Time {
            epoch_secs,
            nanos: i_crtime_extra,
        }),
        link_count: i_links_count,
    };

    Ok(::Inode {
        stat,
        number: inode,
        flags: ::InodeFlags::from_bits(i_flags)
            .expect("unrecognised inode flags"),
        block,
    })
}
