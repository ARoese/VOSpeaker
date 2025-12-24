// Prevent console window in addition to Slint window in Windows release builds when, e.g., starting the app via file manager. Ignored on other platforms.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config_map_file;
mod topic_dir;
mod project_dir;
mod topic_lines;
mod topic_file;
mod dialog_generator;
mod chatterbox_generator;
mod hashes;
mod models;
mod progress;

use crate::chatterbox_generator::{ChatterboxGenerator, ChatterboxGeneratorConfig};
use crate::dialog_generator::{ConfigHashable, DialogGenerator};
use crate::models::{MassGenerationOptions, TopicModel};
use crate::progress::ProgressState::{Done, Inflight};
use crate::progress::ProgressVal::{Determinate, Indeterminate};
use crate::progress::ProgressHandle;
use crate::project_dir::ProjectDir;
use async_compat::Compat;
use rodio::Sink;
use slint::{spawn_local, CloseRequestResponse, JoinHandle, Model, ModelRc, SharedString, VecModel, Weak};
use std::cell::{Cell, RefCell};
use std::error::Error;
use std::ffi::OsStr;
use std::fs::File;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use tokio::sync::watch;
use tokio_util::future::FutureExt;
use tokio_util::sync::CancellationToken;

slint::include_modules!();

fn pick_wav_file(previous: SharedString) -> SharedString {
    let mut dialog = rfd::FileDialog::new();
    dialog = dialog.set_title("Select a wav file");
    dialog = dialog.add_filter("Wav File", &["wav"]);

    dialog.pick_file()
        .map(|inner| inner.to_string_lossy().to_string().into())
        .unwrap_or_else(|| previous)
}

fn format_path(p: SharedString) -> SharedString {
    PathBuf::from(&p)
        .file_name()
        .unwrap_or(OsStr::new(""))
        .to_string_lossy()
        .into_owned()
        .into()
}

// TODO: make this result for error prop
async fn generate_dialogue_future(ui_weak: Weak<AppWindow>, topic_idx: i32, line_idx: i32) -> Option<()> {
    let config: ChatterboxGeneratorConfig = ui_weak.upgrade()?.get_genConfig().try_into().ok()?;

    let temp = ui_weak.upgrade()?
        .get_topicListModel()
        .row_data(topic_idx as usize)?;
    // TODO: is it ok to hold these strong references across async?
    let topic = temp.dialog_lines.as_any().downcast_ref::<TopicModel>()
        .expect("Topic model was not custom type");
    let line = topic.line(line_idx as usize)?;
    let clean_line = line.clean();
    let target_path = topic.audio_path(line_idx as usize)?;
    
    let vo_hash = clean_line.vo_hash();
    let config_hash = config.config_hash();

    let result = ChatterboxGenerator::generate_dialog(config, clean_line).await;

    if let Ok(result) = result {
        // TODO: naughty blocking call. The add_vo function should not be responsible for
        // TODO: writing the wav file
        topic.topic_dir.borrow_mut().add_vo(&vo_hash, &config_hash, result).ok()?;
        topic.wav_written_for(line_idx as usize);
    }

    Some(())
}

async fn batch_generate_dialogue_future(ui_weak: Weak<AppWindow>, handle: &ProgressHandle, options: &MassGenerationOptions) -> Option<()> {
    let ui = ui_weak.upgrade()?;

    for (i, topic_item) in ui.get_topicListModel().iter().enumerate() {
        let name = topic_item.topic_name.to_string();
        let topic = topic_item.dialog_lines.as_any()
            .downcast_ref::<TopicModel>()
            .expect("Topic model was not custom type");

        let num_to_generate = (0..topic.row_count())
            .map(|line_idx| topic.should_generate(line_idx, &options))
            .filter(|b| *b)
            .count();

        let mut num_generated = 0;
        for line_idx in 0..topic.row_count() {
            if !topic.should_generate(line_idx, &options) {continue}

            handle.progress_sender.send(Inflight(Determinate {
                status: format!("Generating topic [{}]", name),
                range: 0..num_to_generate as u64,
                progress: num_generated,
            })).ok()?;

            generate_dialogue_future(ui_weak.clone(), i as i32, line_idx as i32).await?;
            num_generated += 1;
        }
    }

    handle.progress_sender.send(Done).ok()?;

    Some(())
}

