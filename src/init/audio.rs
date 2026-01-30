use crate::models::TopicModel;
use crate::{AppWindow, Audio};
use rodio::Sink;
use slint::{ComponentHandle, Model};
use std::cell::Cell;
use std::fs::File;

pub fn init_dialogue_audio(ui: &AppWindow) {
    ui.global::<Audio>().on_play_dialog({
        let ui_weak = ui.as_weak();
        let stream_handle = rodio::OutputStreamBuilder::open_default_stream()
            .expect("open default audio stream");
        let shared_sink  = Cell::<Option<Sink>>::default();
        move |topic_idx, line_idx| {
            || -> Option<()> // lets me use ? for early return
                {
                    let path = ui_weak.upgrade()?
                        .get_topicListModel()
                        .row_data(topic_idx as usize)?
                        .dialog_lines
                        .as_any().downcast_ref::<TopicModel>()
                        .expect("Topic model was not custom type")
                        .audio_path(line_idx as usize)?;
                    println!("Attempting to play audio {topic_idx}:{line_idx} at {}", path.display());

                    // Play the sound directly on the device
                    let sink = rodio::play(
                        &stream_handle.mixer(),
                        File::open(&path).ok()?
                    ).ok()?;
                    shared_sink.set(Some(sink));
                    None
                }();
        }
    });
}