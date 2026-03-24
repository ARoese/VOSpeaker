use std::error::Error;
use std::path::Path;
use tokio::fs::read_dir;
use crate::project_dir::topic_file::read_topic_lines_from_file;

fn is_topic_file(file_name: &str) -> bool {
    if file_name.contains(".txt") && file_name.contains("topic") {
        return true;
    }

    if file_name.contains(".esp.txt") {
        return true
    }

    false
}

fn topic_needs_attn(file_name: &str) -> bool {
    for marker in ["DBVO", "- READ -", "- READ", "READ -"] {
        if file_name.contains(marker) {
            return true
        }
    }
    false
}

// TODO: do this, but don't require unzipping the zip file first
pub async fn extract_fomod_topics(fomod_dir: &Path, out_dir: &Path) -> Result<(), Box<dyn Error>> {
    if !fomod_dir.is_dir() {
        return Err(format!("'{}' is not a directory", fomod_dir.display()).into());
    }
    if !out_dir.is_dir() {
        return Err(format!("'{}' is not a directory", out_dir.display()).into());
    }
    let needs_attn_subdir = out_dir.join("needs_attn");
    tokio::fs::create_dir_all(&needs_attn_subdir).await?;


    let mut group_dirs = read_dir(fomod_dir).await?;
    while let Some(group_dir) = group_dirs.next_entry().await? {
        if !group_dir.file_type().await?.is_dir() {
            continue;
        }

        let mut group_dir = read_dir(group_dir.path()).await?;
        while let Some(mod_dir) = group_dir.next_entry().await? {
            if !mod_dir.file_type().await?.is_dir() {
                continue;
            }

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

                let these_topic_lines = read_topic_lines_from_file(&topic_file.path())?;
                topic_lines.extend(these_topic_lines.into_iter());
            }

            if topic_lines.is_empty() { continue }
            let dest_topic_file = if needs_attn {
                needs_attn_subdir.join(&mod_name)
            }else{
                out_dir.join(&mod_name)
            }.with_added_extension("topic");
            
            let joined_strings = topic_lines.join("\n");
            tokio::fs::write(dest_topic_file, joined_strings).await?;
        }
    }

    Ok(())
}