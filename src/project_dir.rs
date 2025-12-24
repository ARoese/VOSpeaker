use std::fs;
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};
use crate::topic_dir::TopicDir;

pub struct ProjectDir {
    pub path: PathBuf,
    pub topics: Vec<TopicDir>
}

fn topics_path(path: &Path) -> PathBuf {
    path.join("topics")
}

impl ProjectDir {
    pub fn new(path: &Path) -> Result<Self, Error> {
        if !path.is_dir() {
            return Err(Error::new(ErrorKind::InvalidInput, "path is not a directory"));
        }

        let topics_path = topics_path(path);
        fs::create_dir_all(&topics_path)?;

        // TODO: logging
        let dirs = fs::read_dir(topics_path)?
            .filter_map(Result::ok)
            .filter(|e| e.path().is_dir())
            .collect::<Vec<_>>();
        let topics = dirs.iter()
            .filter_map(|d| TopicDir::new(&d.path()).ok())
            .collect::<Vec<_>>();

        Ok(ProjectDir {
            path: path.to_path_buf(),
            topics
        })
    }

    pub fn add_topic(&mut self, topic_file: &Path) -> Result<(), Error> {
        let new_dir = TopicDir::create_new(&topics_path(&self.path), &topic_file)?;
        self.topics.push(new_dir);
        Ok(())
    }
    
    pub fn remove_topic(&mut self, index: usize) -> Result<(), Error> {
        if index >= self.topics.len() {
            return Err(Error::new(ErrorKind::InvalidInput, "index out of range"));
        }
        let deleted = self.topics.remove(index);
        deleted.delete()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read(){
        let test_folder_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_assets/VOSpeaker_test_project");
        let project = ProjectDir::new(&test_folder_path).unwrap();
        for topic in project.topics {
            println!("{}", topic.name());
        }
    }
}