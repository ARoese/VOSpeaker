use crate::chatterbox_generator::ChatterboxGeneratorConfig;
use crate::project_dir::project_dir::ProjectDir;
use crate::{AppWindow, FilePicking, TopicListItem, TopicsModel};
use slint::{ComponentHandle, ModelRc, SharedString};
use std::ffi::OsStr;
use std::path::PathBuf;

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

pub fn init_generator(ui: &AppWindow, project_dir: &ProjectDir) {
    ui.global::<FilePicking>().on_pick_wav_file(pick_wav_file);
    ui.global::<FilePicking>().on_format_path(format_path);
    let chatterbox_config_disk = project_dir.load_chatterbox_config()
        .unwrap_or(ChatterboxGeneratorConfig{
            cfg_weight: 0.5,
            endpoint: "localhost:9005".into(),
            exaggeration: 0.5,
            temperature: 0.5,
            voice_path: Default::default()
        });

    if let Some(config) = chatterbox_config_disk.try_into().ok() {
        ui.set_genConfig(config);
    }else{
        println!("Failed to parse chatterbox config from disk. Using defaults instead");
    }
}