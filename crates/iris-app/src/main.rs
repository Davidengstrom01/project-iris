//! Project Iris: a small, fast RAW photo editor.
//!
//!   iris [PHOTO]

mod action;
mod app;
mod crop_tool;
mod curve_editor;
mod dialogs;
mod document;
mod mask_editor;
mod panels;
mod retouch_editor;
mod session;
mod settings;
mod texture;
mod theme;
#[cfg(test)]
mod ui_tests;
mod view;
mod widgets;

use std::path::PathBuf;

fn options(renderer: eframe::Renderer) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Project Iris")
            .with_app_id("project-iris")
            .with_inner_size([1500.0, 950.0])
            .with_min_inner_size([800.0, 500.0])
            .with_drag_and_drop(true)
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../../../packaging/project-iris.png"))
                    .unwrap_or_default(),
            ),
        renderer,
        persist_window: true,
        ..Default::default()
    }
}

fn run(renderer: eframe::Renderer, open: Option<PathBuf>) -> eframe::Result {
    eframe::run_native("iris", options(renderer), Box::new(move |cc| Ok(Box::new(app::IrisApp::new(cc, open)))))
}

fn main() -> eframe::Result {
    let open = std::env::args_os().nth(1).map(PathBuf::from);
    // The rendering threads get the same roomy stacks as the session's workers.
    if let Err(e) = rayon::ThreadPoolBuilder::new().stack_size(session::WORKER_STACK).build_global() {
        eprintln!("iris: cannot configure the rendering threads: {e}");
    }
    // wgpu (Vulkan, or its OpenGL backend); plain OpenGL if that cannot start, so a
    // dedicated GPU is never required.
    match run(eframe::Renderer::Wgpu, open.clone()) {
        Err(e) => {
            eprintln!("iris: the wgpu renderer failed ({e}); falling back to OpenGL");
            run(eframe::Renderer::Glow, open)
        }
        ok => ok,
    }
}
