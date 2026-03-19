use std::error::Error;
use std::ops::Deref;
use std::path::PathBuf;
use tokio::process::Command;

pub struct WavPath(PathBuf);
impl From<PathBuf> for WavPath {
    fn from(path: PathBuf) -> Self {Self(path)}
}
impl Deref for WavPath {
    type Target = PathBuf;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct Mp3Path(PathBuf);
impl From<PathBuf> for Mp3Path {
    fn from(path: PathBuf) -> Self {Self(path)}
}
impl Deref for Mp3Path {
    type Target = PathBuf;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

static MP3_BITRATE: &str = "96k"; 
pub async fn wav_to_mp3(src: &WavPath, dst: &Mp3Path) -> Result<(), Box<dyn Error>> {
    // TODO: make this platform-independent
    let dst_certain = dst.with_extension("mp3");
    let result = Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(src.deref())
        .arg("-b:a")
        .arg(MP3_BITRATE)
        .arg(&dst_certain)
        .output().await?;

    if !result.status.success() {
        tokio::fs::remove_file(dst_certain).await.ok(); // the other error is more important
        return Err(format!("ffmpeg failed.\n\tStdErr: {}", String::from_utf8_lossy(&result.stderr)).into());
    }
    
    if !dst_certain.eq(dst.deref()) {
        tokio::fs::rename(dst_certain, dst.deref()).await?;
    }
    Ok(())
}

pub async fn mp3_to_wav(src: &Mp3Path, dst: &WavPath) -> Result<(), Box<dyn Error>> {
    // TODO: make this platform-independent
    // NOTE: Bits per sample MUST be 16.
    // NOTE: Otherwise, the tools used in the project will fail.
    let dst_certain = dst.with_extension("wav");
    let result = Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(src.deref())
        .arg("-sample_fmt")
        .arg("s16")
        .arg(&dst_certain)
        .output().await?;

    if !result.status.success() {
        tokio::fs::remove_file(dst_certain).await.ok(); // the other error is more important
        return Err(format!("ffmpeg failed.\n\tStdErr: {}", String::from_utf8_lossy(&result.stderr)).into());
    }
    
    if !dst_certain.eq(dst.deref()) {
        tokio::fs::rename(dst_certain, dst.deref()).await?;
    }

    Ok(())
}