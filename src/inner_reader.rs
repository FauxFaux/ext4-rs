use std::cmp::min;
use std::io;
use std::io::ErrorKind;

use anyhow::Error;
use positioned_io::ReadAt;

pub trait MetadataCrypto {
    fn decrypt(&self, page: &mut [u8], page_addr: u64) -> Result<(), Error>;
}

#[derive(Debug)]
pub struct InnerReader<R: ReadAt, M: MetadataCrypto> {
    pub inner: R,
    pub metadata_crypto: M,
}

impl<R: ReadAt, M: MetadataCrypto> InnerReader<R, M> {
    pub fn new(inner: R, metadata_crypto: M) -> InnerReader<R, M> {
        Self {
            inner,
            metadata_crypto,
        }
    }

    pub fn read_at_without_decrypt(&self, pos: u64, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read_at(pos, buf)
    }

    fn decrypt<F: Fn(u64, &mut [u8]) -> io::Result<usize>>(
        &self,
        pos: u64,
        buf: &mut [u8],
        read_fn: F,
    ) -> io::Result<usize> {
        let mut read_offset = 0;
        const CHUNK_SIZE: usize = 0x1000;
        let to_read = buf.len();

        while read_offset < to_read {
            let mut block_buffer = vec![0u8; CHUNK_SIZE];
            let address = pos + read_offset as u64;
            read_fn(address, &mut block_buffer)?;
            self.metadata_crypto
                .decrypt(&mut block_buffer, address)
                .map_err(|error| io::Error::new(ErrorKind::Other, error.to_string()))?;

            let expected_size = min(to_read - read_offset, CHUNK_SIZE);
            buf[read_offset..read_offset + expected_size]
                .copy_from_slice(&block_buffer[..expected_size]);

            read_offset += CHUNK_SIZE;
        }

        Ok(read_offset)
    }
}

impl<R: ReadAt, M: MetadataCrypto> ReadAt for InnerReader<R, M> {
    fn read_at(&self, pos: u64, buf: &mut [u8]) -> io::Result<usize> {
        self.decrypt(pos, buf, |offset, buffer| {
            self.read_at_without_decrypt(offset, buffer)
        })
    }

    fn read_exact_at(&self, pos: u64, buf: &mut [u8]) -> io::Result<()> {
        self.decrypt(pos, buf, |offset, buffer| {
            self.inner.read_exact_at(offset, buffer)?;
            Ok(0)
        })?;

        Ok(())
    }
}
