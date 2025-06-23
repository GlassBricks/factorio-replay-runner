use anyhow::{Context, Result};
use itertools::Itertools;
use std::{
    fs::File,
    io::{self, Read, Seek, Write},
    path::Path,
};
use zip::{ZipArchive, ZipWriter, read::ZipFile, result::ZipResult, write::SimpleFileOptions};

use crate::factorio_install_dir::VersionStr;

/**
 * Utils for handling save files.
 */
pub struct SaveFile<F: Read + Seek> {
    zip: ZipArchive<F>,
    save_name: String,
    // for lazy loading
    control_lua_contents: Option<String>,
}

impl<F: Read + Seek> SaveFile<F> {
    pub fn new(file: F) -> Result<Self> {
        let mut zip = ZipArchive::new(file)?;
        let save_name = find_save_name(&mut zip)?;
        Ok(Self {
            zip,
            save_name,
            control_lua_contents: None,
        })
    }

    pub fn save_name(&self) -> &str {
        &self.save_name
    }
}

fn find_save_name<R: Read + Seek>(zip: &mut ZipArchive<R>) -> Result<String> {
    let save_name = (0..zip.len())
        .into_iter()
        .filter_map(|i| zip.by_index_raw(i).ok().and_then(|f| f.enclosed_name()))
        .filter_map(|p| {
            p.components()
                .next()
                .map(|f| f.as_os_str().to_string_lossy().into_owned())
        })
        .unique()
        .exactly_one()
        .map_err(|mut err| {
            let names = err.join(", ");
            if names.is_empty() {
                anyhow::anyhow!("Failed to find save name in save file: no folder found")
            } else {
                anyhow::anyhow!(
                    "Failed to find save name in save file, multiple folders found: {}",
                    names
                )
            }
        })?;

    Ok(save_name)
}

fn read_to_new_string<R: Read>(mut reader: R) -> io::Result<String> {
    let mut contents = String::new();
    reader.read_to_string(&mut contents)?;
    Ok(contents)
}

impl<F: Read + Seek> SaveFile<F> {
    fn inner_file_path(&self, path: impl AsRef<Path>) -> String {
        Path::new(&self.save_name)
            .join(path)
            .to_string_lossy()
            .into_owned()
    }

    fn get_inner_file(&mut self, path: impl AsRef<Path>) -> Result<ZipFile<F>> {
        let path = self.inner_file_path(path);
        self.zip
            .by_name(&path)
            .with_context(|| format!("{} not found in zip file", path))
    }

    pub fn get_control_lua_contents(&mut self) -> Result<&str> {
        if self.control_lua_contents.is_none() {
            let contents = read_to_new_string(self.get_inner_file("control.lua")?)?;
            self.control_lua_contents = Some(contents);
        }
        Ok(self.control_lua_contents.as_ref().unwrap())
    }

    pub fn get_factorio_version(&mut self) -> Result<VersionStr> {
        let mut level_init_file = self
            .get_inner_file("level-init.dat")
            .context("Failed to get level-init.dat from save file")?;

        let mut buffer = [0u8; 6];
        level_init_file
            .read_exact(&mut buffer)
            .context("Failed to read version bytes from level-init.dat")?;

        let major = u16::from_le_bytes([buffer[0], buffer[1]]);
        let minor = u16::from_le_bytes([buffer[2], buffer[3]]);
        let patch = u16::from_le_bytes([buffer[4], buffer[5]]);

        Ok(VersionStr::new(major, minor, patch))
    }

    fn copy_files_except(
        &mut self,
        out: &mut ZipWriter<impl Seek + Write>,
        exclude_file: &str,
    ) -> ZipResult<()> {
        let zip = &mut self.zip;
        for i in 0..zip.len() {
            let entry = zip.by_index(i).unwrap();
            if entry.name() == exclude_file {
                continue;
            }
            out.raw_copy_file(entry)?;
        }
        Ok(())
    }

