#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod cli;
mod codegen;
mod http;
mod icons;
mod import;
mod dataset;
mod exec;
mod mcp;
mod mock;
mod oauth2;
mod model;
mod openapi;
mod pm;
mod run;
mod script;
mod sign;
mod storage;
mod theme;
mod vars;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    if let Some(code) = cli::dispatch(&args) {
        std::process::exit(code);
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Rustman")
            .with_app_id("rustman")
            .with_inner_size([1600.0, 1000.0])
            .with_min_inner_size([800.0, 500.0])
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../assets/logo/icon-256.png"))
                    .expect("bundled app icon is a valid PNG"),
            ),
        ..Default::default()
    };
    eframe::run_native("Rustman", options, Box::new(|cc| Ok(Box::new(app::RustmanApp::new(cc)))))
}
