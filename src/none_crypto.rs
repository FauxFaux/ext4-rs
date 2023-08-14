use crate::{Crypto, MetadataCrypto};
use anyhow::Error;

pub struct NoneCrypto {}

impl MetadataCrypto for NoneCrypto {
    fn decrypt(&self, _page: &mut [u8], _page_addr: u64) -> Result<(), Error> {
        Ok(())
    }
}

impl Crypto for NoneCrypto {
    fn decrypt_filename(&self, _context: &[u8], encrypted_name: &[u8]) -> Result<Vec<u8>, Error> {
        Ok(encrypted_name.to_vec())
    }

    fn decrypt_page(
        &self,
        _context: &[u8],
        _page: &mut [u8],
        _page_offset: u64,
        _page_addr: u64,
        _ino: u32,
    ) -> Result<(), Error> {
        Ok(())
    }
}
