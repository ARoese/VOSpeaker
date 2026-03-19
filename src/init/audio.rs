use crate::models::TopicModel;
use crate::{AppWindow, Audio, TopicsModel};
use rodio::Sink;
use slint::{ComponentHandle, Model};
use std::cell::Cell;
use std::fs::File;
use std::ops::Deref;
use std::rc::Rc;

pub fn init_dialogue_audio(ui: &AppWindow, topics_model: &Rc<TopicsModel>) {
    ui.global::<Audio>().on_play_dialog({
        let topics_model = topics_model.clone();
        let stream_handle = rodio::OutputStreamBuilder::open_default_stream()
            .expect("open default audio stream");
        let shared_sink  = Cell::<Option<Sink>>::default();
        move |topic_idx, line_idx| {
            || -> Option<()> // lets me use ? for early return
                {
                    let path = topics_model
                        .row_data(topic_idx as usize)?
                        .audio_path(line_idx as usize)?;
                    println!("Attempting to play audio {topic_idx}:{line_idx} at {}", path.display());

                    // Play the sound directly on the device
                    let sink = rodio::play(
                        &stream_handle.mixer(),
                        File::open(&path.deref()).ok()?
                    ).ok()?;
                    shared_sink.set(Some(sink));
                    None
                }();
        }
    });
}