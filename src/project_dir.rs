use std::collections::HashMap;
use std::fs;
use std::io::{Error, ErrorKind};
use std::error::Error as ErrorTrait;
use std::path::{Path, PathBuf};
use crate::chatterbox_generator::ChatterboxGeneratorConfig;
use crate::dbvo_manifest::DBVOManifest;
use crate::topic_dir::TopicDir;
use crate::topic_lines::TopicExpansionConfig;

pub struct ProjectDir {
    pub path: PathBuf
}

impl ProjectDir {
    pub fn new(path: &Path) -> Result<Self, Error> {
        if !path.is_dir() {
            return Err(Error::new(ErrorKind::InvalidInput, "path is not a directory"));
        }

        Ok(ProjectDir {
            path: path.to_path_buf()
        })
    }

    /// path to the topics subdir
    pub fn topics_path(&self) -> PathBuf {
        self.path.join("topics")
    }

    /// this is very likely to fail if previously requested topic dirs weren't closed yet.
    /// make sure to drop the topic dirs provided before calling this function again
    pub fn get_topic_dirs(&self) -> Result<Vec<TopicDir>, Error> {
        let topics_path = self.topics_path();
        fs::create_dir_all(&topics_path)?;

        // TODO: logging
        let dirs = fs::read_dir(topics_path)?
            .filter_map(Result::ok)
            .filter(|e| e.path().is_dir())
            .collect::<Vec<_>>();
        let topics = dirs.iter()
            .filter_map(|d| TopicDir::new(&d.path()).ok())
            .collect::<Vec<_>>();
        Ok(topics)
    }

    const EXPANSIONS_CONF_NAME: &str = "expansions.toml";
    pub fn load_expansion_config(&self) -> Result<TopicExpansionConfig, Box<dyn ErrorTrait>> {
        let expansions_path = self.path.join(Self::EXPANSIONS_CONF_NAME);
        let expansions_text = fs::read_to_string(&expansions_path)?;
        Ok(toml::from_str::<TopicExpansionConfig>(&expansions_text)?)
    }

    pub fn save_expansion_config(&self, mut config: TopicExpansionConfig) -> Result<(), Box<dyn ErrorTrait>> {
        // do not save expansions with empty lists. Instead, let them be initialized as default upon reading
        config.expansions = config.expansions.into_iter().filter(|(k,v)| !v.is_empty()).collect();

        let expansions_string = toml::to_string(&config)?;
        let expansions_path = self.path.join(Self::EXPANSIONS_CONF_NAME);
        Ok(fs::write(&expansions_path, expansions_string)?)
    }

    const CHATTERBOX_CONFIG_NAME: &str = "chatterbox-generator-config.toml";
    pub fn load_chatterbox_config(&self) -> Result<ChatterboxGeneratorConfig, Box<dyn ErrorTrait>> {
        let expansions_path = self.path.join(Self::CHATTERBOX_CONFIG_NAME);
        let chatterbox_text = fs::read_to_string(&expansions_path)?;
        Ok(toml::from_str::<ChatterboxGeneratorConfig>(&chatterbox_text)?)
    }

    pub fn save_chatterbox_config(&self, config: ChatterboxGeneratorConfig) -> Result<(), Box<dyn ErrorTrait>> {
        let chatterbox_text = toml::to_string(&config)?;
        let chatterbox_path = self.path.join(Self::CHATTERBOX_CONFIG_NAME);
        Ok(fs::write(&chatterbox_path, chatterbox_text)?)
    }
    
    const SUBSTITUTIONS_CONFIG_NAME: &str = "substitutions.toml";
    pub fn load_substitutions(&self) -> Result<HashMap<String, String>, Box<dyn ErrorTrait>> {
        let substitutions_path = self.path.join(Self::SUBSTITUTIONS_CONFIG_NAME);
        let substitutions_text = fs::read_to_string(&substitutions_path)?;
        Ok(toml::from_str::<HashMap<String, String>>(&substitutions_text)?)
    }
    
    pub fn save_substitutions(&self, substitutions: HashMap<String, String>) -> Result<(), Box<dyn ErrorTrait>> {
        let substitutions_path = self.path.join(Self::SUBSTITUTIONS_CONFIG_NAME);
        let substitutions_text = toml::to_string(&substitutions)?;
        Ok(fs::write(&substitutions_path, substitutions_text)?)
    }
    
    const DBVO_MANIFEST_NAME: &str = "last-dbvo-manifest.json";
    pub fn load_last_dbvo_manifest(&self) -> Result<DBVOManifest, Box<dyn ErrorTrait>> {
        let dbvo_manifest_path = self.path.join(Self::DBVO_MANIFEST_NAME);
        let dbvo_manifest_text = fs::read_to_string(&dbvo_manifest_path)?;
        Ok(serde_json::from_str::<DBVOManifest>(&dbvo_manifest_text)?)
    }
    
    pub fn save_last_dbvo_manifest(&self, dbvo_manifest: DBVOManifest) -> Result<(), Box<dyn ErrorTrait>> {
        let dbvo_manifest_path = self.path.join(Self::DBVO_MANIFEST_NAME);
        let dbvo_manifest_text = serde_json::to_string(&dbvo_manifest)?;
        Ok(fs::write(&dbvo_manifest_path, dbvo_manifest_text)?)
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
        let topics = project.get_topic_dirs().unwrap();
        for topic in topics {
            println!("{}", topic.name());
        }
    }
}