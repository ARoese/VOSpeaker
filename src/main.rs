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
mod create_fuz;
mod static_resources;
mod dbvo_manifest;

use crate::chatterbox_generator::{ChatterboxGenerator, ChatterboxGeneratorConfig};
use crate::dialog_generator::{ConfigHashable, DialogGenerationError, DialogGenerator};
use crate::models::{MassGenerationOptions, TopicModel};
use crate::progress::ProgressState::{Done, Inflight};
use crate::progress::ProgressVal::{Determinate, Indeterminate};
use crate::progress::{ProgressHandle, ProgressHandleSpawner, ProgressState, ProgressVal};
use crate::project_dir::ProjectDir;
use async_compat::Compat;
use rodio::Sink;
use slint::{run_event_loop, spawn_local, CloseRequestResponse, JoinHandle, Model, ModelRc, SharedString, ToSharedString, VecModel, Weak};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt::{Display, Formatter};
use std::fs;
use std::fs::File;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use futures::{stream, StreamExt};
use lazy_regex::regex;
use tokio::sync::watch::Sender;
use tokio::sync::{mpsc, watch};
use tokio_util::future::FutureExt;
use tokio_util::sync::CancellationToken;
use log::error;
use crate::create_fuz::wav_to_fuz;
use crate::dbvo_manifest::DBVOManifest;
use crate::static_resources::{deinit_resources_dir, init_resources_dir};
use crate::topic_dir::TopicDir;
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

/// simultaneously makes a UIError struct and prints the error to the console
fn raise(reason: &str) -> UIError {
    eprintln!("ERROR: {}", reason);
    UIError {
        message: reason.into(),
    }
}

/// makes a UIError struct
fn make_error(message: &str) -> UIError {
    UIError {
        message: message.into()
    }
}

impl Display for UIError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}

impl Error for UIError {}

// TODO: make this result for error prop
async fn generate_dialogue_future(ui_weak: Weak<AppWindow>, topic_idx: i32, line_idx: i32) -> Result<(), UIError> {
    let ui = ui_weak.upgrade().unwrap();
    let config: ChatterboxGeneratorConfig = ui.get_genConfig()
        .try_into()
        .map_err(|()| raise("Chatterbox config is invalid"))?;

    let temp = ui
        .get_topicListModel()
        .row_data(topic_idx as usize)
        .ok_or(make_error(&format!("Dialogue line with idx '{}' does not exist", topic_idx)))?;

    let topic = temp.dialog_lines.as_any().downcast_ref::<TopicModel>()
        .expect("Topic model was not custom type");

    // slightly naughty construction, but the model is casting from this anyways.
    // TODO: this can be better done by holding and passing around an Rc to the underlying model, and using map models
    let clean_line = SpokenTopicLine(
        topic.row_data(line_idx as usize)
            .ok_or(make_error(&format!("Dialogue line with idx '{}' does not exist", line_idx)))?
            .clean_line.to_string()
    );
    //let target_path = topic.audio_path(line_idx as usize)?;


    let vo_hash = clean_line.vo_hash();
    let config_hash = config.config_hash();

    let result = ChatterboxGenerator::generate_dialog(config, clean_line).await
        .map_err(|e| make_error(&format!("{:?}", e)))?;

    // TODO: naughty blocking call. The add_vo function should not be responsible for
    // TODO: writing the wav file
    let borrow = topic.topic_dir.borrow();
    let topic_dir = borrow.deref().as_ref().ok_or(make_error("topic dir was moved out of model during generation"))?;
    topic_dir.add_vo(&vo_hash, &config_hash, result)
        .map_err(|err| make_error(&format!("Error writing vo file: {}", err)))?;
    topic.wav_written_for(line_idx as usize);
    Ok(())
}

async fn batch_generate_dialogue_future(ui_weak: Weak<AppWindow>, handle: &ProgressHandle, options: &MassGenerationOptions) -> Result<(), UIError> {
    let ui = ui_weak.upgrade().expect("ui upgrade failed");

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
            })).expect("Progress sender failed prematurely");

            generate_dialogue_future(ui_weak.clone(), i as i32, line_idx as i32)
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