    pub(crate) fn install_replay_script_to(
        &mut self,
        out_save_file: &mut File,
        replay_script: &str,
    ) -> Result<()> {
        let ctrl_lua_path = self.inner_file_path("control.lua");
        let ctrl_lua_contents = self.get_control_lua_contents()?.to_string();

        let mut zip = ZipWriter::new(out_save_file);
        self.copy_files_except(&mut zip, &ctrl_lua_path)?;

        zip.start_file(ctrl_lua_path, SimpleFileOptions::default())?;
        zip.write_fmt(format_args!(
            "{}\n-- Begin replay script\n",
            ctrl_lua_contents
        ))?;
        zip.write_all(replay_script.as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) const TEST_VERSION: VersionStr = VersionStr::new(2, 0, 57);

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn create_test_zip(files: &[(&str, &str)]) -> Result<NamedTempFile> {
        let temp_file = NamedTempFile::new()?;
        let mut zip = ZipWriter::new(temp_file.reopen()?);

        for &(name, content) in files {
            if name.contains(".") {
                zip.start_file(name, SimpleFileOptions::default())?;
                zip.write_all(content.as_bytes())?;
            } else {
                zip.add_directory(name, SimpleFileOptions::default())?;
            }
        }

        zip.finish()?;
        Ok(temp_file)
    }

    fn simple_test_zip(names: &[&str]) -> Result<NamedTempFile> {
        let files = names.iter().map(|&name| (name, "test")).collect_vec();
        create_test_zip(&files)
    }

    fn save_name_result(names: &[&str]) -> Result<String> {
        let temp_file = simple_test_zip(names)?;
        let mut zip = ZipArchive::new(temp_file)?;
        find_save_name(&mut zip)
    }

    #[test]
    fn test_find_save_name_valid() {
        let save_name = save_name_result(&["my-save/control.lua", "my-save/level-init.dat"]);
        assert!(save_name.is_ok());
        assert_eq!(save_name.unwrap(), "my-save");

        let save_name =
            save_name_result(&["my-save", "my-save/control.lua", "my-save/level-init.dat"]);
        assert!(save_name.is_ok());
        assert_eq!(save_name.unwrap(), "my-save");
    }

    #[test]
    fn test_find_save_name_multiple_directories_error() -> Result<()> {
        let save_name = save_name_result(&["save1/control.lua", "save2/level-init.dat"]);
        assert!(save_name.is_err());
        let save_name = save_name_result(&["file1.txt", "file2.txt"]);
        assert!(save_name.is_err());
        Ok(())
    }

    fn mock_save_file() -> Result<NamedTempFile> {
        let VersionStr(major, minor, patch) = TEST_VERSION;
        // Create version bytes in little-endian format
        let mut version_bytes = Vec::new();
        version_bytes.extend_from_slice(&(major as u16).to_le_bytes());
        version_bytes.extend_from_slice(&(minor as u16).to_le_bytes());
        version_bytes.extend_from_slice(&patch.to_le_bytes());

        let version_data = String::from_utf8_lossy(&version_bytes).into_owned();
        let files = vec![
            ("my-save/control.lua", "--mock ctrl lua contents"),
            ("my-save/level-init.dat", &version_data),
        ];
        create_test_zip(&files)
    }

    #[test]
    fn test_mock_save_file() -> Result<()> {
        let file = mock_save_file()?;
        let mut save_file =
            SaveFile::new(file).context("Failed to create SaveFile from mock file")?;
        assert_eq!(save_file.save_name(), "my-save");
        let ctrl_lua_contents = save_file.get_control_lua_contents()?;
        assert_eq!(ctrl_lua_contents, "--mock ctrl lua contents");

        let version = save_file.get_factorio_version()?;
        assert_eq!(version, TEST_VERSION);
        Ok(())
    }

    impl SaveFile<File> {
        pub(crate) fn get_test_save_file() -> Result<SaveFile<File>> {
            let file = File::open("fixtures/TEST.zip")?;
            let save_file = SaveFile::new(file)?;
            Ok(save_file)
        }
    }

    #[test]
    fn test_get_fixture() -> Result<()> {
        let mut save_file = SaveFile::get_test_save_file()?;
        assert_eq!(save_file.save_name(), "TEST");
        let ctrl_lua_contents = save_file.get_control_lua_contents()?;

        assert_eq!(
            ctrl_lua_contents,
            "require('__base__/script/freeplay/control.lua')\n"
        );
        Ok(())
    }

    #[test]
    fn test_get_factorio_version() -> Result<()> {
        let mut save_file = SaveFile::get_test_save_file()?;
        let version = save_file.get_factorio_version()?;
        let expected = TEST_VERSION;
        assert_eq!(version, expected);
        Ok(())
    }
}
