use std::fs;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use log::debug;

use crate::Error;

pub fn extract(
    extract_to: impl AsRef<Path>,
    archive: impl AsRef<Path>,
) -> Result<(), Error>
{
    let file = std::fs::File::open(&archive)?;

    let mut archive = zip::ZipArchive::new(file)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = match file.enclosed_name() {
            Some(path) => {
                extract_to.as_ref().join(path)
            }
            None => continue,
        };

        if (*file.name()).ends_with('/') {
            debug!("File {} extracted to \"{}\"", i, outpath.display());
            fs::create_dir_all(&outpath)?;
        } else {
            debug!(
                "File {} extracted to \"{}\" ({} bytes)",
                i,
                outpath.display(),
                file.size()
            );
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    fs::create_dir_all(&p)?;
                }
            }
            let mut outfile = OpenOptions::new().create(true).write(true).open(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }
    Ok(())
}