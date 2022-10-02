use std::env;
use std::path::PathBuf;

#[derive(Debug)]
struct AssumeU64;

impl bindgen::callbacks::ParseCallbacks for AssumeU64 {
    fn int_macro(&self, _name: &str, _value: i64) -> Option<bindgen::callbacks::IntKind> {
        Some(bindgen::callbacks::IntKind::U64)
    }
}

fn main() {
    println!("cargo:rerun-if-changed=wrapper.h");

    // The bindgen::Builder is the main entry point
    // to bindgen, and lets you build up options for
    // the resulting bindings.
    let bindings = bindgen::Builder::default()
        .parse_callbacks(Box::new(AssumeU64))
        // The input header we would like to generate
        // bindings for.
        .header("wrapper.h")
        // Finish the builder and generate the bindings.
        .generate()
        // Unwrap the Result and panic on failure.
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("framebuffer_raw.rs"))
        .expect("Couldn't write bindings!");
}
