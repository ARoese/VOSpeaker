use crate::models::TopicModel;
use crate::project_dir::project_dir::ProjectDir;
use crate::{AppWindow, SubstitutionsActions};
use lazy_regex::regex;
use slint::{ComponentHandle, Model, ToSharedString, Weak};
use std::collections::HashMap;

pub fn get_substitutions(ui: &AppWindow) -> HashMap<String, String> {
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

pub fn handle_substitution_change(weak_ui: Weak<AppWindow>) -> HashMap<String, String> {
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

pub fn init_substitutions(ui: &AppWindow, project_dir: &ProjectDir) {
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