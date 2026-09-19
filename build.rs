use std::fs::File;
use std::io::{self, BufRead, Write};

fn main() {
    println!("cargo:rerun-if-changed=Cargo.toml");
    let mut file = File::create("./src/version.rs").unwrap();
    for line in io::BufReader::new(File::open("Cargo.toml").unwrap()).lines().flatten() {
        let ab: Vec<&str> = line.split('=').map(|x| x.trim()).collect();
        if ab.len() == 2 && ab[0] == "version" {
            file.write_all(format!("pub const VERSION: &str = {};\n", ab[1]).as_bytes())
                .ok();
            break;
        }
    }
    let build_date = format!("{}", chrono::Local::now().format("%Y-%m-%d %H:%M"));
    file.write_all(
        format!("#[allow(dead_code)]\npub const BUILD_DATE: &str = \"{build_date}\";\n").as_bytes(),
    )
    .ok();
    file.sync_all().ok();
}
