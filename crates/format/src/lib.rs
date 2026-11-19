use animo_core::{
    CoreError, Document, Mesh, MeshId, Model, ModelId, Node, NodeId, TextureId, TextureRef, Vec2,
    Vertex,
};
use psd::{ColorMode, Psd, PsdDepth};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

pub const FORMAT_VERSION: u32 = 2;

#[derive(Serialize)]
struct ModelFile<'a> {
    format_version: u32,
    model: &'a Model,
}

#[derive(Deserialize)]
struct ModelEnvelope {
    format_version: u32,
    model: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImportedAsset {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImportResult {
    pub model: Model,
    pub assets: Vec<ImportedAsset>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub enum FormatError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Psd(psd::PsdError),
    Png(png::EncodingError),
    UnsupportedVersion(u32),
    UnsupportedPsd(String),
    InvalidData(String),
    InvalidAssetPath(PathBuf),
    MissingAsset(PathBuf),
    InvalidModel(CoreError),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "file I/O failed: {error}"),
            Self::Json(error) => write!(f, "invalid model JSON: {error}"),
            Self::Psd(error) => write!(f, "PSD import failed: {error}"),
            Self::Png(error) => write!(f, "PNG encoding failed: {error}"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported model format version {version}")
            }
            Self::UnsupportedPsd(message) => write!(f, "unsupported PSD: {message}"),
            Self::InvalidData(message) => write!(f, "invalid model data: {message}"),
            Self::InvalidAssetPath(path) => write!(f, "invalid asset path: {}", path.display()),
            Self::MissingAsset(path) => write!(f, "missing texture asset: {}", path.display()),
            Self::InvalidModel(error) => write!(f, "model validation failed: {error}"),
        }
    }
}

impl std::error::Error for FormatError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Psd(error) => Some(error),
            Self::Png(error) => Some(error),
            Self::InvalidModel(error) => Some(error),
            Self::UnsupportedVersion(_)
            | Self::UnsupportedPsd(_)
            | Self::InvalidData(_)
            | Self::InvalidAssetPath(_)
            | Self::MissingAsset(_) => None,
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

impl From<CoreError> for FormatError {
    fn from(error: CoreError) -> Self {
        Self::InvalidModel(error)
    }
}

impl From<psd::PsdError> for FormatError {
    fn from(error: psd::PsdError) -> Self {
        Self::Psd(error)
    }
}

impl From<png::EncodingError> for FormatError {
    fn from(error: png::EncodingError) -> Self {
        Self::Png(error)
    }
}

