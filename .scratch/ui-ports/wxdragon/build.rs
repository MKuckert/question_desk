//! Embeds the Windows application manifest (Common Controls 6, per-monitor DPI awareness)
//! into the executables. Nothing to do on other platforms.

fn main() {
    println!("cargo::rerun-if-changed=assets/windows/question-desk.rc");
    println!("cargo::rerun-if-changed=assets/windows/question-desk.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/windows/question-desk.rc", embed_resource::NONE)
            .manifest_required()
            .expect("embedding the Windows manifest");
    }
}
