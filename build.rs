use slint_build::CompilerConfiguration;
use static_files::resource_dir;

fn main() {
    println!("cargo:rerun-if-changed=./static");
    resource_dir("./static").build().expect("Failed to build resource directory");

    slint_build::compile_with_config(
        "ui/app-window.slint",
        CompilerConfiguration::new().with_style("fluent".to_string())
    ).expect("Slint build failed");
}