fn batch_generate_dialogue_action(ui_weak: Weak<AppWindow>, handle: ProgressHandle, options: MassGenerationOptions) -> JoinHandle<()> {
    println!("batch generating dialogues");

    let future = Compat::new(async move {
        let result = batch_generate_dialogue_future(ui_weak, &handle, &options)
            .with_cancellation_token(&handle.cancellation)
            .await;

        if let Some(Err(e)) = result {
            handle.error_sender.send(e).await.expect("Error sender closed prematurely");
        }
        handle.progress_sender.send(Done).expect("Error sender closed prematurely");
    });
    spawn_local(future).expect("Spawning of local future failed")
}

fn generate_dialogue_action(ui_weak: Weak<AppWindow>, handle: ProgressHandle, topic_idx: i32, line_idx: i32) -> JoinHandle<()> {
    // TODO: send errors
    println!("generation requested for {}:{}", topic_idx, line_idx);

    let ui_weak = ui_weak.clone();
    let future = Compat::new(async move {
        handle.progress_sender.send(
            Inflight(Indeterminate {status: "Generating dialogue".into()})
        ).ok();

        let result = generate_dialogue_future(ui_weak, topic_idx, line_idx)
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

fn get_substitutions(ui: &AppWindow) -> HashMap<String, String> {
    let substitutions_string = ui.get_substitutions_text().to_string();
    substitutions_string.lines()
        .filter_map(|l| {
            let parts = l.split("->").collect::<Vec<_>>();
            return if parts.len() != 2 {
                None
            } else {
                Some((parts[0].trim().to_string(), parts[1].trim().to_string()))
            }
        }).collect::<HashMap<String, String>>()
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
    let substitutions = get_substitutions(&ui);

    // assign new substitutions to models.
    for topic in ui.get_topicListModel().iter() {
        let custom = topic.dialog_lines.as_any()
            .downcast_ref::<TopicModel>()
            .expect("Topic model was not custom type");

        custom.set_substitutions(substitutions.clone());
    }

    substitutions
}

fn init_generation(ui: &AppWindow, error_sender: &mpsc::Sender<UIError>, progress_sender: &Sender<ProgressState>, cancellation_token: &Rc<RefCell<CancellationToken>>) {
    ui.global::<GenerationActions>().on_generate_dialogue({
        let ui_weak = ui.as_weak();
        let sender = progress_sender.clone();
        let error_sender = error_sender.clone();
        let ct = cancellation_token.clone();
        move |topic_idx, line_idx| {
            generate_dialogue_action(
                ui_weak.clone(),
                ProgressHandle{ error_sender: error_sender.clone(), progress_sender: sender.clone(), cancellation: ct.borrow().clone() },
                topic_idx, line_idx
            );
        }
    });

    ui.global::<GenerationActions>().on_mass_generate_dialogue({
        let ui_weak = ui.as_weak();
        let sender = progress_sender.clone();
        let error_sender = error_sender.clone();
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
                        error_sender: error_sender.clone(),
                        progress_sender: sender.clone(),
                        cancellation: ct.borrow().clone() },
                    options,
                );
            }
        }
    });
}