pub fn import_psd(path: impl AsRef<Path>) -> Result<ImportResult, FormatError> {
    let path = path.as_ref();
    let bytes = fs::read(path)?;
    let psd = Psd::from_bytes(&bytes)?;
    if psd.depth() != PsdDepth::Eight || psd.color_mode() != ColorMode::Rgb {
        return Err(FormatError::UnsupportedPsd(
            "only 8-bit RGB PSD documents are supported".into(),
        ));
    }
    let width = psd.width();
    let height = psd.height();
    if width == 0 || height == 0 || width > 32_768 || height > 32_768 {
        return Err(FormatError::InvalidData(
            "PSD canvas dimensions are out of range".into(),
        ));
    }

    let model_name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Imported PSD")
        .to_string();
    let mut model = Model::new(ModelId::new(1), model_name.clone());
    let mut warnings = Vec::new();
    let mut group_nodes = HashMap::new();
    for group_id in psd.group_ids_in_order() {
        if psd.groups().contains_key(group_id) {
            let id = model.allocate_id()?;
            group_nodes.insert(*group_id, NodeId::new(id.get()));
        }
    }
    let mut group_order = HashMap::new();
    let mut next_group_order = i32::try_from(psd.layers().len())
        .map_err(|_| FormatError::InvalidData("PSD has too many layers".into()))?;
    for group_id in psd.group_ids_in_order() {
        if let Some(group) = psd.groups().get(group_id) {
            group_order.insert(*group_id, next_group_order);
            next_group_order = next_group_order
                .checked_add(1)
                .ok_or_else(|| FormatError::InvalidData("PSD group order overflow".into()))?;
            if group.is_clipping_mask() {
                warnings.push(format!(
                    "Group '{}' is a clipping mask; the clipping relationship is not preserved",
                    group.name()
                ));
            }
        }
    }

    let mut assets = Vec::new();
    let mut layer_nodes = Vec::with_capacity(psd.layers().len());
    let mut layer_orders = Vec::with_capacity(psd.layers().len());
    for (index, layer) in psd.layers().iter().enumerate() {
        let id = model.allocate_id()?;
        let node_id = NodeId::new(id.get());
        let parent = layer
            .parent_id()
            .and_then(|parent| group_nodes.get(&parent).copied());
        let order = i32::try_from(index)
            .map_err(|_| FormatError::InvalidData("PSD has too many layers".into()))?;
        let node = Node {
            id: node_id,
            name: layer.name().to_string(),
            parent,
            part: None,
            mesh: None,
            texture: None,
            visible: layer.visible(),
            opacity: f32::from(layer.opacity()) / 255.0,
            draw_order: order,
        };
        if layer.blend_mode() as u8 != 1 {
            warnings.push(format!(
                "Layer '{}' uses a non-normal blend mode; it will render as normal",
                layer.name()
            ));
        }
        if layer.is_clipping_mask() {
            warnings.push(format!(
                "Layer '{}' is a clipping mask; the clipping relationship is not preserved",
                layer.name()
            ));
        }
        layer_nodes.push((node_id, layer.name().to_string()));
        layer_orders.push(order);
        model.nodes.insert(node_id, node);
    }

    for group_id in psd.group_ids_in_order() {
        if let Some(group) = psd.groups().get(group_id) {
            let node_id = *group_nodes.get(group_id).ok_or_else(|| {
                FormatError::InvalidData(format!("PSD group {group_id} is missing its node ID"))
            })?;
            let parent = group
                .parent_id()
                .and_then(|parent| group_nodes.get(&parent).copied());
            let order = *group_order.get(group_id).ok_or_else(|| {
                FormatError::InvalidData(format!("PSD group {group_id} has no draw order"))
            })?;
            model.nodes.insert(
                node_id,
                Node {
                    id: node_id,
                    name: group.name().to_string(),
                    parent,
                    part: None,
                    mesh: None,
                    texture: None,
                    visible: group.visible(),
                    opacity: f32::from(group.opacity()) / 255.0,
                    draw_order: order,
                },
            );
            if group.blend_mode() as u8 != 0 {
                warnings.push(format!(
                    "Group '{}' uses pass-through or non-normal blending; group blending is not preserved",
                    group.name()
                ));
            }
        }
    }

    let safe_model_name: String = model_name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();
    let asset_directory = format!("{safe_model_name}_assets");
    for (index, layer) in psd.layers().iter().enumerate() {
        let node_id = layer_nodes[index].0;
        let left = layer.layer_left();
        let top = layer.layer_top();
        let layer_width = u32::from(layer.width());
        let layer_height = u32::from(layer.height());
        if layer_width == 0 || layer_height == 0 {
            warnings.push(format!("Layer '{}' is empty and was skipped", layer.name()));
            continue;
        }
        let rgba = layer.rgba();
        let layer_pixels = rgba.len() / 4;
        if rgba.len() % 4 != 0 || layer_pixels == 0 {
            warnings.push(format!(
                "Layer '{}' has no usable RGBA pixels and was skipped",
                layer.name()
            ));
            continue;
        }
        let declared_pixels = usize::try_from(layer_width)
            .ok()
            .and_then(|w| {
                usize::try_from(layer_height)
                    .ok()
                    .and_then(|h| w.checked_mul(h))
            })
            .ok_or_else(|| FormatError::InvalidData("PSD layer dimensions overflow".into()))?;
        if declared_pixels != layer_pixels {
            warnings.push(format!(
                "Layer '{}' bounds disagree with its pixel data and was skipped",
                layer.name()
            ));
            continue;
        }
        if !rgba.chunks_exact(4).any(|pixel| pixel[3] != 0) {
            warnings.push(format!("Layer '{}' is fully transparent", layer.name()));
        }
        let asset_path = PathBuf::from(&asset_directory).join(format!("layer-{index}.png"));
        let mut png_bytes = Vec::new();
        {
            let mut encoder =
                png::Encoder::new(Cursor::new(&mut png_bytes), layer_width, layer_height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header()?;
            writer
                .write_image_data(&rgba)
                .map_err(|error| FormatError::InvalidData(error.to_string()))?;
        }
        let mesh_raw_id = model.allocate_id()?;
        let texture_raw_id = model.allocate_id()?;
        let mesh_id = MeshId::new(mesh_raw_id.get());
        let texture_id = TextureId::new(texture_raw_id.get());
        let bottom = top
            .checked_add(i32::try_from(layer_height).map_err(|_| {
                FormatError::InvalidData("PSD layer height exceeds coordinate range".into())
            })?)
            .ok_or_else(|| FormatError::InvalidData("PSD layer coordinate overflow".into()))?;
        let right = left
            .checked_add(i32::try_from(layer_width).map_err(|_| {
                FormatError::InvalidData("PSD layer width exceeds coordinate range".into())
            })?)
            .ok_or_else(|| FormatError::InvalidData("PSD layer coordinate overflow".into()))?;
        let canvas_height = height as f32;
        let x0 = left as f32 - width as f32 * 0.5;
        let x1 = right as f32 - width as f32 * 0.5;
        let y0 = canvas_height * 0.5 - top as f32;
        let y1 = canvas_height * 0.5 - bottom as f32;
        model.meshes.insert(
            mesh_id,
            Mesh {
                id: mesh_id,
                vertices: vec![
                    Vertex {
                        position: Vec2 { x: x0, y: y0 },
                        uv: Vec2 { x: 0.0, y: 0.0 },
                    },
                    Vertex {
                        position: Vec2 { x: x1, y: y0 },
                        uv: Vec2 { x: 1.0, y: 0.0 },
                    },
                    Vertex {
                        position: Vec2 { x: x1, y: y1 },
                        uv: Vec2 { x: 1.0, y: 1.0 },
                    },
                    Vertex {
                        position: Vec2 { x: x0, y: y1 },
                        uv: Vec2 { x: 0.0, y: 1.0 },
                    },
                ],
                indices: vec![0, 1, 2, 0, 2, 3],
            },
        );
        model.textures.insert(
            texture_id,
            TextureRef {
                id: texture_id,
                path: asset_path.to_string_lossy().replace('\\', "/"),
            },
        );
        let node = model
            .nodes
            .get_mut(&node_id)
            .expect("layer node was inserted");
        node.mesh = Some(mesh_id);
        node.texture = Some(texture_id);
        assets.push(ImportedAsset {
            path: asset_path,
            bytes: png_bytes,
        });
    }

    for (index, (node_id, _)) in layer_nodes.iter().enumerate() {
        if let Some(node) = model.nodes.get_mut(node_id) {
            node.draw_order = layer_orders[index];
        }
    }
    model.validate().map_err(FormatError::InvalidModel)?;
    Ok(ImportResult {
        model,
        assets,
        warnings,
    })
}

pub fn encode(model: &Model) -> Result<Vec<u8>, FormatError> {
    model.validate().map_err(FormatError::InvalidModel)?;
    Ok(serde_json::to_vec_pretty(&ModelFile {
        format_version: FORMAT_VERSION,
        model,
    })?)
}

pub fn decode(bytes: &[u8]) -> Result<Model, FormatError> {
    let envelope: ModelEnvelope = serde_json::from_slice(bytes)?;
    let model_value = match envelope.format_version {
        1 => migrate_v1_model(envelope.model)?,
        FORMAT_VERSION => envelope.model,
        version => return Err(FormatError::UnsupportedVersion(version)),
    };
    let model: Model = serde_json::from_value(model_value)?;
    model.validate().map_err(FormatError::InvalidModel)?;
    Ok(model)
}

fn migrate_v1_model(mut model: serde_json::Value) -> Result<serde_json::Value, FormatError> {
    let nodes = model
        .get_mut("nodes")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| FormatError::InvalidData("version 1 model nodes are missing".into()))?;
    for (draw_order, node) in nodes.values_mut().enumerate() {
        let draw_order = i32::try_from(draw_order)
            .map_err(|_| FormatError::InvalidData("too many nodes in version 1 model".into()))?;
        let node = node.as_object_mut().ok_or_else(|| {
            FormatError::InvalidData("version 1 model contains an invalid node".into())
        })?;
        node.insert("visible".into(), serde_json::Value::Bool(true));
        node.insert("opacity".into(), serde_json::json!(1.0));
        node.insert("draw_order".into(), serde_json::json!(draw_order));
    }
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
    fn migrates_version_one_nodes_with_default_display_properties() {
        let bytes = br#"{
            "format_version":1,
            "model":{
                "id":1,
                "name":"legacy",
                "parts":{},
                "nodes":{"2":{"id":2,"name":"layer","parent":null,"part":null,"mesh":null,"texture":null}},
                "meshes":{},
                "textures":{},
                "parameters":{},
                "next_id":3
            }
        }"#;
        let model = decode(bytes).unwrap();
        let node = &model.nodes[&animo_core::NodeId::new(2)];
        assert!(node.visible);
        assert_eq!(node.opacity, 1.0);
        assert_eq!(node.draw_order, 0);
    }

    #[test]
    fn imports_the_supplied_layered_psd_into_valid_textured_meshes() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../model/DoudouEmma_import.psd");
        let imported = import_psd(path).unwrap();
        imported.model.validate().unwrap();
        assert!(!imported.model.nodes.is_empty());
        assert!(!imported.model.meshes.is_empty());
        assert!(!imported.model.textures.is_empty());
        assert!(imported.model.nodes.len() < 10_000);
        assert!(imported.model.meshes.len() < 10_000);
        assert!(imported.assets.len() < 10_000);
        assert_eq!(imported.assets.len(), imported.model.textures.len());
        assert!(
            imported
                .model
                .meshes
                .values()
                .all(|mesh| mesh.indices.len() == 6 && mesh.vertices.len() == 4)
        );
        assert!(
            imported
                .assets
                .iter()
                .all(|asset| asset.bytes.starts_with(b"\x89PNG\r\n\x1a\n"))
        );
    }

    #[test]
    fn json_round_trip_preserves_model() {
        let model = Model::new(ModelId::new(1), "sample");
        let decoded = decode(&encode(&model).unwrap()).unwrap();
        assert_eq!(decoded, model);
    }

    #[test]
    fn rejects_unknown_format_version() {
        let result = decode(br#"{"format_version":3,"model":{}}"#);
        assert!(matches!(result, Err(FormatError::UnsupportedVersion(3))));
    }
}
