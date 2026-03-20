use std::cell::RefCell;
use std::collections::HashSet;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use rfd::MessageButtons;
use serde::{Deserialize, Serialize};
use slint::{quit_event_loop, ComponentHandle, ModelRc, StandardListViewItem, ToSharedString, VecModel};
use crate::project_dir::project_dir::ProjectDir;
use crate::ProjectSelector;

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
        project_dir.topics_path().exists()
    }else{
        false
    }
}

pub fn run_project_picker_gui() -> Result<Option<PathBuf>, Box<dyn Error>> {
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