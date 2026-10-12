// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The packaging identity: the app id that names the window, the desktop
//! file and the other desktop integration files.

use std::path::{Path, PathBuf};

use xtrude_app::APP_ID;
use xtrude_model::VERSION;

/// The files under `packaging/linux/` that are not named after the app id.
const NOT_NAMED_AFTER_APP_ID: &[&str] = &["install.sh"];

fn linux_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/linux")
}

fn hicolor_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/icons/hicolor")
}

/// Every file under `dir`, as paths relative to it.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(d) = pending.pop() {
        for entry in std::fs::read_dir(&d).expect("a readable directory") {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path.strip_prefix(dir).unwrap().to_path_buf());
            }
        }
    }
    files.sort();
    files
}

/// The value of `key` in the desktop file's `[Desktop Entry]` group.
fn desktop_entry(key: &str) -> Option<String> {
    let path = linux_dir().join(format!("{APP_ID}.desktop"));
    let text = std::fs::read_to_string(&path).expect("the desktop file");
    text.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
        .map(str::to_owned)
}

#[test]
fn app_id_is_the_reverse_dns_of_the_project_domain() {
    assert_eq!(APP_ID, "org.xtrudecad.XtrudeCAD");
}

#[test]
fn every_linux_packaging_file_is_named_after_the_app_id() {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(linux_dir()).expect("packaging/linux/") {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        if !NOT_NAMED_AFTER_APP_ID.contains(&name.as_str()) {
            assert!(name.starts_with(&format!("{APP_ID}.")), "{name}");
        }
        names.push(name);
    }
    names.sort();
    assert!(names.contains(&format!("{APP_ID}.desktop")), "{names:?}");
    assert!(names.contains(&format!("{APP_ID}.mime.xml")), "{names:?}");
    assert!(
        names.contains(&format!("{APP_ID}.metainfo.xml")),
        "{names:?}"
    );
}

#[test]
fn every_hicolor_icon_is_named_after_the_app_id() {
    let files = files_under(&hicolor_dir());
    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy();
        assert!(
            name == format!("{APP_ID}.svg") || name == format!("{APP_ID}.png"),
            "{}",
            f.display()
        );
        assert_eq!(f.parent().and_then(Path::file_name), Some("apps".as_ref()));
    }
    let scalable = Path::new("scalable/apps").join(format!("{APP_ID}.svg"));
    assert!(files.contains(&scalable), "{files:?}");
}

#[test]
fn the_desktop_file_runs_the_binary_and_matches_the_window() {
    let bin = Path::new(env!("CARGO_BIN_EXE_xtrude"));
    let bin = bin.file_stem().unwrap().to_string_lossy();
    assert_eq!(desktop_entry("Exec"), Some(format!("{bin} %f")));
    assert_eq!(desktop_entry("Icon").as_deref(), Some(APP_ID));
    assert_eq!(desktop_entry("StartupWMClass").as_deref(), Some(APP_ID));
}

