use crate::models::TopicModel;
use crate::project_dir::project_dir::ProjectDir;
use crate::project_dir::topic_lines::TopicExpansionConfig;
use crate::{AppWindow, Expansion, Mappings, TopicListItem};
use slint::{ComponentHandle, Model, ModelRc, SharedString, ToSharedString, VecModel, Weak};
use std::collections::{HashMap, HashSet};

pub fn get_expansion_config(ui: &AppWindow) -> TopicExpansionConfig {
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

pub fn handle_expansion_change(weak_ui: Weak<AppWindow>) -> TopicExpansionConfig {
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

pub fn init_expansions(ui: &AppWindow, topics_model: &ModelRc<TopicListItem>, project_dir: &ProjectDir) {
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