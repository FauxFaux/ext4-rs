use std;
use std::io;

use ::as_u16;
use ::as_u32;

use ::errors::Result;
use ::errors::ErrorKind::*;

#[derive(Debug)]
struct Extent {
    block: u32,
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
where R: io::Read + io::Seek {
    pub fn new(mut inner: R, block_size: u32, size: u64, block: [u8; 4 * 15]) -> Result<TreeReader<R>> {
        let extents = load_extent_tree(&mut inner, block, block_size)?;
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

enum FoundBlock<'a> {
    Actual(&'a Extent),
    Sparse(u32),
}

fn find_block(block: u32, extents: &[Extent]) -> FoundBlock {
    for extent in extents {
        if block < extent.block {
            // we've gone past it
            return FoundBlock::Sparse(extent.block - block);
        }

        if block >= extent.block && block < extent.block + extent.len as u32 {
            // we're inside it
            return FoundBlock::Actual(extent);
        }
    }

    return FoundBlock::Sparse(std::u32::MAX);
}

impl<R> io::Read for TreeReader<R>
    where R: io::Read + io::Seek {


    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let block_size = self.block_size as u64;

        let wanted_block = (self.pos / block_size) as u32;
        let read_of_this_block = self.pos % block_size;

        match find_block(wanted_block, &self.extents) {
            FoundBlock::Actual(extent) => {
                let bytes_through_extent = (block_size * (wanted_block - extent.block) as u64) + read_of_this_block;
                let remaining_bytes_in_extent = (extent.len as u64 * block_size) - bytes_through_extent;
                let to_read = std::cmp::min(remaining_bytes_in_extent, buf.len() as u64) as usize;
                let to_read = std::cmp::min(to_read as u64, self.len - self.pos) as usize;
                self.inner.seek(io::SeekFrom::Start(extent.start as u64 * block_size + bytes_through_extent))?;
                let read = self.inner.read(&mut buf[0..to_read])?;
                self.pos += read as u64;
                return Ok(read);
            }
            FoundBlock::Sparse(max) => {
                let max_bytes = max as u64 * block_size;
                let read = std::cmp::min(max_bytes, buf.len() as u64) as usize;
                let read = std::cmp::min(read as u64, self.len - self.pos) as usize;
                zero(&mut buf[0..read]);
                self.pos += read as u64;
                return Ok(read);
            }
        }
    }
}

impl<R> io::Seek for TreeReader<R>
where R: io::Read + io::Seek {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        match pos {
            io::SeekFrom::Start(set) => { self.pos = set }
            io::SeekFrom::Current(diff) => { self.pos = (self.pos as i64 + diff) as u64 }
            io::SeekFrom::End(set) => {
                assert!(set >= 0);
                self.pos = self.len - set as u64
            }
        }

        assert!(self.pos <= self.len);

        Ok(self.pos)
    }
}


fn add_found_extents<R>(
    block_size: u32,
    mut inner: &mut R,
    block: &[u8],
    expected_depth: u16,
    extents: &mut Vec<Extent>) -> Result<()>
where R: io::Read + io::Seek {

    ensure!(0x0a == block[0] && 0xf3 == block[1],
        AssumptionFailed("invalid extent magic".to_string()));

    let extent_entries = as_u16(&block[2..]);
    // 4..: max; doesn't seem to be useful during read
    let depth = as_u16(&block[6..]);
    // 8..: generation, not used in standard ext4

    ensure!(expected_depth == depth,
        AssumptionFailed(format!("depth incorrect: {} != {}", expected_depth, depth)));

    if 0 == depth {
        for en in 0..extent_entries {
            let raw_extent = &block[12 + en as usize * 12..];
            let ee_block = as_u32(raw_extent);
            let ee_len = as_u16(&raw_extent[4..]);
            let ee_start_hi = as_u16(&raw_extent[6..]);
            let ee_start_lo = as_u32(&raw_extent[8..]);
            let ee_start = ee_start_lo as u64 + 0x1000 * ee_start_hi as u64;

            extents.push(Extent {
                block: ee_block,
                start: ee_start,
                len: ee_len,
            });
        }

        return Ok(());
    }

    for en in 0..extent_entries {
        let extent_idx = &block[12 + en as usize * 12..];
        //            let ei_block = as_u32(extent_idx);
        let ei_leaf_lo = as_u32(&extent_idx[4..]);
        let ei_leaf_hi = as_u16(&extent_idx[8..]);
        let ee_leaf: u64 = ei_leaf_lo as u64 + ((ei_leaf_hi as u64) << 32);
        inner.seek(io::SeekFrom::Start(block_size as u64 * ee_leaf))?;
        let mut block = vec![0u8; block_size as usize];
        inner.read_exact(&mut block)?;
        add_found_extents(block_size, inner, &block, depth - 1, extents)?;
    }

    Ok(())
}

fn load_extent_tree<R>(mut inner: R, start: [u8; 4 * 15], block_size: u32) -> Result<Vec<Extent>>
    where R: io::Read + io::Seek {
    ensure!(0x0a == start[0] && 0xf3 == start[1],
        AssumptionFailed("invalid extent magic".to_string()));

    let extent_entries = as_u16(&start[2..]);
    // 4..: max; doesn't seem to be useful during read
    let depth = as_u16(&start[6..]);

    ensure!(depth <= 5,
        AssumptionFailed(format!("initial depth too high: {}", depth)));

    let mut extents = Vec::with_capacity(extent_entries as usize + depth as usize * 200);

    add_found_extents(block_size, &mut inner, &start, depth, &mut extents)?;

    extents.sort_by_key(|e| e.block);

    Ok(extents)
}

fn zero(mut buf: &mut [u8]) {
    unsafe {
        std::ptr::write_bytes(buf.as_mut_ptr(), 0u8, buf.len())
    }
}


#[cfg(test)]
mod tests {
    use std::io;
    use std::io::Read;
    use ::extents::TreeReader;
    use ::extents::Extent;

    #[test]
    fn simple_tree() {
        let data = (0..255u8).collect::<Vec<u8>>();
        let size = 4 + 4 * 2;
        let all_bytes = io::Cursor::new(data);
        let mut reader = TreeReader::create(all_bytes, 4, size as u64,
            vec![
                Extent {
                    block: 0,
                    start: 10,
                    len: 1,
                },
                Extent {
                    block: 1,
                    start: 20,
                    len: 2,
                }
            ]
        ).unwrap();

        let mut res = Vec::new();
        assert_eq!(size, reader.read_to_end(&mut res).unwrap());

        assert_eq!(vec![
            40, 41, 42, 43,
            80, 81, 82, 83,
            84, 85, 86, 87,
        ], res);
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
