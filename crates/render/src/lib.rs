#![forbid(unsafe_code)]

use animo_core::{Mesh, MeshId, Model, NodeId, TextureId};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewPoint {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewTriangle {
    pub indices: [u32; 3],
    pub vertices: [PreviewPoint; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewBounds {
    pub min: PreviewPoint,
    pub max: PreviewPoint,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeshPreview {
    pub triangles: Vec<PreviewTriangle>,
    pub bounds: Option<PreviewBounds>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TexturedDrawRecord {
    pub node: NodeId,
    pub mesh: MeshId,
    pub texture: TextureId,
    pub opacity: f32,
    pub draw_order: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RenderError {
    InvalidTriangleIndexCount(usize),
    VertexIndexOutOfRange { index: u32, vertex_count: usize },
    NonFiniteVertex,
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTriangleIndexCount(count) => {
                write!(f, "mesh index count {count} is not divisible by three")
            }
            Self::VertexIndexOutOfRange {
                index,
                vertex_count,
            } => {
                write!(f, "mesh index {index} exceeds vertex count {vertex_count}")
            }
            Self::NonFiniteVertex => f.write_str("mesh contains a non-finite vertex position"),
        }
    }
}

impl std::error::Error for RenderError {}

pub fn textured_draw_records(model: &Model) -> Vec<TexturedDrawRecord> {
    let mut records = Vec::new();
    for node in model.nodes.values() {
        let (Some(mesh), Some(texture)) = (node.mesh, node.texture) else {
            continue;
        };
        let mut visible = true;
        let mut opacity = 1.0_f32;
        let mut current = Some(node.id);
        let mut ancestors = Vec::new();
        while let Some(node_id) = current {
            let Some(ancestor) = model.nodes.get(&node_id) else {
                visible = false;
                break;
            };
            ancestors.push(ancestor);
            current = ancestor.parent;
            if ancestors.len() > model.nodes.len() {
                visible = false;
                break;
            }
        }
        for ancestor in ancestors {
            visible &= ancestor.visible;
            opacity *= ancestor.opacity;
        }
        if visible && opacity > 0.0 {
            records.push(TexturedDrawRecord {
                node: node.id,
                mesh,
                texture,
                opacity,
                draw_order: node.draw_order,
            });
        }
    }
    records.sort_by_key(|record| (record.draw_order, record.node.get()));
    records
}

impl MeshPreview {
    pub fn from_mesh(mesh: &Mesh) -> Result<Self, RenderError> {
        if !mesh.indices.len().is_multiple_of(3) {
            return Err(RenderError::InvalidTriangleIndexCount(mesh.indices.len()));
        }
        let mut min = PreviewPoint {
            x: f32::INFINITY,
            y: f32::INFINITY,
        };
        let mut max = PreviewPoint {
            x: f32::NEG_INFINITY,
            y: f32::NEG_INFINITY,
        };
        for vertex in &mesh.vertices {
            let position = vertex.position;
            if !position.x.is_finite() || !position.y.is_finite() {
                return Err(RenderError::NonFiniteVertex);
            }
            min.x = min.x.min(position.x);
            min.y = min.y.min(position.y);
            max.x = max.x.max(position.x);
            max.y = max.y.max(position.y);
        }
        let triangles = mesh
            .indices
            .chunks_exact(3)
            .map(|indices| {
                let vertices = indices
                    .iter()
                    .map(|index| {
                        mesh.vertices
                            .get(*index as usize)
                            .map(|vertex| PreviewPoint {
                                x: vertex.position.x,
                                y: vertex.position.y,
                            })
                            .ok_or(RenderError::VertexIndexOutOfRange {
                                index: *index,
                                vertex_count: mesh.vertices.len(),
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(PreviewTriangle {
                    indices: [indices[0], indices[1], indices[2]],
                    vertices: [vertices[0], vertices[1], vertices[2]],
                })
            })
            .collect::<Result<Vec<_>, RenderError>>()?;
        let bounds = if mesh.vertices.is_empty() {
            None
        } else {
            Some(PreviewBounds { min, max })
        };
        Ok(Self { triangles, bounds })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use animo_core::{MeshId, Vec2, Vertex};

    fn triangle() -> Mesh {
        Mesh {
            id: MeshId::new(1),
            vertices: vec![
                Vertex {
                    position: Vec2 { x: -1.0, y: -2.0 },
                    uv: Vec2 { x: 0.0, y: 0.0 },
                },
                Vertex {
                    position: Vec2 { x: 2.0, y: -2.0 },
                    uv: Vec2 { x: 1.0, y: 0.0 },
                },
                Vertex {
                    position: Vec2 { x: 0.0, y: 3.0 },
                    uv: Vec2 { x: 0.5, y: 1.0 },
                },
            ],
            indices: vec![0, 1, 2],
        }
    }

    #[test]
    fn converts_triangle_and_calculates_bounds() {
        let preview = MeshPreview::from_mesh(&triangle()).unwrap();
        assert_eq!(preview.triangles.len(), 1);
        assert_eq!(
            preview.bounds,
            Some(PreviewBounds {
                min: PreviewPoint { x: -1.0, y: -2.0 },
                max: PreviewPoint { x: 2.0, y: 3.0 },
            })
        );
    }

    #[test]
    fn rejects_invalid_index_and_non_finite_position() {
        let mut mesh = triangle();
        mesh.indices[2] = 3;
        assert!(matches!(
            MeshPreview::from_mesh(&mesh),
            Err(RenderError::VertexIndexOutOfRange { .. })
        ));
        let mut mesh = triangle();
        mesh.vertices[0].position.x = f32::NAN;
        assert_eq!(
            MeshPreview::from_mesh(&mesh),
            Err(RenderError::NonFiniteVertex)
        );
    }

    #[test]
    fn textured_draw_records_inherit_visibility_opacity_and_order() {
        use animo_core::{Model, ModelId, Node, TextureId, TextureRef};
        let mut model = Model::new(ModelId::new(1), "render test");
        let group_id = NodeId::new(2);
        let hidden_group_id = NodeId::new(3);
        let first_id = NodeId::new(4);
        let second_id = NodeId::new(5);
        let first_mesh = MeshId::new(6);
        let second_mesh = MeshId::new(7);
        let first_texture = TextureId::new(8);
        let second_texture = TextureId::new(9);
        for (node_id, name, parent, visible, opacity, draw_order, mesh, texture) in [
            (group_id, "visible group", None, true, 0.5, 0, None, None),
            (hidden_group_id, "hidden group", None, false, 1.0, 1, None, None),
            (first_id, "first", Some(group_id), true, 0.5, 8, Some(first_mesh), Some(first_texture)),
            (second_id, "second", Some(hidden_group_id), true, 1.0, -2, Some(second_mesh), Some(second_texture)),
        ] {
            model.nodes.insert(node_id, Node {
                id: node_id,
                name: name.into(),
                parent,
                part: None,
                mesh,
                texture,
                visible,
                opacity,
                draw_order,
            });
        }
        model.textures.insert(first_texture, TextureRef { id: first_texture, path: "first.png".into() });
        model.textures.insert(second_texture, TextureRef { id: second_texture, path: "second.png".into() });
        let records = textured_draw_records(&model);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].node, first_id);
        assert_eq!(records[0].opacity, 0.25);
    }

    #[test]
    fn empty_mesh_has_no_bounds_or_triangles() {
        let mesh = Mesh {
            id: MeshId::new(1),
            vertices: Vec::new(),
            indices: Vec::new(),
        };
        let preview = MeshPreview::from_mesh(&mesh).unwrap();
        assert!(preview.triangles.is_empty());
        assert_eq!(preview.bounds, None);
    }
}
