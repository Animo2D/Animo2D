use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct Id(u64);

impl Id {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

macro_rules! id_type {
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
            #[serde(transparent)]
            pub struct $name(pub Id);

            impl $name {
                pub const fn new(value: u64) -> Self { Self(Id::new(value)) }
                pub const fn get(self) -> u64 { self.0.get() }
            }
        )+
    };
}

id_type!(
    ModelId,
    PartId,
    NodeId,
    MeshId,
    TextureId,
    ParameterId,
    KeyformId
);

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Vertex {
    pub position: Vec2,
    pub uv: Vec2,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Mesh {
    pub id: MeshId,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TextureRef {
    pub id: TextureId,
    pub path: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub parent: Option<NodeId>,
    pub part: Option<PartId>,
    pub mesh: Option<MeshId>,
    pub texture: Option<TextureId>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Part {
    pub id: PartId,
    pub name: String,
    pub nodes: Vec<NodeId>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Parameter {
    pub id: ParameterId,
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub keyforms: Vec<Keyform>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Keyform {
    pub id: KeyformId,
    pub value: f32,
    pub node_values: BTreeMap<NodeId, Vec2>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Model {
    pub id: ModelId,
    pub name: String,
    pub parts: BTreeMap<PartId, Part>,
    pub nodes: BTreeMap<NodeId, Node>,
    pub meshes: BTreeMap<MeshId, Mesh>,
    pub textures: BTreeMap<TextureId, TextureRef>,
    pub parameters: BTreeMap<ParameterId, Parameter>,
    next_id: u64,
}

impl Model {
    pub fn new(id: ModelId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            parts: BTreeMap::new(),
            nodes: BTreeMap::new(),
            meshes: BTreeMap::new(),
            textures: BTreeMap::new(),
            parameters: BTreeMap::new(),
            next_id: 1,
        }
    }

    pub fn allocate_id(&mut self) -> Result<Id, CoreError> {
        loop {
            let id = Id::new(self.next_id);
            self.next_id = self
                .next_id
                .checked_add(1)
                .ok_or_else(|| CoreError::InvalidInput("model ID space exhausted".into()))?;
            if !self.contains_id(id.get()) {
                return Ok(id);
            }
        }
    }

    fn contains_id(&self, id: u64) -> bool {
        self.id.get() == id
            || self.parts.values().any(|entry| entry.id.get() == id)
            || self.nodes.values().any(|entry| entry.id.get() == id)
            || self.meshes.values().any(|entry| entry.id.get() == id)
            || self.textures.values().any(|entry| entry.id.get() == id)
            || self.parameters.values().any(|entry| {
                entry.id.get() == id || entry.keyforms.iter().any(|keyform| keyform.id.get() == id)
            })
    }

    pub fn validate(&self) -> Result<(), CoreError> {
        if self.name.trim().is_empty() {
            return Err(CoreError::InvalidInput("model name cannot be empty".into()));
        }
        let mut all_ids = BTreeSet::from([self.id.get()]);
        let mut check_id = |id: u64| {
            if all_ids.insert(id) {
                Ok(())
            } else {
                Err(CoreError::DuplicateId(id))
            }
        };
        for (part_id, part) in &self.parts {
            if *part_id != part.id {
                return Err(CoreError::InvalidInput(format!(
                    "part map key does not match ID {}",
                    part.id.get()
                )));
            }
            check_id(part.id.get())?;
            if part.name.trim().is_empty() {
                return Err(CoreError::InvalidInput("part name cannot be empty".into()));
            }
            let mut unique_nodes = BTreeSet::new();
            for node_id in &part.nodes {
                if !unique_nodes.insert(*node_id) {
                    return Err(CoreError::InvalidInput(format!(
                        "node {} is listed more than once in part {}",
                        node_id.get(),
                        part.id.get()
                    )));
                }
                let node = self.nodes.get(node_id).ok_or(CoreError::NotFound {
                    kind: "node",
                    id: node_id.get(),
                })?;
                if node.part != Some(part.id) {
                    return Err(CoreError::InvalidInput(format!(
                        "node {} has inconsistent part membership",
                        node_id.get()
                    )));
                }
            }
        }
        for (mesh_id, mesh) in &self.meshes {
            if *mesh_id != mesh.id {
                return Err(CoreError::InvalidInput(format!(
                    "mesh map key does not match ID {}",
                    mesh.id.get()
                )));
            }
            check_id(mesh.id.get())?;
            if mesh.indices.len() % 3 != 0 {
                return Err(CoreError::InvalidInput(format!(
                    "mesh {} indices must describe triangles",
                    mesh.id.get()
                )));
            }
            for index in &mesh.indices {
                if *index as usize >= mesh.vertices.len() {
                    return Err(CoreError::InvalidInput(format!(
                        "mesh {} index {} is out of range",
                        mesh.id.get(),
                        index
                    )));
                }
            }
            for vertex in &mesh.vertices {
                if !finite_vec2(vertex.position) || !finite_vec2(vertex.uv) {
                    return Err(CoreError::InvalidInput(format!(
                        "mesh {} contains a non-finite vertex",
                        mesh.id.get()
                    )));
                }
            }
        }
        for (texture_id, texture) in &self.textures {
            if *texture_id != texture.id {
                return Err(CoreError::InvalidInput(format!(
                    "texture map key does not match ID {}",
                    texture.id.get()
                )));
            }
            check_id(texture.id.get())?;
            if texture.path.trim().is_empty() {
                return Err(CoreError::InvalidInput(
                    "texture path cannot be empty".into(),
                ));
            }
        }
        for (node_id, node) in &self.nodes {
            if *node_id != node.id {
                return Err(CoreError::InvalidInput(format!(
                    "node map key does not match ID {}",
                    node.id.get()
                )));
            }
            check_id(node.id.get())?;
            if node.name.trim().is_empty() {
                return Err(CoreError::InvalidInput("node name cannot be empty".into()));
            }
            if let Some(parent) = node.parent {
                if !self.nodes.contains_key(&parent) {
                    return Err(CoreError::NotFound {
                        kind: "parent node",
                        id: parent.get(),
                    });
                }
                if parent == node.id || self.is_ancestor(parent, node.id) {
                    return Err(CoreError::InvalidInput(format!(
                        "node {} hierarchy contains a cycle",
                        node.id.get()
                    )));
                }
            }
            if let Some(part_id) = node.part {
                let part = self.parts.get(&part_id).ok_or(CoreError::NotFound {
                    kind: "part",
                    id: part_id.get(),
                })?;
                if !part.nodes.contains(&node.id) {
                    return Err(CoreError::InvalidInput(format!(
                        "node {} is missing from part {}",
                        node.id.get(),
                        part_id.get()
                    )));
                }
            }
            if let Some(mesh) = node.mesh
                && !self.meshes.contains_key(&mesh)
            {
                return Err(CoreError::NotFound {
                    kind: "mesh",
                    id: mesh.get(),
                });
            }
            if let Some(texture) = node.texture
                && !self.textures.contains_key(&texture)
            {
                return Err(CoreError::NotFound {
                    kind: "texture",
                    id: texture.get(),
                });
            }
        }
        for (parameter_id, parameter) in &self.parameters {
            if *parameter_id != parameter.id {
                return Err(CoreError::InvalidInput(format!(
                    "parameter map key does not match ID {}",
                    parameter.id.get()
                )));
            }
            check_id(parameter.id.get())?;
            if parameter.name.trim().is_empty()
                || !parameter.min.is_finite()
                || !parameter.max.is_finite()
                || !parameter.default.is_finite()
                || parameter.min >= parameter.max
                || parameter.default < parameter.min
                || parameter.default > parameter.max
            {
                return Err(CoreError::InvalidInput(format!(
                    "parameter {} has invalid name or range",
                    parameter.id.get()
                )));
            }
            let mut key_ids = BTreeSet::new();
            let mut values = BTreeSet::new();
            for keyform in &parameter.keyforms {
                check_id(keyform.id.get())?;
                if !key_ids.insert(keyform.id) {
                    return Err(CoreError::InvalidInput(format!(
                        "parameter {} has duplicate keyform IDs",
                        parameter.id.get()
                    )));
                }
                if !keyform.value.is_finite()
                    || keyform.value < parameter.min
                    || keyform.value > parameter.max
                    || !values.insert(keyform.value.to_bits())
                {
                    return Err(CoreError::InvalidInput(format!(
                        "parameter {} has an invalid or duplicate keyform",
                        parameter.id.get()
                    )));
                }
                for (node, value) in &keyform.node_values {
                    if !self.nodes.contains_key(node) {
                        return Err(CoreError::NotFound {
                            kind: "keyform node",
                            id: node.get(),
                        });
                    }
                    if !finite_vec2(*value) {
                        return Err(CoreError::InvalidInput(
                            "keyform contains a non-finite value".into(),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn is_ancestor(&self, node: NodeId, possible_ancestor: NodeId) -> bool {
        let mut current = self.nodes.get(&node).and_then(|entry| entry.parent);
        while let Some(parent) = current {
            if parent == possible_ancestor {
                return true;
            }
            current = self.nodes.get(&parent).and_then(|entry| entry.parent);
        }
        false
    }
}

fn finite_vec2(value: Vec2) -> bool {
    value.x.is_finite() && value.y.is_finite()
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum EditCommand {
    AddPart(Part),
    RemovePart(PartId),
    AddNode(Node),
    RemoveNode(NodeId),
    RenameNode {
        id: NodeId,
        name: String,
    },
    SetNodeParent {
        id: NodeId,
        parent: Option<NodeId>,
    },
    AddMesh(Mesh),
    AddMeshVertex {
        mesh: MeshId,
        vertex: Vertex,
    },
    SetVertexPosition {
        mesh: MeshId,
        vertex_index: usize,
        position: Vec2,
    },
    SetVertexUv {
        mesh: MeshId,
        vertex_index: usize,
        uv: Vec2,
    },
    SetNodeMesh {
        node: NodeId,
        mesh: Option<MeshId>,
    },
    RemoveMesh(MeshId),
    AddTexture(TextureRef),
    RemoveTexture(TextureId),
    AddParameter(Parameter),
    RemoveParameter(ParameterId),
    SetKeyform {
        parameter: ParameterId,
        keyform: Keyform,
    },
    RemoveKeyform {
        parameter: ParameterId,
        keyform: KeyformId,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EditTransaction {
    pub label: String,
    pub expected_revision: u64,
    pub commands: Vec<EditCommand>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EditRecord {
    pub label: String,
    pub revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreError {
    InvalidInput(String),
    DuplicateId(u64),
    NotFound { kind: &'static str, id: u64 },
    AlreadyExists { kind: &'static str, id: u64 },
    InUse { kind: &'static str, id: u64 },
    RevisionConflict { expected: u64, actual: u64 },
    EmptyTransaction,
    NothingToUndo,
    NothingToRedo,
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) => write!(f, "invalid input: {message}"),
            Self::DuplicateId(id) => write!(f, "ID {id} is used by more than one object"),
            Self::NotFound { kind, id } => write!(f, "{kind} {id} was not found"),
            Self::AlreadyExists { kind, id } => write!(f, "{kind} {id} already exists"),
            Self::InUse { kind, id } => write!(f, "{kind} {id} is still in use"),
            Self::RevisionConflict { expected, actual } => {
                write!(f, "revision conflict: expected {expected}, found {actual}")
            }
            Self::EmptyTransaction => f.write_str("transaction contains no commands"),
            Self::NothingToUndo => f.write_str("there is no edit to undo"),
            Self::NothingToRedo => f.write_str("there is no edit to redo"),
        }
    }
}

impl std::error::Error for CoreError {}

#[derive(Clone, Debug)]
pub struct Document {
    model: Model,
    revision: u64,
    history: Vec<HistoryEntry>,
    history_cursor: usize,
    history_limit: usize,
    edit_history: Vec<EditRecord>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct HistoryEntry {
    label: String,
    before: Model,
    after: Model,
}

impl Document {
    pub fn new(model: Model) -> Result<Self, CoreError> {
        model.validate()?;
        Ok(Self {
            model,
            revision: 0,
            history: Vec::new(),
            history_cursor: 0,
            history_limit: 256,
            edit_history: Vec::new(),
        })
    }

    pub fn model(&self) -> &Model {
        &self.model
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn edit_history(&self) -> &[EditRecord] {
        &self.edit_history
    }

    pub fn can_undo(&self) -> bool {
        self.history_cursor > 0
    }

    pub fn can_redo(&self) -> bool {
        self.history_cursor < self.history.len()
    }

    pub fn set_history_limit(&mut self, limit: usize) {
        self.history_limit = limit;
        if self.history.len() > limit {
            let remove = self.history.len() - limit;
            self.history.drain(0..remove);
            self.history_cursor = self.history_cursor.saturating_sub(remove);
        }
    }

    pub fn execute(&mut self, transaction: EditTransaction) -> Result<u64, CoreError> {
        if transaction.expected_revision != self.revision {
            return Err(CoreError::RevisionConflict {
                expected: transaction.expected_revision,
                actual: self.revision,
            });
        }
        if transaction.commands.is_empty() {
            return Err(CoreError::EmptyTransaction);
        }
        let mut candidate = self.model.clone();
        for command in transaction.commands {
            apply_command(&mut candidate, command)?;
        }
        candidate.validate()?;
        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| CoreError::InvalidInput("document revision space exhausted".into()))?;
        let before = std::mem::replace(&mut self.model, candidate.clone());
        let entry = HistoryEntry {
            label: transaction.label,
            before,
            after: candidate,
        };
        let label = entry.label.clone();
        self.history.truncate(self.history_cursor);
        if self.history_limit > 0 {
            self.history.push(entry);
            self.history_cursor += 1;
            if self.history.len() > self.history_limit {
                self.history.remove(0);
                self.history_cursor -= 1;
            }
        } else {
            self.history_cursor = 0;
        }
        self.revision = next_revision;
        self.edit_history.push(EditRecord {
            label,
            revision: self.revision,
        });
        Ok(self.revision)
    }

    pub fn undo(&mut self) -> Result<u64, CoreError> {
        if self.history_cursor == 0 {
            return Err(CoreError::NothingToUndo);
        }
        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| CoreError::InvalidInput("document revision space exhausted".into()))?;
        self.history_cursor -= 1;
        let entry = &self.history[self.history_cursor];
        self.model = entry.before.clone();
        self.revision = next_revision;
        self.edit_history.push(EditRecord {
            label: format!("Undo {}", entry.label),
            revision: self.revision,
        });
        Ok(self.revision)
    }

    pub fn redo(&mut self) -> Result<u64, CoreError> {
        if self.history_cursor == self.history.len() {
            return Err(CoreError::NothingToRedo);
        }
        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| CoreError::InvalidInput("document revision space exhausted".into()))?;
        let entry = &self.history[self.history_cursor];
        self.model = entry.after.clone();
        let label = entry.label.clone();
        self.history_cursor += 1;
        self.revision = next_revision;
        self.edit_history.push(EditRecord {
            label: format!("Redo {label}"),
            revision: self.revision,
        });
        Ok(self.revision)
    }
}

fn apply_command(model: &mut Model, command: EditCommand) -> Result<(), CoreError> {
    match command {
        EditCommand::AddPart(part) => {
            if model.parts.contains_key(&part.id) {
                return Err(CoreError::AlreadyExists {
                    kind: "part",
                    id: part.id.get(),
                });
            }
            let mut unique_nodes = BTreeSet::new();
            for node_id in &part.nodes {
                if !unique_nodes.insert(*node_id) {
                    return Err(CoreError::InvalidInput(format!(
                        "node {} appears more than once in part",
                        node_id.get()
                    )));
                }
                let node = model.nodes.get(node_id).ok_or(CoreError::NotFound {
                    kind: "node",
                    id: node_id.get(),
                })?;
                if node.part.is_some() {
                    return Err(CoreError::InUse {
                        kind: "node",
                        id: node_id.get(),
                    });
                }
            }
            for node_id in &part.nodes {
                model
                    .nodes
                    .get_mut(node_id)
                    .expect("node existence checked")
                    .part = Some(part.id);
            }
            model.parts.insert(part.id, part);
        }
        EditCommand::RemovePart(id) => {
            let part = model.parts.remove(&id).ok_or(CoreError::NotFound {
                kind: "part",
                id: id.get(),
            })?;
            for node_id in part.nodes {
                if let Some(node) = model.nodes.get_mut(&node_id) {
                    node.part = None;
                }
            }
        }
        EditCommand::AddNode(node) => {
            if model.nodes.contains_key(&node.id) {
                return Err(CoreError::AlreadyExists {
                    kind: "node",
                    id: node.id.get(),
                });
            }
            if let Some(part_id) = node.part {
                let part = model.parts.get_mut(&part_id).ok_or(CoreError::NotFound {
                    kind: "part",
                    id: part_id.get(),
                })?;
                if part.nodes.contains(&node.id) {
                    return Err(CoreError::InvalidInput(format!(
                        "part {} already lists node {}",
                        part_id.get(),
                        node.id.get()
                    )));
                }
                part.nodes.push(node.id);
            }
            model.nodes.insert(node.id, node);
        }
        EditCommand::RemoveNode(id) => {
            if !model.nodes.contains_key(&id) {
                return Err(CoreError::NotFound {
                    kind: "node",
                    id: id.get(),
                });
            }
            if model.nodes.values().any(|node| node.parent == Some(id)) {
                return Err(CoreError::InUse {
                    kind: "node",
                    id: id.get(),
                });
            }
            let node = model.nodes.remove(&id).expect("node existence checked");
            if let Some(part_id) = node.part
                && let Some(part) = model.parts.get_mut(&part_id)
            {
                part.nodes.retain(|entry| *entry != id);
            }
            for parameter in model.parameters.values_mut() {
                for keyform in &mut parameter.keyforms {
                    keyform.node_values.remove(&id);
                }
            }
        }
        EditCommand::RenameNode { id, name } => {
            if name.trim().is_empty() {
                return Err(CoreError::InvalidInput("node name cannot be empty".into()));
            }
            model
                .nodes
                .get_mut(&id)
                .ok_or(CoreError::NotFound {
                    kind: "node",
                    id: id.get(),
                })?
                .name = name;
        }
        EditCommand::SetNodeParent { id, parent } => {
            if !model.nodes.contains_key(&id) {
                return Err(CoreError::NotFound {
                    kind: "node",
                    id: id.get(),
                });
            }
            if let Some(parent_id) = parent
                && !model.nodes.contains_key(&parent_id)
            {
                return Err(CoreError::NotFound {
                    kind: "parent node",
                    id: parent_id.get(),
                });
            }
            model
                .nodes
                .get_mut(&id)
                .expect("node existence checked")
                .parent = parent;
        }
        EditCommand::AddMesh(mesh) => {
            if model.meshes.contains_key(&mesh.id) {
                return Err(CoreError::AlreadyExists {
                    kind: "mesh",
                    id: mesh.id.get(),
                });
            }
            model.meshes.insert(mesh.id, mesh);
        }
        EditCommand::AddMeshVertex { mesh, vertex } => {
            let mesh = model.meshes.get_mut(&mesh).ok_or(CoreError::NotFound {
                kind: "mesh",
                id: mesh.get(),
            })?;
            if mesh.vertices.len() > u32::MAX as usize {
                return Err(CoreError::InvalidInput(
                    "mesh vertex index space is exhausted".into(),
                ));
            }
            mesh.vertices.push(vertex);
        }
        EditCommand::SetVertexPosition {
            mesh,
            vertex_index,
            position,
        } => {
            let mesh = model.meshes.get_mut(&mesh).ok_or(CoreError::NotFound {
                kind: "mesh",
                id: mesh.get(),
            })?;
            let vertex = mesh.vertices.get_mut(vertex_index).ok_or_else(|| {
                CoreError::InvalidInput(format!("vertex index {vertex_index} is out of range"))
            })?;
            vertex.position = position;
        }
        EditCommand::SetVertexUv {
            mesh,
            vertex_index,
            uv,
        } => {
            let mesh = model.meshes.get_mut(&mesh).ok_or(CoreError::NotFound {
                kind: "mesh",
                id: mesh.get(),
            })?;
            let vertex = mesh.vertices.get_mut(vertex_index).ok_or_else(|| {
                CoreError::InvalidInput(format!("vertex index {vertex_index} is out of range"))
            })?;
            vertex.uv = uv;
        }
        EditCommand::SetNodeMesh { node, mesh } => {
            if let Some(mesh_id) = mesh
                && !model.meshes.contains_key(&mesh_id)
            {
                return Err(CoreError::NotFound {
                    kind: "mesh",
                    id: mesh_id.get(),
                });
            }
            let node = model.nodes.get_mut(&node).ok_or(CoreError::NotFound {
                kind: "node",
                id: node.get(),
            })?;
            node.mesh = mesh;
        }
        EditCommand::RemoveMesh(id) => {
            if model.nodes.values().any(|node| node.mesh == Some(id)) {
                return Err(CoreError::InUse {
                    kind: "mesh",
                    id: id.get(),
                });
            }
            model.meshes.remove(&id).ok_or(CoreError::NotFound {
                kind: "mesh",
                id: id.get(),
            })?;
        }
        EditCommand::AddTexture(texture) => {
            if model.textures.contains_key(&texture.id) {
                return Err(CoreError::AlreadyExists {
                    kind: "texture",
                    id: texture.id.get(),
                });
            }
            model.textures.insert(texture.id, texture);
        }
        EditCommand::RemoveTexture(id) => {
            if model.nodes.values().any(|node| node.texture == Some(id)) {
                return Err(CoreError::InUse {
                    kind: "texture",
                    id: id.get(),
                });
            }
            model.textures.remove(&id).ok_or(CoreError::NotFound {
                kind: "texture",
                id: id.get(),
            })?;
        }
        EditCommand::AddParameter(parameter) => {
            if model.parameters.contains_key(&parameter.id) {
                return Err(CoreError::AlreadyExists {
                    kind: "parameter",
                    id: parameter.id.get(),
                });
            }
            model.parameters.insert(parameter.id, parameter);
        }
        EditCommand::RemoveParameter(id) => {
            model.parameters.remove(&id).ok_or(CoreError::NotFound {
                kind: "parameter",
                id: id.get(),
            })?;
        }
        EditCommand::SetKeyform { parameter, keyform } => {
            let entry = model
                .parameters
                .get_mut(&parameter)
                .ok_or(CoreError::NotFound {
                    kind: "parameter",
                    id: parameter.get(),
                })?;
            if let Some(existing) = entry
                .keyforms
                .iter_mut()
                .find(|existing| existing.id == keyform.id)
            {
                *existing = keyform;
            } else {
                entry.keyforms.push(keyform);
            }
        }
        EditCommand::RemoveKeyform { parameter, keyform } => {
            let entry = model
                .parameters
                .get_mut(&parameter)
                .ok_or(CoreError::NotFound {
                    kind: "parameter",
                    id: parameter.get(),
                })?;
            let index = entry
                .keyforms
                .iter()
                .position(|existing| existing.id == keyform)
                .ok_or(CoreError::NotFound {
                    kind: "keyform",
                    id: keyform.get(),
                })?;
            entry.keyforms.remove(index);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document() -> Document {
        Document::new(Model::new(ModelId::new(1), "sample")).unwrap()
    }

    fn transaction(revision: u64, commands: Vec<EditCommand>) -> EditTransaction {
        EditTransaction {
            label: "edit".into(),
            expected_revision: revision,
            commands,
        }
    }

    #[test]
    fn transaction_is_atomic_on_failure() {
        let mut document = document();
        let result = document.execute(transaction(
            0,
            vec![
                EditCommand::AddNode(Node {
                    id: NodeId::new(2),
                    name: "root".into(),
                    parent: None,
                    part: None,
                    mesh: None,
                    texture: None,
                }),
                EditCommand::RemoveNode(NodeId::new(99)),
            ],
        ));
        assert!(result.is_err());
        assert!(document.model().nodes.is_empty());
        assert_eq!(document.revision(), 0);
    }

    #[test]
    fn revision_conflicts_and_history_tracks_undo_redo() {
        let mut document = document();
        let add = EditCommand::AddNode(Node {
            id: NodeId::new(2),
            name: "root".into(),
            parent: None,
            part: None,
            mesh: None,
            texture: None,
        });
        document.execute(transaction(0, vec![add])).unwrap();
        assert!(matches!(
            document.execute(transaction(
                0,
                vec![EditCommand::RenameNode {
                    id: NodeId::new(2),
                    name: "new".into()
                }]
            )),
            Err(CoreError::RevisionConflict { .. })
        ));
        document.undo().unwrap();
        assert!(document.model().nodes.is_empty());
        document.redo().unwrap();
        assert!(document.model().nodes.contains_key(&NodeId::new(2)));
    }

    fn triangle_mesh(id: MeshId) -> Mesh {
        Mesh {
            id,
            vertices: vec![
                Vertex {
                    position: Vec2 { x: 0.0, y: 0.0 },
                    uv: Vec2 { x: 0.0, y: 0.0 },
                },
                Vertex {
                    position: Vec2 { x: 1.0, y: 0.0 },
                    uv: Vec2 { x: 1.0, y: 0.0 },
                },
                Vertex {
                    position: Vec2 { x: 0.0, y: 1.0 },
                    uv: Vec2 { x: 0.0, y: 1.0 },
                },
            ],
            indices: vec![0, 1, 2],
        }
    }

    #[test]
    fn mesh_vertex_edits_are_atomic_and_undoable() {
        let mut document = document();
        let mesh_id = MeshId::new(2);
        document
            .execute(transaction(
                0,
                vec![EditCommand::AddMesh(triangle_mesh(mesh_id))],
            ))
            .unwrap();
        let original = document.model().meshes[&mesh_id].clone();
        document
            .execute(transaction(
                1,
                vec![
                    EditCommand::AddMeshVertex {
                        mesh: mesh_id,
                        vertex: Vertex {
                            position: Vec2 { x: 2.0, y: 2.0 },
                            uv: Vec2 { x: 0.5, y: 0.5 },
                        },
                    },
                    EditCommand::SetVertexPosition {
                        mesh: mesh_id,
                        vertex_index: 0,
                        position: Vec2 { x: -1.0, y: -1.0 },
                    },
                    EditCommand::SetVertexUv {
                        mesh: mesh_id,
                        vertex_index: 0,
                        uv: Vec2 { x: 0.25, y: 0.75 },
                    },
                ],
            ))
            .unwrap();
        let edited = document.model().meshes[&mesh_id].clone();
        assert_eq!(edited.vertices.len(), 4);
        assert_eq!(edited.indices, vec![0, 1, 2]);
        assert_eq!(edited.vertices[0].position, Vec2 { x: -1.0, y: -1.0 });
        assert_eq!(edited.vertices[0].uv, Vec2 { x: 0.25, y: 0.75 });

        document.undo().unwrap();
        assert_eq!(document.model().meshes[&mesh_id], original);
        document.redo().unwrap();
        assert_eq!(document.model().meshes[&mesh_id], edited);
    }

    #[test]
    fn rejects_invalid_mesh_vertex_edits_without_partial_changes() {
        let mut document = document();
        let mesh_id = MeshId::new(2);
        document
            .execute(transaction(
                0,
                vec![EditCommand::AddMesh(triangle_mesh(mesh_id))],
            ))
            .unwrap();
        let original = document.model().meshes[&mesh_id].clone();
        let result = document.execute(transaction(
            1,
            vec![
                EditCommand::SetVertexPosition {
                    mesh: mesh_id,
                    vertex_index: 0,
                    position: Vec2 { x: 5.0, y: 5.0 },
                },
                EditCommand::SetVertexUv {
                    mesh: mesh_id,
                    vertex_index: 99,
                    uv: Vec2 { x: 0.0, y: 0.0 },
                },
            ],
        ));
        assert!(matches!(result, Err(CoreError::InvalidInput(_))));
        assert_eq!(document.model().meshes[&mesh_id], original);
        assert_eq!(document.revision(), 1);

        let result = document.execute(transaction(
            1,
            vec![EditCommand::SetVertexPosition {
                mesh: mesh_id,
                vertex_index: 0,
                position: Vec2 {
                    x: f32::NAN,
                    y: 0.0,
                },
            }],
        ));
        assert!(matches!(result, Err(CoreError::InvalidInput(_))));
        assert_eq!(document.model().meshes[&mesh_id], original);
    }

    #[test]
    fn assigns_and_unassigns_meshes_through_commands() {
        let mut document = document();
        let mesh_id = MeshId::new(2);
        let node_id = NodeId::new(3);
        document
            .execute(transaction(
                0,
                vec![
                    EditCommand::AddMesh(triangle_mesh(mesh_id)),
                    EditCommand::AddNode(Node {
                        id: node_id,
                        name: "mesh node".into(),
                        parent: None,
                        part: None,
                        mesh: None,
                        texture: None,
                    }),
                ],
            ))
            .unwrap();
        document
            .execute(transaction(
                1,
                vec![EditCommand::SetNodeMesh {
                    node: node_id,
                    mesh: Some(mesh_id),
                }],
            ))
            .unwrap();
        assert_eq!(document.model().nodes[&node_id].mesh, Some(mesh_id));
        assert!(matches!(
            document.execute(transaction(2, vec![EditCommand::RemoveMesh(mesh_id)])),
            Err(CoreError::InUse { kind: "mesh", .. })
        ));
        document
            .execute(transaction(
                2,
                vec![
                    EditCommand::SetNodeMesh {
                        node: node_id,
                        mesh: None,
                    },
                    EditCommand::RemoveMesh(mesh_id),
                ],
            ))
            .unwrap();
        assert!(document.model().meshes.is_empty());
        assert_eq!(document.model().nodes[&node_id].mesh, None);
    }

    #[test]
    fn validates_hierarchy_cycles() {
        let mut model = Model::new(ModelId::new(1), "sample");
        model.nodes.insert(
            NodeId::new(2),
            Node {
                id: NodeId::new(2),
                name: "a".into(),
                parent: Some(NodeId::new(3)),
                part: None,
                mesh: None,
                texture: None,
            },
        );
        model.nodes.insert(
            NodeId::new(3),
            Node {
                id: NodeId::new(3),
                name: "b".into(),
                parent: Some(NodeId::new(2)),
                part: None,
                mesh: None,
                texture: None,
            },
        );
        assert!(model.validate().is_err());
    }
}
