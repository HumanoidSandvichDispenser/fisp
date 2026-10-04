use std::{env, io};

use fuser::Config;

pub mod evaluator;
pub mod filesystem;

fn main() -> io::Result<()> {
    let mountpoint = match env::args().nth(1) {
        Some(path) => Ok(path),
        None => {
            println!("Usage: {} <MOUNTPOINT>", env::args().nth(0).unwrap());
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Missing mountpoint",
            ))
        }
    }?;

    let config = Config::default();

    fuser::mount(filesystem::FispFilesystem::new(), &mountpoint, &config)?;

    Ok(())
}
