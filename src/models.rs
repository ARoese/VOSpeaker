use crate::hashes::ConfigHash;
use crate::topic_dir::TopicDir;
use crate::topic_lines::{SubstitutedTopicLine, TopicSubstituteConfig};
use crate::TopicDialogLine;
use slint::{Model, ModelNotify, ModelTracker};
use std::any::Any;
use std::cell::RefCell;
use std::path::PathBuf;

pub struct TopicModel {
    // this is still required since we need to add mutation functions later
    pub topic_dir: RefCell<TopicDir>,
    lines: RefCell<Vec<SubstitutedTopicLine>>,
    // the ModelNotify will allow to notify the UI that the model changes
    notify: ModelNotify
}

pub struct MassGenerationOptions {
    pub current_config_hash: Option<ConfigHash>
}

impl TopicModel {
    pub fn new(topic_dir: TopicDir) -> TopicModel {
        TopicModel {
            lines: Self::compute_expanded_lines(&topic_dir).into(),
            topic_dir: topic_dir.into(),
            notify: Default::default(),
        }
    }

    fn make_model_type(&self, line_idx: usize) -> Option<TopicDialogLine> {
        let lines_ref = self.lines.borrow();
        let line = lines_ref.get(line_idx)?;
        Some(TopicDialogLine {
            substituted_line: line.0.clone().into(),
            clean_line: line.clean().0.into(),
            can_play: self.topic_dir.borrow().wav_path(&line.clean().vo_hash()).exists()
        })
    }

    pub fn should_generate(&self, line_idx: usize, options: &MassGenerationOptions) -> bool {
        // don't generate if the line doesn't exist
        self.lines.borrow().get(line_idx).map_or(false, |line| {
            let clean_line = line.clean();
            let vo_hash = clean_line.vo_hash();
            let config_hash = self.topic_dir.borrow().get_config_hash(&vo_hash).cloned();

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

    fn compute_expanded_lines(topic_dir: &TopicDir) -> Vec<SubstitutedTopicLine> {
        // TODO: make this configurable
        let config = TopicSubstituteConfig{
            substitutions: Default::default(),
            max_expansions: 16,
        };

        topic_dir.topic_file_ref().lines()
            .iter().flat_map(|line| {line.substitute(&config)})
            // do not show empty lines
            .filter(|line| !line.clean().0.is_empty())
            /*.map(|line| {
                TopicDialogLine {
                    substituted_line: line.0.clone().into(),
                    clean_line: line.clean().0.into(),
                    can_play: topic_dir.wav_path(&line.clean().vo_hash()).exists()
                }
            })*/
            .collect()
    }

    pub fn audio_path(&self, line: usize) -> Option<PathBuf> {
        let vo_line = self.lines.borrow().get(line)?.clone();
        Some(self.topic_dir.borrow().wav_path(&vo_line.clean().vo_hash()))
    }
    
    pub fn line(&self, line_idx: usize) -> Option<SubstitutedTopicLine> {
        self.lines.borrow().get(line_idx)?.clone().into()
    }
    
    pub fn wav_written_for(&self, line_idx: usize) {
        self.notify.row_changed(line_idx);
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