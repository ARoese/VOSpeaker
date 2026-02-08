use std::cell::RefCell;
use std::error::Error;
use std::ops::Deref;
use std::rc::Rc;
use async_compat::Compat;
use rfd::MessageButtons;
use rfd::MessageDialogResult::{No, Yes};
use slint::{spawn_local, ComponentHandle, Model, ModelRc, Weak};
use thiserror::__private17::AsDisplay;
use tokio_util::future::FutureExt;
use tokio_util::sync::CancellationToken;
use crate::{AppWindow, Dialogs, TopicListItem, UIError};
use crate::init::{ErrorSender, ProgressHandle, ProgressHandleSpawner, ProgressSender};
use crate::init::errors::make_error;
use crate::init::ProgressState::{Done, Inflight};
use crate::init::ProgressVal::Determinate;
use crate::models::TopicModel;
use crate::project_dir::topic_lines::SpokenTopicLine;

fn is_short_dialogue(dialogue: &SpokenTopicLine) -> bool {
    let num_words = dialogue.0.split(" ").count();
    let num_chars = dialogue.0.chars().filter(|c| !c.is_ascii_punctuation()).count();
    num_words <= 4 || num_chars <= 10
}

async fn delete_short_voicelines_action(topics: &ModelRc<TopicListItem>, progress_handle: &ProgressHandle) -> Result<(), Box<UIError>> {
    if rfd::MessageDialog::new()
        .set_title("Delete Audio for Short Dialogue?")
        .set_description("This will delete the audio files for all dialogue which is considered 'short'. This means <=10 characters or <= 4 words. This could delete a significant portion of your generated audio!\n\nAre you sure?")
        .set_buttons(MessageButtons::YesNo)
        .show() != Yes {
        return Ok(());
    }
    
    for topic_item in topics.iter() {
        let dialogue_lines_count = topic_item.dialog_lines.row_count();
        let topic = topic_item.dialog_lines.as_any()
            .downcast_ref::<TopicModel>()
            .expect("Topic Model of custom type");

        for (i,dialogue) in topic.iter().enumerate() {
            progress_handle.progress_sender.send(
                Inflight(Determinate {
                    status: format!("[{}] Checking for short voicelines", topic_item.topic_name),
                    range: 0..dialogue_lines_count as u64,
                    progress: i as u64,
                })
            ).ok();

            let spoken = SpokenTopicLine(dialogue.clean_line.to_string());
            if is_short_dialogue(&spoken) {
                topic.delete_audio_file_for(i).await.map_err(|e| make_error(&format!("Failed to delete short voicelines: {e}")))?;
            }
        }
    }
    Ok(())
}

pub fn init_batch_tools(ui: &AppWindow, topics: &ModelRc<TopicListItem>, phs: ProgressHandleSpawner) -> Result<(), Box<dyn Error>>{
    ui.global::<Dialogs>().on_delete_short_voicelines({
        let ui_weak = ui.as_weak();
        let topics = topics.clone();
        move || {
            let handle = phs.spawn();
            let ui_weak = ui_weak.clone();
            let topics = topics.clone();
            let ct = handle.cancellation.clone();
            let fut = Compat::new(async move {
                delete_short_voicelines_action(&topics, &handle)
                    .with_cancellation_token(&handle.cancellation).await;
                handle.progress_sender.send(Done).ok();
            });

            spawn_local(fut).expect("Spawn batch delete task");
        }
    });

    Ok(())
}