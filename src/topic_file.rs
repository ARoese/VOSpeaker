use std::fs::{File, OpenOptions};
use std::io;
use std::io::{BufRead, BufReader, Bytes, Error, Read};
use std::path::{Path, PathBuf};
use rodio::cpal::BufferSize::Default;
use crate::topic_lines::RawTopicLine;

fn read_topic_lines_from_file(path: &Path) -> Result<Vec<RawTopicLine>, Error> {
    let file = OpenOptions::new().read(true).open(path)?;
    let mut reader = io::BufReader::new(file);

    let mut bytes: Vec<u8> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    let mut line = 1;
    while reader.read_until(b'\n', &mut bytes)? != 0 {
        if let Ok(str) = String::from_utf8(bytes.clone()) {
            // remove newline and carriage return, if present
            let clean_line = str.replace("\r", "")
                .replace("\n", "");
            lines.push(clean_line);
        }else{
            eprintln!("'{}' Line {line} is not valid utf8. It will be ignored", path.to_string_lossy());
        }
        line+=1;
        bytes.clear();
    }

    let lines = lines
        .into_iter()
        .map(|l| RawTopicLine::new(&l))
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