use std::{
    fs,
    path::{Path, PathBuf},
};
fn walk(root: &Path, path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_dir() {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_name() != ".git" && entry.file_name() != "__pycache__" {
                walk(root, &entry.path(), files);
            }
        }
    } else if path.is_file() {
        files.push(path.strip_prefix(root).unwrap().to_owned());
    }
}
fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let mut files = Vec::new();
    for name in [
        "src",
        "assets",
        "tests",
        "docs",
        "tools",
        "Cargo.toml",
        "Cargo.lock",
        "build.rs",
        "README.md",
        "LICENSE",
        "THIRD_PARTY.md",
        "CHANGELOG.md",
        "start.sh",
    ] {
        let p = root.join(name);
        if p.exists() {
            walk(&root, &p, &mut files);
        }
    }
    files.sort();
    let mut output = String::from("pub const SOURCE_FILES: &[(&str, &[u8])] = &[\n");
    for file in files {
        println!("cargo:rerun-if-changed={}", file.display());
        output.push_str(&format!(
            "({:?}, include_bytes!({:?})),\n",
            file.to_string_lossy().replace('\\', "/"),
            root.join(file).to_string_lossy()
        ));
    }
    output.push_str("];\n");
    fs::write(
        PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("source_bundle.rs"),
        output,
    )
    .unwrap();
}
