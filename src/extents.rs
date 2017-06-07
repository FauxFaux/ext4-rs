use std;
use std::io;

use ::as_u16;
use ::as_u32;

use ::errors::*;
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
    block_size: u32,
    extents: Vec<Extent>,
    sparse_bytes: Option<u64>,
}

impl<R> TreeReader<R>
where R: io::Read + io::Seek {
    pub fn new(mut inner: R, block_size: u32, block: [u8; 4 * 15]) -> Result<TreeReader<R>> {
        let extents = load_extent_tree(&mut inner, block, block_size)?;
        TreeReader::create(inner, block_size, extents)
    }

    fn create(mut inner: R, block_size: u32, extents: Vec<Extent>) -> Result<TreeReader<R>> {
        assert_eq!(0, extents[0].block);

        inner.seek(io::SeekFrom::Start(extents[0].start as u64 * block_size as u64))?;

        Ok(TreeReader {
            pos: 0,
            inner,
            extents,
            block_size,
            sparse_bytes: None,
        })
    }
}

impl<R> io::Read for TreeReader<R>
    where R: io::Read + io::Seek {


    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.extents.is_empty() {
            return Ok(0);
        }

        // we're feeding them some sparse bytes, keep doing so, and mark as done if we're done
        if let Some(remaining_sparse) = self.sparse_bytes {
            return if (buf.len() as u64) < remaining_sparse {
                self.sparse_bytes = Some(remaining_sparse - buf.len() as u64);
                zero(buf);
                Ok(buf.len())
            } else {
                self.sparse_bytes = None;
                zero(&mut buf[0..remaining_sparse as usize]);
                Ok(remaining_sparse as usize)
            };
        }

        // we must be feeding them a real extent; keep doing so
        let read;
        {
            // first self.extents is the block we're reading from
            // we've read self.pos from it already
            let reading_extent = &self.extents[0];
            let this_extent_len_bytes = reading_extent.len as u64 * self.block_size as u64;

            let bytes_until_end = this_extent_len_bytes - self.pos;

            let to_read = std::cmp::min(buf.len() as u64, bytes_until_end) as usize;

            read = self.inner.read(&mut buf[0..to_read])?;
            assert_ne!(0, read);

            // if, while reading, we didn't reach the end of this extent, everything is okay
            if (read as u64) != bytes_until_end {
                self.pos += read as u64;
                return Ok(read);
            }
        }

        // we finished reading the current extent
        let last = self.extents.remove(0);
        self.pos = 0;

        if !self.extents.is_empty() {
            let next = &self.extents[0];

            // check for HOLES
            let last_ended = last.block as u64 + last.len as u64;
            let new_starts = next.block as u64;
            let hole_size = (new_starts - last_ended) * self.block_size as u64;
            if 0 != hole_size {
                // before feeding them the next extent, lets feed them the hole
                self.sparse_bytes = Some(hole_size);
            } else {
                self.inner.seek(io::SeekFrom::Start(self.block_size as u64 * next.start))?;
            }
        }

        Ok(read)
    }
}

fn add_found_extents<R>(
    block_size: u32,
    mut inner: &mut R,
    block: &[u8],
    expected_depth: u16,
    extents: &mut Vec<Extent>) -> Result<()>
    where R: io::Read + io::Seek {

    assert_eq!(0x0a, block[0]);
    assert_eq!(0xf3, block[1]);

    let extent_entries = as_u16(&block[2..]);
    // 4..: max; doesn't seem to be useful during read
    let depth = as_u16(&block[6..]);
    // 8..: generation, not used in standard ext4

    assert_eq!(expected_depth, depth);

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
    assert_eq!(0x0a, start[0]);
    assert_eq!(0xf3, start[1]);

    let extent_entries = as_u16(&start[2..]);
    // 4..: max; doesn't seem to be useful during read
    let depth = as_u16(&start[6..]);

    assert!(depth <= 5);

    let mut extents = Vec::with_capacity(extent_entries as usize + depth as usize * 200);

    add_found_extents(block_size, &mut inner, &start, depth, &mut extents)?;

    extents.sort_by_key(|e| e.block);

    Ok(extents)
}

fn zero(buf: &mut [u8]) {
    for i in buf {
        *i = 0;
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
        let all_bytes = io::Cursor::new((0..255u8).collect::<Vec<u8>>());
        let mut reader = TreeReader::create(all_bytes,
            4,
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
        assert_eq!(4 + 4 * 2, reader.read_to_end(&mut res).unwrap());

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
