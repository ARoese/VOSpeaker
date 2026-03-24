use crate::project_dir::topic_lines::RawTopicLine;
use std::cell::RefCell;
use std::fs::OpenOptions;
use std::io;
use std::io::{BufRead, Error};
use std::path::{Path, PathBuf};

pub fn read_topic_lines_from_file(path: &Path) -> Result<Vec<String>, Error> {
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
    
    Ok(lines)
}

fn read_raw_lines_from_file(path: &Path) -> Result<Vec<RawTopicLine>, Error> {
    let mut lines = read_topic_lines_from_file(path)?;
    lines.sort_unstable();
    lines.dedup();

    Ok(lines.iter().map(|l| RawTopicLine::new(&l)).collect())
}

pub struct TopicFile {
    path: PathBuf,
    lines: RefCell<Vec<RawTopicLine>>,
}

impl TopicFile {
    pub fn new(path: &Path) -> Result<TopicFile, Error> {
        let lines = read_raw_lines_from_file(path)?;
        Ok(TopicFile {
            path: path.into(),
            lines: lines.into()
        })
    }
    
    pub fn path(&self) -> &Path {
        &self.path
    }
    
    pub fn lines(&self) -> Vec<RawTopicLine> {
        self.lines.borrow().clone()
    }

    pub fn update_topic_file(&self, new_file: &Path) -> Result<Vec<RawTopicLine>, Error> {
        std::fs::copy(new_file, &self.path)?;

        *self.lines.borrow_mut() = read_raw_lines_from_file(&self.path)?;
            
        Ok(self.lines.borrow().clone())
    }
}