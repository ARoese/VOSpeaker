use crate::{AppWindow, DialogueFilterOptions, FilterActions, TopicDialogLine, TopicListItem};
use slint::{ComponentHandle, ModelRc, SharedString};

#[derive(Default)]
struct DialogueFilter {
    contains: SharedString,
    search_in_spoken: bool,
    search_in_expanded: bool
}

fn filter_line(options: DialogueFilterOptions, line: TopicDialogLine) -> bool {
    let search_term = options.search_term.to_lowercase();
    // pass empty searches
    if search_term.is_empty(){
        return true;
    }
    if !options.search_in_expanded && !options.search_in_spoken {
        return true;
    }
    if options.search_in_expanded && line.substituted_line.to_lowercase().contains(&search_term) {
        return true;
    }
    if options.search_in_spoken && line.clean_line.to_lowercase().contains(&search_term) {
        return true
    }
    return false;
}

pub fn init_filters(ui: &AppWindow, topics_modelrc: &ModelRc<TopicListItem>) {
    ui.global::<FilterActions>().on_filter_accepts(filter_line);
}