#![forbid(unsafe_code)]

use animo_core::{
    EditCommand, EditTransaction, Mesh, MeshId, Model, ModelId, Node, NodeId, Vec2, Vertex,
};
use animo_editor::EditorSession;
use animo_render::MeshPreview;
use eframe::egui::{self, Align, Align2, Color32, FontId, Layout, Pos2, Rect, RichText, Stroke};
use std::path::PathBuf;

const VERTEX_HIT_RADIUS: f32 = 9.0;

#[derive(Clone, Copy)]
struct ViewTransform {
    center: Pos2,
    model_center: Vec2,
    scale: f32,
}

impl ViewTransform {
    fn fit(bounds: animo_render::PreviewBounds, canvas: Rect) -> Self {
        let width = (bounds.max.x - bounds.min.x).max(0.001);
        let height = (bounds.max.y - bounds.min.y).max(0.001);
        let scale = (canvas.width().max(1.0) / width)
            .min(canvas.height().max(1.0) / height)
            .min(500.0);
        Self {
            center: canvas.center(),
            model_center: Vec2 {
                x: (bounds.min.x + bounds.max.x) * 0.5,
                y: (bounds.min.y + bounds.max.y) * 0.5,
            },
            scale,
        }
    }

    fn to_screen(self, point: Vec2) -> Pos2 {
        Pos2::new(
            self.center.x + (point.x - self.model_center.x) * self.scale,
            self.center.y - (point.y - self.model_center.y) * self.scale,
        )
    }

    fn to_model(self, point: Pos2) -> Vec2 {
        Vec2 {
            x: self.model_center.x + (point.x - self.center.x) / self.scale,
            y: self.model_center.y - (point.y - self.center.y) / self.scale,
        }
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
}

struct AnimoEditorApp {
    session: EditorSession,
    selected_node: Option<NodeId>,
    selected_vertex: Option<(MeshId, usize)>,
    vertex_drag: Option<VertexDrag>,
    next_node_number: u64,
    current_path: Option<PathBuf>,
    saved_model: Model,
    pending_action: Option<PendingAction>,
    status: Option<String>,
}

impl AnimoEditorApp {
    fn new() -> Self {
        let model = Model::new(ModelId::new(1), "Untitled Model");
        Self {
            session: EditorSession::new(model.clone()).expect("the initial model is valid"),
            selected_node: None,
            selected_vertex: None,
            vertex_drag: None,
            next_node_number: 1,
            current_path: None,
            saved_model: model,
            pending_action: None,
            status: None,
        }
    }

    fn is_dirty(&self) -> bool {
        self.session.document().model() != &self.saved_model
    }

    fn reset_session(&mut self, model: Model, path: Option<PathBuf>) {
        self.session = EditorSession::new(model.clone()).expect("loaded or new model is valid");
        self.selected_node = None;
        self.selected_vertex = None;
        self.vertex_drag = None;
        self.next_node_number = 1;
        self.current_path = path;
        self.saved_model = model;
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
        }
    }

    fn open_dialog(&mut self) {
        let path = rfd::FileDialog::new()
            .add_filter("Animo2D Model", &["animo2d"])
            .pick_file();
        let Some(path) = path else {
            return;
        };
        match animo_format::load_model(&path) {
            Ok(model) => match EditorSession::new(model.clone()) {
                Ok(session) => {
                    self.session = session;
                    self.selected_node = None;
                    self.selected_vertex = None;
                    self.vertex_drag = None;
                    self.next_node_number = 1;
                    self.current_path = Some(path);
                    self.saved_model = model;
                    self.pending_action = None;
                    self.status = None;
                }
                Err(error) => self.status = Some(error.to_string()),
            },
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
        match animo_format::save_document(&path, self.session.document()) {
            Ok(()) => {
                self.current_path = Some(path);
                self.saved_model = self.session.document().model().clone();
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
                let nodes: Vec<_> = self
                    .session
                    .document()
                    .model()
                    .nodes
                    .values()
                    .map(|node| (node.id, node.name.clone()))
                    .collect();
                if nodes.is_empty() {
                    ui.label("No nodes yet");
                }
                for (node_id, name) in nodes {
                    let selected = self.selected_node == Some(node_id);
                    if ui
                        .selectable_label(selected, format!("{}  #{}", name, node_id.get()))
                        .clicked()
                    {
                        self.selected_node = Some(node_id);
                        self.status = None;
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
                    .map(|mesh| (mesh_id, MeshPreview::from_mesh(mesh)))
            });

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

        egui::CentralPanel::default().show(context, |ui| {
            let available = ui.available_rect_before_wrap();
            let painter = ui.painter();
            painter.rect_filled(available, 4.0, Color32::from_gray(28));
            painter.rect_stroke(
                available,
                4.0,
                Stroke::new(1.0_f32, Color32::from_gray(70)),
                egui::StrokeKind::Inside,
            );
            let canvas = available.shrink(32.0);
            match &selected_preview {
                Some((_, Ok(preview))) if !preview.triangles.is_empty() => {
                    if let Some(bounds) = preview.bounds {
                        let width = (bounds.max.x - bounds.min.x).max(0.001);
                        let height = (bounds.max.y - bounds.min.y).max(0.001);
                        let scale = (canvas.width() / width)
                            .min(canvas.height() / height)
                            .min(500.0);
                        let center = canvas.center();
                        let model_center = egui::pos2(
                            (bounds.min.x + bounds.max.x) * 0.5,
                            (bounds.min.y + bounds.max.y) * 0.5,
                        );
                        let to_screen = |point: animo_render::PreviewPoint| {
                            egui::pos2(
                                center.x + (point.x - model_center.x) * scale,
                                center.y - (point.y - model_center.y) * scale,
                            )
                        };
                        for triangle in &preview.triangles {
                            let points = triangle.vertices.map(to_screen);
                            painter.add(egui::Shape::convex_polygon(
                                points.to_vec(),
                                Color32::from_rgba_unmultiplied(84, 145, 205, 120),
                                Stroke::new(2.0_f32, Color32::from_rgb(135, 190, 245)),
                            ));
                        }
                    }
                }
                Some((_, Ok(_))) => {
                    painter.text(
                        available.center(),
                        Align2::CENTER_CENTER,
                        "Selected mesh has no triangles",
                        FontId::proportional(16.0),
                        Color32::LIGHT_GRAY,
                    );
                }
                Some((_, Err(error))) => {
                    painter.text(
                        available.center(),
                        Align2::CENTER_CENTER,
                        error.to_string(),
                        FontId::proportional(16.0),
                        Color32::LIGHT_RED,
                    );
                }
                None => {
                    painter.text(
                        available.center() - egui::vec2(0.0, 14.0),
                        Align2::CENTER_CENTER,
                        "Viewport",
                        FontId::proportional(24.0),
                        Color32::WHITE,
                    );
                    painter.text(
                        available.center() + egui::vec2(0.0, 14.0),
                        Align2::CENTER_CENTER,
                        "Select a node with a mesh to preview it",
                        FontId::proportional(16.0),
                        Color32::LIGHT_GRAY,
                    );
                }
            }
            ui.allocate_rect(available, egui::Sense::hover());
        });

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
