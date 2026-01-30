use crate::init::errors::make_error;
use crate::init::expansions::handle_expansion_change;
use crate::init::substitutions::handle_substitution_change;
use crate::models::TopicModel;
use crate::project_dir::project_dir::ProjectDir;
use crate::project_dir::topic_dir::TopicDir;
use crate::project_dir::topic_lines::TopicExpansionConfig;
use crate::{AppWindow, TopicListItem, UIError};
use slint::{spawn_local, ComponentHandle, Model, ModelRc, VecModel};
use std::collections::HashMap;
use std::ops::DerefMut;
use std::rc::Rc;
use tokio::sync::mpsc;

pub fn init_topics(ui: &AppWindow, project_dir: &Rc<ProjectDir>, error_sender: &mpsc::Sender<UIError>) -> Rc<VecModel<TopicListItem>> {
    let expand_config = Rc::new(TopicExpansionConfig::default());
    let substitutions = Rc::new(HashMap::<String, String>::default());

    let topic_dirs = project_dir.get_topic_dirs().expect("failed to load project topic dirs")
        .into_iter().map(|topic_dir|
        TopicListItem{
            topic_name: topic_dir.name().into(),
            dialog_lines: ModelRc::new(TopicModel::new(topic_dir, (*substitutions).clone(), (*expand_config).clone())),
        }
    ).collect::<Vec<_>>();
    let topics_model = Rc::from(VecModel::from(topic_dirs));


    ui.on_add_topic_from_path({
        let error_sender = error_sender.clone();
        let ui_weak = ui.as_weak();
        let project_dir = project_dir.clone();
        let topics_model = topics_model.clone();
        move || {
            // TODO: Give this its own function
            let mut dialog = rfd::FileDialog::new();
            dialog = dialog.set_title("Select topic file(s)")
                .add_filter("Topic File (.topic)", &["topic"])
                .add_filter("Text File (.txt)", &["txt"])
                .add_filter("All Files (*)", &["*"]);

            for path in dialog.pick_files().unwrap_or_default() {
                let topics_dir = project_dir.topics_path();
                let topic_prefix = path.file_prefix().expect("It shouldn't be possible to pick an empty path").to_string_lossy().to_string();
                let new_topic_dir = topics_dir.join(
                    topic_prefix.clone() + ".topic.d"
                );

                match TopicDir::create_new(&new_topic_dir, &path) {
                    Ok(new_dir) => {
                        // TODO: extract substitution and expansion state to a pair of Rc<RefCell> that get shared by all these models
                        // TODO: as it is now, each topic has its own model. They should get notified that there is a new version available
                        // TODO: That way, this thing can just pass in clones of the Rc instead of lying that it changed so it gets re-fetched
                        let expansion_config = handle_expansion_change(ui_weak.clone());
                        let substitutions = handle_substitution_change(ui_weak.clone());
                        topics_model.push(TopicListItem{
                            topic_name: new_dir.name().into(),
                            dialog_lines: ModelRc::new(TopicModel::new(new_dir, substitutions, expansion_config)),
                        });
                    }
                    Err(e) => {
                        spawn_local({
                            let error_sender = error_sender.clone();
                            async move {
                                error_sender
                                    .send(make_error(&format!("Cannot add topic file '{}': {:?}", topic_prefix, e)))
                                    .await
                                    .expect("Failed to send Error");
                            }
                        }).expect("failed to spawn async local");
                    }
                }
            }
        }
    });

    ui.on_remove_topic({
        let error_sender = error_sender.clone();
        let topics_model = topics_model.clone();
        move |idx| {
            let removed_model = topics_model.remove(idx as usize);
            let model = removed_model.dialog_lines.as_any().downcast_ref::<TopicModel>().unwrap();
            if let Some(topic_dir) = Option::take(model.topic_dir.borrow_mut().deref_mut()) {
                if let Err(e) = topic_dir.delete() {
                    spawn_local({
                        let error_sender = error_sender.clone();
                        let idx = idx;
                        async move {
                            error_sender.send(make_error(&format!("Failed to remove topic {idx}: {:?}", e)))
                                .await
                                .expect("Failed to send Error");
                        }
                    }).expect("failed to spawn async local");
                }
            }
        }
    });

    topics_model
}