#[test]
fn the_install_script_is_executable_and_rewrites_the_desktop_exec() {
    use std::os::unix::fs::PermissionsExt;
    let path = linux_dir().join("install.sh");
    let mode = std::fs::metadata(&path)
        .expect("install.sh")
        .permissions()
        .mode();
    assert_eq!(mode & 0o111, 0o111, "install.sh is executable: {mode:o}");
    let script = std::fs::read_to_string(&path).expect("install.sh");
    assert!(script.starts_with("#!/usr/bin/env bash\n"));
    // install.sh points the entry at the installed binary by matching
    // the `Exec` the desktop file has.
    assert!(desktop_entry("Exec").unwrap().starts_with("xtrude "));
    assert!(script.contains(r#""Exec=xtrude "*"#), "{script}");
}

/// The types the desktop entry opens that shared-mime-info already
/// defines; start-up imports them (`MainWindow::open_on_start`).
const SYSTEM_TYPES: &[&str] = &["image/vnd.dxf"];

#[test]
fn the_desktop_file_opens_igz_and_the_system_types_start_up_imports() {
    let mime = std::fs::read_to_string(linux_dir().join(format!("{APP_ID}.mime.xml")))
        .expect("the MIME file");
    assert!(mime.contains(r#"<mime-type type="application/x-ingetrazo">"#));
    assert!(mime.contains(r#"<glob pattern="*.igz"/>"#));
    assert!(!mime.contains("<magic"), "an .igz is a zip: no magic");
    let expected: String = std::iter::once("application/x-ingetrazo")
        .chain(SYSTEM_TYPES.iter().copied())
        .map(|t| format!("{t};"))
        .collect();
    assert_eq!(desktop_entry("MimeType"), Some(expected));
    for t in SYSTEM_TYPES {
        assert!(!mime.contains(t), "{t} is shared-mime-info's to declare");
    }
}

fn metainfo() -> String {
    std::fs::read_to_string(linux_dir().join(format!("{APP_ID}.metainfo.xml")))
        .expect("the metainfo file")
}

/// The text of the metainfo's first `<tag>` element directly under
/// `<component>`.
fn component_child<'a>(doc: &'a roxmltree::Document, tag: &str) -> Option<&'a str> {
    doc.root_element()
        .children()
        .find(|n| n.has_tag_name(tag))
        .and_then(|n| n.text())
}

#[test]
fn the_metainfo_names_the_app_id_and_launches_the_desktop_file() {
    let text = metainfo();
    let doc = roxmltree::Document::parse(&text).expect("well-formed XML");
    assert!(doc.root_element().has_tag_name("component"));
    assert_eq!(component_child(&doc, "id"), Some(APP_ID));
    let launchable = doc
        .descendants()
        .find(|n| n.has_tag_name("launchable"))
        .expect("a launchable");
    assert_eq!(launchable.attribute("type"), Some("desktop-id"));
    assert_eq!(
        launchable.text(),
        Some(format!("{APP_ID}.desktop").as_str())
    );
}

#[test]
fn the_newest_metainfo_release_is_the_workspace_version() {
    let text = metainfo();
    let doc = roxmltree::Document::parse(&text).expect("well-formed XML");
    // AppStream lists releases newest first.
    let newest = doc
        .descendants()
        .filter(|n| n.has_tag_name("releases"))
        .flat_map(|n| n.children().filter(|c| c.has_tag_name("release")))
        .next()
        .expect("a release");
    assert_eq!(newest.attribute("version"), Some(VERSION));
}

#[test]
fn the_metainfo_gives_the_project_contacts() {
    let text = metainfo();
    let doc = roxmltree::Document::parse(&text).expect("well-formed XML");
    assert_eq!(
        component_child(&doc, "update_contact"),
        Some("hello@xtrudecad.org")
    );
    let url = |kind: &str| {
        doc.root_element()
            .children()
            .find(|n| n.has_tag_name("url") && n.attribute("type") == Some(kind))
            .and_then(|n| n.text())
    };
    assert_eq!(url("contact"), Some("https://xtrudecad.org/#contact"));
    assert_eq!(
        url("contribute"),
        Some("https://github.com/xtrudecad/xtrude-cad/blob/main/CONTRIBUTING.md")
    );
}

fn flatpak_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/flatpak")
}

fn flatpak_manifest() -> String {
    std::fs::read_to_string(flatpak_dir().join(format!("{APP_ID}.yml")))
        .expect("the Flatpak manifest")
}

fn read_json(path: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(path).expect("a JSON file");
    serde_json::from_str(&text).expect("valid JSON")
}

/// The value of a top-level `key: value` line of the Flatpak manifest.
fn manifest_key(key: &str) -> Option<String> {
    flatpak_manifest()
        .lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(": "))
        .map(|v| v.trim_matches('"').to_owned())
}

#[test]
fn the_flatpak_manifest_is_the_app_id_and_runs_the_binary() {
    let bin = Path::new(env!("CARGO_BIN_EXE_xtrude"));
    let bin = bin.file_stem().unwrap().to_string_lossy();
    assert_eq!(manifest_key("id").as_deref(), Some(APP_ID));
    assert_eq!(manifest_key("command"), Some(bin.into_owned()));
    // CARGO_HOME names the module's build folder.
    let manifest = flatpak_manifest();
    assert!(manifest.contains("  - name: xtrude-cad\n"), "{manifest}");
    assert!(manifest.contains("CARGO_HOME: /run/build/xtrude-cad/cargo"));
}

#[test]
fn the_flatpak_installs_both_binaries_and_the_desktop_files() {
    let manifest = flatpak_manifest();
    for installed in [
        "${FLATPAK_DEST}/bin/xtrude\n",
        "${FLATPAK_DEST}/bin/xtrude-step\n",
        "${FLATPAK_DEST}/share/applications/${FLATPAK_ID}.desktop\n",
        "${FLATPAK_DEST}/share/metainfo/${FLATPAK_ID}.metainfo.xml\n",
        "${FLATPAK_DEST}/share/mime/packages/${FLATPAK_ID}.xml\n",
        "${FLATPAK_DEST}/share/icons/hicolor/$size/apps/",
    ] {
        assert!(manifest.contains(installed), "installs {installed}");
    }
    // The helper is the real one, built with OpenCascade.
    assert!(manifest.contains("-p xtrude-step --features occt\n"));
    // No network: every crate comes from cargo-sources.json.
    assert!(!manifest.contains("--share=network\n"));
    assert!(manifest.contains("CARGO_NET_OFFLINE: \"true\""));
}

#[test]
fn flathub_builds_x86_64_only() {
    let json = read_json(&flatpak_dir().join("flathub.json"));
    assert_eq!(json, serde_json::json!({ "only-arches": ["x86_64"] }));
}

#[test]
fn the_lint_exceptions_name_the_app_id_and_flagged_finish_args() {
    let json = read_json(&flatpak_dir().join("lint-exceptions.json"));
    let apps = json.as_object().expect("an object");
    assert_eq!(apps.keys().collect::<Vec<_>>(), [APP_ID]);
    let manifest = flatpak_manifest();
    let flagged = [
        ("finish-args-home-filesystem-access", "--filesystem=home"),
        ("finish-args-contains-both-x11-and-wayland", "--socket=x11"),
    ];
    let exceptions = apps[APP_ID].as_object().expect("an object");
    assert_eq!(exceptions.len(), flagged.len(), "{exceptions:?}");
    for (lint, arg) in flagged {
        assert!(exceptions[lint].as_str().is_some_and(|r| !r.is_empty()));
        assert!(manifest.contains(&format!("  - {arg}\n")), "{arg}");
    }
}

#[test]
fn the_flatpak_workflow_builds_the_manifest_on_its_runtime_branch() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/flatpak.yml");
    let workflow = std::fs::read_to_string(path).expect("the Flatpak workflow");
    let runtime = manifest_key("runtime-version").expect("a runtime-version");
    let image = format!("ghcr.io/flathub-infra/flatpak-github-actions:freedesktop-{runtime}\n");
    assert!(workflow.contains(&image), "the image for runtime {runtime}");
    assert!(workflow.contains(&format!("MANIFEST: packaging/flatpak/{APP_ID}.yml\n")));
    // The build folder is one the manifest's `dir` source skips, so the
    // checkout copied into the build does not include it.
    assert!(workflow.contains("build-dir: build-dir\n"));
    assert!(flatpak_manifest().contains("          - build-dir\n"));
    assert!(workflow.contains("BUNDLE=xtrude-cad-$version-x86_64.flatpak"));
}

#[test]
fn the_release_workflow_attests_except_in_a_private_repository() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/release.yml");
    let workflow = std::fs::read_to_string(path).expect("the release workflow");
    // GitHub refuses attestations for a private repository on the
    // organisation's plan: a dry run there skips them, with a warning,
    // and a public release attests every file.
    let attest = workflow
        .split("      - ")
        .find(|step| step.starts_with("uses: actions/attest-build-provenance@"))
        .expect("the attestation step");
    assert!(attest.contains("        if: ${{ !github.event.repository.private }}\n"));
    assert!(attest.contains("          subject-path: assets/*\n"));
    let warning = workflow
        .split("      - ")
        .find(|step| step.starts_with("name: No attestations in a private repository\n"))
        .expect("the warning step");
    assert!(warning.contains("        if: ${{ github.event.repository.private }}\n"));
    assert!(warning.contains("::warning::"));
}

