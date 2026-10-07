//! Generates bindings to the system LibRaw and links it.
//!
//! With `LIBRAW_STATIC=1` LibRaw (and the libjpeg it uses) are linked statically, so a
//! packaged binary does not depend on the distribution's LibRaw version. Its other
//! dependencies (LittleCMS, zlib, OpenMP, libstdc++) stay shared: they have the same package
//! names across Debian and Ubuntu releases.

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=LIBRAW_STATIC");
    let static_link = env::var_os("LIBRAW_STATIC").is_some_and(|v| v != "0");

    let library = pkg_config::Config::new()
        .atleast_version("0.20")
        .cargo_metadata(!static_link)
        .probe("libraw_r")
        .expect("LibRaw (libraw_r) was not found by pkg-config; install the LibRaw development package");

    if static_link {
        // pkg-config leaves system directories out of the link paths; the archive is in libdir.
        if let Ok(libdir) = pkg_config::get_variable("libraw_r", "libdir") {
            println!("cargo:rustc-link-search=native={libdir}");
        }
        for dir in &library.link_paths {
            println!("cargo:rustc-link-search=native={}", dir.display());
        }
        println!("cargo:rustc-link-lib=static=raw_r");
        println!("cargo:rustc-link-lib=static=jpeg");
        println!("cargo:rustc-link-lib=dylib=lcms2");
        println!("cargo:rustc-link-lib=dylib=z");
        println!("cargo:rustc-link-lib=dylib=stdc++");
        println!("cargo:rustc-link-lib=dylib=gomp");
    }

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
