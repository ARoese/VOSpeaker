use crate::audio_conversion::Mp3Path;
use crate::project_dir::config_map_file::ConfigMapFile;
use crate::project_dir::hashes::{ConfigHash, VOHash};
use crate::project_dir::topic_file::TopicFile;
use crate::project_dir::topic_lines::RawTopicLine;
use std::cell::RefCell;
use std::fs;
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};

pub struct TopicDir {
    path: PathBuf,
    config_map: RefCell<ConfigMapFile>,
    topic_file: TopicFile
}

impl TopicDir {
    /// attach to an existing TopicDir
    pub fn new(path: &Path) -> Result<Self, Error> {
        if !path.is_dir() {
            return Err(Error::new(ErrorKind::InvalidInput, "path is not a directory"));
        }
        let topic_file_path = topic_file(path);
        if !topic_file_path.exists() || !topic_file_path.is_file() {
            return Err(Error::new(
                ErrorKind::InvalidData, "dir contains no topic file or it is invalid")
            );
        }
        let config_map_path = path.join("configMap.bin");
        Ok(TopicDir {
            path: path.to_path_buf(),
            config_map: ConfigMapFile::new(&config_map_path)?.into(),
            topic_file: TopicFile::new(&topic_file_path)?,
        })
    }

    /// take a topic file, construct a new TopicDir, and attach to it
    pub fn create_new(path: &Path, topic_file_path: &Path) -> Result<Self, Error> {
        if path.exists() {
            return Err(Error::new(ErrorKind::InvalidInput, format!("'{}' already exists. Use a different name.", path.to_string_lossy())));
        }

        let res = {
            fs::create_dir_all(path)?;
            let target_topic_file_path = topic_file(path);
            fs::copy(&topic_file_path, &target_topic_file_path)?;

            // the target is now valid; attach to it as normal
            Self::new(path)
        };

        if res.is_err() {
            fs::remove_dir_all(path)?;
        }

        res
    }

    /// get the path to the wav file associated with the given VOHash. 
    /// The file is not guaranteed to exist.
    pub fn mp3_path(&self, hash: &VOHash) -> Mp3Path {
        let mp3_file_name = hash.to_string().to_lowercase() + ".mp3";
        Mp3Path::from(self.path.join(mp3_file_name))
    }
    
    pub fn topic_file_ref(&self) -> &TopicFile {
        &self.topic_file
    }

    /// get the config hash associated with the given VOHash, if one exists.
    pub fn get_config_hash(&self, hash: &VOHash) -> Option<ConfigHash> {
        self.config_map.borrow().get_hash(hash).map(|hash| hash.clone())
    }

    /// add a vo wav file to the topic dir
    pub fn add_vo(&self, vo_hash: &VOHash, config_hash: &ConfigHash) -> Result<(), Error> {
        self.config_map.borrow_mut().set_hash(vo_hash, config_hash)?;
        Ok(())
    }

    /// the name of the topic controlled by this dir
    pub fn name(&self) -> String {
        self.path.file_name().unwrap().to_str().unwrap().to_string().replace(".topic.d", "")
    }

    /// deletes the TopicDir on the file system.
    /// naturally, this struct will no longer be valid afterwards
    pub fn delete(self) -> Result<(), Error> {
        drop(self.config_map);
        fs::remove_dir_all(&self.path)?;
        Ok(())
    }
    
    pub fn update_topic_file(&self, other_topic: &Path) -> Result<Vec<RawTopicLine>, Error> {
        self.topic_file.update_topic_file(other_topic)
    }
}

fn topic_file(path: &Path) -> PathBuf {
    // this only panics if the topic dir is the fs root, which is silly
    let topic_name = path.file_name()
        .unwrap().to_str().unwrap().to_string()
        .replace(".topic.d", "");
    path.join(topic_name + ".topic")
}