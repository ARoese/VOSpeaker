use std::cell::RefCell;
use crate::init::errors::make_error;
use crate::models::{IndexedModel, TopicModel};
use crate::project_dir::project_dir::ProjectDir;
use crate::project_dir::topic_dir::TopicDir;
use crate::project_dir::topic_lines::TopicExpansionConfig;
use crate::{AppWindow, TopicDialogLine, TopicListItem, UIError};
use slint::{spawn_local, ComponentHandle, Model, ModelExt, ModelRc, SortModel, ToSharedString, VecModel, Weak};
use std::collections::HashMap;
use std::ops::DerefMut;
use std::path::PathBuf;
use std::rc::Rc;
use tokio::sync::mpsc;
use crate::init::{init_expansions, init_substitutions, ErrorSender, ExpansionsConfigModel};

pub fn add_topic_files(project_dir: &Rc<ProjectDir>, topics_model: &Rc<VecModel<Rc<TopicModel>>>, expansions: Rc<RefCell<TopicExpansionConfig>>, substitutions: Rc<RefCell<HashMap<String, String>>>,  error_sender: &ErrorSender, topic_files: &Vec<PathBuf>) {
    for path in topic_files {
        let topics_dir = project_dir.topics_path();
        let topic_prefix = path.file_prefix().expect("It shouldn't be possible to pick an empty path").to_string_lossy().to_string();
        let new_topic_dir = topics_dir.join(
            topic_prefix.clone() + ".topic.d"
        );
        
        // update if a topic with the same name exists
        if let Some(existing_topic) = topics_model.iter().find(|topic| topic.get_topic_name().to_string() == topic_prefix) {
            if let Err(e) = existing_topic.update_topic_file(&path) {
                let name = existing_topic.get_topic_name();
                spawn_local({
                    let error_sender = error_sender.clone();
                    async move {
                        error_sender.send(make_error(&format!("Error updating topic '{}': {e:?}", name))).await.ok();
                    }
                }).expect("failed to spawn async local");
            }
        } else { // make a new one if the topic doesn't exist
            match TopicDir::create_new(&new_topic_dir, &path) {
                Ok(new_dir) => {
                    let new_topic_file = Rc::new(TopicModel::new(new_dir, substitutions.clone(), expansions.clone()));
                    topics_model.push(new_topic_file);
                }
                Err(e) => {
                    spawn_local({
                        let error_sender = error_sender.clone();
                        async move {
                            error_sender
                                .send(make_error(&format!("Cannot add topic file '{}': {:?}", topic_prefix, e)))
                                .await
                                .expect("Failed to send Error");
                        }
                    }).expect("failed to spawn async local");
                }
            }
        }
    }
}

pub fn init_topics(ui: &AppWindow, project_dir: &Rc<ProjectDir>, expand_config: &Rc<RefCell<TopicExpansionConfig>>, substitutions: &Rc<RefCell<HashMap<String, String>>>, error_sender: &mpsc::Sender<UIError>) -> Rc<VecModel<Rc<TopicModel>>> {
    let topic_dirs = project_dir.get_topic_dirs().expect("failed to load project topic dirs")
        .into_iter()
        .map(|topic_dir| {
            Rc::new(TopicModel::new(topic_dir, substitutions.clone(), expand_config.clone()))
        }
    ).collect::<Vec<_>>();
    let topics_model = Rc::new(VecModel::from(topic_dirs));

    ui.on_add_topic_from_path({
        let error_sender = error_sender.clone();
        let project_dir = project_dir.clone();
        let topics_model = topics_model.clone();
        let expand_config = expand_config.clone();
        let substitutions = substitutions.clone();
        move || {
            let mut dialog = rfd::FileDialog::new();
            dialog = dialog.set_title("Select topic file(s)")
                .add_filter("Topic File (.topic)", &["topic"])
                .add_filter("Text File (.txt)", &["txt"])
                .add_filter("All Files (*)", &["*"]);

            if let Some(files) = dialog.pick_files() {
                add_topic_files(&project_dir, &topics_model, expand_config.clone(), substitutions.clone(),  &error_sender, &files);
            }
        }
    });

    ui.on_remove_topic({
        let error_sender = error_sender.clone();
        let topics_model = topics_model.clone();
        move |idx| {
            let removed_model = topics_model.remove(idx as usize);
            if let Some(topic_dir) = Option::take(removed_model.topic_dir.borrow_mut().deref_mut()) {
                if let Err(e) = topic_dir.delete() {
                    spawn_local({
                        let error_sender = error_sender.clone();
                        let idx = idx;
                        async move {
                            error_sender.send(make_error(&format!("Failed to remove topic {idx}: {:?}", e)))
                                .await
                                .expect("Failed to send Error");
                        }
                    }).expect("failed to spawn async local");
                }
            }
        }
    });

    let expansions_config_model = init_expansions(ui, &topics_model, project_dir, expand_config.clone());
    init_substitutions(ui, &topics_model, project_dir, substitutions.clone());

    let sorted_topics = IndexedModel::new(topics_model.clone().into())
        .sort_by(|lhs, rhs|
            lhs.data.get_topic_name().to_lowercase().cmp(&rhs.data.get_topic_name().to_lowercase())
        );

    let ui_topics = sorted_topics.map(|e| {
        let topic_name = e.get_topic_name().to_shared_string();
        let topic_lines = IndexedModel::new(ModelRc::from(e.data))
            .map(|line| {
                TopicDialogLine {
                    can_play: line.audio_path.exists(),
                    clean_line: line.spoken_topic_line.0.clone().into(),
                    index: line.idx as i32,
                    substituted_line: line.substituted_line.0.clone().into(),
                }
            });

        TopicListItem {
            dialog_lines: ModelRc::new(topic_lines),
            topic_name: topic_name,
            index: e.idx as i32,
        }
    });
    let ui_topics = ModelRc::new(ui_topics);

    ui.set_topicListModel(ui_topics);
    
    topics_model
}