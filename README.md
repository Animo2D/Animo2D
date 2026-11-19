# Animo2D

## Core foundation

- ~~Cargo workspace with all planned crates~~
- ~~Core model, parts, node hierarchy, mesh, vertex, UV, and texture references~~
- ~~Stable typed IDs, parameters, and keyforms~~
- ~~Validated edit commands, revision conflict detection, and atomic transactions~~
- ~~Undo/redo and edit history~~
- ~~Versioned JSON model save/load through `animo-format`~~
- ~~Core error types and validation~~
- ~~Mesh vertex addition, position/UV editing, and node mesh assignment~~
- ~~Triangle mesh creation and viewport preview~~

`editor` provides a native `eframe/egui` shell over the shared Core edit API. Run it with `cargo run -p animo-editor`. Use **File > New/Open/Save/Save As** to manage `.animo2d` JSON models; unsaved edits prompt before New/Open. Add a node, then use **Add triangle mesh** in the Inspector to preview it. The viewport supports static triangle previews and vertex selection/dragging; each drag is a single undoable edit. Use the mouse wheel to zoom around the cursor and the middle mouse button to pan. PSD import and animation controls are not implemented yet. `rigging`, `physics`, `ai`, and `script` remain buildable placeholders for later implementation.
