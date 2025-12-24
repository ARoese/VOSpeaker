use std::io;
use async_trait::async_trait;
use crate::hashes::ConfigHash;
use thiserror::Error;
use serde::de::DeserializeOwned;
use crate::topic_lines::CleanTopicLine;

/// ConfigHashable is unique to a normal hash in that
/// it does not need to satisfy a == b -> config_hash(a) == config_hash(b).
/// This hash should be used to compare objects for *functional* equality
/// rather than objective equality. This notion of equality is dependent on
/// the trait implementor.
///
/// For example, the partial hash of a configuration for a remote service
/// should not depend on the endpoint of that service
///
/// TODO: consider breaking up objects which need this property into
/// TODO: a struct composing fields irrelevant for equality and a
/// TODO: substruct of fields that should be hash-equivalent
pub trait ConfigHashable
where Self: DeserializeOwned {
    fn config_hash(&self) -> ConfigHash;
}

#[derive(Debug, Error)]
pub enum DialogGenerationError {
    #[error("config is invalid")]
    InvalidConfig(String),

    #[error("I/O error: {source}")]
    Io {
        #[from]
        source: io::Error,
    },
}

#[async_trait]
pub trait DialogGenerator {
    type Config: ConfigHashable;

    async fn generate_dialog(config: Self::Config, dialog: CleanTopicLine) -> Result<Vec<u8>, DialogGenerationError>;
}