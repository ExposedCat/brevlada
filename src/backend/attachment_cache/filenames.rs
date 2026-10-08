use super::{Index, write_index};
use anyhow::Result;
use std::path::Path;

pub(super) fn available(directory: &Path, name: &str, index: &Index) -> Result<String> {
    let path = Path::new(name);
    let stem = path.file_stem().unwrap().to_string_lossy();
    let extension = path
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    let mut candidate = name.to_owned();
    let mut number = 2;
    while candidate == ".attachments.json"
        || index.values().any(|name| *name == candidate)
        || directory.join(&candidate).try_exists()?
    {
        candidate = format!("{stem} ({number}){extension}");
        number += 1;
    }
    Ok(candidate)
}

pub(super) fn migrate(directory: &Path, index: &mut Index) -> Result<()> {
    if !directory.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let identity = entry.file_name().to_string_lossy().into_owned();
        if identity.len() != 64
            || !identity.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !entry.file_type()?.is_dir()
        {
            continue;
        }
        let files: Vec<_> = std::fs::read_dir(entry.path())?.collect::<std::io::Result<_>>()?;
        if files.len() != 1 || !files[0].file_type()?.is_file() {
            continue;
        }
        let name = files[0].file_name().to_string_lossy().into_owned();
        let name = match index.get(&identity) {
            Some(name) => name.clone(),
            None => {
                let name = available(directory, &name, index)?;
                index.insert(identity, name.clone());
                write_index(directory, index)?;
                name
            }
        };
        anyhow::ensure!(
            crate::models::attachment::safe_name(&name) == name && name != ".attachments.json",
            "Invalid saved attachment filename"
        );
        let destination = directory.join(name);
        if !destination.try_exists()? {
            std::fs::rename(files[0].path(), destination)?;
            std::fs::remove_dir(entry.path())?;
        }
    }
    Ok(())
}
