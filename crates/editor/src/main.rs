#![forbid(unsafe_code)]

use animo_core::{
    EditCommand, EditTransaction, Mesh, MeshId, Model, ModelId, Node, NodeId, TextureId, Vec2,
    Vertex,
};
use animo_format::{ImportResult, ImportedAsset};
use std::collections::HashMap;
use animo_editor::EditorSession;
use animo_render::MeshPreview;
use eframe::egui::{self, Align, Align2, Color32, FontId, Layout, Pos2, Rect, RichText, Stroke};
use std::path::{Path, PathBuf};

const VERTEX_HIT_RADIUS: f32 = 9.0;

#[derive(Clone, Copy)]
struct ViewTransform {
    center: Pos2,
    model_center: Vec2,
    scale: f32,
    pan: Vec2,
}

impl ViewTransform {
    fn fit(bounds: animo_render::PreviewBounds, canvas: Rect, zoom: f32, pan: Vec2) -> Self {
        let width = (bounds.max.x - bounds.min.x).max(0.001);
        let height = (bounds.max.y - bounds.min.y).max(0.001);
        let fit_scale = (canvas.width().max(1.0) / width)
            .min(canvas.height().max(1.0) / height)
            .min(500.0);
        Self {
            center: canvas.center(),
            model_center: Vec2 {
                x: (bounds.min.x + bounds.max.x) * 0.5,
                y: (bounds.min.y + bounds.max.y) * 0.5,
            },
            scale: fit_scale * zoom,
            pan,
        }
    }

    fn to_screen(self, point: Vec2) -> Pos2 {
        Pos2::new(
            self.center.x + self.pan.x + (point.x - self.model_center.x) * self.scale,
            self.center.y + self.pan.y - (point.y - self.model_center.y) * self.scale,
        )
    }

    fn to_model(self, point: Pos2) -> Vec2 {
        Vec2 {
            x: self.model_center.x + (point.x - self.center.x - self.pan.x) / self.scale,
            y: self.model_center.y - (point.y - self.center.y - self.pan.y) / self.scale,
        }
    }
}

#[derive(Clone, Copy)]
struct ViewportCamera {
    mesh: MeshId,
    bounds: animo_render::PreviewBounds,
    zoom: f32,
    pan: Vec2,
}

impl ViewportCamera {
    fn new(mesh: MeshId, bounds: animo_render::PreviewBounds) -> Self {
        Self {
            mesh,
            bounds,
            zoom: 1.0,
            pan: Vec2 { x: 0.0, y: 0.0 },
        }
    }

    fn transform(self, canvas: Rect) -> ViewTransform {
        ViewTransform::fit(self.bounds, canvas, self.zoom, self.pan)
    }

    fn zoom_at(&mut self, canvas: Rect, pointer: Pos2, factor: f32) {
        let before = self.transform(canvas).to_model(pointer);
        self.zoom = (self.zoom * factor).clamp(0.1, 20.0);
        let after = self.transform(canvas).to_screen(before);
        self.pan.x += pointer.x - after.x;
        self.pan.y += pointer.y - after.y;
    }

    fn pan_by(&mut self, delta: egui::Vec2) {
        self.pan.x += delta.x;
        self.pan.y += delta.y;
    }
}

fn nearest_vertex(
    vertices: &[Vec2],
    transform: ViewTransform,
    pointer: Pos2,
    max_distance: f32,
) -> Option<usize> {
    vertices
        .iter()
        .enumerate()
        .map(|(index, vertex)| (index, transform.to_screen(*vertex).distance(pointer)))
        .filter(|(_, distance)| *distance <= max_distance)
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(index, _)| index)
}

#[derive(Clone, Copy)]
struct VertexDrag {
    mesh: MeshId,
    vertex_index: usize,
    start_revision: u64,
    original: Vec2,
    position: Vec2,
}

#[derive(Clone, Copy)]
enum PendingAction {
    New,
    Open,
    ImportPsd,
}

struct AssetCache {
    assets: Vec<ImportedAsset>,
    source_root: Option<PathBuf>,
    textures: HashMap<TextureId, egui::TextureHandle>,
}

struct AnimoEditorApp {
    session: EditorSession,
    asset_cache: AssetCache,
    selected_node: Option<NodeId>,
    selected_vertex: Option<(MeshId, usize)>,
    vertex_drag: Option<VertexDrag>,
    viewport_camera: Option<ViewportCamera>,
    middle_pan_active: bool,
    next_node_number: u64,
    current_path: Option<PathBuf>,
    saved_model: Model,
    force_dirty: bool,
    import_warnings: Vec<String>,
    pending_action: Option<PendingAction>,
    status: Option<String>,
}

