use std::cell::RefCell;
use crate::project_dir::project_dir::ProjectDir;
use crate::project_dir::topic_lines::TopicExpansionConfig;
use crate::{AppWindow, Expansion, Mappings, TopicsModel};
use slint::{ComponentHandle, Model, ModelExt, ModelNotify, ModelRc, ModelTracker, SharedString, ToSharedString};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

pub struct ExpansionsConfigModel {
    pub expansion_config: Rc<RefCell<TopicExpansionConfig>>,
    index_keys: RefCell<Vec<String>>,
    notify: ModelNotify
}

impl ExpansionsConfigModel {
    pub fn new(tec: Rc<RefCell<TopicExpansionConfig>>) -> ExpansionsConfigModel {
        let index_keys = RefCell::from(Self::collect_index_keys(&tec.borrow()));
        ExpansionsConfigModel {
            index_keys: index_keys,
            expansion_config: tec,
            notify: ModelNotify::default()
        }
    }
}

impl ExpansionsConfigModel {
    fn collect_index_keys(config: &TopicExpansionConfig) -> Vec<String> {
        let mut idx_keys = config.expansions.keys().cloned().collect::<Vec<String>>();
        idx_keys.sort();
        idx_keys
    }

    fn set_expansions(&self, global: &SharedString, new_expansions: &SharedString) {
        let mut ec_borrow = self.expansion_config.borrow_mut();
        let Some(expansions) = ec_borrow.expansions.get_mut(&global.to_string()) else {
            return
        };

        *expansions = parse_expansions(new_expansions);
    }

    fn expansion_config_reset(&self) {
        *self.index_keys.borrow_mut() = Self::collect_index_keys(&self.expansion_config.borrow());
        self.notify.reset();
    }
}

impl Model for ExpansionsConfigModel {
    type Data = Expansion;

    fn row_count(&self) -> usize {
        self.expansion_config.borrow().expansions.len()
    }

    fn row_data(&self, row: usize) -> Option<Self::Data> {
        let key = self.index_keys.borrow().get(row)?.clone();
        let expansions = self.expansion_config.borrow().expansions.get(&key).expect("index_keys is 1:1")
            .join("\n");

        Expansion {
            index: row as i32,
            name: key.into(),
            substitutions: expansions.to_shared_string(),
        }.into()
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }
}

fn parse_expansions(to_parse: &SharedString) -> Vec<String> {
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
        .map( |s| s.to_string() )
        .collect::<Vec<String>>();

    substitutions
}

pub fn init_expansions(ui: &AppWindow, topics_model: &Rc<TopicsModel>, project_dir: &ProjectDir, global_expansions_config: Rc<RefCell<TopicExpansionConfig>>) -> Rc<ExpansionsConfigModel> {
    let expand_config_disk = project_dir.load_expansion_config().unwrap_or(TopicExpansionConfig::default());
    let generated_expand_mappings = topics_model.iter()
        .flat_map(|topic| topic.collect_globals())
        .map(|s| (s, vec![]))
        .collect::<HashMap<_, Vec<String>>>();

    let generated_config = TopicExpansionConfig {
        expansions: generated_expand_mappings,
        max_expansions: 1,
    };

    let expand_config = generated_config.merge_with(&expand_config_disk);
    ui.set_allowed_expansions(expand_config.max_expansions as i32);
    *global_expansions_config.borrow_mut() = expand_config;
    let expansions_config_model = Rc::new(ExpansionsConfigModel::new(global_expansions_config));
    // TODO: this should be updated when topics are added/removed

    let expansions_model = ModelRc::from(expansions_config_model.clone());
    ui.set_expansions(expansions_model.clone());

    let notify_expansion_listeners = {
        let topics_model = topics_model.clone();
        move || {
            topics_model.iter().for_each(|topic| {
                topic.expansion_config_changed();
            })
        }
    };

    notify_expansion_listeners();

    ui.global::<Mappings>().on_expansionNames(|es| {
            let new_model = es
                .clone()
                .map(|e| e.name);

            ModelRc::new(new_model)
        }
    );

    ui.global::<Mappings>().on_expansion_changed({
        let expansions_config_model = expansions_config_model.clone();
        let nel = notify_expansion_listeners.clone();
        move |expansion, new_expansions| {
            expansions_config_model.set_expansions(&expansion, &new_expansions);
            nel();
        }
    });

    ui.global::<Mappings>().on_max_expansions_changed(notify_expansion_listeners);

    expansions_config_model
}