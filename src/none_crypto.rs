use crate::Crypto;

pub struct NoneCrypto {}

impl Crypto for NoneCrypto {
    fn decrypt_filename(
        &self,
        _context: &[u8],
        encrypted_name: &[u8],
    ) -> Result<Vec<u8>, anyhow::Error> {
        Ok(encrypted_name.to_vec())
    }

    fn decrypt_page(
        &self,
        _context: &[u8],
        _page: &mut [u8],
        _page_addr: u64,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
}
