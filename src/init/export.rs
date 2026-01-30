use crate::create_fuz::wav_to_fuz;
use crate::dbvo_manifest::DBVOManifest;
use crate::init::errors::make_error;
use crate::init::receivers::{ErrorSender, ProgressSender};
use crate::init::ProgressHandleSpawner;
use crate::init::ProgressState::{Done, Inflight};
use crate::init::ProgressVal::Determinate;
use crate::models::TopicModel;
use crate::project_dir::hashes::VOHash;
use crate::project_dir::project_dir::ProjectDir;
use crate::project_dir::topic_lines::SpokenTopicLine;
use crate::{AppWindow, DBVOExportDialogue, DBVOExportOptions, Dialogs, FolderExportDialogue, FolderExportOptions, FolderNamingPolicy, TopicListItem, UIError};
use async_compat::Compat;
use futures::{stream, StreamExt};
use lazy_regex::regex;
use slint::{spawn_local, ComponentHandle, Model, ToSharedString, VecModel};
use std::cell::RefCell;
use std::collections::HashSet;
use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use tokio_util::future::FutureExt;
use tokio_util::sync::CancellationToken;

pub struct PackedDialogues {
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

/// true if file a is newer than file b
fn is_newer_than(a: &Path, b: &Path) -> bool {
    let a_metadata = fs::metadata(a);
    let b_metadata = fs::metadata(b);
    if let Ok(a_metadata) = a_metadata && let Ok(b_metadata) = b_metadata {
        if let Ok(a_mod_time) = a_metadata.modified() && let Ok(b_mod_time) = b_metadata.modified() {
            a_mod_time > b_mod_time
        }else{
            false
        }
    }else{
        false
    }
}

async fn process_export_fuz(i: usize, topic_model: &TopicModel, audio_dir: &Path, processing_set: Rc<RefCell<HashSet<VOHash>>>) -> Result<(), UIError> {
    // mut access to the processing_set refcell is safe here if the mut ref is never held across an async boundary
    // and futures are not advanced on separate threads (this scenario should be impossible to compile anyways because there is no mutex)
    let changed_err = make_error("Model changed when exporting.");
    let src = topic_model.audio_path(i).ok_or(changed_err.clone())?;
    if !src.exists() {
        return Ok(());
    }

    let data = topic_model.row_data(i).ok_or(changed_err.clone())?;
    let hash = SpokenTopicLine(data.clean_line.to_string()).vo_hash();
    if processing_set.borrow().contains(&hash) {
        // someone else is already processing this. Let them handle it.
        return Ok(());
    }
    // mark this hash as being processed
    // TODO: make this cleaner with a dropguard or something
    processing_set.borrow_mut().insert(hash);
    let expanded = topic_model.line(i).ok_or(changed_err.clone())?;
    let file_name = format_as_file(expanded.0.clone());
    let cache_dest = src.with_extension("fuz");
    let final_dest = audio_dir.join(&file_name).with_added_extension("fuz");
    // if the fuz file is newer than the wav file, we don't need to re-process it
    if cache_dest.exists() && is_newer_than(&cache_dest, &src) {
        // unmark this hash as being processed. From this point on,
        // the fuz file's existence will prevent reprocessing
        tokio::fs::copy(&cache_dest, &final_dest).await
            .map_err(|e| make_error(&format!("Failed to move created fuz '{}': {e}", &final_dest.to_string_lossy())))?;
        processing_set.borrow_mut().remove(&hash);
        return Ok(());
    }

    wav_to_fuz(&src, &OsString::from(expanded.0), &cache_dest).await
        .map_err(|e| make_error(&format!("Failed to create fuz '{}': {e}", &final_dest.to_string_lossy())))?;

    tokio::fs::copy(&cache_dest, &final_dest).await
        .map_err(|e| make_error(&format!("Failed to move created fuz '{}': {e}", &final_dest.to_string_lossy())))?;

    // unmark this hash as being processed. From this point on,
    // the fuz file's existence will prevent reprocessing
    processing_set.borrow_mut().remove(&hash);
    Ok(())
}

/// Creates a complete DBVO dir, respecting the options given, and returns the directory in which fuz files should be placed.
/// The audio directory will have already been created
async fn make_dbvo_dirs(export_folder: &Path, options: &DBVOExportOptions, topic_name: &str) -> Result<PathBuf, UIError> {
    let manifest_str = serde_json::to_string_pretty(&DBVOManifest{
        voice_pack_name: options.voice_pack_name.clone().into(),
        voice_pack_id: options.voice_pack_id.clone().into(),
    }).expect("Failed to serialize DBVOManifest, which shouldn't be possible since it's so simple.");

    let export_folder = if(options.separate_topics) {
        export_folder.join(format!("{} - {topic_name}", options.voice_pack_name))
    }else{
        export_folder.into()
    };

    let manifest_dir = export_folder.join("DragonbornVoiceOver").join("voice_packs");
    let audio_dir = export_folder.join("Sound").join("DBVO").join(options.voice_pack_id.clone());

    tokio::fs::create_dir_all(&audio_dir).await
        .map_err(|e| make_error(&format!("Failed to create DBVO audio folder '{}': {e}", audio_dir.to_string_lossy())))?;
    tokio::fs::create_dir_all(&manifest_dir).await
        .map_err(|e| make_error(&format!("Failed to create DBVO manifest folder '{}': {e}", audio_dir.to_string_lossy())))?;

    tokio::fs::write(manifest_dir.join(options.voice_pack_id.clone()).with_extension("json"), manifest_str).await
        .map_err(|e| make_error(&format!("Failed to write DBVO manifest: {e}")))?;
    
    Ok(audio_dir)
}

async fn do_export_to_dbvo(topics_model: &Rc<VecModel<TopicListItem>>, options: &DBVOExportOptions, progress_sender: &ProgressSender) -> Result<(), UIError> {
    if options.voice_pack_name.is_empty() || options.voice_pack_id.is_empty() {
        return Err(make_error("Exporting with an empty voice pack name or voice pack id makes no sense. These values are important for loading the DBVO!"));
    }
    let export_folder = rfd::FileDialog::new()
        .set_title("Select Export Folder")
        .pick_folder();

    if export_folder.is_none() { return Ok(()); }
    let export_folder = export_folder.unwrap();

    let file_count = fs::read_dir(export_folder.clone())
        .map_err(|_| make_error(&format!("The folder '{}' could not be accessed.", export_folder.to_string_lossy())))?
        .count();

    let export_folder = if file_count > 0 {
        // if the selected folder is equal to the voice pack name, the user probably doesn't want to
        // make a subfolder in it
        if export_folder.file_name().unwrap().to_string_lossy() != options.voice_pack_name.to_string() {
            export_folder.join(options.voice_pack_name.clone())
        }else{
            export_folder
        }
    }else{
        export_folder
    };

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

        let fuz_dir = make_dbvo_dirs(&export_folder, &options, &topic.topic_name).await?;

        const CONCURRENCY_FACTOR: usize = 32;
        let processing_set = Rc::new(RefCell::new(HashSet::<VOHash>::with_capacity(CONCURRENCY_FACTOR)));
        let mut processing_stream = stream::iter(0..num_dialogues)
            .map(|i| {
                process_export_fuz(i, &topic_model, &fuz_dir, processing_set.clone())
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

pub fn init_export(
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
            separate_topics: false
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