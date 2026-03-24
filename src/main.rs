// Prevent console window in addition to Slint window in Windows release builds when, e.g., starting the app via file manager. Ignored on other platforms.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod dialog_generator;
mod chatterbox_generator;
mod models;
mod create_fuz;
mod static_resources;
mod dbvo_manifest;
mod init;
mod project_dir;
mod audio_conversion;
mod project_picker;
mod extract_fomod;

use crate::init::{init_batch_tools, init_filters, ProgressHandleSpawner};
use crate::models::TopicModel;
use crate::project_dir::topic_lines::TopicExpansionConfig;
use crate::static_resources::init_resources_dir;
use clap::Parser;
use init::{init_dialogue_audio, init_export, init_generation, init_generator, init_receivers, init_topics};
use project_dir::project_dir::ProjectDir;
use slint::VecModel;
use std::cell::RefCell;
use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;
use std::rc::Rc;
use crate::project_picker::run_project_picker_gui;

slint::include_modules!();

type TopicsModel = VecModel<Rc<TopicModel>>;
fn run_main_app(project_dir: PathBuf) -> Result<(), Box<dyn Error>> {
    // TODO: on all async functions, weak rcs should be moved into the closure rather than
    // strong rcs. The strong RCs will create refloop memory leaks across calls to this function.
    // Although they are memory leaks, they are very minor.
    let ui = AppWindow::new()?;

    let project_dir = Rc::from(ProjectDir::new(&project_dir)?);

    let (error_sender, progress_sender, cancellation_token) = init_receivers(&ui);

    let phs = ProgressHandleSpawner {
        progress_sender,
        error_sender: error_sender.clone(),
        cancellation: cancellation_token
    };

    let expand_config = Rc::new(RefCell::new(TopicExpansionConfig::default()));
    let substitutions = Rc::new(RefCell::new(HashMap::default()));
    let topics_model: Rc<TopicsModel> = init_topics(&ui, &project_dir, &expand_config, &substitutions, &error_sender);

    init_generator(&ui, &project_dir);

    init_generation(&ui, &topics_model, &phs);
    init_dialogue_audio(&ui, &topics_model);
    init_filters(&ui);

    let _packed_dialogs = init_export(&ui, &topics_model, &project_dir, &phs)?;
    init_batch_tools(&ui, &topics_model, &phs)?;
    ui.run()?;

    // save configs
    project_dir.save_expansion_config(expand_config.borrow().clone())?;

    if let Some(chatterbox_config) = ui.get_genConfig().try_into().ok() {
        project_dir.save_chatterbox_config(chatterbox_config)?;
    }else{
        eprintln!("Failed to parse chatterbox config, so cannot save it");
    }

    project_dir.save_substitutions(substitutions.borrow().clone())?;

    Ok(())
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// project dir to open or create
    project_dir: Option<PathBuf>
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let cli_had_project_dir = cli.project_dir.is_some();
    let project_dir = if let Some(project_dir) = cli.project_dir {
        project_dir
    }else{
        if let Some(project_dir) = run_project_picker_gui()? {
            project_dir
        }else{
            return Ok(());
        }
    };

    let _resources_guard = init_resources_dir();
    run_main_app(project_dir)?;
    
    while !cli_had_project_dir && let Some(project_dir) = run_project_picker_gui()? {
        run_main_app(project_dir)?;
    }
    Ok(())
}