fn batch_generate_dialogue_action(ui_weak: Weak<AppWindow>, handle: ProgressHandle, options: MassGenerationOptions) -> JoinHandle<()> {
    println!("batch generating dialogues");

    let future = Compat::new(async move {
        batch_generate_dialogue_future(ui_weak, &handle, &options)
            .with_cancellation_token(&handle.cancellation)
            .await;
        handle.progress_sender.send(Done).unwrap();
    });
    spawn_local(future).unwrap()
}

fn generate_dialogue_action(ui_weak: Weak<AppWindow>, handle: ProgressHandle, topic_idx: i32, line_idx: i32) -> JoinHandle<()> {
    println!("generation requested for {}:{}", topic_idx, line_idx);

    let ui_weak = ui_weak.clone();
    let future = Compat::new(async move {
        handle.progress_sender.send(
            Inflight(Indeterminate {status: "Generating dialogue".into()})
        ).ok();

        generate_dialogue_future(ui_weak, topic_idx, line_idx)
            .with_cancellation_token(&handle.cancellation).await;

        handle.progress_sender.send(Done).ok();
    });
    spawn_local(future).unwrap()
}

fn main() -> Result<(), Box<dyn Error>> {
    let ui = AppWindow::new()?;

    let project_dir = ProjectDir::new(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_assets/VOSpeaker_test_project")
    )?;

    let topic_dirs = project_dir.topics
        .into_iter().map(|topic_dir|
            TopicListItem{
                topic_name: topic_dir.name().into(),
                dialog_lines: ModelRc::new(TopicModel::new(topic_dir)),
            }
        ).collect::<Vec<_>>();
    let topics_model = VecModel::from(topic_dirs);

    ui.set_topicListModel(ModelRc::new(topics_model));
    ui.global::<FilePicking>().on_pick_wav_file(pick_wav_file);
    ui.global::<FilePicking>().on_format_path(format_path);
    ui.set_genConfig(ChatterboxConfig{
        cfg_weight: 0.5,
        endpoint: "localhost:9005".into(), // TODO: leave this default when done testing
        exaggeration: 0.5,
        temperature: 0.5,
        voicePath: Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_assets/female-khajiit.wav")
            .to_string_lossy().into_owned().into(), // TODO: leave this as default when done testing
    });

    let (progress_sender, mut progress_receiver) = watch::channel(Done);
    let cancellation_token = Rc::new(RefCell::new(CancellationToken::new()));

    ui.window().on_close_requested({
        let ct = cancellation_token.clone();
        move || {
            ct.borrow().cancel();
            CloseRequestResponse::HideWindow
        }
    });

    ui.global::<ProgressActions>().on_cancel({
        let ui_weak = ui.as_weak();
        let ct = cancellation_token.clone();
        move || {
            ct.borrow().cancel();
            ui_weak.upgrade().unwrap().set_progress(Progress {
                active: false,
                indeterminate: false,
                progress_percent: 0.0,
                text: Default::default(),
            });
            ct.replace(CancellationToken::new());
        }
    });

    spawn_local({
        let ui_weak = ui.as_weak();
        async move {
            while let Ok(_) = progress_receiver.changed().await {
                let ui = ui_weak.upgrade().unwrap();
                ui.set_progress(progress_receiver.borrow().deref().into())
            }
        }
    }).expect("failed to start progress watcher");

    ui.global::<GenerationActions>().on_generate_dialogue({
        let ui_weak = ui.as_weak();
        let sender = progress_sender.clone();
        let ct = cancellation_token.clone();
        move |topic_idx, line_idx| {
            generate_dialogue_action(
                ui_weak.clone(),
                ProgressHandle{ progress_sender: sender.clone(), cancellation: ct.borrow().clone() },
                topic_idx, line_idx
            );
        }
    });

    ui.global::<GenerationActions>().on_mass_generate_dialogue({
        let ui_weak = ui.as_weak();
        let sender = progress_sender.clone();
        let ct = cancellation_token.clone(); 
        move |regen_stale| {
            let current_config: Option<ChatterboxGeneratorConfig> = ui_weak
                .upgrade()
                .map(|inner| inner.get_genConfig().try_into().ok())
                .flatten();
            
            if let Some(current_config) = current_config {
                let options = MassGenerationOptions {
                    current_config_hash: if regen_stale {
                        Some(current_config.config_hash())
                    }else{
                        None
                    }
                };

                batch_generate_dialogue_action(
                    ui_weak.clone(),
                    ProgressHandle{ 
                        progress_sender: sender.clone(), 
                        cancellation: ct.borrow().clone() },
                    options,
                );
            }
        }
    });

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

    ui.run()?;

    Ok(())
}