impl AnimoEditorApp {
    fn new() -> Self {
        let model = Model::new(ModelId::new(1), "Untitled Model");
        Self {
            session: EditorSession::new(model.clone()).expect("the initial model is valid"),
            asset_cache: AssetCache {
                assets: Vec::new(),
                source_root: None,
                textures: HashMap::new(),
            },
            selected_node: None,
            selected_vertex: None,
            vertex_drag: None,
            viewport_camera: None,
            middle_pan_active: false,
            next_node_number: 1,
            current_path: None,
            saved_model: model,
            force_dirty: false,
            pending_action: None,
            status: None,
        }
    }

    fn is_dirty(&self) -> bool {
        self.force_dirty || self.session.document().model() != &self.saved_model
    }

    fn reset_session(&mut self, model: Model, path: Option<PathBuf>) {
        self.session = EditorSession::new(model.clone()).expect("loaded or new model is valid");
        self.asset_cache = AssetCache {
            assets: Vec::new(),
            source_root: None,
            textures: HashMap::new(),
        };
        self.selected_node = None;
        self.selected_vertex = None;
        self.vertex_drag = None;
        self.viewport_camera = None;
        self.middle_pan_active = false;
        self.next_node_number = 1;
        self.current_path = path;
        self.saved_model = model;
        self.force_dirty = false;
        self.pending_action = None;
        self.status = None;
    }

    fn request_action(&mut self, action: PendingAction) {
        if self.is_dirty() {
            self.pending_action = Some(action);
        } else {
            self.perform_action(action);
        }
    }

    fn perform_action(&mut self, action: PendingAction) {
        match action {
            PendingAction::New => {
                self.reset_session(Model::new(ModelId::new(1), "Untitled Model"), None);
            }
            PendingAction::Open => self.open_dialog(),
            PendingAction::ImportPsd => self.import_psd_dialog(),
        }
    }

    fn open_dialog(&mut self) {
        let path = rfd::FileDialog::new()
            .add_filter("Animo2D Model", &["animo2d"])
            .pick_file();
        let Some(path) = path else {
            return;
        };
        match animo_format::load_project(&path) {
            Ok(project) => match EditorSession::new(project.model.clone()) {
                Ok(session) => {
                    self.session = session;
                    self.asset_cache = AssetCache {
                        assets: project.assets,
                        source_root: path.parent().map(Path::to_path_buf),
                        textures: HashMap::new(),
                    };
                    self.selected_node = None;
                    self.selected_vertex = None;
                    self.vertex_drag = None;
                    self.viewport_camera = None;
                    self.middle_pan_active = false;
                    self.next_node_number = 1;
                    self.current_path = Some(path);
                    self.saved_model = project.model;
                    self.force_dirty = false;
                    self.pending_action = None;
                    self.status = None;
                }
                Err(error) => self.status = Some(error.to_string()),
            },
            Err(error) => self.status = Some(error.to_string()),
        }
    }

    fn import_psd_dialog(&mut self) {
        let path = rfd::FileDialog::new()
            .add_filter("Photoshop document", &["psd"])
            .pick_file();
        let Some(path) = path else {
            return;
        };
        match animo_format::import_psd(&path) {
            Ok(imported) => self.install_import(imported),
            Err(error) => self.status = Some(error.to_string()),
        }
    }

    fn install_import(&mut self, imported: ImportResult) {
        match EditorSession::new(imported.model.clone()) {
            Ok(session) => {
                self.session = session;
                self.asset_cache = AssetCache {
                    assets: imported.assets,
                    source_root: None,
                    textures: HashMap::new(),
                };
                self.selected_node = None;
                self.selected_vertex = None;
                self.vertex_drag = None;
                self.viewport_camera = None;
                self.middle_pan_active = false;
                self.next_node_number = 1;
                self.current_path = None;
                self.saved_model = imported.model.clone();
                self.force_dirty = true;
                self.pending_action = None;
                self.status = if imported.warnings.is_empty() {
                    Some("PSD imported; save the model to keep its assets".into())
                } else {
                    Some(format!(
                        "PSD imported with {} warnings; save to keep its assets",
                        imported.warnings.len()
                    ))
                };
            }
            Err(error) => self.status = Some(error.to_string()),
        }
    }

    fn save(&mut self) -> bool {
        if let Some(path) = self.current_path.clone() {
            return self.save_to(path);
        }
        self.save_as()
    }

    fn save_as(&mut self) -> bool {
        let path = rfd::FileDialog::new()
            .add_filter("Animo2D Model", &["animo2d"])
            .set_file_name("Untitled.animo2d")
            .save_file();
        let Some(path) = path else {
            return false;
        };
        self.save_to(path)
    }

