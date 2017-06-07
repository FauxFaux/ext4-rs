/*!

Support for reading MBR (not GPT) partition tables, and getting an `io::Read` for a partition.
*/

use std;

use std::io::Error;
use std::io::ErrorKind;
use std::io::Read;
use std::io::Result;
use std::io::Seek;
use std::io::SeekFrom;

use ::as_u32;

/// An entry in the partition table.
#[derive(Debug)]
pub struct Partition {
    pub id: usize,
    pub bootable: bool,
    pub type_code: u8,
    pub first_byte: u64,
    pub len: u64,
}

/// Produced by `read_partition`.
pub struct RangeReader<R> {
    inner: R,
    first_byte: u64,
    len: u64,
}

impl<R: Seek> RangeReader<R> {
    fn new(mut inner: R, first_byte: u64, len: u64) -> Result<RangeReader<R>> {
        assert!(first_byte <= std::i64::MAX as u64);
        assert!(len <= std::i64::MAX as u64);

        assert_eq!(first_byte, inner.seek(SeekFrom::Start(first_byte))?);

        Ok(RangeReader {
            inner,
            first_byte,
            len,
        })
    }
}

impl<R> Read for RangeReader<R>
where R: Read + Seek
{
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let pos = self.inner.seek(SeekFrom::Current(0))? - self.first_byte;
        let remaining = self.len - pos;
        if remaining >= buf.len() as u64 {
            self.inner.read(buf)
        } else {
            self.inner.read(&mut buf[0..(remaining as usize)])
        }
    }
}

impl<R: Seek> Seek for RangeReader<R> {
    fn seek(&mut self, action: SeekFrom) -> Result<u64> {

        let new_pos = self.inner.seek(match action {
            SeekFrom::Start(dist) => SeekFrom::Start(
                self.first_byte.checked_add(dist).expect("start overflow")),
            SeekFrom::Current(dist) => SeekFrom::Current(dist),
            SeekFrom::End(dist) => {
                assert!(dist >= 0, "can't seek negatively at end");
                // TODO: checked?
                SeekFrom::Start(self.first_byte + self.len - dist as u64)
            }
        })?;

        assert!(new_pos >= self.first_byte && new_pos < self.first_byte + self.len,
                "out of bound seek: {:?} must leave us between {} and {}, but was {}",
                action, self.first_byte, self.len, new_pos);

        Ok(new_pos - self.first_byte)
    }
}

/// Read a DOS/MBR partition table from a reader positioned at the appropriate sector.
/// The sector size for the disc is assumed to be 512 bytes.
pub fn read_partition_table<R: Read>(mut reader: R) -> Result<Vec<Partition>> {
    let mut sector = [0u8; 512];
    reader.read_exact(&mut sector)?;

    parse_partition_table(&sector, 512)
}

/// Read a DOS/MBR partition table from a 512-byte boot sector, providing a disc sector size.
pub fn parse_partition_table(sector: &[u8], sector_size: u16) -> Result<Vec<Partition>> {
    let mut partitions = Vec::with_capacity(4);

    for entry_id in 0..4 {
        let first_entry_offset = 446;
        let entry_size = 16;
        let entry_offset = first_entry_offset + entry_id * entry_size;
        let partition = &sector[entry_offset..entry_offset + entry_size];
        let status = partition[0];
        let bootable = match status {
            0x00 => false,
            0x80 => true,
            _ => return Err(Error::new(ErrorKind::InvalidData,
                                           format!("invalid status code in partition {}: {:x}",
                                                   entry_id, status))),
        };

        let type_code = partition[4];

        if 0 == type_code {
            continue;
        }

        let first_byte = as_u32(&partition[8..]) as u64 * sector_size as u64;
        let len = first_byte + as_u32(&partition[12..]) as u64 * sector_size as u64;

        partitions.push(Partition {
            id: entry_id,
            bootable,
            type_code,
            first_byte,
            len,
        });
    }

    Ok(partitions)
}

/// Open the contents of a partition for reading.
pub fn read_partition<R>(inner: R, part: &Partition) -> Result<RangeReader<R>>
where R: Read + Seek
{
    RangeReader::new(inner, part.first_byte, part.len)
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::io::Read;
    use std::io::Seek;
    use std::io::SeekFrom;

    #[test]
    fn reader() {
        let data = io::Cursor::new([0u8, 1, 2, 3, 4, 5, 6, 7]);
        let mut reader = ::mbr::RangeReader::new(data, 2, 5).expect("setup");
        let mut buf = [0u8, 2];
        reader.read_exact(&mut buf).expect("read");
        assert_eq!(2, buf[0]);
        assert_eq!(3, buf[1]);
        reader.read_exact(&mut buf).expect("read");
        assert_eq!(4, buf[0]);
        assert_eq!(5, buf[1]);
        assert_eq!(1, reader.read(&mut buf).expect("read"));
        assert_eq!(6, buf[0]);
        assert_eq!(0, reader.read(&mut buf).expect("read"));

        reader.seek(SeekFrom::Start(0)).expect("seek");
        reader.read_exact(&mut buf).expect("read");
        assert_eq!(2, buf[0]);
        assert_eq!(3, buf[1]);

        reader.seek(SeekFrom::End(2)).expect("seek");
        reader.read_exact(&mut buf).expect("read");
        assert_eq!(5, buf[0]);
        assert_eq!(6, buf[1]);

        reader.seek(SeekFrom::Start(2)).expect("seek");
        reader.seek(SeekFrom::Current(-1)).expect("seek");
        reader.read_exact(&mut buf).expect("read");
        assert_eq!(3, buf[0]);
        assert_eq!(4, buf[1]);
    }

    #[test]
    fn parse() {
        let parts = ::mbr::parse_partition_table(include_bytes!("test-data/mbr-ubuntu-raspi3-16.04.img"), 512)
            .expect("success");

        assert_eq!(2, parts.len());

        assert_eq!(0, parts[0].id);
        assert_eq!(true, parts[0].bootable);
        assert_eq!(12, parts[0].type_code);
        assert_eq!(4194304, parts[0].first_byte);
        assert_eq!(138412032, parts[0].len);

        assert_eq!(1, parts[1].id);
        assert_eq!(false, parts[1].bootable);
        assert_eq!(131, parts[1].type_code);
        assert_eq!(138412032, parts[1].first_byte);
        assert_eq!(3999268864, parts[1].len);
    }
}
