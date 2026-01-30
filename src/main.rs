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

use crate::dialog_generator::{ConfigHashable, DialogGenerator};
use crate::static_resources::init_resources_dir;
use clap::Parser;
use futures::StreamExt;
use init::{get_expansion_config, get_substitutions, init_dialogue_audio, init_expansions, init_export, init_filters, init_generation, init_generator, init_receivers, init_substitutions, init_topics};
use project_dir::project_dir::ProjectDir;
use rfd::MessageButtons;
use serde::{Deserialize, Serialize};
use slint::{quit_event_loop, Model, ModelRc, StandardListViewItem, ToSharedString, VecModel};
use std::cell::RefCell;
use std::collections::HashSet;
use std::error::Error;
use std::fmt::Display;
use std::fs;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use tokio_util::future::FutureExt;

slint::include_modules!();

fn run_main_app(project_dir: PathBuf) -> Result<(), Box<dyn Error>> {
    let ui = AppWindow::new()?;

    let project_dir = Rc::from(ProjectDir::new(&project_dir)?);

    let (error_sender, progress_sender, cancellation_token) = init_receivers(&ui);


    let topics_model = init_topics(&ui, &project_dir, &error_sender);
    let topics_modelrc = ModelRc::new(topics_model.clone());

    init_generator(&ui, &topics_modelrc, &project_dir);

    init_expansions(&ui, &topics_modelrc, &project_dir);
    init_substitutions(&ui, &project_dir);

    init_generation(&ui, &error_sender, &progress_sender, &cancellation_token);
    init_dialogue_audio(&ui);
    init_filters(&ui, &topics_modelrc);

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

const PROJECT_DIRS_CONFIG_NAME: &str = "project_dirs.toml";
const APP_NAME: &str = "VOSpeaker";
#[derive(Serialize, Deserialize)]
struct PreviousProjectLocations {
    previous_project_locations: HashSet<PathBuf>,
}
fn read_project_locations() -> Option<HashSet<PathBuf>> {
    let config_dir = dirs::config_dir()?;
    let project_locations_path = config_dir.join(APP_NAME).join(PROJECT_DIRS_CONFIG_NAME);
    let project_locations_toml = fs::read_to_string(&project_locations_path).ok()?;
    let project_locations = toml::from_str::<PreviousProjectLocations>(&project_locations_toml).ok()?;
    Some(project_locations.previous_project_locations)
}

fn write_project_locations(project_locations: &HashSet<PathBuf>) -> Option<()> {
    let config_dir = dirs::config_dir()?.join(APP_NAME);
    let project_locations_path = config_dir.join(PROJECT_DIRS_CONFIG_NAME);
    //let as_strings = project_locations.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>();
    let project_locations_string = toml::to_string(&PreviousProjectLocations{previous_project_locations: project_locations.clone()}).unwrap();
    fs::create_dir_all(&config_dir).ok()?;
    fs::write(&project_locations_path, project_locations_string).ok()?;
    Some(())
}

fn looks_like_project_dir(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    if let Ok(project_dir) = ProjectDir::new(path) {
        return project_dir.topics_path().exists();
    }else{
        return false;
    }
}

fn run_project_picker_gui() -> Result<Option<PathBuf>, Box<dyn Error>> {
    let mut past_project_dirs = read_project_locations()
        .unwrap_or_default().into_iter()
        .filter(|p| p.exists())
        .filter(|p| looks_like_project_dir(&p))
        .collect::<HashSet<PathBuf>>();
    let mut past_project_dirs_vec = Vec::from_iter(past_project_dirs.clone().into_iter());
    past_project_dirs_vec.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
    let previous_paths = ModelRc::new(
        VecModel::from(
            past_project_dirs_vec.iter().map(|p| StandardListViewItem::from(p.display().to_shared_string())).collect::<Vec<_>>()
        )
    );

    let path: Rc<RefCell<Option<PathBuf>>> = Rc::from(RefCell::new(None));
    let selector = ProjectSelector::new()?;

    selector.set_previous_paths(previous_paths);

    selector.on_open_project({
        let path = path.clone();
        move || {
            let picker = rfd::FileDialog::new().set_title("Select a VOSpeaker project");
            if let Some(res) = picker.pick_folder() {
                if !looks_like_project_dir(&res) {
                    rfd::MessageDialog::new().set_title("This directory does not look like a VOProject")
                        .set_buttons(MessageButtons::Ok).show();
                    return;
                }
                path.borrow_mut().replace(res);
                quit_event_loop().expect("Failed to quit event loop");
            }
        }
    });

    selector.on_open_project_index({
        let path = path.clone();
        move |idx| {
            if let Some(res) = past_project_dirs_vec.get(idx as usize) {
                if !looks_like_project_dir(res) {
                    rfd::MessageDialog::new().set_title("This directory does not look like a VOProject")
                        .set_buttons(MessageButtons::Ok).show();
                    return;
                }
                path.borrow_mut().replace((*res).clone());
                quit_event_loop().expect("Failed to quit event loop");
            }
        }
    });

    selector.on_new_project({
        let path = path.clone();
        move || {
            let picker = rfd::FileDialog::new().set_title("Select a location for the new VOSpeaker project");
            if let Some(res) = picker.pick_folder() {
                if !res.is_dir() {
                    rfd::MessageDialog::new().set_title("Pick a directory").set_buttons(MessageButtons::Ok).show();
                    return;
                } else {
                    if let Ok(read_result) = fs::read_dir(&res){
                        if read_result.count() > 0 {
                            rfd::MessageDialog::new().set_title("The directory must be empty").set_buttons(MessageButtons::Ok).show();
                            return;
                        }else{
                            path.borrow_mut().replace(res);
                            quit_event_loop().expect("Failed to quit event loop");
                        }
                    }else{
                        eprintln!("Failed to read the picked directory");
                        return;
                    }
                }
            }
        }
    });
    selector.run()?;

    if let Some(path) = path.borrow().as_ref() {
        past_project_dirs.insert(path.clone());
        write_project_locations(&past_project_dirs);
    }

    Ok(path.take())
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

    let resources_guard = init_resources_dir();
    run_main_app(project_dir)?;
    
    while !cli_had_project_dir && let Some(project_dir) = run_project_picker_gui()? {
        run_main_app(project_dir)?;
    }
    Ok(())
}
