use crate::init::errors::make_error;
use crate::init::{format_as_file, init_expansions, init_substitutions, ErrorSender};
use crate::models::{IndexedModel, TopicModel};
use crate::project_dir::project_dir::ProjectDir;
use crate::project_dir::topic_dir::TopicDir;
use crate::project_dir::topic_lines::TopicExpansionConfig;
use crate::{AppWindow, Audio, TopicDialogLine, TopicListItem, UIError};
use slint::{spawn_local, ComponentHandle, Model, ModelExt, ModelRc, ToSharedString, VecModel};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::DerefMut;
use std::path::PathBuf;
use std::rc::Rc;
use async_compat::{Compat};
use tempfile::{NamedTempFile};
use tokio::sync::mpsc;
use crate::audio_conversion::{any_to_mp3, mp3_to_any, Mp3Path};
use crate::project_dir::hashes::ConfigHash;

pub fn add_topic_files(project_dir: &Rc<ProjectDir>, topics_model: &Rc<VecModel<Rc<TopicModel>>>, expansions: Rc<RefCell<TopicExpansionConfig>>, substitutions: Rc<RefCell<HashMap<String, String>>>, error_sender: &ErrorSender, topic_files: &Vec<PathBuf>) {
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

    ui.global::<Audio>().on_import_audio({
        let topics_model = topics_model.clone();
        let error_sender = error_sender.clone();
        move |topic_idx, line_idx| {
            let topics_model = topics_model.clone();
            let error_sender = error_sender.clone();
            println!("Importing {topic_idx}, {line_idx}");
            let compat = Compat::new(async move {
                let Some(source_file) = rfd::FileDialog::new()
                    .set_title("Select audio file")
                    .add_filter("Audio", &["wav", "mp3", "flac"])
                    .add_filter("Other Audio Files", &["*"])
                    .pick_file() else { return };


                let tmp_mp3 = match NamedTempFile::with_suffix(".mp3") {
                    Ok(s) => s,
                    Err(e) => {
                        error_sender.send(make_error(&format!("Cannot create temp MP3 file: {e}")))
                            .await.expect("Failed to send Error");
                        return;
                    }
                };

                let copy_source: Mp3Path = if source_file.extension().unwrap_or_default() == "mp3" {
                    source_file.into()
                }else{
                    let tmp_mp3_path = tmp_mp3.path().to_path_buf().into();

                    if let Err(e) = any_to_mp3(&source_file, &tmp_mp3_path).await {
                        error_sender.send(make_error(&format!("Cannot convert file to mp3: {e}")))
                            .await.expect("Failed to send Error");
                        return;
                    }

                    tmp_mp3_path
                };

                let Some(topic) = topics_model.row_data(topic_idx as usize) else {return};
                let Some(line) = topic.row_data(line_idx as usize) else {return};

                if let Err(e) = tokio::fs::copy(&copy_source.to_path_buf(), &line.audio_path.to_path_buf()).await {
                    error_sender.send(make_error(&format!("Failed to copy mp3 to project directory: {e}")))
                        .await.expect("Failed to send Error");
                    return;
                }

                let topic_dir = topic.topic_dir.borrow();
                if let Some(topic_dir) = topic_dir.as_ref() {
                    if let Err(e) = topic_dir.add_vo(&line.spoken_topic_line.vo_hash(), &ConfigHash::make_null_hash()) {
                        error_sender.send(make_error(&format!("Failed to update ConfigHash: {e}")))
                            .await.expect("Failed send Error");
                        return;
                    }
                }else{
                    return
                }
                topic.mp3_modified_for(line_idx as usize);
            });

            spawn_local(compat).expect("failed to spawn async local");
        }
    });

    ui.global::<Audio>().on_export_audio({
         let topics_model = topics_model.clone();
         let error_sender = error_sender.clone();
         move |topic_idx, line_idx| {
             let topics_model = topics_model.clone();
             let error_sender = error_sender.clone();
             println!("Exporting {topic_idx}, {line_idx}");
             let compat = Compat::new(async move {
                 let Some(topic) = topics_model.row_data(topic_idx as usize) else {return};
                 let Some(line) = topic.row_data(line_idx as usize) else {return};
                 if !line.audio_path.exists() {
                     error_sender.send(make_error(&"Dialogue line has no audio to export".to_string()))
                         .await.expect("Error sending failed");
                     return;
                 }

                 let recommended_file_name = format_as_file(line.substituted_line.0);
                 let Some(dest_file) = rfd::FileDialog::new()
                     .set_title("Select export file")
                     .add_filter("mp3", &["mp3"])
                     .add_filter("wav", &["wav"])
                     .add_filter("flac", &["flac"])
                     .add_filter("Other Audio Files", &["*"])
                     .set_file_name(format!("{recommended_file_name}.mp3"))
                     .save_file() else { return };

                 if matches!(dest_file.extension(), Some(x) if x == "mp3") {
                     // just copy the internal mp3
                     if let Err(e) = tokio::fs::copy(line.audio_path.as_path(), &dest_file).await {
                         error_sender.send(make_error(&format!("Failed to copy audio: {e}")))
                             .await.expect("Error sending failed");
                     }
                 } else {
                     // convert internal mp3 to whatever
                     if let Err(e) = mp3_to_any(&line.audio_path, &dest_file).await {
                         error_sender.send(make_error(&format!("Cannot convert audio: {e}")))
                             .await.expect("Failed to send Error");
                     }
                 };
             });

             spawn_local(compat).expect("failed to spawn async local");
         }
    });

    ui.global::<Audio>().on_delete_audio({
        let topics_model = topics_model.clone();
        let error_sender = error_sender.clone();

        move |topic_idx, line_idx| {
            println!("Deleting {topic_idx}, {line_idx}");
            let topics_model = topics_model.clone();
            let error_sender = error_sender.clone();
            let compat = Compat::new(async move {
                let Some(topic) = topics_model.row_data(topic_idx as usize) else {return};
                let Some(line) = topic.row_data(line_idx as usize) else {return};

                if let Err(e) = tokio::fs::remove_file(line.audio_path.to_path_buf()).await {
                    error_sender.send(make_error(&format!("Failed to delete mp3 file: {e}")))
                        .await.expect("Failed to send Error");
                }
                let topic_dir = topic.topic_dir.borrow();
                if let Some(topic_dir) = topic_dir.as_ref() {
                    if let Err(e) = topic_dir.add_vo(&line.spoken_topic_line.vo_hash(), &ConfigHash::make_null_hash()) {
                        error_sender.send(make_error(&format!("Failed to update ConfigHash: {e}")))
                            .await.expect("Failed to send Error");
                    }
                }else{
                    return
                }
                topic.mp3_modified_for(line_idx as usize);
            });

            spawn_local(compat).expect("failed to spawn async local");
        }
    });

    let _expansions_config_model = init_expansions(ui, &topics_model, project_dir, expand_config.clone());
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