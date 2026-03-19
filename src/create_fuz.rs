use crate::audio_conversion::WavPath;
use crate::static_resources;
use lazy_regex::regex;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt::{Debug, Display, Formatter};
use std::ops::Deref;
#[cfg(target_family = "unix")]
use std::os::unix::prelude::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use tokio::join;
use tokio::process::Command;
use tokio::sync::Semaphore;
// NOTE: All paths will be unix paths until the instant of a subprocess's execution. A user of this
// module should NOT have to worry about platform-specific stuff

// only this winepath processes allowed at once, because it is very CPU-intensive.
// This lets us create tons of fuz files without the initial cpu spike causing chaos.
const WINEPATH_SEMAPHORE: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(16)));
async fn windows_paths<const LEN: usize>(paths: [&Path; LEN]) -> Result<[PathBuf; LEN], Box<dyn Error>> {
    #[cfg(target_family = "unix")]
    {
        let semaphore_ref = WINEPATH_SEMAPHORE.deref().clone();
        let _winepath_permit = semaphore_ref.acquire().await?;
        let mut command = Command::new("winepath");
        let mut command = command.arg("-w").arg("-0");

        for path in paths {
            command = command.arg(path);
        }
        let output = command.output().await?;

        if !output.status.success() {
            return Err(format!("winepath failed. \n\tStdErr: {}", String::from_utf8_lossy(&output.stderr)).into());
        }

        let paths: [PathBuf;LEN] = output.stdout
            .split(|c| c.eq(&0))
            .filter(|path| !path.is_empty())
            .map(|p| PathBuf::from(OsString::from_vec(p.to_vec())))
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| {
                eprintln!("{:?}", str::from_utf8(&output.stdout));
                "Invalid winepath output".to_string()
            })?;

        Ok(paths)
    }

    #[cfg(target_family = "windows")]
    {
        let arr: [PathBuf; LEN] = paths.iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>()
            .try_into()
            .expect("same-size mapping");
        Ok(arr)
    }
}

/// uses winepath to get the correct windows path for a given unix path
async fn windows_path(path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let [res] = windows_paths([path]).await?;
    Ok(res)
}

// Here, `path` must be a valid windows path. Use `windows_path()` for this
fn agnostic_command(path: &OsStr) -> Command {
    // REQUIRES that there is a valid and full-featured wine prefix set in the environment.

    #[cfg(target_family = "unix")]
    {
        let mut c = Command::new("wine");
        c.arg(path);
        c
    }

    #[cfg(target_family = "windows")]
    {
        let mut c = Command::new(path);
        c
    }
}

/// takes a wav file and converts it into an xwm file
pub async fn create_xwm(wav_path: &WavPath, xwm_destination_path: &Path) -> Result<(), Box<dyn Error>> {
    // convert paths to windows paths
    let bin_path_file_name = static_resources::as_real_file(static_resources::WMA_ENCODE_BIN).await?;
    let [
        encode_bin_path,
        wav_path,
        xwm_destination_path
    ] = windows_paths([
        &bin_path_file_name,
        wav_path,
        xwm_destination_path
    ]).await?;

    // run xWMAEncode command
    let mut command = agnostic_command(encode_bin_path.as_os_str());
    let command = command.arg(wav_path.as_os_str())
        .arg(xwm_destination_path.as_os_str());
    let debug_command = format!("{command:?}");
    let command = command.output().await?;

    if !command.status.success() {
        Err(format!("xWMAEncode.exe failed.\n\tCommand: {debug_command}\n\tStdErr: {}", String::from_utf8_lossy(&command.stderr)).into())
    }else{
        // output file is present at xwm_path
        Ok(())
    }
}

/// turn a str into something that's safe to pass on the command line.
/// removes non-whitespace and non-word characters.
/// mostly for stripping dialogue_text
fn cmdline_string(string: &str) -> String {
    // TODO: slow regex
    let invalid = regex!(r"\W|\S");
    invalid.replace_all(string, "").to_string()
}

