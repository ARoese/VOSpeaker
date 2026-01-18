use std::error::Error;
use std::ffi::{OsStr, OsString};
#[cfg(target_family = "unix")]
use std::os::unix::prelude::OsStringExt;
use std::path::{Path, PathBuf};
use tokio::join;
use tokio::process::Command;
use crate::static_resources;
use crate::static_resources::as_real_file;
// NOTE: All paths will be unix paths until the instant of a subprocess's execution. A user of this
// module should NOT have to worry about platform-specific stuff

/// uses winepath to get the correct windows path for a given unix path
async fn windows_path(path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    #[cfg(target_family = "unix")]
    {
        let output = Command::new("winepath")
            .arg("-w")
            .arg(path)
            .output().await?;

        if !output.status.success() {
            return Err(format!("winepath failed. StdOut: {}", String::from_utf8_lossy(&output.stderr)).into());
        }

        let mut stdout = output.stdout;
        // remove newline if present; it's not part of the real path
        if stdout[stdout.len() - 1] == b'\n' {
            stdout.pop();
        }

        /*
        for item in stdout.iter_mut() {
            if *item == b'\\' {
                *item = b'/';
            }
        }
         */

        Ok(PathBuf::from(OsString::from_vec(stdout)))
    }

    #[cfg(target_family = "windows")]
    {
        Ok(PathBuf::from(path))
    }
}

// Here, `path` must be a valid windows path. Use `windows_path()` for this
fn agnostic_command(path: &OsStr) -> Command {
    // REQUIRES that there is a valid and full-featured wine prefix set in the environment.

    #[cfg(target_family = "unix")]
    {
        let mut c = Command::new("wine");
        c.arg(path)
            .kill_on_drop(true);
        c
    }

    #[cfg(target_family = "windows")]
    {
        let mut c = Command::new(path);
        c.kill_on_drop(true);
        c
    }
}

/// takes a wav file and converts it into an xwm file
pub async fn create_xwm(wav_path: &Path, xwm_destination_path: &Path) -> Result<(), Box<dyn Error>> {
    // convert paths to windows paths
    let bin_path_file_name = as_real_file(static_resources::WMA_ENCODE_BIN).await?;
    let (
        encode_bin_path,
        wav_path,
        xwm_destination_path
    ) = join!(
        windows_path(&bin_path_file_name),
        windows_path(wav_path),
        windows_path(xwm_destination_path)
    );

    let encode_bin_path = encode_bin_path?;
    let wav_path = wav_path?;
    let xwm_destination_path = xwm_destination_path?;

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

/// creates a lip file
pub async fn create_lip(
    wav_path: &Path,
    resampled_wav_path: &Path,
    lip_destination_path: &Path,
    dialogue_text: &OsStr
) -> Result<(), Box<dyn Error>> {
    // convert paths to windows paths
    let bin_path_file_name = as_real_file(static_resources::FACE_FX_BIN).await?;
    let data_path_file_name = as_real_file(static_resources::FONIX_DATA).await?;
    let (
        fx_bin_path,
        fonix_data_path,
        wav_path,
        resampled_wav_path,
        lip_destination_path
    ) = join!(
        windows_path(&bin_path_file_name),
        windows_path(&data_path_file_name),
        windows_path(wav_path),
        windows_path(resampled_wav_path),
        windows_path(lip_destination_path)
    );

    let (
        fx_bin_path,
        fonix_data_path,
        wav_path,
        resampled_wav_path,
        lip_destination_path
    ) = (
        fx_bin_path?,
        fonix_data_path?,
        wav_path?,
        resampled_wav_path?,
        lip_destination_path?
    );

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
    let bin_path_file_name = as_real_file(static_resources::BML_ENCODE_BIN).await?;
    let (
        bml_bin_path,
        xwm_path,
        lip_path,
        fuz_output_path
    ) = join!(
        windows_path(&bin_path_file_name),
        windows_path(xwm_path),
        windows_path(lip_path),
        windows_path(fuz_output_path)
    );

    let (
        bml_bin_path,
        xwm_path,
        lip_path,
        fuz_output_path
    ) = (
        bml_bin_path?,
        xwm_path?,
        lip_path?,
        fuz_output_path?
    );

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

// TODO: It might be necessary to make a tmpdir as a working dir for these files. The command lines get quite long, and the dialogue text appears
// TODO: on them. Shortening the paths by putting them in a tmp dir will make more space. This hasn't been shown to be an issue yet.
pub async fn wav_to_fuz(wav_path: &Path, dialogue_text: &OsStr, fuz_destination_path: &Path) -> Result<(), Box<dyn Error>> {
    let xwm_path = wav_path.with_extension("xwm");
    let lip_path = wav_path.with_extension("lip");
    let resampled_wav_path = wav_path
        .with_extension("resampled")
        .with_added_extension("wav");

    let (
        xwm_encode_result,
        lip_encode_result,
    ) = join!(
        create_xwm(&wav_path, &xwm_path),
        create_lip(&wav_path, &resampled_wav_path, &lip_path, dialogue_text)
    );

    xwm_encode_result?;
    lip_encode_result?;

    // don't need the resampled file
    // TODO: join these futures for performance
    tokio::fs::remove_file(&resampled_wav_path).await?;

    create_fuz(&xwm_path, &lip_path, fuz_destination_path).await?;

    // clean up intermediates
    if true {
        tokio::fs::remove_file(&lip_path).await?;
        tokio::fs::remove_file(&xwm_path).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::static_resources::{deinit_resources_dir, init_resources_dir};
    use super::*;

    #[tokio::test]
    async fn test_make_fuz() {
        init_resources_dir();

        let test_dir = tempfile::tempdir().unwrap();
        let test_wav_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_assets/sample_voiceline.wav");
        let fuz_destination = test_dir.path().join("sample_voiceline.fuz");

        //let command = Command::new()

        wav_to_fuz(&test_wav_path, &OsString::from("You will no longer talk down to people like that if you're dead!"), &fuz_destination).await.unwrap();
        assert!(fuz_destination.exists());

        deinit_resources_dir();
    }
}