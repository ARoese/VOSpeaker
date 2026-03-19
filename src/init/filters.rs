use crate::{AppWindow, DialogueFilterOptions, FilterActions, TopicDialogLine};
use slint::ModelExt;
use slint::{ComponentHandle, ModelRc};

fn filter_line(options: &DialogueFilterOptions, line: &TopicDialogLine) -> bool {
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
    
    false
}

pub fn init_filters(ui: &AppWindow) {
    //let filtered_topics
    ui.global::<FilterActions>().on_filtered({
        move |filter_options, model| {
            let filtered = model.filter(
                {
                    move |line| {
                        filter_line(&filter_options, line)
                    }
                }
            );
            
            ModelRc::new(filtered)
        }
    });
}