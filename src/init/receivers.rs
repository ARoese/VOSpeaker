use crate::init::ProgressState::{Done, Inflight};
use crate::init::ProgressVal::{Determinate, Indeterminate};
use crate::{AppWindow, ErrorToastActions, Progress, ProgressActions, UIError};
use slint::{spawn_local, CloseRequestResponse, ComponentHandle, Model, ModelExt, ModelRc, VecModel};
use std::cell::RefCell;
use std::ops::{Deref, Range};
use std::rc::Rc;
use tokio::sync::watch::Sender;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct ProgressHandle {
    pub progress_sender: watch::Sender<ProgressState>,
    pub error_sender: mpsc::Sender<UIError>,
    pub cancellation: CancellationToken
}

#[derive(Clone)]
pub struct ProgressHandleSpawner {
    pub progress_sender: watch::Sender<ProgressState>,
    pub error_sender: mpsc::Sender<UIError>,
    pub cancellation: Rc<RefCell<CancellationToken>>
}

impl ProgressHandleSpawner {
    pub fn spawn(&self) -> ProgressHandle {
        ProgressHandle {
            progress_sender: self.progress_sender.clone(),
            error_sender: self.error_sender.clone(),
            cancellation: self.cancellation.borrow().clone()
        }
    }
}

#[derive(Clone, Debug)]
pub enum ProgressVal {
    Indeterminate{
        status: String,
    },
    Determinate {
        status: String,
        range: Range<u64>,
        progress: u64
    }
}
#[derive(Debug, Clone)]
pub enum ProgressState {
    Inflight(ProgressVal),
    Done
}

impl Into<Progress> for &ProgressState {
    fn into(self) -> Progress {
        match self {
            Done => {
                Progress{
                    active: false,
                    indeterminate: false,
                    progress_percent: 0.0,
                    text: Default::default(),
                }
            },
            Inflight (val) => {
                match val {
                    Indeterminate { status } => {
                        Progress {
                            active: true,
                            indeterminate: true,
                            progress_percent: 0.5,
                            text: status.into(),
                        }
                    },
                    Determinate { status, range, progress } => {
                        Progress {
                            active: true,
                            indeterminate: false,
                            progress_percent: (progress - range.start) as f32 / (range.end - range.start) as f32,
                            text: format!("({}/{}) {}", progress, range.end, status).into(),
                        }
                    }
                }
            }
        }
    }
}


pub type ProgressSender = Sender<ProgressState>;
pub type ErrorSender = mpsc::Sender<UIError>;
pub fn init_receivers(ui: &AppWindow) -> (ErrorSender, ProgressSender, Rc<RefCell<CancellationToken>>) {
    let (progress_sender, mut progress_receiver) = watch::channel(Done);
    let cancellation_token = Rc::new(RefCell::new(CancellationToken::new()));

    ui.window().on_close_requested({
        let ct = cancellation_token.clone();
        move || {
            ct.borrow().cancel();
            CloseRequestResponse::HideWindow
        }
    });

    ui.global::<ProgressActions>().on_cancel({
        let ui_weak = ui.as_weak();
        let ct = cancellation_token.clone();
        move || {
            ct.borrow().cancel();
            ui_weak.upgrade().unwrap().set_progress(Progress {
                active: false,
                indeterminate: false,
                progress_percent: 0.0,
                text: Default::default(),
            });
            ct.replace(CancellationToken::new());
        }
    });

    spawn_local({
        let ui_weak = ui.as_weak();
        async move {
            while let Ok(_) = progress_receiver.changed().await {
                let ui = ui_weak.upgrade().unwrap();
                ui.set_progress(progress_receiver.borrow().deref().into())
            }
        }
    }).expect("failed to start progress watcher");

    let errors_model = Rc::new(VecModel::<UIError>::from(vec![
        /*
        UIError{
            message: "test error 1".to_shared_string()
        },
        UIError{
            message: "test error 2".to_shared_string()
        },
        UIError{
            message: "test error 3".to_shared_string()
        }
         */
    ]));
    ui.set_errors(ModelRc::new(errors_model.clone().reverse()));
    ui.global::<ErrorToastActions>().on_dismiss_error({
        let model = errors_model.clone();
        move |i| {
            model.remove(model.row_count()-1 - i as usize);
        }
    });

    let (error_sender, mut error_receiver) = mpsc::channel::<UIError>(128);
    spawn_local({
        async move {
            while let Some(error) = error_receiver.recv().await {
                // TODO: proper logging library
                eprintln!("ERROR: {:?}", error);
                errors_model.push(error);
            }
        }
    }).expect("failed to start progress watcher");

    (error_sender, progress_sender, cancellation_token)
}