/// The crates.io packages of `Cargo.lock`, as `(name-version, checksum)`.
fn locked_registry_crates() -> Vec<(String, String)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    let lock = std::fs::read_to_string(path).expect("Cargo.lock");
    let field = |pkg: &str, key: &str| {
        pkg.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix(" = "))
            .map(|v| v.trim_matches('"').to_owned())
    };
    let mut crates: Vec<_> = lock
        .split("[[package]]")
        .filter(|pkg| {
            field(pkg, "source").as_deref()
                == Some("registry+https://github.com/rust-lang/crates.io-index")
        })
        .map(|pkg| {
            let name = field(pkg, "name").expect("a name");
            let version = field(pkg, "version").expect("a version");
            let checksum = field(pkg, "checksum").expect("a checksum");
            (format!("{name}-{version}"), checksum)
        })
        .collect();
    crates.sort();
    crates
}

#[test]
fn the_flatpak_cargo_sources_match_cargo_lock() {
    // Fails after a Cargo.lock change: run
    // scripts/release/update_cargo_sources.sh.
    let json = read_json(&flatpak_dir().join("cargo-sources.json"));
    let mut archives: Vec<(String, String)> = json
        .as_array()
        .expect("a list of sources")
        .iter()
        .filter(|s| s["type"] == "archive")
        .map(|s| {
            let dest = s["dest"].as_str().expect("a dest");
            let name = dest.strip_prefix("cargo/vendor/").expect("a vendor dest");
            let sha = s["sha256"].as_str().expect("a sha256");
            (name.to_owned(), sha.to_owned())
        })
        .collect();
    archives.sort();
    assert_eq!(archives, locked_registry_crates());
}

