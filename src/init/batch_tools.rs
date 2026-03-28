use std::collections::HashSet;
use crate::init::errors::make_error;
use crate::init::ProgressState::{Done, Inflight};
use crate::init::ProgressVal::Determinate;
use crate::init::{ProgressHandle, ProgressHandleSpawner};
use crate::project_dir::topic_lines::SpokenTopicLine;
use crate::{AppWindow, Dialogs, Tools, TopicsModel, UIError};
use async_compat::Compat;
use rfd::{MessageButtons, MessageDialogResult, MessageLevel};
use rfd::MessageDialogResult::Yes;
use slint::{spawn_local, ComponentHandle, Model};
use std::error::Error;
use std::rc::Rc;
use elementtree::WriteOptions;
use tokio_util::future::FutureExt;
use crate::extract_fomod::extract_fomod_topics;
use crate::validate_fomod::{validate_fomod, MissingPath};

fn is_short_dialogue(dialogue: &SpokenTopicLine) -> bool {
    let num_words = dialogue.0.split(" ").count();
    let num_chars = dialogue.0.chars().filter(|c| !c.is_ascii_punctuation()).count();
    num_words <= 4 || num_chars <= 10
}

async fn delete_short_voicelines_action(topics: &Rc<TopicsModel>, progress_handle: &ProgressHandle) -> Result<(), Box<UIError>> {
    if rfd::MessageDialog::new()
        .set_title("Delete Audio for Short Dialogue?")
        .set_description("This will delete the audio files for all dialogue which is considered 'short'. This means <=10 characters or <= 4 words. This could delete a significant portion of your generated audio!\n\nAre you sure?")
        .set_buttons(MessageButtons::YesNo)
        .show() != Yes {
        return Ok(());
    }
    
    for topic_item in topics.iter() {
        let dialogue_lines_count = topic_item.row_count();

        for (i,dialogue) in topic_item.iter().enumerate() {
            progress_handle.progress_sender.send(
                Inflight(Determinate {
                    status: format!("[{}] Checking for short voicelines", topic_item.get_topic_name()),
                    range: 0..dialogue_lines_count as u64,
                    progress: i as u64,
                })
            ).ok();
            
            let is_user_provided = dialogue.config_hash.map(|ch| ch.is_null_hash()).unwrap_or(false);

            let spoken = dialogue.spoken_topic_line;
            if !is_user_provided && is_short_dialogue(&spoken) {
                topic_item.delete_audio_file_for(i).await.map_err(|e| make_error(&format!("Failed to delete short voicelines: {e}")))?;
            }
        }
    }
    Ok(())
}

pub fn init_batch_tools(ui: &AppWindow, topics: &Rc<TopicsModel>, phs: &ProgressHandleSpawner) -> Result<(), Box<dyn Error>> {
    ui.global::<Dialogs>().on_delete_short_voicelines({
        let topics = topics.clone();
        let phs = phs.clone();
        move || {
            let handle = phs.spawn();
            let topics = topics.clone();
            let fut = Compat::new(async move {
                delete_short_voicelines_action(&topics, &handle)
                    .with_cancellation_token(&handle.cancellation).await;
                handle.progress_sender.send(Done).ok();
            });

            spawn_local(fut).expect("Spawn batch delete task");
        }
    });

    ui.global::<Tools>().on_extract_fomod_topics({
        let phs = phs.clone();
        move || {
            let Some(fomod_dir) = rfd::FileDialog::new()
                .set_title("Select FOMOD directory")
                .pick_folder() else {return};
            
            let Some(extract_output) = rfd::FileDialog::new()
                .set_title("Select output directory")
                .pick_folder() else {return};
            
            let ph = phs.spawn();
            
            let compat = Compat::new(async move {
                if let Err(e) = extract_fomod_topics(&fomod_dir, &extract_output).await {
                    ph.error_sender.send(make_error(&format!("Failed to extract FOMOD topics: {e}")))
                        .await.expect("Failed to send err");
                }
            });
            
            spawn_local(compat).expect("Failed to spawn local async task");
        }
    });

    ui.global::<Tools>().on_verify_fomod({
        let phs = phs.clone();
        move || {
            let Some(fomod_dir) = rfd::FileDialog::new()
                .set_title("Select FOMOD directory")
                .pick_folder() else {return};

            let ph = phs.spawn();

            let compat = Compat::new(async move {
                let (root, issues) = match validate_fomod(&fomod_dir).await {
                    Ok(issues) => issues,
                    Err(e) => {
                        ph.error_sender.send(make_error(&format!("Failed to validate fomod file: {e}"))).await.expect("Failed to send err");
                        return;
                    }
                };

                if issues.is_empty() {
                    let _ = rfd::MessageDialog::new()
                        .set_title("FOMOD is valid")
                        .set_buttons(MessageButtons::Ok)
                        .set_level(MessageLevel::Info)
                        .set_description("No issues were detected in this FOMOD")
                        .show();
                    return;
                }
                
                let issues = {
                    let mut issues = issues;
                    issues.sort_by(|lhs, rhs| lhs.mod_name.cmp(&rhs.mod_name));
                    issues.dedup_by(|lhs, rhs| lhs.mod_name == rhs.mod_name);
                    issues
                };
                
                let bad_mods = issues.into_iter().map(|e| e.mod_name).collect::<Vec<_>>().join("\n\t");
                let desc = format!("The following mods reference paths that do not exist:\n\t{bad_mods}\n\nWould you like to write a new ModuleConfig.xml.fixed that does not have these mods?");
                let choice = rfd::MessageDialog::new()
                    .set_title("FOMOD is not valid")
                    .set_buttons(MessageButtons::YesNo)
                    .set_level(MessageLevel::Warning)
                    .set_description(desc)
                    .show();
                
                if choice != Yes {
                    return;
                }
                
                let mut writer = vec![];
                if let Err(e) = root.to_writer_with_options(
                    &mut writer,
                    WriteOptions::new()
                        .set_perform_indent(true)
                ) {
                    ph.error_sender.send(make_error(&format!("Failed to write FOMOD ModuleConfig to buffer {e}")))
                        .await.expect("Failed to send err");
                }

                let fixed_module_config_path = fomod_dir.join("fomod").join("ModuleConfig.xml.fixed");
                if let Err(e) = tokio::fs::write(&fixed_module_config_path, &writer).await {
                    ph.error_sender.send(make_error(&format!("Failed to write FOMOD ModuleConfig to file {e}")))
                        .await.expect("Failed to send err");
                }
            });

            spawn_local(compat).expect("Failed to spawn local async task");
        }
    });


    Ok(())
}