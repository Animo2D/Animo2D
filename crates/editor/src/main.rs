#![forbid(unsafe_code)]

use animo_core::{
    EditCommand, EditTransaction, Mesh, MeshId, Model, ModelId, Node, NodeId, Vec2, Vertex,
};
use animo_editor::EditorSession;
use animo_render::MeshPreview;
use eframe::egui::{self, Align, Align2, Color32, FontId, Layout, RichText, Stroke};

struct AnimoEditorApp {
    session: EditorSession,
    selected_node: Option<NodeId>,
    next_node_id: u64,
    next_node_number: u64,
    status: Option<String>,
}

impl AnimoEditorApp {
    fn new() -> Self {
        Self {
            session: EditorSession::new(Model::new(ModelId::new(1), "Untitled Model"))
                .expect("the initial model is valid"),
            selected_node: None,
            next_node_id: 2,
            next_node_number: 1,
            status: None,
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
                self.next_node_id += 1;
                self.next_node_number += 1;
                self.status = None;
            }
            Err(error) => self.status = Some(error.to_string()),
        }
    }

    fn add_triangle_mesh(&mut self, node_id: NodeId) {
        let model = self.session.document().model();
        let next_id = {
            let mut model = model.clone();
            model.allocate_id().and_then(|mesh_id| {
                model
                    .allocate_id()
                    .map(|next_id| (mesh_id.get(), next_id.get()))
            })
        };
        let (mesh_raw_id, next_raw_id) = match next_id {
            Ok(ids) => ids,
            Err(error) => {
                self.status = Some(error.to_string());
                return;
            }
        };
        let mesh_id = MeshId::new(mesh_raw_id);
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
                self.next_node_id = self.next_node_id.max(next_raw_id);
                self.status = None;
            }
            Err(error) => self.status = Some(error.to_string()),
        }
    }

    fn undo(&mut self) {
        if let Err(error) = self.session.undo() {
            self.status = Some(error.to_string());
        } else {
            self.status = None;
        }
    }

    fn redo(&mut self) {
        if let Err(error) = self.session.redo() {
            self.status = Some(error.to_string());
        } else {
            self.status = None;
        }
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
                ui.heading("Animo2D");
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