type ProgressSender = Sender<ProgressState>;
type ErrorSender = mpsc::Sender<UIError>;
fn init_receivers(ui: &AppWindow) -> (ErrorSender, ProgressSender, Rc<RefCell<CancellationToken>>) {
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

    let errors_model = Rc::new(VecModel::<UIError>::from(vec![
        /*
        UIError{
            message: "test error 1".to_shared_string()
        },
        UIError{
            message: "test error 2".to_shared_string()
        },
        UIError{
            message: "test error 3".to_shared_string()
        }
         */
    ]));
    ui.set_errors(ModelRc::new(errors_model.clone().reverse()));
    ui.global::<ErrorToastActions>().on_dismiss_error({
        let model = errors_model.clone();
        move |i| {
            model.remove(model.row_count()-1 - i as usize);
        }
    });

    let (error_sender, mut error_receiver) = mpsc::channel::<UIError>(128);
    spawn_local({
        async move {
            while let Some(error) = error_receiver.recv().await {
                // TODO: proper logging library
                eprintln!("ERROR: {:?}", error);
                errors_model.push(error);
            }
        }
    }).expect("failed to start progress watcher");

    (error_sender, progress_sender, cancellation_token)
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

fn init_expansions(ui: &AppWindow, topics_model: &ModelRc<TopicListItem>, project_dir: &ProjectDir) {
    let expand_config_disk = project_dir.load_expansion_config().unwrap_or(TopicExpansionConfig::default());
    let generated_expand_mappings = topics_model.iter()
        .flat_map(|topic| topic.dialog_lines.as_any().downcast_ref::<TopicModel>().unwrap().collect_globals())
        .map(|s| (s, vec![]))
        .collect::<HashMap<_, Vec<String>>>();

    let generated_config = TopicExpansionConfig {
        expansions: generated_expand_mappings,
        max_expansions: 1,
    };

    let expand_config = generated_config.merge_with(&expand_config_disk);
    let mut expansions = expand_config.expansions.iter().map(|(name, expansions)| Expansion{
        name: name.to_shared_string(),
        substitutions: ModelRc::new(VecModel::from(expansions.iter().map(|x| x.to_shared_string()).collect::<Vec<_>>())),
    }).collect::<Vec<Expansion>>();
    expansions.sort_by_key(|x| x.name.clone());
    // TODO: this should be updated when topics are added/removed

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
        to_collapse.iter()
            .map(|s| s.to_string())
            .collect::<Vec<String>>()
            .join("\n").trim().into()
    });
}

fn init_substitutions(ui: &AppWindow, project_dir: &ProjectDir) {
    ui.global::<SubstitutionsActions>().on_substitutions_changed({
        let ui_weak = ui.as_weak();
        move || {
            let ui = ui_weak.upgrade().unwrap();
            let substitutions = get_substitutions(&ui);
            // assign new substitutions to models.
            for topic in ui.get_topicListModel().iter() {
                let custom = topic.dialog_lines.as_any()
                    .downcast_ref::<TopicModel>()
                    .expect("Topic model was not custom type");

                custom.set_substitutions(substitutions.clone());
            }
        }
    });

    let disk_substitutions = project_dir.load_substitutions().unwrap_or_default();
    let substitutions_text = disk_substitutions.into_iter()
        .map(|(target, replacement)| format!("{} -> {}", target, replacement))
        .collect::<Vec<String>>()
        .join("\n");
    ui.set_substitutions_text(substitutions_text.to_shared_string());
}

fn init_generator(ui: &AppWindow, topics_model: &ModelRc<TopicListItem>, project_dir: &ProjectDir) {
    ui.set_topicListModel(topics_model.clone());
    ui.global::<FilePicking>().on_pick_wav_file(pick_wav_file);
    ui.global::<FilePicking>().on_format_path(format_path);
    let chatterbox_config_disk = project_dir.load_chatterbox_config()
        .unwrap_or(ChatterboxGeneratorConfig{
            cfg_weight: 0.5,
            endpoint: "localhost:9005".into(), // TODO: leave this default when done testing
            exaggeration: 0.5,
            temperature: 0.5,
            voice_path: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("test_assets/female-khajiit.wav"), // TODO: leave this as default when done testing
        });

    if let Some(config) = chatterbox_config_disk.try_into().ok() {
        ui.set_genConfig(config);
    }else{
        println!("Failed to parse chatterbox config from disk. Using defaults instead");
    }
}