    fn save_to(&mut self, path: PathBuf) -> bool {
        match animo_format::save_document(
            &path,
            self.session.document(),
            &self.asset_cache.assets,
            self.asset_cache.source_root.as_deref(),
        ) {
            Ok(()) => {
                self.current_path = Some(path.clone());
                self.asset_cache.source_root = path.parent().map(Path::to_path_buf);
                self.saved_model = self.session.document().model().clone();
                self.force_dirty = false;
                self.status = None;
                true
            }
            Err(error) => {
                self.status = Some(error.to_string());
                false
            }
        }
    }

    fn add_node(&mut self) {
        let node_id = match self.session.document().model().clone().allocate_id() {
            Ok(id) => NodeId::new(id.get()),
            Err(error) => {
                self.status = Some(error.to_string());
                return;
            }
        };
        let transaction = EditTransaction {
            label: format!("Add Node {}", self.next_node_number),
            expected_revision: self.session.revision(),
            commands: vec![EditCommand::AddNode(Node {
                id: node_id,
                name: format!("Node {}", self.next_node_number),
                parent: None,
                part: None,
                mesh: None,
                texture: None,
                visible: true,
                opacity: 1.0,
                draw_order: 0,
            })],
        };
        match self.session.execute(transaction) {
            Ok(_) => {
                self.selected_node = Some(node_id);
                self.next_node_number += 1;
                self.status = None;
            }
            Err(error) => self.status = Some(error.to_string()),
        }
    }

    fn add_triangle_mesh(&mut self, node_id: NodeId) {
        let model = self.session.document().model();
        let mesh_id = match model.clone().allocate_id() {
            Ok(id) => MeshId::new(id.get()),
            Err(error) => {
                self.status = Some(error.to_string());
                return;
            }
        };
        let mesh = Mesh {
            id: mesh_id,
            vertices: vec![
                Vertex {
                    position: Vec2 { x: -1.0, y: -1.0 },
                    uv: Vec2 { x: 0.0, y: 0.0 },
                },
                Vertex {
                    position: Vec2 { x: 1.0, y: -1.0 },
                    uv: Vec2 { x: 1.0, y: 0.0 },
                },
                Vertex {
                    position: Vec2 { x: 0.0, y: 1.0 },
                    uv: Vec2 { x: 0.5, y: 1.0 },
                },
            ],
            indices: vec![0, 1, 2],
        };
        let transaction = EditTransaction {
            label: "Create triangle mesh".into(),
            expected_revision: self.session.revision(),
            commands: vec![
                EditCommand::AddMesh(mesh),
                EditCommand::SetNodeMesh {
                    node: node_id,
                    mesh: Some(mesh_id),
                },
            ],
        };
        match self.session.execute(transaction) {
            Ok(_) => {
                self.status = None;
            }
            Err(error) => self.status = Some(error.to_string()),
        }
    }

    fn undo(&mut self) {
        self.vertex_drag = None;
        if let Err(error) = self.session.undo() {
            self.status = Some(error.to_string());
        } else {
            self.status = None;
        }
    }

    fn redo(&mut self) {
        self.vertex_drag = None;
        if let Err(error) = self.session.redo() {
            self.status = Some(error.to_string());
        } else {
            self.status = None;
        }
    }

    fn clear_invalid_vertex_selection(&mut self) {
        let valid = self.selected_vertex.is_some_and(|(mesh_id, vertex_index)| {
            self.session
                .document()
                .model()
                .meshes
                .get(&mesh_id)
                .is_some_and(|mesh| vertex_index < mesh.vertices.len())
                && self
                    .selected_node
                    .and_then(|node_id| self.session.document().model().nodes.get(&node_id))
                    .is_some_and(|node| node.mesh == Some(mesh_id))
        });
        if !valid {
            self.selected_vertex = None;
            self.vertex_drag = None;
        }
    }

