// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! With the `occt` feature: build OpenCascade (static, from the sources
//! `occt-sys` vendors), compile the C++ reader against it and link it.
//!
//! `XTRUDE_OCCT_DIR` names an OpenCascade that an earlier build installed
//! (the `lib` and `include` folders of `target/OCCT`, kept elsewhere):
//! the build links it instead of building OpenCascade again. CI caches
//! one that way.

fn main() {
    #[cfg(feature = "occt")]
    occt::build();
}

#[cfg(feature = "occt")]
mod occt {
    use std::path::PathBuf;

    /// The OpenCascade toolkits the reader uses, dependents before their
    /// dependencies (static linking resolves left to right).
    const LIBS: &[&str] = &[
        "TKDESTEP",
        "TKXCAF",
        "TKVCAF",
        // XCAF's visual attributes pull in the presentation toolkits,
        // built without any windowing or font library.
        "TKV3d",
        "TKHLR",
        "TKService",
        "TKCAF",
        "TKLCAF",
        "TKCDF",
        "TKXSBase",
        "TKDE",
        "TKMesh",
        "TKShHealing",
        "TKBool",
        "TKBO",
        "TKPrim",
        "TKTopAlgo",
        "TKGeomAlgo",
        "TKBRep",
        "TKGeomBase",
        "TKG3d",
        "TKG2d",
        "TKMath",
        "TKernel",
    ];

    /// The OpenCascade installation to build against: `XTRUDE_OCCT_DIR`
    /// when set, else one built from `occt-sys`' sources.
    fn occt_root() -> PathBuf {
        println!("cargo:rerun-if-env-changed=XTRUDE_OCCT_DIR");
        if let Some(dir) = std::env::var_os("XTRUDE_OCCT_DIR") {
            let root = PathBuf::from(dir);
            for sub in ["lib", "include"] {
                assert!(
                    root.join(sub).is_dir(),
                    "XTRUDE_OCCT_DIR={} has no {sub} folder",
                    root.display()
                );
            }
            return root;
        }
        occt_sys::build_occt();
        occt_sys::occt_path()
    }

    pub fn build() {
        let root = occt_root();
        println!(
            "cargo:rustc-link-search=native={}",
            root.join("lib").display()
        );
        cxx_build::bridge("src/occt.rs")
            .file("src/reader.cpp")
            .std("c++17")
            .include(root.join("include"))
            .compile("xtrude-step-reader");
        for lib in LIBS {
            println!("cargo:rustc-link-lib=static={lib}");
        }
        for f in ["src/occt.rs", "src/reader.cpp", "src/reader.hpp"] {
            println!("cargo:rerun-if-changed={f}");
        }
    }
}
