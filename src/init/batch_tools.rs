use crate::init::errors::make_error;
use crate::init::ProgressState::{Done, Inflight};
use crate::init::ProgressVal::Determinate;
use crate::init::{ProgressHandle, ProgressHandleSpawner};
use crate::project_dir::topic_lines::SpokenTopicLine;
use crate::{AppWindow, Dialogs, TopicsModel, UIError};
use async_compat::Compat;
use rfd::MessageButtons;
use rfd::MessageDialogResult::Yes;
use slint::{spawn_local, ComponentHandle, Model};
use std::error::Error;
use std::rc::Rc;
use tokio_util::future::FutureExt;

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

    Ok(())
}