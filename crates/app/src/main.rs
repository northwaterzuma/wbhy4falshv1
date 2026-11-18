use mdm_app::RustCatchDesktopApp;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RustCatch Download Manager")
            .with_inner_size([1024.0, 640.0])
            .with_min_inner_size([720.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native(
        "RustCatch",
        native_options,
        Box::new(|cc| {
            mdm_app::fonts::install_chinese_font(&cc.egui_ctx);
            let root = mdm_app::data_dir().ok_or_else(|| std::io::Error::other("无法找到 RustCatch 数据目录"))?;
            let mut app = RustCatchDesktopApp::open(&root)?;
            app.start_browser_bridge(&root, cc.egui_ctx.clone());
            Ok(Box::new(app))
        }),
    )
}
