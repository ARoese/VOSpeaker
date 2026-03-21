use crate::audio_conversion::Mp3Path;
use crate::project_dir::hashes::ConfigHash;
use crate::project_dir::topic_dir::TopicDir;
use crate::project_dir::topic_lines::{RawTopicLine, SpokenTopicLine, SubstitutedTopicLine, TopicExpansionConfig};
use rayon::iter::IntoParallelRefIterator;
use rayon::iter::ParallelIterator;
use rayon::slice::ParallelSliceMut;
use slint::{Model, ModelNotify, ModelRc, ModelTracker};
use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::Error;
use std::ops::Deref;
use std::path::Path;
use std::rc::Rc;

type SubstitutionsMap = HashMap<String, String>;

pub struct MassGenerationOptions {
    pub current_config_hash: Option<ConfigHash>
}

pub struct TopicLine {
    pub raw_line: RawTopicLine,
    pub substituted_line: SubstitutedTopicLine,
    pub spoken_topic_line: SpokenTopicLine,
    pub audio_path: Mp3Path,
    pub config_hash: Option<ConfigHash>
}

pub struct TopicModel {
    // this is still required since we need to add mutation functions later
    pub topic_dir: RefCell<Option<TopicDir>>,
    substitutions: Rc<RefCell<SubstitutionsMap>>,
    expansion_config: Rc<RefCell<TopicExpansionConfig>>,
    lines: RefCell<Vec<SubstitutedTopicLine>>,
    // the ModelNotify will allow to notify the UI that the model changes
    notify: ModelNotify
}

impl TopicModel {
    pub fn new(topic_dir: TopicDir, substitutions: Rc<RefCell<SubstitutionsMap>>, expansions: Rc<RefCell<TopicExpansionConfig>>) -> TopicModel {
        let expanded_lines = Self::compute_expanded_lines(&topic_dir.topic_file_ref().lines(), &expansions.borrow(), &substitutions.borrow());
        TopicModel {
            lines: expanded_lines.into(),
            topic_dir: Some(topic_dir).into(),
            notify: Default::default(),
            substitutions: substitutions,
            expansion_config: expansions
        }
    }

    pub fn get_topic_name(&self) -> String {
        let borrow = self.topic_dir.borrow();
        let Some(borrow) = borrow.as_ref() else {
            return "Unknown".into()
        };

        borrow.name()
    }

    pub fn substitutions_changed(&self){
        // this will greatly change the model, but
        // changes will be available on re-request of model values
        self.notify.reset();
    }

    fn should_skip_expansion_config_changed(&self) -> bool {
        // if a global is present in this topic, and it has expansions, we must refresh
        let present_expansions = self.collect_globals();
        for global_name in present_expansions {
            let Some(replacement_set) = self.expansion_config.borrow().expansions.get(&global_name).cloned() else {continue};

            if replacement_set.len() != 0 {
                return false;
            }
        }

        // if a global is used by this topic (I.E. it contains a cached line using that global) but there is no replacement set,
        // that means we have stale expansions and must refresh
        let used_expansions = self.lines.borrow().iter().flat_map(|l| l.1.0.global_names()).collect::<HashSet<_>>();
        for global_name in used_expansions {
            let Some(replacement_set) = self.expansion_config.borrow().expansions.get(&global_name).cloned() else {continue};

            if replacement_set.len() == 0 {
                return false;
            }
        }

        true
    }
    pub fn expansion_config_changed(&self){
        /*
        if self.should_skip_expansion_config_changed(){
            return;
        }
        */
        if let Some(topic_dir) = self.topic_dir.borrow().deref() {
            self.lines.replace(Self::compute_expanded_lines(
                &topic_dir.topic_file_ref().lines(),
                &self.expansion_config.borrow(),
                &self.substitutions.borrow()
            ));
            self.notify.reset(); // this will also greatly change the model
        }
    }

    pub async fn delete_audio_file_for(&self, idx: usize) -> Result<(), Box<dyn std::error::Error>> {
        let path = self.audio_path(idx).ok_or(format!("idx {idx} does not exist"))?;
        if path.exists() {
            tokio::fs::remove_file(&*path).await?;
        }
        self.notify.row_changed(idx);
        Ok(())
    }

    fn compute_expanded_lines(raw_lines: &Vec<RawTopicLine>, config: &TopicExpansionConfig, substitutions: &SubstitutionsMap) -> Vec<SubstitutedTopicLine> {
        let mut res = raw_lines
            .par_iter().flat_map(|line| {line.substitute(&config)})
            // do not show empty lines
            .filter(|line| !line.spoken(&substitutions).0.is_empty())
            .collect::<Vec<_>>();

        res.par_sort_by(|lhs, rhs| lhs.0.to_lowercase().cmp(&rhs.0.to_lowercase()));

        res
    }

    pub fn audio_path(&self, line: usize) -> Option<Mp3Path> {
        let vo_line = self.lines.borrow().get(line)?.clone();
        Some(self.topic_dir.borrow().as_ref()?.mp3_path(&vo_line.spoken(&self.substitutions.borrow()).vo_hash()))
    }
    
    pub fn mp3_modified_for(&self, line_idx: usize) {
        self.notify.row_changed(line_idx);
    }

    pub fn collect_globals(&self) -> HashSet<String> {
        let borrow = self.topic_dir.borrow();
        let topic_dir = if let Some(topic_dir) = borrow.deref() {
            topic_dir
        }else{
            return Default::default();
        };
        
        topic_dir.topic_file_ref().lines()
            .iter()
            .flat_map(|line|
                line.0.global_names()
            ).collect::<HashSet<_>>()
    }
    
    pub fn update_topic_file(&self, other_topic: &Path) -> Result<(), Error> {
        let topic_dir = self.topic_dir.borrow();
        let Some(topic_dir) = topic_dir.as_ref() else{
            return Ok(())
        };
        
        let new_lines = topic_dir.update_topic_file(other_topic)?;
        *self.lines.borrow_mut() = Self::compute_expanded_lines(&new_lines, &self.expansion_config.borrow(), &self.substitutions.borrow());
        self.notify.reset();
        
        Ok(())
    }
}

impl Model for TopicModel {
    type Data = TopicLine;

    fn row_count(&self) -> usize {
        self.lines.borrow().len()
    }

    fn row_data(&self, row: usize) -> Option<Self::Data> {
        let Some(line) = self.lines.borrow().get(row).cloned() else {
            return None;
        };

        let spoken = line.spoken(&self.substitutions.borrow());
        let audio_path = self.audio_path(row).expect("row existence was just checked");
        let config_hash = self.topic_dir.borrow().as_ref()?.get_config_hash(&spoken.vo_hash());
        Some(
            TopicLine {
                raw_line: line.1.clone(),
                substituted_line: line.clone(),
                config_hash,
                spoken_topic_line: spoken,
                audio_path
            }
        )
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub struct Indexed<T> {
    pub idx: usize,
    pub data: T,
}
impl<T> Deref for Indexed<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

pub struct IndexedModel<T> {
    source: ModelRc<T>
}

impl<T> IndexedModel<T> {
    pub fn new(source: ModelRc<T>) -> Self {
        IndexedModel {
            source
        }
    }
}

impl<T> Model for IndexedModel<T> {
    type Data = Indexed<T>;

    fn row_count(&self) -> usize {
        self.source.row_count()
    }

    fn row_data(&self, row: usize) -> Option<Self::Data> {
        self.source.row_data(row)
            .map(|s| Indexed{idx: row, data: s})
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        self.source.model_tracker()
    }
}