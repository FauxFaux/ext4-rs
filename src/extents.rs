use std;
use std::io;

use read_le16;
use read_le32;

use errors::Result;
use errors::ErrorKind::*;

#[derive(Debug)]
struct Extent {
    /// The docs call this 'block' (like everything else). I've invented a different name.
    part: u32,
    start: u64,
    len: u16,
}

pub struct TreeReader<R> {
    inner: R,
    pos: u64,
    len: u64,
    block_size: u32,
    extents: Vec<Extent>,
}

impl<R> TreeReader<R>
where
    R: io::Read + io::Seek,
{
    pub fn new(
        mut inner: R,
        block_size: u32,
        size: u64,
        core: [u8; ::INODE_CORE_SIZE],
        checksum_prefix: Option<u32>,
    ) -> Result<TreeReader<R>> {
        let extents = load_extent_tree(
            &mut |block| ::load_disc_bytes(&mut inner, block_size, block),
            core,
            checksum_prefix,
        )?;
        TreeReader::create(inner, block_size, size, extents)
    }

    fn create(inner: R, block_size: u32, size: u64, extents: Vec<Extent>) -> Result<TreeReader<R>> {
        Ok(TreeReader {
            pos: 0,
            len: size,
            inner,
            extents,
            block_size,
        })
    }

    pub fn into_inner(self) -> R {
        self.inner
    }
}

enum FoundPart<'a> {
    Actual(&'a Extent),
    Sparse(u32),
}

fn find_part(part: u32, extents: &[Extent]) -> FoundPart {
    for extent in extents {
        if part < extent.part {
            // we've gone past it
            return FoundPart::Sparse(extent.part - part);
        }

        if part >= extent.part && part < extent.part + extent.len as u32 {
            // we're inside it
            return FoundPart::Actual(extent);
        }
    }

    FoundPart::Sparse(std::u32::MAX)
}

impl<R> io::Read for TreeReader<R>
where
    R: io::Read + io::Seek,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let block_size = self.block_size as u64;

        let wanted_block = (self.pos / block_size) as u32;
        let read_of_this_block = self.pos % block_size;

        match find_part(wanted_block, &self.extents) {
            FoundPart::Actual(extent) => {
                let bytes_through_extent =
                    (block_size * (wanted_block - extent.part) as u64) + read_of_this_block;
                let remaining_bytes_in_extent =
                    (extent.len as u64 * block_size) - bytes_through_extent;
                let to_read = std::cmp::min(remaining_bytes_in_extent, buf.len() as u64) as usize;
                let to_read = std::cmp::min(to_read as u64, self.len - self.pos) as usize;
                self.inner.seek(io::SeekFrom::Start(
                    extent.start as u64 * block_size + bytes_through_extent,
                ))?;
                let read = self.inner.read(&mut buf[0..to_read])?;
                self.pos += read as u64;
                Ok(read)
            }
            FoundPart::Sparse(max) => {
                let max_bytes = max as u64 * block_size;
                let read = std::cmp::min(max_bytes, buf.len() as u64) as usize;
                let read = std::cmp::min(read as u64, self.len - self.pos) as usize;
                zero(&mut buf[0..read]);
                self.pos += read as u64;
                Ok(read)
            }
        }
    }
}

impl<R> io::Seek for TreeReader<R>
where
    R: io::Read + io::Seek,
{
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        match pos {
            io::SeekFrom::Start(set) => self.pos = set,
            io::SeekFrom::Current(diff) => self.pos = (self.pos as i64 + diff) as u64,
            io::SeekFrom::End(set) => {
                assert!(set >= 0);
                self.pos = self.len - set as u64
            }
        }

        assert!(self.pos <= self.len);

        Ok(self.pos)
    }
}