/// creates a lip file
pub async fn create_lip(
    wav_path: &WavPath,
    resampled_wav_path: &WavPath,
    lip_destination_path: &Path,
    dialogue_text: &OsStr
) -> Result<(), Box<dyn Error>> {
    // make this safe to pass on the commandline
    let dialogue_text = OsString::from(cmdline_string(dialogue_text.to_str().unwrap()));
    // convert paths to windows paths
    let bin_path_file_name = static_resources::as_real_file(static_resources::FACE_FX_BIN).await?;
    let data_path_file_name = static_resources::as_real_file(static_resources::FONIX_DATA).await?;
    let [
        fx_bin_path,
        fonix_data_path,
        wav_path,
        resampled_wav_path,
        lip_destination_path
    ] = windows_paths([
        &bin_path_file_name,
        &data_path_file_name,
        wav_path,
        resampled_wav_path,
        lip_destination_path
    ]).await?;

    // run FaceFXWrapper command
    let mut command = agnostic_command(fx_bin_path.as_os_str());
    let command = command
        .arg("Skyrim")
        .arg("USEnglish")
        .arg(fonix_data_path.as_os_str())
        .arg(wav_path.as_os_str())
        .arg(resampled_wav_path.as_os_str())
        .arg(lip_destination_path.as_os_str())
        .arg(dialogue_text);

    let debug_command = format!("{command:?}");
    let command = command.output().await?;

    if !command.status.success() {
        Err(format!("FaceFxWrapper.exe failed. \n\tCommand: {debug_command}\n\tStdErr: {}", String::from_utf8_lossy(&command.stderr)).into())
    }else{
        // output file is present at xwm_path
        Ok(())
    }
}

pub async fn create_fuz(
    xwm_path: &Path,
    lip_path: &Path,
    fuz_output_path: &Path,
) -> Result<(), Box<dyn Error>> {
    // convert paths to windows paths
    let bin_path_file_name = static_resources::as_real_file(static_resources::BML_ENCODE_BIN).await?;
    let [
        bml_bin_path,
        xwm_path,
        lip_path,
        fuz_output_path
    ] = windows_paths([
        &bin_path_file_name,
        xwm_path,
        lip_path,
        fuz_output_path
    ]).await?;

    let mut command = agnostic_command(bml_bin_path.as_os_str());
    let command = command
        .arg(fuz_output_path.as_os_str())
        .arg(xwm_path.as_os_str())
        .arg(lip_path.as_os_str());

    let debug_command = format!("{command:?}");
    let command = command.output().await?;

    if !command.status.success() {
        Err(format!("BmlFuzEncode.exe failed. \n\tCommand: {debug_command}\n\tStdErr: {}", String::from_utf8_lossy(&command.stderr)).into())
    }else{
        // output file is present at fuz_output_path
        Ok(())
    }
}

#[derive(Debug)]
struct WavToFuzError {
    reason: String,
}

impl Display for WavToFuzError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}

impl Error for WavToFuzError {}

// TODO: It might be necessary to make a tmpdir as a working dir for these files. The command lines get quite long, and the dialogue text appears
// TODO: on them. Shortening the paths by putting them in a tmp dir will make more space. This hasn't been shown to be an issue yet.
pub async fn wav_to_fuz(wav_path: &WavPath, dialogue_text: &OsStr, fuz_destination_path: &Path) -> Result<(), Box<dyn Error>> {
    let xwm_path = wav_path.with_extension("xwm");
    let lip_path = wav_path.with_extension("lip");
    let resampled_wav_path = wav_path
        .with_extension("resampled")
        .with_added_extension("wav");
    let resampled_wav_path = WavPath::from(resampled_wav_path);

    let (
        xwm_encode_result,
        lip_encode_result,
    ) = join!(
        create_xwm(&wav_path, &xwm_path),
        create_lip(&wav_path, &resampled_wav_path, &lip_path, dialogue_text)
    );

    xwm_encode_result?;
    lip_encode_result?;

    create_fuz(&xwm_path, &lip_path, fuz_destination_path).await?;

    // clean up intermediates
    // don't need the resampled file
    tokio::fs::remove_file(&resampled_wav_path.deref()).await?;
    tokio::fs::remove_file(&lip_path).await?;
    tokio::fs::remove_file(&xwm_path).await?;

    if !fuz_destination_path.exists() {
        return Err(Box::new(WavToFuzError{reason: format!("Output fuz '{}' does not exist, but command also did not fail.", fuz_destination_path.to_string_lossy())}));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::static_resources::init_resources_dir;

    #[tokio::test]
    async fn test_make_fuz() {
        let guard = init_resources_dir();

        let test_dir = tempfile::tempdir().unwrap();
        let folder_with_spaces = test_dir.path().join("Folder With Spaces");
        tokio::fs::create_dir(&folder_with_spaces).await.unwrap();
        let test_wav_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_assets/sample_voiceline.wav");
        let test_wav_path = WavPath::from(test_wav_path);
        let fuz_destination = folder_with_spaces.join("sample_voiceline.fuz");

        //let command = Command::new()

        wav_to_fuz(&test_wav_path, &OsString::from("You will no longer talk down to people like that if you're dead!"), &fuz_destination).await.unwrap();
        assert!(fuz_destination.exists());
    }
}