use std::cell::RefCell;
use crate::project_dir::project_dir::ProjectDir;
use crate::{AppWindow, SubstitutionsActions, TopicsModel};
use slint::{ComponentHandle, Model, SharedString, ToSharedString};
use std::collections::HashMap;
use std::rc::Rc;

fn parse_substitutions(subs: SharedString) -> HashMap<String, String> {
    subs.lines()
        .filter_map(|l| {
            let parts = l.split("->").collect::<Vec<_>>();
            return if parts.len() != 2 {
                None
            } else {
                Some((parts[0].trim().to_string(), parts[1].trim().to_string()))
            }
        }).collect::<HashMap<String, String>>()
}

pub fn init_substitutions(ui: &AppWindow, topics_model: &Rc<TopicsModel>, project_dir: &ProjectDir, global_substitutions: Rc<RefCell<HashMap<String, String>>>) {
    let disk_substitutions = project_dir.load_substitutions().unwrap_or_default();
    let substitutions_text = disk_substitutions.iter()
        .map(|(target, replacement)| format!("{} -> {}", target, replacement))
        .collect::<Vec<String>>()
        .join("\n");

    *global_substitutions.borrow_mut() = disk_substitutions;

    let notify_listeners = {
        let topics_model = topics_model.clone();
        move || {
            topics_model.iter().for_each(|topic| {
                topic.substitutions_changed()
            })
        }
    };
    
    notify_listeners();

    ui.global::<SubstitutionsActions>().on_substitutions_changed({
        let global_substitutions = global_substitutions.clone();
        move |new_subs| {
            *global_substitutions.borrow_mut() = parse_substitutions(new_subs);
            // assign new substitutions to models.
            notify_listeners()
        }
    });

    ui.set_substitutions_text(substitutions_text.to_shared_string());
}