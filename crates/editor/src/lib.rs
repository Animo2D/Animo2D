#![forbid(unsafe_code)]

use animo_core::{CoreError, Document, EditRecord, EditTransaction, Model};

#[derive(Clone, Debug)]
pub struct EditorSession {
    document: Document,
}

impl EditorSession {
    pub fn new(model: Model) -> Result<Self, CoreError> {
        Ok(Self {
            document: Document::new(model)?,
        })
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn revision(&self) -> u64 {
        self.document.revision()
    }

    pub fn edit_history(&self) -> &[EditRecord] {
        self.document.edit_history()
    }

    pub fn can_undo(&self) -> bool {
        self.document.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.document.can_redo()
    }

    pub fn execute(&mut self, transaction: EditTransaction) -> Result<u64, CoreError> {
        self.document.execute(transaction)
    }

    pub fn undo(&mut self) -> Result<u64, CoreError> {
        self.document.undo()
    }

    pub fn redo(&mut self) -> Result<u64, CoreError> {
        self.document.redo()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use animo_core::{EditCommand, Mesh, MeshId, ModelId, Node, NodeId, Vec2, Vertex};

    #[test]
    fn session_executes_mesh_edit_commands() {
        let model = Model::new(ModelId::new(1), "sample");
        let mut session = EditorSession::new(model).unwrap();
        let mesh_id = MeshId::new(2);
        let node_id = NodeId::new(3);
        session
            .execute(EditTransaction {
                label: "Create mesh and node".into(),
                expected_revision: session.revision(),
                commands: vec![
                    EditCommand::AddMesh(Mesh {
                        id: mesh_id,
                        vertices: Vec::new(),
                        indices: Vec::new(),
                    }),
                    EditCommand::AddNode(Node {
                        id: node_id,
                        name: "mesh node".into(),
                        parent: None,
                        part: None,
                        mesh: None,
                        texture: None,
                    }),
                ],
            })
            .unwrap();
        session
            .execute(EditTransaction {
                label: "Edit mesh".into(),
                expected_revision: session.revision(),
                commands: vec![
                    EditCommand::AddMeshVertex {
                        mesh: mesh_id,
                        vertex: Vertex {
                            position: Vec2 { x: 1.0, y: 2.0 },
                            uv: Vec2 { x: 0.0, y: 0.0 },
                        },
                    },
                    EditCommand::SetNodeMesh {
                        node: node_id,
                        mesh: Some(mesh_id),
                    },
                    EditCommand::SetVertexUv {
                        mesh: mesh_id,
                        vertex_index: 0,
                        uv: Vec2 { x: 0.5, y: 0.75 },
                    },
                ],
            })
            .unwrap();
        let model = session.document().model();
        assert_eq!(model.nodes[&node_id].mesh, Some(mesh_id));
        assert_eq!(model.meshes[&mesh_id].vertices.len(), 1);
        assert_eq!(
            model.meshes[&mesh_id].vertices[0].uv,
            Vec2 { x: 0.5, y: 0.75 }
        );
    }

    #[test]
    fn session_uses_core_edit_api_and_history() {
        let model = Model::new(ModelId::new(1), "sample");
        let mut session = EditorSession::new(model).unwrap();
        let transaction = EditTransaction {
            label: "Add root".into(),
            expected_revision: session.revision(),
            commands: vec![EditCommand::AddNode(Node {
                id: NodeId::new(2),
                name: "Root".into(),
                parent: None,
                part: None,
                mesh: None,
                texture: None,
            })],
        };
        assert_eq!(session.execute(transaction).unwrap(), 1);
        assert_eq!(session.document().model().nodes.len(), 1);
        assert_eq!(session.undo().unwrap(), 2);
        assert!(session.document().model().nodes.is_empty());
        assert_eq!(session.redo().unwrap(), 3);
        assert_eq!(session.edit_history().len(), 3);
    }
}
