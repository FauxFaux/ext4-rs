use std::cmp::min;
use std::io;
use std::io::{ErrorKind, SeekFrom};

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

        let aligned_address = (pos / CHUNK_SIZE as u64) * CHUNK_SIZE as u64;
        let aligned_delta = (pos - aligned_address) as usize;

        let data_size = buf.len();
        let to_read = data_size + aligned_delta;

        let to_read = if to_read - ((to_read / CHUNK_SIZE) * CHUNK_SIZE) == 0 {
            to_read
        } else {
            ((to_read / CHUNK_SIZE) * CHUNK_SIZE) + CHUNK_SIZE
        };

        let mut buffer = vec![0u8; to_read];

        for block_buffer in buffer.chunks_mut(CHUNK_SIZE) {
            let address = aligned_address + read_offset as u64;
            read_fn(address, block_buffer)?;

            self.metadata_crypto
                .decrypt(block_buffer, address)
                .map_err(|error| io::Error::new(ErrorKind::Other, error.to_string()))?;

            read_offset += CHUNK_SIZE;
        }

        buf.copy_from_slice(&buffer[aligned_delta..buf.len() + aligned_delta]);

        Ok(data_size)
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
