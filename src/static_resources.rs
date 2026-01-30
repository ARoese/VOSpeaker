use static_files::Resource;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::{LazyLock, RwLock};
use tempfile::TempDir;
use tokio::sync::Mutex;

include!(concat!(env!("OUT_DIR"), "/generated.rs"));

pub static RESOURCES: LazyLock<HashMap<&str, Resource>> = LazyLock::new(|| generate());
static RESOURCES_DIR: RwLock<Option<TempDir>> = RwLock::new(None);
static RESOURCES_DIR_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

pub static FONIX_DATA: &str = "FonixData.cdf";
pub static FACE_FX_BIN: &str = "FaceFXWrapper.exe";
pub static WMA_ENCODE_BIN: &str = "xWMAEncode.exe";
pub static BML_ENCODE_BIN: &str = "BmlFuzEncode.exe";

pub struct StaticResourcesGuard {
    should_delete: RefCell<bool>,
}
impl StaticResourcesGuard {
    /// leaks the static resources. I.E. the resources will not be deinitialized when this
    /// is dropped
    pub fn leak(self) {
        self.should_delete.replace(false);
    }
    fn new() -> Self {
        StaticResourcesGuard { should_delete: true.into() }
    }
}
impl Drop for StaticResourcesGuard {
    fn drop(&mut self) {
        if *self.should_delete.borrow() {
            deinit_resources_dir();
        }
    }
}

pub fn init_resources_dir() -> StaticResourcesGuard {
    let temp_dir = tempfile::tempdir().expect("Could not create temporary directory");
    RESOURCES_DIR.write().expect("Could not lock").replace(temp_dir);
    StaticResourcesGuard::new()
}

pub fn deinit_resources_dir() {
    // if it existed, the TempDir is dropped here and the directory is deleted
    RESOURCES_DIR.write().expect("Could not lock").take();
}

/// get a valid path to the resource file. The actual location is not guaranteed,
/// and the location might not be valid until this method is called
pub async fn as_real_file(resource: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let _lock = RESOURCES_DIR_LOCK.lock().await;
    // if the resources dir was not initialized yet, then do it here.
    // This will likely leave the directory dangling, but will prevent errors
    if RESOURCES_DIR.read().expect("Could not lock").is_none() {
        let backtrace = std::backtrace::Backtrace::force_capture();
        eprintln!("Implicitly initializing static resources. This should not happen. Trace: {backtrace:?}");
        init_resources_dir().leak();
    }

    let temp_dir_path = RESOURCES_DIR.read()
        .expect("Could not lock")
        .as_ref()
        .expect("Resources were not initialized (this is not possible)")
        .path()
        .to_path_buf();
    
    #[cfg(test)]
    let temp_dir_path = {
        let with_spaces = temp_dir_path.join("Path With Spaces");
        tokio::fs::create_dir_all(&with_spaces).await?;
        with_spaces
    };

    let resource_path = temp_dir_path.join(resource);
    // if the resource path doesn't exist, (it has not been written out yet) then write it out.
    if !resource_path.exists() {
        let resources_ref = RESOURCES.deref();
        tokio::fs::write(resource_path.as_path(), resources_ref[resource].data).await?;
    }

    Ok(resource_path)
}