    fn texture_handle(
        &mut self,
        context: &egui::Context,
        texture_id: TextureId,
    ) -> Result<egui::TextureHandle, String> {
        if let Some(texture) = self.asset_cache.textures.get(&texture_id) {
            return Ok(texture.clone());
        }
        let texture_ref = self
            .session
            .document()
            .model()
            .textures
            .get(&texture_id)
            .ok_or_else(|| format!("Texture {} was not found", texture_id.get()))?;
        let bytes = if let Some(asset) = self
            .asset_cache
            .assets
            .iter()
            .find(|asset| asset.path == Path::new(&texture_ref.path))
        {
            asset.bytes.clone()
        } else if let Some(root) = &self.asset_cache.source_root {
            let relative = Path::new(&texture_ref.path);
            if relative.is_absolute()
                || relative
                    .components()
                    .any(|component| matches!(component, std::path::Component::ParentDir | std::path::Component::RootDir | std::path::Component::Prefix(_)))
            {
                return Err(format!("Unsafe texture path: {}", texture_ref.path));
            }
            std::fs::read(root.join(relative)).map_err(|error| error.to_string())?
        } else {
            return Err(format!("Texture asset is not loaded: {}", texture_ref.path));
        };
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().map_err(|error| error.to_string())?;
        let mut buffer = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buffer).map_err(|error| error.to_string())?;
        let pixels = &buffer[..info.buffer_size()];
        let rgba = match info.color_type {
            png::ColorType::Rgba => pixels.to_vec(),
            png::ColorType::Rgb => pixels
                .chunks_exact(3)
                .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
                .collect(),
            png::ColorType::GrayscaleAlpha => pixels
                .chunks_exact(2)
                .flat_map(|pixel| [pixel[0], pixel[0], pixel[0], pixel[1]])
                .collect(),
            png::ColorType::Grayscale => pixels
                .iter()
                .flat_map(|pixel| [*pixel, *pixel, *pixel, 255])
                .collect(),
            png::ColorType::Indexed => return Err("Indexed PNG was not expanded".into()),
        };
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [info.width as usize, info.height as usize],
            &rgba,
        );
        let texture = context.load_texture(
            texture_ref.path.clone(),
            image,
            egui::TextureOptions::LINEAR,
        );
        self.asset_cache.textures.insert(texture_id, texture.clone());
        Ok(texture)
    }

    fn ensure_viewport_camera(&mut self, mesh: MeshId, bounds: animo_render::PreviewBounds) {
        match &mut self.viewport_camera {
            Some(camera) if camera.mesh == mesh => camera.bounds = bounds,
            _ => {
                self.viewport_camera = Some(ViewportCamera::new(mesh, bounds));
                self.middle_pan_active = false;
            }
        }
    }

    fn finish_vertex_drag(&mut self, drag: VertexDrag) {
        if drag.position == drag.original {
            return;
        }
        if self.session.revision() != drag.start_revision {
            self.status = Some("The document changed during the vertex drag".into());
            return;
        }
        let transaction = EditTransaction {
            label: "Move mesh vertex".into(),
            expected_revision: self.session.revision(),
            commands: vec![EditCommand::SetVertexPosition {
                mesh: drag.mesh,
                vertex_index: drag.vertex_index,
                position: drag.position,
            }],
        };
        self.status = self
            .session
            .execute(transaction)
            .err()
            .map(|error| error.to_string());
    }

    fn clear_removed_selection(&mut self) {
        if self
            .selected_node
            .is_some_and(|node_id| !self.session.document().model().nodes.contains_key(&node_id))
        {
            self.selected_node = None;
        }
    }
}

fn combined_bounds(
    layers: &[(
        animo_render::TexturedDrawRecord,
        Mesh,
        MeshPreview,
        egui::TextureHandle,
    )],
) -> Option<animo_render::PreviewBounds> {
    let mut bounds: Option<animo_render::PreviewBounds> = None;
    for (_, _, preview, _) in layers {
        let Some(layer_bounds) = preview.bounds else {
            continue;
        };
        bounds = Some(match bounds {
            Some(mut bounds) => {
                bounds.min.x = bounds.min.x.min(layer_bounds.min.x);
                bounds.min.y = bounds.min.y.min(layer_bounds.min.y);
                bounds.max.x = bounds.max.x.max(layer_bounds.max.x);
                bounds.max.y = bounds.max.y.max(layer_bounds.max.y);
                bounds
            }
            None => layer_bounds,
        });
    }
    bounds
}

fn hierarchy_row(
    ui: &mut egui::Ui,
    nodes: &[(NodeId, String, Option<NodeId>, i32, bool)],
    node_id: NodeId,
    depth: usize,
    selected: Option<NodeId>,
) -> Option<NodeId> {
    let (_, name, _, _, visible) = nodes.iter().find(|node| node.0 == node_id)?;
    let label = format!("{}{}  #{}", "  ".repeat(depth), name, node_id.get());
    let response = ui.add_enabled(*visible, egui::SelectableLabel::new(selected == Some(node_id), label));
    let mut clicked = response.clicked().then_some(node_id);
    let mut children: Vec<_> = nodes
        .iter()
        .filter(|node| node.2 == Some(node_id))
        .map(|node| node.0)
        .collect();
    children.sort_by_key(|child| nodes.iter().find(|node| node.0 == *child).map(|node| node.3));
    for child in children {
        clicked = hierarchy_row(ui, nodes, child, depth + 1, selected).or(clicked);
    }
    clicked
}

