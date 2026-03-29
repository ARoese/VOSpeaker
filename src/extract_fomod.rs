use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::ops::DerefMut;
use std::path::{Component, Path};
use tokio::fs::read_dir;
use zip::ZipArchive;
use crate::project_dir::topic_file::read_topic_lines_from_file;

fn is_topic_file(file_name: &str) -> bool {
    let file_name = file_name.to_lowercase();
    if file_name.contains(".txt") || file_name.contains(".topic") {
        return true;
    }

    false
}

fn topic_needs_attn(file_name: &str) -> bool {
    let file_name = file_name.to_uppercase();
    for marker in ["DBVO", "- READ -", "- READ", "READ -", "NO TOPIC FILE"] {
        if file_name.contains(marker) {
            return true
        }
    }
    false
}

struct Mod {
    pub needs_attn: bool,
    pub topic_lines: Vec<String>
}

pub async fn extract_fomod_topics(fomod: &Path, out_dir: &Path) -> Result<(), Box<dyn Error>> {
    let mods = if let Some(extension) = fomod.extension() && extension.to_ascii_lowercase() == "zip" {
        extract_fomod_topics_zip(fomod).await?
    } else {
        extract_fomod_topics_dir(fomod).await?
    };

    let needs_attn_subdir = out_dir.join("needs_attn");
    tokio::fs::create_dir_all(&needs_attn_subdir).await?;

    for (mod_name, m) in mods {
        if m.topic_lines.is_empty() { continue }
        let dest_topic_file = if m.needs_attn {
            needs_attn_subdir.join(&mod_name)
        }else{
            out_dir.join(&mod_name)
        }.with_added_extension("topic");

        let joined_strings = m.topic_lines.join("\n");
        tokio::fs::write(dest_topic_file, joined_strings).await?;
    }

    Ok(())
}

async fn extract_fomod_topics_zip(fomod_path: &Path) -> Result<HashMap<String, Mod>, Box<dyn Error>> {
    let mut fomod_zip = ZipArchive::new(File::open(fomod_path)?)?;

    let mut extracted_topics: HashMap<String, Mod> = HashMap::new();
    for idx in 0..fomod_zip.len() {
        let file = fomod_zip.by_index(idx)?;
        if !file.is_file(){ continue }

        let Some(enclosed_path) = file.enclosed_name() else {continue};
        let Some(file_name) = enclosed_path.file_name() else {continue};
        let mod_dir_name = match enclosed_path.components().into_iter().nth_back(1) {
            Some(Component::Normal(name)) => name.to_string_lossy(),
            _ => continue
        };

        let mod_name = match mod_dir_name.split_once(" - ") {
            Some((_, mod_name)) => mod_name.to_string(),
            _ => continue,
        };

        if is_topic_file(&file_name.to_string_lossy()) {
            let needs_attn = topic_needs_attn(&file_name.to_string_lossy());
            let topic_file_contents = read_topic_lines_from_file(file, &enclosed_path.to_string_lossy())?;
            let mut entry = extracted_topics.entry(mod_name)
                .or_insert(Mod{needs_attn, topic_lines: Vec::new()});
            let r = entry
                .deref_mut();

            r.topic_lines.extend(topic_file_contents);
            r.needs_attn |= r.needs_attn || needs_attn;
        }
    }

    Ok(extracted_topics)
}

async fn extract_fomod_topics_dir(fomod_dir: &Path) -> Result<HashMap<String, Mod>, Box<dyn Error>> {
    if !fomod_dir.is_dir() {
        return Err(format!("'{}' is not a directory", fomod_dir.display()).into());
    }

    let mut topics_map = HashMap::new();
    let mut group_dirs = read_dir(fomod_dir).await?;
    while let Some(group_dir) = group_dirs.next_entry().await? {
        if !group_dir.file_type().await?.is_dir() { continue }

        let mut group_dir = read_dir(group_dir.path()).await?;
        while let Some(mod_dir) = group_dir.next_entry().await? {
            if !mod_dir.file_type().await?.is_dir() { continue }

            // TODO: to_string_lossy might be bad here, because some of these files ACTUALLY DO have
            // bad utf-8 in their names
            let mod_name = match mod_dir.file_name().to_string_lossy().split_once(" - ") {
                Some((_, mod_name)) => mod_name.to_string(),
                _ => continue,
            };

            let mut topics = read_dir(mod_dir.path()).await?;
            let mut topic_lines = vec!();
            let mut needs_attn = false;
            while let Some(topic_file) = topics.next_entry().await? {
                let topic_file_name = topic_file.file_name().to_string_lossy().to_string();
                if !topic_file.file_type().await?.is_file() { continue }
                if !is_topic_file(&topic_file_name) { continue }
                if topic_needs_attn(&topic_file_name) {
                    needs_attn = true;
                }

                let opened_file = File::open(topic_file.path())?;
                let these_topic_lines = read_topic_lines_from_file(opened_file, &topic_file.path().to_string_lossy())?;
                topic_lines.extend(these_topic_lines.into_iter());
            }

            if topic_lines.is_empty() { continue }
            topics_map.insert(mod_name, Mod{needs_attn, topic_lines});
        }
    }

    Ok(topics_map)
}