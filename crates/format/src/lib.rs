use animo_core::{CoreError, Document, Model};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

pub const FORMAT_VERSION: u32 = 1;

#[derive(Deserialize, Serialize)]
struct ModelFile {
    format_version: u32,
    model: Model,
}

#[derive(Deserialize)]
struct ModelEnvelope {
    format_version: u32,
    model: serde_json::Value,
}

#[derive(Debug)]
pub enum FormatError {
    Io(std::io::Error),
    Json(serde_json::Error),
    UnsupportedVersion(u32),
    InvalidModel(CoreError),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "file I/O failed: {error}"),
            Self::Json(error) => write!(f, "invalid model JSON: {error}"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported model format version {version}")
            }
            Self::InvalidModel(error) => write!(f, "model validation failed: {error}"),
        }
    }
}

impl std::error::Error for FormatError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::InvalidModel(error) => Some(error),
            Self::UnsupportedVersion(_) => None,
        }
    }
}

impl From<std::io::Error> for FormatError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for FormatError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

pub fn encode(model: &Model) -> Result<Vec<u8>, FormatError> {
    model.validate().map_err(FormatError::InvalidModel)?;
    Ok(serde_json::to_vec_pretty(&ModelFile {
        format_version: FORMAT_VERSION,
        model: model.clone(),
    })?)
}

pub fn decode(bytes: &[u8]) -> Result<Model, FormatError> {
    let envelope: ModelEnvelope = serde_json::from_slice(bytes)?;
    if envelope.format_version != FORMAT_VERSION {
        return Err(FormatError::UnsupportedVersion(envelope.format_version));
    }
    let model: Model = serde_json::from_value(envelope.model)?;
    model.validate().map_err(FormatError::InvalidModel)?;
    Ok(model)
}

pub fn save_model(path: impl AsRef<Path>, model: &Model) -> Result<(), FormatError> {
    let bytes = encode(model)?;
    fs::write(path, bytes).map_err(FormatError::Io)
}

pub fn load_model(path: impl AsRef<Path>) -> Result<Model, FormatError> {
    let mut file = File::open(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    decode(&bytes)
}

pub fn save_document(path: impl AsRef<Path>, document: &Document) -> Result<(), FormatError> {
    save_model(path, document.model())
}

#[cfg(test)]
mod tests {
    use super::*;
    use animo_core::ModelId;

    #[test]
    fn json_round_trip_preserves_model() {
        let model = Model::new(ModelId::new(1), "sample");
        let decoded = decode(&encode(&model).unwrap()).unwrap();
        assert_eq!(decoded, model);
    }

    #[test]
    fn rejects_unknown_format_version() {
        let result = decode(br#"{"format_version":2,"model":{}}"#);
        assert!(matches!(result, Err(FormatError::UnsupportedVersion(2))));
    }
}
