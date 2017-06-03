use std::io;
use std::path::Path;
use std::process;

fn extract_test_images() -> io::Result<()> {
    const DEST: &str = "tests/generated/";

    if Path::new(format!("{}{}", DEST, "all-types.img").as_str()).exists() {
        return Ok(());
    }

    assert!(
        process::Command::new("tar")
            .args(&["-C", DEST,
                    "-xf", "scripts/generate-images/images.tgz"])
            .spawn()?
            .wait()?
            .success()
    );

    Ok(())
}

fn main() {
    extract_test_images().unwrap();
}