fn add_found_extents<F>(
    load_block: &mut F,
    data: &[u8],
    expected_depth: u16,
    extents: &mut Vec<Extent>,
    checksum_prefix: Option<u32>,
    first_level: bool,
) -> Result<()>
where
    F: FnMut(u64) -> Result<Vec<u8>>,
{
    ensure!(
        0x0a == data[0] && 0xf3 == data[1],
        AssumptionFailed("invalid extent magic".to_string())
    );

    let extent_entries = read_le16(&data[2..]);
    // 4..: max; doesn't seem to be useful during read
    let depth = read_le16(&data[6..]);
    // 8..: generation, not used in standard ext4

    ensure!(
        expected_depth == depth,
        AssumptionFailed(format!("depth incorrect: {} != {}", expected_depth, depth))
    );

    if !first_level && checksum_prefix.is_some() {
        let end_of_entries = data.len() - 4;
        let on_disc = read_le32(&data[end_of_entries..(end_of_entries + 4)]);
        let computed =
            ::parse::ext4_style_crc32c_le(checksum_prefix.unwrap(), &data[..end_of_entries]);

        ensure!(
            computed == on_disc,
            AssumptionFailed(format!(
                "extent checksum mismatch: {:08x} != {:08x} @ {}",
                on_disc,
                computed,
                data.len()
            ))
        );
    }

    if 0 == depth {
        for en in 0..extent_entries {
            let raw_extent = &data[12 + en as usize * 12..];
            let ee_block = read_le32(raw_extent);
            let ee_len = read_le16(&raw_extent[4..]);
            let ee_start_hi = read_le16(&raw_extent[6..]);
            let ee_start_lo = read_le32(&raw_extent[8..]);
            let ee_start = ee_start_lo as u64 + 0x1000 * ee_start_hi as u64;

            extents.push(Extent {
                part: ee_block,
                start: ee_start,
                len: ee_len,
            });
        }

        return Ok(());
    }

    for en in 0..extent_entries {
        let extent_idx = &data[12 + en as usize * 12..];
        //            let ei_block = as_u32(extent_idx);
        let ei_leaf_lo = read_le32(&extent_idx[4..]);
        let ei_leaf_hi = read_le16(&extent_idx[8..]);
        let ee_leaf: u64 = ei_leaf_lo as u64 + ((ei_leaf_hi as u64) << 32);
        let data = load_block(ee_leaf)?;
        add_found_extents(
            load_block,
            &data,
            depth - 1,
            extents,
            checksum_prefix,
            false,
        )?;
    }

    Ok(())
}

fn load_extent_tree<F>(
    load_block: &mut F,
    core: [u8; ::INODE_CORE_SIZE],
    checksum_prefix: Option<u32>,
) -> Result<Vec<Extent>>
where
    F: FnMut(u64) -> Result<Vec<u8>>,
{
    ensure!(
        0x0a == core[0] && 0xf3 == core[1],
        AssumptionFailed("invalid extent magic".to_string())
    );

    let extent_entries = read_le16(&core[2..]);
    // 4..: max; doesn't seem to be useful during read
    let depth = read_le16(&core[6..]);

    ensure!(
        depth <= 5,
        AssumptionFailed(format!("initial depth too high: {}", depth))
    );

    let mut extents = Vec::with_capacity(extent_entries as usize + depth as usize * 200);

    add_found_extents(
        load_block,
        &core,
        depth,
        &mut extents,
        checksum_prefix,
        true,
    )?;

    extents.sort_by_key(|e| e.part);

    Ok(extents)
}

fn zero(mut buf: &mut [u8]) {
    unsafe { std::ptr::write_bytes(buf.as_mut_ptr(), 0u8, buf.len()) }
}


#[cfg(test)]
mod tests {
    use std::io;
    use std::io::Read;
    use extents::TreeReader;
    use extents::Extent;

    #[test]
    fn simple_tree() {
        let data = (0..255u8).collect::<Vec<u8>>();
        let size = 4 + 4 * 2;
        let all_bytes = io::Cursor::new(data);
        let mut reader = TreeReader::create(
            all_bytes,
            4,
            size as u64,
            vec![
                Extent {
                    part: 0,
                    start: 10,
                    len: 1,
                },
                Extent {
                    part: 1,
                    start: 20,
                    len: 2,
                },
            ],
        ).unwrap();

        let mut res = Vec::new();
        assert_eq!(size, reader.read_to_end(&mut res).unwrap());

        assert_eq!(vec![40, 41, 42, 43, 80, 81, 82, 83, 84, 85, 86, 87], res);
    }

    #[test]
    fn zero_buf() {
        let mut buf = [7u8; 5];
        assert_eq!(7, buf[0]);
        ::extents::zero(&mut buf);
        for i in &buf {
            assert_eq!(0, *i);
        }
    }
}
