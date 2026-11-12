fn main() {
    if let Err(error) = glyph_workspace::browser::run() {
        eprintln!("Glyph Web: {error:#}");
        std::process::exit(1);
    }
}
