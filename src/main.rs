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
mod expansions;

use crate::chatterbox_generator::{ChatterboxGenerator, ChatterboxGeneratorConfig};
use crate::dialog_generator::{ConfigHashable, DialogGenerator};
use crate::models::{MassGenerationOptions, TopicModel};
use crate::progress::ProgressState::{Done, Inflight};
use crate::progress::ProgressVal::{Determinate, Indeterminate};
use crate::progress::{ProgressHandle, ProgressState};
use crate::project_dir::ProjectDir;
use async_compat::Compat;
use rodio::Sink;
use slint::{spawn_local, CloseRequestResponse, JoinHandle, Model, ModelRc, SharedString, ToSharedString, VecModel, Weak};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::ffi::OsStr;
use std::fs::File;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use lazy_regex::regex;
use tokio::sync::watch::Sender;
use tokio::sync::watch;
use tokio_util::future::FutureExt;
use tokio_util::sync::CancellationToken;
use crate::topic_lines::{SpokenTopicLine, TopicExpansionConfig};

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
    let clean_line = SpokenTopicLine(topic.row_data(topic_idx as usize)?.clean_line.to_string());
    let target_path = topic.audio_path(line_idx as usize)?;

    // slightly naughty construction
    let vo_hash = clean_line.vo_hash();
    let config_hash = config.config_hash();

    let result = ChatterboxGenerator::generate_dialog(config, clean_line).await;

    if let Ok(result) = result {
        // TODO: naughty blocking call. The add_vo function should not be responsible for
        // TODO: writing the wav file
        topic.topic_dir.add_vo(&vo_hash, &config_hash, result).ok()?;
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

fn get_expansion_config(ui: &AppWindow) -> TopicExpansionConfig {
    let expansions = ui.get_expansions().iter()
        .map(|expansion|
            (
                expansion.name.to_string(),
                expansion.substitutions.iter().map(|ss| ss.to_string()).collect::<Vec<_>>(),
            )
        )
        .collect::<HashMap<_, _>>();
    // create new config
    TopicExpansionConfig {
        expansions,
        max_expansions: ui.get_allowed_expansions() as usize
    }
}

fn handle_expansion_change(weak_ui: Weak<AppWindow>) -> TopicExpansionConfig {
    let ui = weak_ui.upgrade().unwrap();
    let expansions_config = get_expansion_config(&ui);
    // assign new config to models.
    for topic in ui.get_topicListModel().iter() {
        let custom = topic.dialog_lines.as_any()
            .downcast_ref::<TopicModel>()
            .expect("Topic model was not custom type");

        custom.set_expansion_config(expansions_config.clone());
    }

    expansions_config
}

fn handle_substitution_change(weak_ui: Weak<AppWindow>) -> HashMap<String, String> {
    let substitution_regex = regex!(r"^(.+) ?-> ?(.*)$");
    let ui = weak_ui.upgrade().unwrap();
    let substitutions = ui.get_substitutions_text().lines()
        .filter_map(|line| {
            let captures = substitution_regex.captures(line)?;
            Some((
                captures[1].to_string(),
                captures[2].to_string(),
            ))
        }).collect::<HashMap<_, _>>();

    // assign new substitutions to models.
    for topic in ui.get_topicListModel().iter() {
        let custom = topic.dialog_lines.as_any()
            .downcast_ref::<TopicModel>()
            .expect("Topic model was not custom type");

        custom.set_substitutions(substitutions.clone());
    }

    substitutions
}

fn init_generation(ui: &AppWindow, progress_sender: &Sender<ProgressState>, cancellation_token: &Rc<RefCell<CancellationToken>>) {
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
}

fn init_receivers(ui: &AppWindow) -> (Sender<ProgressState>, Rc<RefCell<CancellationToken>>) {
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

    (progress_sender, cancellation_token)
}

fn init_dialogue_audio(ui: &AppWindow) {
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

fn init_expansions(ui: &AppWindow, topics_model: ModelRc<TopicListItem>, expand_config_disk: TopicExpansionConfig) {
    let generated_expand_mappings = topics_model.iter()
        .flat_map(|topic| topic.dialog_lines.as_any().downcast_ref::<TopicModel>().unwrap().collect_globals())
        .map(|s| (s, vec![]))
        .collect::<HashMap<_, Vec<String>>>();

    let generated_config = TopicExpansionConfig {
        expansions: generated_expand_mappings,
        max_expansions: 1,
    };

    let expand_config = generated_config.merge_with(&expand_config_disk);
    let expansions: Vec<Expansion> = expand_config.expansions.iter().map(|(name, expansions)| Expansion{
        name: name.to_shared_string(),
        substitutions: ModelRc::new(VecModel::from(expansions.iter().map(|x| x.to_shared_string()).collect::<Vec<_>>())),
    }).collect();

    let expansions_model = ModelRc::new(VecModel::from(expansions));
    ui.set_expansions(expansions_model.clone());
    ui.set_allowed_expansions(expand_config.max_expansions as i32);
    handle_expansion_change(ui.as_weak());

    ui.global::<Mappings>().on_expansionNames(|es|
        ModelRc::new(
            VecModel::<SharedString>::from(
                es.iter().map(|e| e.name.into()).collect::<Vec<_>>()
            )
        )
    );

    ui.global::<Mappings>().on_expansion_changed({
        let weak = ui.as_weak();
        move |index, new_expansions| {
            if let Some(old_expansion) = expansions_model.row_data(index as usize){
                expansions_model.set_row_data(index as usize, Expansion {
                    name: old_expansion.name,
                    substitutions: new_expansions
                });
                //expansions_model.iter().for_each(|i| i.substitutions.iter().for_each(|i2| println!("EXPANSION_ENTRY_DEBUG: {:?}", i2)));
                handle_expansion_change(weak.clone());
            }
        }
    });

    ui.global::<Mappings>().on_max_expansions_changed({
        let weak = ui.as_weak();
        move || {
            handle_expansion_change(weak.clone());
        }
    });

    ui.global::<Mappings>().on_parseSubstitutions(|to_parse| {
        let mut seen = HashSet::<String>::new();
        let substitutions = to_parse
            .lines()
            // process lines some
            .filter_map(|line| {if !line.trim().is_empty() {Some(line.trim())} else {None}})
            // deduplicate in-place
            .filter(|sub| {
                let as_string = sub.to_string();
                return if seen.contains(&as_string) {
                    false
                } else {
                    seen.insert(as_string);
                    true
                }
            })
            // NOTE: inefficient duplicate allocations
            .map(|sub| sub.to_string().into())
            .collect::<Vec<SharedString>>();

        ModelRc::new(VecModel::from(substitutions))
    });

    ui.global::<Mappings>().on_collapseSubstitutions(|to_collapse| {
        let text_block = to_collapse.iter()
            .map(|s| s.to_string())
            .collect::<Vec<String>>()
            .join("\n");
        // ensure newline at the end so user doesn't have to fight the parses
        format!("{}\n", text_block).into()
    });
}

fn main() -> Result<(), Box<dyn Error>> {
    let ui = AppWindow::new()?;

    let project_dir = ProjectDir::new(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_assets/VOSpeaker_test_project")
    )?;


    let expand_config = Rc::new(TopicExpansionConfig::default());
    let substitutions = Rc::new(HashMap::<String, String>::default());

    let expand_config_disk = project_dir.load_expansion_config().unwrap_or(TopicExpansionConfig::default());
    let topic_dirs = project_dir.get_topic_dirs().expect("failed to load project topic dirs")
        .into_iter().map(|topic_dir|
            TopicListItem{
                topic_name: topic_dir.name().into(),
                dialog_lines: ModelRc::new(TopicModel::new(topic_dir, (*substitutions).clone(), (*expand_config).clone())),
            }
        ).collect::<Vec<_>>();
    let topics_model = ModelRc::new(VecModel::from(topic_dirs));

    ui.set_topicListModel(topics_model.clone());
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

    init_expansions(&ui, topics_model.clone(), expand_config_disk);

    let (progress_sender, cancellation_token) = init_receivers(&ui);
    init_generation(&ui, &progress_sender, &cancellation_token);
    init_dialogue_audio(&ui);

    ui.run()?;
    project_dir.save_expansion_config(get_expansion_config(&ui))?;
    Ok(())
}
