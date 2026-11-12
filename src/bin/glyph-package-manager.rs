fn main() {
    if let Err(error) = glyph_workspace::package_install::run() {
        eprintln!("Glyph package manager: {error:#}");
        std::process::exit(1);
    }
}
