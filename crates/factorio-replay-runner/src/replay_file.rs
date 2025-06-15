use itertools::Itertools;
use std::{
    fs::File,
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
};
use zip::{ZipArchive, ZipWriter, read::ZipFile, result::ZipResult, write::SimpleFileOptions};

/**
 * Utils for handling replay files.
 */
pub struct ReplayFile<F: Read + Seek> {
    zip: ZipArchive<F>,
    save_name: String,
}

impl<F: Read + Seek> ReplayFile<F> {
    pub fn new(file: F) -> Result<Self, Box<dyn std::error::Error>> {
        let mut zip = ZipArchive::new(file)?;
        let save_name = find_save_name(&mut zip)?;
        Ok(Self { zip, save_name })
    }

    pub fn save_name(&self) -> &str {
        &self.save_name
    }
}

fn find_save_name<R: Read + Seek>(zip: &mut ZipArchive<R>) -> Result<String, String> {
    (0..zip.len())
        .into_iter()
        .filter_map(|i| zip.by_index_raw(i).ok().and_then(|f| f.enclosed_name()))
        .filter_map(|p| {
            p.components()
                .next()
                .map(|f| f.as_os_str().to_string_lossy().into_owned())
        })
        .unique()
        .exactly_one()
        .map_err(|f| format!("Expected one top level folder: {}", f))
}

impl<F: Read + Seek> ReplayFile<F> {
    fn inner_file_path(&self, path: impl AsRef<Path>) -> PathBuf {
        Path::new(&self.save_name).join(path)
    }

    pub fn get_inner_file(&mut self, path: impl AsRef<Path>) -> ZipResult<ZipFile<F>> {
        self.zip
            .by_name(&self.inner_file_path(path).to_string_lossy())
    }

    fn get_inner_file_text(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let mut file = self.get_inner_file(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        Ok(contents)
    }

    pub fn control_lua_contents(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        self.get_inner_file_text("control.lua")
    }

    fn copy_files_to(
        &mut self,
        out: &mut ZipWriter<File>,
        exclude_files: &[&str],
    ) -> ZipResult<()> {
        let zip = &mut self.zip;
        for i in 0..zip.len() {
            let entry = zip.by_index(i).unwrap();
            if exclude_files.contains(&entry.name()) {
                continue;
            }
            out.raw_copy_file(entry)?;
        }
        Ok(())
    }

    /// Creates a new zip, but with control.lua replaced by the given contents
    pub fn with_installed_replay_script(
        &mut self,
        out: &mut ZipWriter<File>,
        new_ctrl_lua: &str,
    ) -> ZipResult<()> {
        self.copy_files_to(out, &["control.lua"])?;
        out.start_file("control.lua", SimpleFileOptions::default())?;
        out.write_all(new_ctrl_lua.as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn create_test_zip(
        files: &[(&str, &str)],
    ) -> Result<NamedTempFile, Box<dyn std::error::Error>> {
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

    fn simple_test_zip(names: &[&str]) -> Result<NamedTempFile, Box<dyn std::error::Error>> {
        let files = names.iter().map(|&name| (name, "test")).collect_vec();
        create_test_zip(&files)
    }

    fn save_name_result(names: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
        let mut zip = ZipArchive::new(simple_test_zip(names)?)?;
        Ok(find_save_name(&mut zip)?)
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
    fn test_find_save_name_multiple_directories_error() -> Result<(), Box<dyn std::error::Error>> {
        let save_name = save_name_result(&["save1/control.lua", "save2/level-init.dat"]);
        assert!(save_name.is_err());
        let save_name = save_name_result(&["file1.txt", "file2.txt"]);
        assert!(save_name.is_err());
        Ok(())
    }

    fn mock_replay_file() -> Result<NamedTempFile, Box<dyn std::error::Error>> {
        let files = vec![
            ("my-save/control.lua", "--mock ctrl lua contents"),
            ("my-save/level-init.dat", "test"),
        ];
        create_test_zip(&files)
    }

    #[test]
    fn test_mock_replay_file() -> Result<(), Box<dyn std::error::Error>> {
        let file = mock_replay_file()?;
        let mut replay_file = ReplayFile::new(file)?;
        assert_eq!(replay_file.save_name(), "my-save");
        let ctrl_lua_contents = replay_file.control_lua_contents()?;
        assert_eq!(ctrl_lua_contents, "--mock ctrl lua contents");
        Ok(())
    }

    //todo
}