fn init_topics(ui: &AppWindow, project_dir: &Rc<ProjectDir>, error_sender: &mpsc::Sender<UIError>) -> Rc<VecModel<TopicListItem>> {
    let expand_config = Rc::new(TopicExpansionConfig::default());
    let substitutions = Rc::new(HashMap::<String, String>::default());

    let topic_dirs = project_dir.get_topic_dirs().expect("failed to load project topic dirs")
        .into_iter().map(|topic_dir|
            TopicListItem{
                topic_name: topic_dir.name().into(),
                dialog_lines: ModelRc::new(TopicModel::new(topic_dir, (*substitutions).clone(), (*expand_config).clone())),
            }
    ).collect::<Vec<_>>();
    let topics_model = Rc::from(VecModel::from(topic_dirs));


    ui.on_add_topic_from_path({
        let error_sender = error_sender.clone();
        let ui_weak = ui.as_weak();
        let project_dir = project_dir.clone();
        let topics_model = topics_model.clone();
        move || {
            // TODO: Give this its own function
            let mut dialog = rfd::FileDialog::new();
            dialog = dialog.set_title("Select topic file(s)")
                .add_filter("Topic File (.topic)", &["topic"])
                .add_filter("Text File (.txt)", &["txt"])
                .add_filter("All Files (*)", &["*"]);

            for path in dialog.pick_files().unwrap_or_default() {
                let topics_dir = project_dir.topics_path();
                let topic_prefix = path.file_prefix().expect("It shouldn't be possible to pick an empty path").to_string_lossy().to_string();
                let new_topic_dir = topics_dir.join(
                    topic_prefix.clone() + ".topic.d"
                );

                match TopicDir::create_new(&new_topic_dir, &path) {
                    Ok(new_dir) => {
                        // TODO: extract substitution and expansion state to a pair of Rc<RefCell> that get shared by all these models
                        // TODO: as it is now, each topic has its own model. They should get notified that there is a new version available
                        // TODO: That way, this thing can just pass in clones of the Rc instead of lying that it changed so it gets re-fetched
                        let expansion_config = handle_expansion_change(ui_weak.clone());
                        let substitutions = handle_substitution_change(ui_weak.clone());
                        topics_model.push(TopicListItem{
                            topic_name: new_dir.name().into(),
                            dialog_lines: ModelRc::new(TopicModel::new(new_dir, substitutions, expansion_config)),
                        });
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
    });

    ui.on_remove_topic({
        let error_sender = error_sender.clone();
        let topics_model = topics_model.clone();
        move |idx| {
            let removed_model = topics_model.remove(idx as usize);
            let model = removed_model.dialog_lines.as_any().downcast_ref::<TopicModel>().unwrap();
            if let Some(topic_dir) = Option::take(model.topic_dir.borrow_mut().deref_mut()) {
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

    topics_model
}

struct PackedDialogues {
    pub export_to_folder: FolderExportDialogue,
    pub export_to_dbvo: DBVOExportDialogue
}

fn format_as_file(name: String) -> String {
    // TODO: This incurs a severe performance penalty because of the regex matching.
    // TODO: It isn't super critical, but it should be optimized.
    /*
    // This is what Absolute Phoenix does, and I trust that for now
    ```Jave
        String result = data.replaceAll("[\\\\/:*?\"<>|]", "_");
        result = result.replaceAll(" ", "_");
        result = result.replaceAll("(_?\\([^)]*\\))+\\s*$", "");
    ```
     */
    let replace_with_underscore = regex!("[\\\\/:*?\"<>| ]");
    let remove = regex!("(_?\\([^)]*\\))+\\s*$");
    let name = replace_with_underscore.replace_all(&name, "_");
    let name = remove.replace_all(&name, "");
    name.to_string()
}

async fn do_export_to_folder(topics_model: &Rc<VecModel<TopicListItem>>, options: &FolderExportOptions, progress_sender: &ProgressSender) -> Result<(), UIError> {
    let export_folder = rfd::FileDialog::new()
        .set_title("Select Export Folder")
        .pick_folder();
    if export_folder.is_none() { return Ok(()); }
    let export_folder = export_folder.unwrap();

    let file_count = fs::read_dir(export_folder.clone())
        .map_err(|_| make_error(&format!("The folder '{}' could not be accessed.", export_folder.to_string_lossy())))?
        .count();
    if file_count > 0 {
        return Err(make_error(&format!("The folder '{}' is not empty.", export_folder.to_string_lossy())));
    }

    for topic in topics_model.iter() {
        let topic_model = topic.dialog_lines
            .as_any()
            .downcast_ref::<TopicModel>()
            .expect("Topic model was not of custom type");

        let num_dialogues = topic_model.row_count();
        let changed_err = make_error("Model changed when exporting.");
        let export_file_root = if options.group_by_topic {
            let sub_folder = export_folder.join(&topic.topic_name);
            if options.topic_suffix {
                sub_folder.with_extension("topic.d")
            }else{
                sub_folder
            }
        } else {
            export_folder.clone()
        };

        tokio::fs::create_dir_all(&export_file_root).await
            .map_err(|e| make_error(&format!("Failed to create topic subdir '{}': {e:?}", export_file_root.to_string_lossy())))?;

        for i in 0..num_dialogues {
            let src = topic_model.audio_path(i).ok_or(changed_err.clone())?;
            if !src.exists() {
                continue;
            }

            let data = topic_model.row_data(i).ok_or(changed_err.clone())?;
            let hash = SpokenTopicLine(data.clean_line.to_string()).vo_hash();
            let expanded = topic_model.line(i).ok_or(changed_err.clone())?;
            let file_name = match options.naming_policy {
                FolderNamingPolicy::ExactSpokenDialogue => { data.clean_line.to_string() }
                FolderNamingPolicy::FormattedSpokenDialogue => { format_as_file(data.clean_line.to_string()) }
                FolderNamingPolicy::ExactExpandedDialogue => { expanded.0.clone() }
                FolderNamingPolicy::FormattedExpandedDialogue => { format_as_file(expanded.0.clone()) }
                FolderNamingPolicy::MD5Hash => { hash.to_string() }
            };

            let dest = export_file_root.join(&file_name).with_added_extension("wav");
            // report progress
            let progress = Inflight(Determinate {
                status: format!("Exporting '{}'", topic.topic_name),
                range: 0..num_dialogues as u64,
                progress: i as u64,
            });
            progress_sender.send(progress).expect("failed to send progress");
            tokio::fs::copy(&src, &dest).await
                .map_err(|e| make_error(&format!("Failed to copy '{}' to '{}': {e:?}", src.to_string_lossy(), dest.to_string_lossy())))?;
            //ProgressState::Inflight(ProgressVal::Determinate {})
        }
    }

    Ok(())
}

async fn process_export_fuz(i: usize, topic_model: &TopicModel, audio_dir: &Path) -> Result<(), UIError> {
    let changed_err = make_error("Model changed when exporting.");
    let src = topic_model.audio_path(i).ok_or(changed_err.clone())?;
    if !src.exists() {
        return Ok(());
    }

    let data = topic_model.row_data(i).ok_or(changed_err.clone())?;
    let hash = SpokenTopicLine(data.clean_line.to_string()).vo_hash();
    let expanded = topic_model.line(i).ok_or(changed_err.clone())?;
    let file_name = format_as_file(expanded.0.clone());

    let dest = audio_dir.join(&file_name).with_added_extension("fuz");
    // TODO: parallelize
    wav_to_fuz(&src, &OsString::from(expanded.0), &dest).await
        .map_err(|e| make_error(&format!("Failed to create fuz '{}': {e}", &dest.to_string_lossy())))?;
    Ok(())
}

async fn do_export_to_dbvo(topics_model: &Rc<VecModel<TopicListItem>>, options: &DBVOExportOptions, progress_sender: &ProgressSender) -> Result<(), UIError> {
    let export_folder = rfd::FileDialog::new()
        .set_title("Select Export Folder")
        .pick_folder();
    if export_folder.is_none() { return Ok(()); }
    let export_folder = export_folder.unwrap();

    let file_count = fs::read_dir(export_folder.clone())
        .map_err(|_| make_error(&format!("The folder '{}' could not be accessed.", export_folder.to_string_lossy())))?
        .count();

    let export_folder = if file_count > 0 {
        export_folder.join(options.voice_pack_name.clone())
    }else{
        export_folder
    };

    let manifest_dir = export_folder.join("DragonbornVoiceOver");
    let audio_dir = export_folder.join("Sound").join("DBVO").join(options.voice_pack_id.clone());

    tokio::fs::create_dir_all(&audio_dir).await
        .map_err(|e| make_error(&format!("Failed to create DBVO audio folder '{}': {e}", audio_dir.to_string_lossy())))?;
    tokio::fs::create_dir_all(&manifest_dir).await
        .map_err(|e| make_error(&format!("Failed to create DBVO manifest folder '{}': {e}", audio_dir.to_string_lossy())))?;

    let manifest_str = serde_json::to_string_pretty(&DBVOManifest{
        voice_pack_name: options.voice_pack_name.clone().into(),
        voice_pack_id: options.voice_pack_id.clone().into(),
    }).expect("Failed to serialize DBVOManifest, which shouldn't be possible since it's so simple.");

    tokio::fs::write(manifest_dir.join(options.voice_pack_id.clone()).with_extension("json"), manifest_str).await
        .map_err(|e| make_error(&format!("Failed to write DBVO manifest: {e}")))?;

    for topic in topics_model.iter() {
        let topic_model = topic.dialog_lines
            .as_any()
            .downcast_ref::<TopicModel>()
            .expect("Topic model was not of custom type");

        let num_dialogues = topic_model.row_count();

        let progress = Inflight(Determinate {
            status: format!("Preparing to export '{}'", topic.topic_name),
            range: 0..num_dialogues as u64,
            progress: 0,
        });
        progress_sender.send(progress).expect("failed to send progress");

        const CONCURRENCY_FACTOR: usize = 32;
        let mut processing_stream = stream::iter(0..num_dialogues)
            .map(|i| {
                process_export_fuz(i, &topic_model, &audio_dir)
            }).buffer_unordered(CONCURRENCY_FACTOR);

        let mut i: u64 = 0;
        while let Some(result) = processing_stream.next().await {
            result?;
            // report progress
            let progress = Inflight(Determinate {
                status: format!("Exporting '{}'", topic.topic_name),
                range: 0..num_dialogues as u64,
                progress: i,
            });
            progress_sender.send(progress).expect("failed to send progress");
            i+=1;
        }
    }

    Ok(())
}

fn init_export(
    ui: &AppWindow,
    topics_model: &Rc<VecModel<TopicListItem>>,
    project_dir: &Rc<ProjectDir>,
    progress_sender: &ProgressSender,
    error_sender: &ErrorSender,
    cancellation_token: &Rc<RefCell<CancellationToken>>
) -> Result<PackedDialogues, Box<dyn Error>> {
    let export_to_folder = FolderExportDialogue::new()?;
    let export_to_dbvo = DBVOExportDialogue::new()?;
    let progress_handle_spawner = ProgressHandleSpawner {
        progress_sender: progress_sender.clone(),
        error_sender: error_sender.clone(),
        cancellation: cancellation_token.clone(),
    };

    let ui_dbvo_manifest = project_dir.load_last_dbvo_manifest()
        .map(|m| DBVOExportOptions {
            voice_pack_id: m.voice_pack_id.to_shared_string(),
            voice_pack_name: m.voice_pack_name.to_shared_string(),
        })
        .unwrap_or_else(|_| DBVOExportOptions::default());
    export_to_dbvo.set_export_options(ui_dbvo_manifest);

    ui.global::<Dialogs>().on_show_export_to_folder({
        let to_folder_weak = export_to_folder.as_weak();
        move || {
            if let Some(strong) = to_folder_weak.upgrade(){
                let res = strong.show();
                res.expect("Failed to show popup");
            }
        }
    });

    ui.global::<Dialogs>().on_show_export_to_dbvo({
        let to_dbvo_weak = export_to_dbvo.as_weak();
        move || {
            if let Some(strong) = to_dbvo_weak.upgrade(){
                let res = strong.show();
                res.expect("Failed to show popup");
            }
        }
    });

    export_to_folder.on_do_export({
        let export_weak = export_to_folder.as_weak();
        let ui_weak = ui.as_weak();
        let topics_model_weak = Rc::downgrade(&topics_model);
        let progress_handle_spawner = progress_handle_spawner.clone();
        move |options| {
            // TODO: panic less here
            let export_to_folder = export_weak.upgrade().expect("failed to upgrade ui");
            let topics_model = topics_model_weak.upgrade().expect("failed to upgrade topics model");
            let ui = ui_weak.upgrade().expect("failed to upgrade ui");
            let progress_handle = progress_handle_spawner.spawn();

            let future = Compat::new(async move {
                let result = do_export_to_folder(&topics_model, &options, &progress_handle.progress_sender)
                    .with_cancellation_token(&progress_handle.cancellation).await;
                if let Some(Err(e)) = result {
                    progress_handle.error_sender.send(e).await.expect("Failed to send error");
                }
                progress_handle.progress_sender.send(Done).expect("failed to send progress");
            });

            spawn_local(future).expect("Failed to spawn export task");
            export_to_folder.hide().expect("Failed to hide dialogue");
        }
    });

    export_to_dbvo.on_do_export({
        let export_weak = export_to_dbvo.as_weak();
        let ui_weak = ui.as_weak();
        let topics_model_weak = Rc::downgrade(&topics_model);
        let progress_handle_spawner = progress_handle_spawner.clone();
        let project_dir = project_dir.clone();
        move |options| {
            // TODO: panic less here
            let export_to_dbvo = export_weak.upgrade().expect("failed to upgrade ui");
            let topics_model = topics_model_weak.upgrade().expect("failed to upgrade topics model");
            let ui = ui_weak.upgrade().expect("failed to upgrade ui");
            let progress_handle = progress_handle_spawner.spawn();
            project_dir.save_last_dbvo_manifest(DBVOManifest{
                voice_pack_name: options.voice_pack_name.to_string(),
                voice_pack_id: options.voice_pack_id.to_string()
            }).ok(); // if the write fails, it's not really a big deal


            let future = Compat::new(async move {
                let result = do_export_to_dbvo(&topics_model, &options, &progress_handle.progress_sender)
                    .with_cancellation_token(&progress_handle.cancellation).await;
                if let Some(Err(e)) = result {
                    progress_handle.error_sender.send(e).await.expect("Failed to send error");
                }
                progress_handle.progress_sender.send(Done).expect("failed to send progress");
            });

            spawn_local(future).expect("Failed to spawn export task");
            export_to_dbvo.hide().expect("Failed to hide dialogue");
        }
    });


    Ok(PackedDialogues {export_to_folder, export_to_dbvo})
}

fn main() -> Result<(), Box<dyn Error>> {
    let resources_guard = init_resources_dir();
    let ui = AppWindow::new()?;

    let project_dir = Rc::from(ProjectDir::new(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_assets/VOSpeaker_test_project")
    )?);

    let (error_sender, progress_sender, cancellation_token) = init_receivers(&ui);


    let topics_model = init_topics(&ui, &project_dir, &error_sender);
    let topics_modelrc = ModelRc::new(topics_model.clone());

    init_generator(&ui, &topics_modelrc, &project_dir);

    init_expansions(&ui, &topics_modelrc, &project_dir);
    init_substitutions(&ui, &project_dir);

    init_generation(&ui, &error_sender, &progress_sender, &cancellation_token);
    init_dialogue_audio(&ui);

    let packed_dialogs = init_export(&ui, &topics_model, &project_dir, &progress_sender, &error_sender, &cancellation_token)?;

    ui.run()?;

    // save configs
    project_dir.save_expansion_config(get_expansion_config(&ui))?;

    if let Some(chatterbox_config) = ui.get_genConfig().try_into().ok() {
        project_dir.save_chatterbox_config(chatterbox_config)?;
    }else{
        eprintln!("Failed to parse chatterbox config, so cannot save it");
    }

    project_dir.save_substitutions(get_substitutions(&ui))?;

    Ok(())
}
