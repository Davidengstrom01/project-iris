use std::env;
use std::path::PathBuf;

fn main() {
    let library = pkg_config::Config::new()
        .atleast_version("0.20")
        .probe("libraw_r")
        .expect("LibRaw (libraw_r) was not found by pkg-config; install the LibRaw development package");

    let mut builder = bindgen::Builder::default()
        .header("wrapper.h")
        .allowlist_function("libraw_.*")
        .allowlist_type("libraw_.*")
        .allowlist_type("LibRaw_.*")
        .allowlist_var("LIBRAW_.*")
        .derive_default(true)
        .layout_tests(false)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));
    for path in &library.include_paths {
        builder = builder.clang_arg(format!("-I{}", path.display()));
    }
    let bindings = builder.generate().expect("Cannot generate LibRaw bindings");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings.write_to_file(out.join("bindings.rs")).expect("Cannot write LibRaw bindings");
}