#[test]
fn the_flatpak_manifold_module_names_the_locked_manifold_csg_sys() {
    // The module builds the Manifold commit that this manifold-csg-sys
    // pins: a new manifold-csg-sys needs the module's commits checked.
    let locked = locked_registry_crates();
    let (sys, _) = locked
        .iter()
        .find(|(c, _)| {
            c.strip_prefix("manifold-csg-sys-")
                .is_some_and(|v| v.starts_with(char::is_numeric))
        })
        .expect("manifold-csg-sys in Cargo.lock");
    let version = sys.strip_prefix("manifold-csg-sys-").unwrap();
    let manifest = flatpak_manifest();
    assert!(
        manifest.contains(&format!("(manifold-csg-sys {version}:")),
        "the manifest's Manifold module names manifold-csg-sys {version}"
    );
    assert!(manifest.contains("MANIFOLD_CSG_LIB_DIR: /app/lib/manifold-csg\n"));
    assert!(
        manifest.contains("      - /lib/manifold-csg\n"),
        "cleaned up"
    );
}

#[test]
fn the_flatpak_opencascade_module_builds_the_locked_occt_sys_sources() {
    // The module builds the OpenCascade that this occt-sys vendors, from
    // its crate archive: a new occt-sys needs the module's archive and
    // build options checked against the crate's build_occt().
    let locked = locked_registry_crates();
    let (sys, checksum) = locked
        .iter()
        .find(|(c, _)| {
            c.strip_prefix("occt-sys-")
                .is_some_and(|v| v.starts_with(char::is_numeric))
        })
        .expect("occt-sys in Cargo.lock");
    let version = sys.strip_prefix("occt-sys-").unwrap();
    let manifest = flatpak_manifest();
    assert!(manifest.contains("  - name: opencascade\n"));
    assert!(
        manifest.contains(&format!(
            "        url: https://static.crates.io/crates/occt-sys/{sys}.crate\n\
             \x20       sha256: {checksum}\n"
        )),
        "the module's archive is occt-sys {version}"
    );
    assert!(manifest.contains(&format!("(occt-sys {version}, src/lib.rs)")));
    // The helper links it instead of building OpenCascade again.
    assert!(manifest.contains("XTRUDE_OCCT_DIR: /app/lib/opencascade\n"));
    assert!(manifest.contains("-DCMAKE_INSTALL_PREFIX=${FLATPAK_DEST}/lib/opencascade\n"));
    assert!(
        manifest.contains("      - /lib/opencascade\n"),
        "cleaned up"
    );
}
