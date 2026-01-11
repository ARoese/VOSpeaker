use std::path::PathBuf;
use async_trait::async_trait;
use crate::hashes::{ConfigHash};
use crate::dialog_generator::{ConfigHashable, DialogGenerationError, DialogGenerator};
use md5::Context;
use slint::ToSharedString;
use tokio::net::TcpStream;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio::io::BufStream;
use crate::ChatterboxConfig;
use crate::topic_lines::SpokenTopicLine;

pub struct ChatterboxGenerator;
#[derive(serde::Deserialize, serde::Serialize)]
pub struct ChatterboxGeneratorConfig{
    pub endpoint: String,
    pub temperature: f32,
    pub cfg_weight: f32,
    pub exaggeration: f32,
    pub voice_path: PathBuf,
}
impl ConfigHashable for ChatterboxGeneratorConfig{
    fn config_hash(&self) -> ConfigHash {
        let mut context = Context::new();

        context.consume(f32::to_be_bytes(self.cfg_weight));
        context.consume(f32::to_be_bytes(self.temperature));
        context.consume(f32::to_be_bytes(self.exaggeration));
        context.consume(self.voice_path.to_str().unwrap_or_default());

        ConfigHash(context.finalize().into())
    }
}

impl TryFrom<ChatterboxConfig> for ChatterboxGeneratorConfig{
    type Error = ();

    fn try_from(value: ChatterboxConfig) -> Result<Self, Self::Error> {
        Ok(ChatterboxGeneratorConfig {
            endpoint: value.endpoint.to_string(),
            temperature: value.temperature,
            cfg_weight: value.cfg_weight,
            exaggeration: value.exaggeration,
            voice_path: PathBuf::from(value.voicePath.to_string()),
        })
    }
}

impl TryFrom<ChatterboxGeneratorConfig> for ChatterboxConfig {
    type Error = ();
    
    fn try_from(value: ChatterboxGeneratorConfig) -> Result<Self, Self::Error> {
        Ok(ChatterboxConfig {
            cfg_weight: value.cfg_weight,
            endpoint: value.endpoint.to_shared_string(),
            exaggeration: value.exaggeration,
            temperature: value.temperature,
            voicePath: value.voice_path.to_string_lossy().to_shared_string(),
        })
    }
}

#[async_trait]
impl DialogGenerator for ChatterboxGenerator {
    type Config = ChatterboxGeneratorConfig;

    async fn generate_dialog(config: Self::Config, dialog: SpokenTopicLine) -> Result<Vec<u8>, DialogGenerationError> {
        let mut stream = BufStream::new(TcpStream::connect(&config.endpoint).await?);
        // send config and voiceline path for cache
        let request_line = format!("{}|{}|{}|{}\n",
            config.voice_path.file_name().unwrap_or_default().to_str().unwrap_or_default(),
            config.exaggeration,
            config.cfg_weight,
            config.temperature
        );
        stream.write_all(request_line.as_bytes()).await?;
        stream.flush().await?;

        // remote will potentially ask for the voiceline file
        let response = {
            let mut response = String::new();
            stream.read_line(&mut response).await?;
            response.trim().to_string()
        };

        // if remote wants the file, then send it
        if response == "SEND_REF" {
            let ref_file_bytes = tokio::fs::read(&config.voice_path).await?;
            stream.write_all(format!("{}\n", ref_file_bytes.len()).as_bytes()).await?;
            stream.write_all(&ref_file_bytes).await?;
        }

        // send the dialogue line to speak
        stream.write_all(format!("{}\n", dialog.0).as_bytes()).await?;
        stream.flush().await?;
        // remainder of the stream will be the wav file
        let result_wav_bytes = {
            let mut result = Vec::new();
            stream.read_to_end(&mut result).await?;
            result
        };

        Ok(result_wav_bytes)
    }
}

#[cfg(test)]
mod test{
    use std::io::{BufReader, Cursor};
    use std::path::Path;
    use super::*;

    #[tokio::test]
    async fn test_generate_dialog(){
        let config = ChatterboxGeneratorConfig{
            endpoint: "localhost:9005".to_string(),
            temperature: 0.5,
            cfg_weight: 0.5,
            exaggeration: 0.5,
            voice_path: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("test_assets/female-khajiit.wav"),
        };
        let dialog = SpokenTopicLine("This is a test message".to_string());
        let result = ChatterboxGenerator::generate_dialog(config, dialog).await.unwrap();
        assert_ne!(result.len(), 0);
        println!("Dialog generation result len: {:?}", result.len());

        let stream_handle = rodio::OutputStreamBuilder::open_default_stream()
            .expect("open default audio stream");

        println!("Playing result audio");
        // fine for testing
        let leaky_box = Box::leak(Box::new(result));
        let mem_file = BufReader::new(Cursor::new(leaky_box));
        let stream = rodio::play(&stream_handle.mixer(), mem_file).expect("play");
        stream.sleep_until_end();
    }
}