impl eframe::App for AnimoEditorApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("toolbar").show(context, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New").clicked() {
                        ui.close_menu();
                        self.request_action(PendingAction::New);
                    }
                    if ui.button("Open...").clicked() {
                        ui.close_menu();
                        self.request_action(PendingAction::Open);
                    }
                    if ui.button("Import PSD...").clicked() {
                        ui.close_menu();
                        self.request_action(PendingAction::ImportPsd);
                    }
                    ui.separator();
                    if ui.button("Save").clicked() {
                        ui.close_menu();
                        self.save();
                    }
                    if ui.button("Save As...").clicked() {
                        ui.close_menu();
                        self.save_as();
                    }
                });
                ui.heading("Animo2D");
                ui.label(self.current_path.as_ref().map_or_else(
                    || "Untitled".to_string(),
                    |path| {
                        path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned()
                    },
                ));
                if self.is_dirty() {
                    ui.label(RichText::new("●").color(Color32::YELLOW));
                }
                ui.separator();
                if ui
                    .add_enabled(self.session.can_undo(), egui::Button::new("Undo"))
                    .clicked()
                {
                    self.undo();
                }
                if ui
                    .add_enabled(self.session.can_redo(), egui::Button::new("Redo"))
                    .clicked()
                {
                    self.redo();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(format!("Revision {}", self.session.revision()));
                });
            });
        });
        self.clear_removed_selection();
        self.clear_invalid_vertex_selection();

        let hierarchy_nodes: Vec<_> = self
            .session
            .document()
            .model()
            .nodes
            .values()
            .map(|node| (node.id, node.name.clone(), node.parent, node.draw_order, node.visible))
            .collect();
        egui::SidePanel::left("hierarchy")
            .resizable(true)
            .default_width(210.0)
            .show(context, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Hierarchy");
                    if ui.button("+ Node").clicked() {
                        self.add_node();
                    }
                });
                ui.separator();
                if hierarchy_nodes.is_empty() {
                    ui.label("No nodes yet");
                }
                let mut ordered = hierarchy_nodes;
                ordered.sort_by_key(|(_, _, _, order, _)| *order);
                let roots: Vec<_> = ordered
                    .iter()
                    .filter(|(_, _, parent, _, _)| parent.is_none())
                    .map(|(id, _, _, _, _)| *id)
                    .collect();
                for root in roots {
                    if let Some(selected) = hierarchy_row(ui, &ordered, root, 0, self.selected_node) {
                        if selected != self.selected_node {
                            self.selected_vertex = None;
                            self.vertex_drag = None;
                            self.viewport_camera = None;
                            self.middle_pan_active = false;
                            self.status = None;
                        }
                        self.selected_node = Some(selected);
                    }
                }
            });

        let selected_node_state = self.selected_node.and_then(|node_id| {
            self.session
                .document()
                .model()
                .nodes
                .get(&node_id)
                .map(|node| {
                    (
                        node_id,
                        node.name.clone(),
                        node.parent,
                        node.mesh,
                        node.texture,
                    )
                })
        });
        let selected_preview = selected_node_state
            .as_ref()
            .and_then(|(_, _, _, mesh_id, _)| *mesh_id)
            .and_then(|mesh_id| {
                self.session
                    .document()
                    .model()
                    .meshes
                    .get(&mesh_id)
                    .map(|mesh| {
                        (
                            mesh_id,
                            MeshPreview::from_mesh(mesh),
                            mesh.vertices
                                .iter()
                                .map(|vertex| vertex.position)
                                .collect::<Vec<_>>(),
                        )
                    })
            });
        let draw_records = animo_render::textured_draw_records(self.session.document().model());
        let mut textured_layers = Vec::new();
        for record in draw_records {
            let layer = self
                .session
                .document()
                .model()
                .meshes
                .get(&record.mesh)
                .and_then(|mesh| MeshPreview::from_mesh(mesh).ok().map(|preview| (mesh, preview)));
            let Some((mesh, preview)) = layer else {
                continue;
            };
            match self.texture_handle(context, record.texture) {
                Ok(texture) => textured_layers.push((record, mesh.clone(), preview, texture)),
                Err(error) => self.status = Some(error),
            }
        }
        let scene_bounds = combined_bounds(&textured_layers);

        egui::SidePanel::right("inspector")
            .resizable(true)
            .default_width(240.0)
            .show(context, |ui| {
                ui.heading("Inspector");
                ui.separator();
                if let Some((node_id, name, parent, mesh, texture)) = &selected_node_state {
                    ui.label(RichText::new(name).strong());
                    ui.label(format!("Node ID: {}", node_id.get()));
                    ui.label(format!(
                        "Parent: {}",
                        parent.map_or_else(|| "None".into(), |id| id.get().to_string())
                    ));
                    ui.label(format!(
                        "Mesh: {}",
                        mesh.map_or_else(|| "None".into(), |id| id.get().to_string())
                    ));
                    ui.label(format!(
                        "Texture: {}",
                        texture.map_or_else(|| "None".into(), |id| id.get().to_string())
                    ));
                    if let Some(node) = self.session.document().model().nodes.get(node_id) {
                        ui.label(format!("Visible: {}", node.visible));
                        ui.label(format!("Opacity: {:.2}", node.opacity));
                        ui.label(format!("Draw order: {}", node.draw_order));
                    }
                    if let Some((selected_mesh, vertex_index)) = self.selected_vertex
                        && Some(selected_mesh) == *mesh
                        && let Some((_, _, vertices)) = &selected_preview
                        && let Some(position) = vertices.get(vertex_index)
                    {
                        let position = self
                            .vertex_drag
                            .filter(|drag| {
                                drag.mesh == selected_mesh && drag.vertex_index == vertex_index
                            })
                            .map_or(*position, |drag| drag.position);
                        ui.label(format!("Vertex {vertex_index}"));
                        ui.label(format!("Position: {:.3}, {:.3}", position.x, position.y));
                    }
                    ui.separator();
                    let button_text = if mesh.is_some() {
                        "Replace with triangle"
                    } else {
                        "Add triangle mesh"
                    };
                    if ui.button(button_text).clicked() {
                        self.add_triangle_mesh(*node_id);
                    }
                } else if self.selected_node.is_some() {
                    ui.label("Selected node was removed");
                    self.selected_node = None;
                } else {
                    ui.label("Select a node to inspect it");
                }
            });

        let mut finished_drag = None;
        egui::CentralPanel::default().show(context, |ui| {
            let available = ui.available_rect_before_wrap();
            let response = ui.allocate_rect(available, egui::Sense::click_and_drag());
            let painter = ui.painter();
            painter.rect_filled(available, 4.0, Color32::from_gray(28));
            painter.rect_stroke(
                available,
                4.0,
                Stroke::new(1.0_f32, Color32::from_gray(70)),
                egui::StrokeKind::Inside,
            );
            let canvas = available.shrink(32.0);
            if let Some(bounds) = scene_bounds {
                let camera_mesh = selected_node_state
                    .as_ref()
                    .and_then(|(_, _, _, mesh, _)| *mesh)
                    .or_else(|| textured_layers.first().map(|(record, _, _, _)| record.mesh))
                    .expect("scene bounds require a mesh");
                self.ensure_viewport_camera(camera_mesh, bounds);
                let (middle_pressed, middle_down, middle_released, pointer_delta, scroll, pointer) =
                    ui.input(|input| {
                        (
                            input.pointer.button_pressed(egui::PointerButton::Middle),
                            input.pointer.button_down(egui::PointerButton::Middle),
                            input.pointer.button_released(egui::PointerButton::Middle),
                            input.pointer.delta(),
                            input.raw_scroll_delta,
                            input.pointer.interact_pos(),
                        )
                    });
                if response.hovered() && middle_pressed && self.vertex_drag.is_none() {
                    self.middle_pan_active = true;
                }
                if self.middle_pan_active {
                    if middle_down
                        && response.hovered()
                        && let Some(camera) = &mut self.viewport_camera
                    {
                        camera.pan_by(pointer_delta);
                    }
                    if middle_released || !middle_down {
                        self.middle_pan_active = false;
                    }
                }
                if response.hovered()
                    && !self.middle_pan_active
                    && self.vertex_drag.is_none()
                    && scroll.y != 0.0
                    && let Some(pointer) = pointer
                    && let Some(camera) = &mut self.viewport_camera
                {
                    camera.zoom_at(canvas, pointer, (scroll.y * 0.002).exp());
                }
                let transform = self
                    .viewport_camera
                    .expect("camera initialized for visible scene")
                    .transform(canvas);

                if let Some((mesh_id, Ok(_), vertices)) = &selected_preview {
                    let press_position = response
                        .drag_started()
                        .then(|| {
                            ui.input(|input| input.pointer.press_origin())
                                .or_else(|| response.interact_pointer_pos())
                        })
                        .flatten();
                    if let Some(pointer) = press_position {
                        let selected =
                            nearest_vertex(vertices, transform, pointer, VERTEX_HIT_RADIUS);
                        self.selected_vertex = selected.map(|index| (*mesh_id, index));
                        self.vertex_drag = selected.map(|vertex_index| VertexDrag {
                            mesh: *mesh_id,
                            vertex_index,
                            start_revision: self.session.revision(),
                            original: vertices[vertex_index],
                            position: vertices[vertex_index],
                        });
                    } else if response.clicked()
                        && let Some(pointer) = response.interact_pointer_pos()
                    {
                        self.selected_vertex =
                            nearest_vertex(vertices, transform, pointer, VERTEX_HIT_RADIUS)
                                .map(|index| (*mesh_id, index));
                    }
                    if response.dragged()
                        && let (Some(pointer), Some(drag)) =
                            (response.interact_pointer_pos(), self.vertex_drag.as_mut())
                        && drag.mesh == *mesh_id
                    {
                        drag.position = transform.to_model(pointer);
                    }
                    if response.drag_stopped() {
                        finished_drag = self.vertex_drag.take();
                    }
                }

                for (record, mesh, _, texture) in &textured_layers {
                    let mut egui_mesh = egui::Mesh::with_texture(texture.id());
                    for (vertex_index, vertex) in mesh.vertices.iter().enumerate() {
                        let position = self
                            .vertex_drag
                            .filter(|drag| {
                                drag.mesh == record.mesh && drag.vertex_index == vertex_index
                            })
                            .map_or(vertex.position, |drag| drag.position);
                        let mut color = Color32::WHITE;
                        color.a = (f32::from(color.a()) * record.opacity).round() as u8;
                        egui_mesh.vertices.push(egui::epaint::Vertex {
                            pos: transform.to_screen(position),
                            uv: egui::pos2(vertex.uv.x, vertex.uv.y),
                            color,
                        });
                    }
                    egui_mesh.indices.extend_from_slice(&mesh.indices);
                    painter.add(egui::Shape::mesh(egui_mesh));
                }

                if let Some((mesh_id, Ok(_), vertices)) = &selected_preview {
                    for (vertex_index, position) in vertices.iter().enumerate() {
                        let position = self
                            .vertex_drag
                            .filter(|drag| drag.mesh == *mesh_id && drag.vertex_index == vertex_index)
                            .map_or(*position, |drag| drag.position);
                        let screen = transform.to_screen(position);
                        let selected = self.selected_vertex == Some((*mesh_id, vertex_index));
                        painter.circle_filled(
                            screen,
                            if selected { 6.0 } else { 4.0 },
                            if selected { Color32::YELLOW } else { Color32::WHITE },
                        );
                        painter.circle_stroke(
                            screen,
                            if selected { 7.0 } else { 5.0 },
                            Stroke::new(1.0_f32, Color32::from_gray(30)),
                        );
                    }
                }
            } else if textured_layers.is_empty() {
                painter.text(
                    available.center(),
                    Align2::CENTER_CENTER,
                    "Import a layered PSD or add a node mesh",
                    FontId::proportional(16.0),
                    Color32::LIGHT_GRAY,
                );
            }
        });
        if let Some(drag) = finished_drag {
            self.finish_vertex_drag(drag);
        }

        if let Some(action) = self.pending_action {
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(context, |ui| {
                    ui.label("Save changes before continuing?");
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            if self.save() {
                                self.perform_action(action);
                            } else {
                                self.pending_action = None;
                            }
                        }
                        if ui.button("Discard").clicked() {
                            self.perform_action(action);
                        }
                        if ui.button("Cancel").clicked() {
                            self.pending_action = None;
                        }
                    });
                });
        }

        egui::TopBottomPanel::bottom("status").show(context, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{} nodes",
                    self.session.document().model().nodes.len()
                ));
                if let Some(status) = &self.status {
                    ui.separator();
                    ui.colored_label(Color32::LIGHT_RED, status);
                }
            });
        });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Animo2D",
        options,
        Box::new(|_creation_context| Ok(Box::new(AnimoEditorApp::new()))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_transform_round_trips_and_flips_screen_y() {
        let transform = ViewTransform::fit(
            animo_render::PreviewBounds {
                min: animo_render::PreviewPoint { x: -2.0, y: -1.0 },
                max: animo_render::PreviewPoint { x: 2.0, y: 1.0 },
            },
            Rect::from_min_max(Pos2::new(10.0, 20.0), Pos2::new(410.0, 220.0)),
            1.0,
            Vec2 { x: 0.0, y: 0.0 },
        );
        let point = Vec2 { x: 1.25, y: 0.5 };
        let screen = transform.to_screen(point);
        assert!(screen.y < transform.center.y);
        let restored = transform.to_model(screen);
        assert!((restored.x - point.x).abs() < 0.0001);
        assert!((restored.y - point.y).abs() < 0.0001);
    }

    #[test]
    fn nearest_vertex_uses_screen_space_tolerance() {
        let transform = ViewTransform::fit(
            animo_render::PreviewBounds {
                min: animo_render::PreviewPoint { x: -1.0, y: -1.0 },
                max: animo_render::PreviewPoint { x: 1.0, y: 1.0 },
            },
            Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(200.0, 200.0)),
            1.0,
            Vec2 { x: 0.0, y: 0.0 },
        );
        let vertices = [Vec2 { x: -1.0, y: 0.0 }, Vec2 { x: 1.0, y: 0.0 }];
        let pointer = transform.to_screen(vertices[0]) + egui::vec2(4.0, 0.0);
        assert_eq!(nearest_vertex(&vertices, transform, pointer, 8.0), Some(0));
        assert_eq!(nearest_vertex(&vertices, transform, pointer, 3.0), None);
    }

    #[test]
    fn degenerate_view_bounds_keep_a_finite_transform() {
        let transform = ViewTransform::fit(
            animo_render::PreviewBounds {
                min: animo_render::PreviewPoint { x: 2.0, y: 3.0 },
                max: animo_render::PreviewPoint { x: 2.0, y: 3.0 },
            },
            Rect::from_min_max(Pos2::new(10.0, 20.0), Pos2::new(10.0, 20.0)),
            1.0,
            Vec2 { x: 0.0, y: 0.0 },
        );
        let screen = transform.to_screen(Vec2 { x: 2.0, y: 3.0 });
        assert!(screen.x.is_finite());
        assert!(screen.y.is_finite());
        assert_eq!(transform.to_model(screen), Vec2 { x: 2.0, y: 3.0 });
    }

    #[test]
    fn zoom_keeps_the_pointer_anchor_fixed_and_clamps_scale() {
        let bounds = animo_render::PreviewBounds {
            min: animo_render::PreviewPoint { x: -2.0, y: -1.0 },
            max: animo_render::PreviewPoint { x: 2.0, y: 1.0 },
        };
        let canvas = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(400.0, 300.0));
        let pointer = Pos2::new(125.0, 86.0);
        let mut camera = ViewportCamera::new(MeshId::new(1), bounds);
        let anchored_model_point = camera.transform(canvas).to_model(pointer);
        camera.zoom_at(canvas, pointer, 1.8);
        let anchored_screen_point = camera.transform(canvas).to_screen(anchored_model_point);
        assert!((anchored_screen_point.x - pointer.x).abs() < 0.0001);
        assert!((anchored_screen_point.y - pointer.y).abs() < 0.0001);
        camera.zoom_at(canvas, pointer, 1000.0);
        assert_eq!(camera.zoom, 20.0);
        camera.zoom_at(canvas, pointer, 0.00001);
        assert_eq!(camera.zoom, 0.1);
    }

    #[test]
    fn pan_offsets_the_model_and_camera_starts_fit_to_view() {
        let bounds = animo_render::PreviewBounds {
            min: animo_render::PreviewPoint { x: -1.0, y: -1.0 },
            max: animo_render::PreviewPoint { x: 1.0, y: 1.0 },
        };
        let canvas = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(200.0, 200.0));
        let mut camera = ViewportCamera::new(MeshId::new(4), bounds);
        let origin = camera.transform(canvas).to_screen(Vec2 { x: 0.0, y: 0.0 });
        camera.pan_by(egui::vec2(30.0, -12.0));
        let panned = camera.transform(canvas).to_screen(Vec2 { x: 0.0, y: 0.0 });
        assert_eq!(panned, origin + egui::vec2(30.0, -12.0));
        assert_eq!(camera.zoom, 1.0);
        assert_eq!(camera.pan, Vec2 { x: 30.0, y: -12.0 });
    }

    #[test]
    fn save_snapshot_tracks_dirty_state_and_new_document_reset() {
        let mut app = AnimoEditorApp::new();
        assert!(!app.is_dirty());
        app.add_node();
        assert!(app.is_dirty());
        let saved = app.session.document().model().clone();
        app.saved_model = saved;
        assert!(!app.is_dirty());
        app.reset_session(Model::new(ModelId::new(1), "Untitled Model"), None);
        assert!(!app.is_dirty());
        assert!(app.session.document().model().nodes.is_empty());
        assert_eq!(app.next_node_number, 1);
        assert!(app.selected_node.is_none());
    }

    #[test]
    fn failed_session_replacement_keeps_current_document() {
        let mut app = AnimoEditorApp::new();
        app.add_node();
        let current_model = app.session.document().model().clone();
        let current_revision = app.session.revision();
        let invalid_json = br#"{"format_version":99,"model":{}}"#;
        let replacement = animo_format::decode(invalid_json).and_then(|model| {
            EditorSession::new(model).map_err(animo_format::FormatError::InvalidModel)
        });
        assert!(replacement.is_err());
        assert_eq!(app.session.document().model(), &current_model);
        assert_eq!(app.session.revision(), current_revision);
    }
}
