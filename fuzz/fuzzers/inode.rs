#![no_main]
#[macro_use] extern crate libfuzzer_sys;
extern crate ext4;

fuzz_target!(|data: &[u8]| {
    ext4::parse::inode(
        data.to_vec(),
        |_| Err(ext4::ErrorKind::UnsupportedFeature("xattr blocks not supported during fuzzing".to_string()).into()),
        None, // no checksums
        1, // inode number; only for checksums
        );
});
