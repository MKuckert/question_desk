//! Compiles the Slint UI. The widget style is set explicitly: since Slint 1.16 the default is
//! Fluent everywhere, and `native` restores the platform-adaptive choice (Cupertino on macOS,
//! Fluent on Windows, Qt or Fluent on Linux). `QUESTION_DESK_STYLE` overrides it, e.g. `qt`
//! together with `--features qt`, or `fluent` / `cupertino` to compare.

fn main() {
    println!("cargo:rerun-if-env-changed=QUESTION_DESK_STYLE");
    let style = std::env::var("QUESTION_DESK_STYLE").unwrap_or_else(|_| {
        if std::env::var_os("CARGO_FEATURE_QT").is_some() {
            "qt".into()
        } else {
            "native".into()
        }
    });
    let config = slint_build::CompilerConfiguration::new().with_style(style);
    slint_build::compile_with_config("src/ui/app.slint", config).expect("Slint UI compiles");
}
