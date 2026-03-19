use crate::audio_conversion::{wav_to_mp3, WavPath};
use crate::chatterbox_generator::{ChatterboxGenerator, ChatterboxGeneratorConfig};
use crate::dialog_generator::{ConfigHashable, DialogGenerator};
use crate::init::errors::{make_error, raise};
use crate::init::ProgressState::{Done, Inflight};
use crate::init::ProgressVal::{Determinate, Indeterminate};
use crate::init::{ProgressHandle, ProgressHandleSpawner};
use crate::models::MassGenerationOptions;
use crate::{AppWindow, GenerationActions, TopicsModel, UIError};
use async_compat::Compat;
use slint::{spawn_local, ComponentHandle, JoinHandle, Model, Weak};
use std::ops::Deref;
use std::rc::Rc;
use tokio_util::future::FutureExt;

// TODO: make this result for error prop
async fn generate_dialogue_future(ui_weak: Weak<AppWindow>, topics_model: Rc<TopicsModel>, topic_idx: i32, line_idx: i32) -> Result<(), UIError> {
    let ui = ui_weak.upgrade().unwrap();
    let config: ChatterboxGeneratorConfig = ui.get_genConfig()
        .try_into()
        .map_err(|()| raise("Chatterbox config is invalid"))?;

    let topic = topics_model
        .row_data(topic_idx as usize)
        .ok_or(make_error(&format!("Dialogue line with idx '{}' does not exist", topic_idx)))?;

    let line = topic.row_data(line_idx as usize)
        .ok_or(make_error(&format!("Dialogue line with idx '{}' does not exist", topic_idx)))?;

    let mp3_path = line.audio_path;

    let clean_line = line.spoken_topic_line;
    //let target_path = topic.audio_path(line_idx as usize)?;

    let vo_hash = clean_line.vo_hash();
    let config_hash = config.config_hash();

    let result = ChatterboxGenerator::generate_dialog(config, clean_line).await
        .map_err(|e| make_error(&format!("{:?}", e)))?;
    
    let borrow = topic.topic_dir.borrow();
    
    let tmp_wav = tempfile::Builder::new().suffix(".wav").tempfile()
        .map_err(|e| make_error(&format!("Failed to make tmp wav file: {e:?}")))?;
    let tmp_wav = WavPath::from(tmp_wav.path().to_path_buf());
    tokio::fs::write(tmp_wav.deref(), result).await
        .map_err(|e| make_error(&format!("Failed to write tmp wav file: {e:?}")))?;
    wav_to_mp3(&tmp_wav, &mp3_path).await
        .map_err(|e| make_error(&format!("Failed to convert wav to mp3: {e:?}")))?;

    let topic_dir = borrow.deref().as_ref().ok_or(make_error("topic dir was moved out of model during generation"))?;
    topic_dir.add_vo(&vo_hash, &config_hash)
        .map_err(|err| make_error(&format!("Error writing vo file: {}", err)))?;
    topic.mp3_written_for(line_idx as usize);
    Ok(())
}

async fn batch_generate_dialogue_future(ui_weak: Weak<AppWindow>, topics_model: Rc<TopicsModel>, handle: &ProgressHandle, options: &MassGenerationOptions) -> Result<(), UIError> {
    for (i, topic) in topics_model.iter().enumerate() {
        let name = topic.get_topic_name().to_string();

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
            })).expect("Progress sender failed prematurely");

            generate_dialogue_future(ui_weak.clone(), topics_model.clone(), i as i32, line_idx as i32)
                .await
                .map_err(|e|
                    make_error(&format!("Error when generating dialogue line ({}, {}): {}", i, line_idx, e.message))
                )?;
            num_generated += 1;
        }
    }

    handle.progress_sender.send(Done).expect("Progress sender closed prematurely");

    Ok(())
}

fn batch_generate_dialogue_action(ui_weak: Weak<AppWindow>, topics_model: Rc<TopicsModel>, handle: ProgressHandle, options: MassGenerationOptions) -> JoinHandle<()> {
    println!("batch generating dialogues");

    let future = Compat::new(async move {
        let result = batch_generate_dialogue_future(ui_weak, topics_model, &handle, &options)
            .with_cancellation_token(&handle.cancellation)
            .await;

        if let Some(Err(e)) = result {
            handle.error_sender.send(e).await.expect("Error sender closed prematurely");
        }
        handle.progress_sender.send(Done).expect("Error sender closed prematurely");
    });
    spawn_local(future).expect("Spawning of local future failed")
}

fn generate_dialogue_action(ui_weak: Weak<AppWindow>, topics_model: Rc<TopicsModel>, handle: ProgressHandle, topic_idx: i32, line_idx: i32) -> JoinHandle<()> {
    // TODO: send errors
    println!("generation requested for {}:{}", topic_idx, line_idx);

    let ui_weak = ui_weak.clone();
    let future = Compat::new(async move {
        handle.progress_sender.send(
            Inflight(Indeterminate {status: "Generating dialogue".into()})
        ).ok();

        let result = generate_dialogue_future(ui_weak, topics_model, topic_idx, line_idx)
            .with_cancellation_token(&handle.cancellation).await;

        if let Some(Err(e)) = result {
            handle.error_sender
                .send(make_error(&format!("Error when generating dialogue ({topic_idx}, {line_idx}): {}", e.message)))
                .await.expect("Error sender closed prematurely");
        }

        handle.progress_sender.send(Done).ok();
    });
    spawn_local(future).unwrap()
}

pub fn init_generation(ui: &AppWindow, topics_model: &Rc<TopicsModel>, phs: &ProgressHandleSpawner) {
    ui.global::<GenerationActions>().on_generate_dialogue({
        let ui_weak = ui.as_weak();
        let topics_model = topics_model.clone();
        let phs = phs.clone();
        move |topic_idx, line_idx| {
            let ph = phs.spawn();
            generate_dialogue_action(
                ui_weak.clone(),
                topics_model.clone(),
                ph,
                topic_idx, line_idx
            );
        }
    });

    ui.global::<GenerationActions>().on_mass_generate_dialogue({
        let ui_weak = ui.as_weak();
        let topics_model = topics_model.clone();
        let phs = phs.clone();
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
                
                let ph = phs.spawn();
                batch_generate_dialogue_action(
                    ui_weak.clone(),
                    topics_model.clone(),
                    ph,
                    options,
                );
            }
        }
    });
}