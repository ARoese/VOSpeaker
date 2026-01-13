use crate::progress::ProgressState::{Done, Inflight};
use crate::progress::ProgressVal::{Determinate, Indeterminate};
use crate::{Progress, UIError};
use std::ops::Range;
use tokio::sync::mpsc;
use tokio::sync::watch::Sender as WatchSender;
use tokio_util::sync::CancellationToken;

pub struct ProgressHandle {
    pub progress_sender: WatchSender<ProgressState>,
    pub error_sender: mpsc::Sender<UIError>,
    pub cancellation: CancellationToken
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

