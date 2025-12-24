use std::fs::{OpenOptions};
use std::io;
use std::io::{BufRead, Error};
use std::path::{Path, PathBuf};
use crate::topic_lines::RawTopicLine;

fn read_topic_lines_from_file(path: &Path) -> Result<Vec<RawTopicLine>, Error> {
    let file = OpenOptions::new().read(true).open(path)?;
    let lines = io::BufReader::new(file)
        .lines()
        .collect::<Result<Vec<String>, Error>>()?
        .into_iter()
        .map(|l| RawTopicLine(l))
        .collect();
    
    Ok(lines)
}

pub struct TopicFile {
    path: PathBuf,
    lines: Vec<RawTopicLine>,
}

impl TopicFile {
    pub fn new(path: &Path) -> Result<TopicFile, Error> {
        let lines = read_topic_lines_from_file(path)?;
        Ok(TopicFile {
            path: path.into(),
            lines
        })
    }
    
    pub fn path(&self) -> &Path {
        &self.path
    }
    
    pub fn lines(&self) -> &Vec<RawTopicLine> {
        &self.lines
    }
}