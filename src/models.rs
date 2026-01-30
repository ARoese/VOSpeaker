use crate::project_dir::hashes::ConfigHash;
use crate::project_dir::topic_dir::TopicDir;
use crate::project_dir::topic_lines::{SubstitutedTopicLine, TopicExpansionConfig};
use crate::TopicDialogLine;
use slint::{Model, ModelNotify, ModelTracker};
use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::path::PathBuf;

type SubstitutionsMap = HashMap<String, String>;

pub struct TopicModel {
    // this is still required since we need to add mutation functions later
    pub topic_dir: RefCell<Option<TopicDir>>,
    substitutions: RefCell<SubstitutionsMap>,
    expansion_config: RefCell<TopicExpansionConfig>,
    lines: RefCell<Vec<SubstitutedTopicLine>>,
    // the ModelNotify will allow to notify the UI that the model changes
    notify: ModelNotify
}

pub struct MassGenerationOptions {
    pub current_config_hash: Option<ConfigHash>
}

impl TopicModel {
    pub fn new(topic_dir: TopicDir, substitutions: SubstitutionsMap, expansions: TopicExpansionConfig) -> TopicModel {
        let expanded_lines = Self::compute_expanded_lines(&topic_dir, &expansions, &substitutions);
        TopicModel {
            lines: expanded_lines.into(),
            topic_dir: Some(topic_dir).into(),
            notify: Default::default(),
            substitutions: substitutions.into(),
            expansion_config: expansions.into()
        }
    }

    pub fn set_substitutions(&self, new_substitutions: SubstitutionsMap){
        self.substitutions.replace(new_substitutions);
        // this will greatly change the model, but
        // changes will be available on re-request of model values
        self.notify.reset();
    }

    pub fn set_expansion_config(&self, new_expansions: TopicExpansionConfig){
        if let Some(topic_dir) = self.topic_dir.borrow().deref() {
            self.expansion_config.replace(new_expansions);
            self.lines.replace(Self::compute_expanded_lines(
                topic_dir,
                &self.expansion_config.borrow(),
                &self.substitutions.borrow()
            ));
            self.notify.reset(); // this will also greatly change the model
        }
    }

    fn make_model_type(&self, line_idx: usize) -> Option<TopicDialogLine> {
        let lines_ref = self.lines.borrow();
        let line = lines_ref.get(line_idx)?;
        let borrow = self.topic_dir.borrow();
        let topic_dir = borrow.as_ref()?;
        Some(TopicDialogLine {
            substituted_line: line.0.clone().into(),
            clean_line: line.spoken(&self.substitutions.borrow()).0.into(),
            can_play: topic_dir.wav_path(&line.spoken(&self.substitutions.borrow()).vo_hash()).exists()
        })
    }

    pub fn should_generate(&self, line_idx: usize, options: &MassGenerationOptions) -> bool {
        let borrow = self.topic_dir.borrow();
        let topic_dir = if let Some(topic_dir) = borrow.deref() {
            topic_dir
        }else{
            return false;
        };

        // don't generate if the line doesn't exist
        self.lines.borrow().get(line_idx).map_or(false, |line| {
            let clean_line = line.spoken(&self.substitutions.borrow());
            let vo_hash = clean_line.vo_hash();
            let config_hash = topic_dir.get_config_hash(&vo_hash);

            if let Some(config_hash) = config_hash {
                if let Some(current_config_hash) = options.current_config_hash {
                    // if there is a config hash to compare against, generate if they are different
                    current_config_hash != config_hash
                }else{
                    false // config hash exists and we aren't regenerating stale
                }
            }else{
                true // there is no config hash. Generate.
            }
        })
    }

    fn compute_expanded_lines(topic_dir: &TopicDir, config: &TopicExpansionConfig, substitutions: &SubstitutionsMap) -> Vec<SubstitutedTopicLine> {
        topic_dir.topic_file_ref().lines()
            .iter().flat_map(|line| {line.substitute(&config)})
            // do not show empty lines
            .filter(|line| !line.spoken(&substitutions).0.is_empty())
            /*.map(|line| {
                TopicDialogLine {
                    substituted_line: line.0.clone().into(),
                    clean_line: line.clean().0.into(),
                    can_play: topic_dir.wav_path(&line.clean().vo_hash()).exists()
                }
            })*/
            .collect::<Vec<_>>()
    }

    pub fn audio_path(&self, line: usize) -> Option<PathBuf> {
        let vo_line = self.lines.borrow().get(line)?.clone();
        Some(self.topic_dir.borrow().as_ref()?.wav_path(&vo_line.spoken(&self.substitutions.borrow()).vo_hash()))
    }
    
    pub fn line(&self, line_idx: usize) -> Option<SubstitutedTopicLine> {
        self.lines.borrow().get(line_idx)?.clone().into()
    }
    
    pub fn wav_written_for(&self, line_idx: usize) {
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
        /*
        self.lines.borrow().iter()
            .flat_map(|line| {line.globals()})
            .collect()
         */
    }
}

impl Model for TopicModel {
    type Data = TopicDialogLine;

    fn row_count(&self) -> usize {
        self.lines.borrow().len()
    }

    fn row_data(&self, row: usize) -> Option<Self::Data> {
        self.make_model_type(row)
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }

    fn as_any(&self) -> &dyn Any { self }
}