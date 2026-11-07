//! Builds every app segment as a Windows/Linux DLL from the unmodified sources
//! in `firmware-source/apps`, so the emulator can load each one fresh on every
//! launch (the radio reloads an app into its RAM arena each time, which resets
//! all of its `static` state; a DLL load does the same on the PC).
//!
//! The original files are never touched. Each segment is copied into
//! `target/emu-apps/<segment>/` with three textual changes:
//!   * `#![no_std]` and `#![no_main]` are dropped (the PC build uses std),
//!   * `#[panic_handler]` is dropped (std provides one; a hook routes panics to
//!     `app_fault` instead),
//!   * a few `emu_*` exports are appended to the crate root.
//! `#[path = "..."]` modules are copied and rewritten the same way.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// One launcher entry: a package name and its segment directories, in order.
pub struct Package {
    pub name: String,
    pub segments: Vec<String>,
}

/// Reads the `APPS = [...]` table from `source-changes/pack_apps.py`, so the
/// emulator's launcher always matches what gets packed for the radio.
pub fn read_packages(pack_apps: &Path) -> Result<Vec<Package>, String> {
    let text = fs::read_to_string(pack_apps)
        .map_err(|e| format!("reading {}: {e}", pack_apps.display()))?;
    let start = text.find("APPS = [").ok_or("no APPS table in pack_apps.py")?;
    let body = &text[start + 8..];
    let mut packages = Vec::new();
    let mut rest = body;
    // Entries look like ("NAME", ["seg", "seg"]), and the table ends at "]\n".
    while let Some(open) = rest.find("(\"") {
        let end_of_table = rest.find("]\n").unwrap_or(usize::MAX);
        if open > end_of_table {
            break;
        }
        let after = &rest[open + 2..];
        let name_end = after.find('"').ok_or("bad APPS entry")?;
        let name = after[..name_end].to_string();
        let list_open = after.find('[').ok_or("bad APPS entry")?;
        let list_close = after.find(']').ok_or("bad APPS entry")?;
        let segments = after[list_open + 1..list_close]
            .split(',')
            .map(|s| s.trim().trim_matches('"').to_string())
            .filter(|s| !s.is_empty())
            .collect();
        packages.push(Package { name, segments });
        rest = &after[list_close + 1..];
    }
    if packages.is_empty() {
        return Err("APPS table in pack_apps.py is empty".into());
    }
    Ok(packages)
}

/// Finds a segment's crate directory: `apps/<seg>` or `apps/<group>/<seg>`.
fn find_segment_dir(apps: &Path, seg: &str) -> Option<PathBuf> {
    let direct = apps.join(seg);
    if direct.join("Cargo.toml").is_file() {
        return Some(direct);
    }
    for entry in fs::read_dir(apps).ok()?.flatten() {
        let nested = entry.path().join(seg);
        if nested.join("Cargo.toml").is_file() {
            return Some(nested);
        }
    }
    None
}

/// Forward slashes, without the `\\?\` prefix Windows' canonicalize adds.
fn slash(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    s.strip_prefix("//?/").map(str::to_string).unwrap_or(s)
}

/// Applies the PC-build changes to one source file.
fn transform(src: &str) -> String {
    let mut out = String::with_capacity(src.len() + 64);
    for line in src.lines() {
        let t = line.trim();
        if t == "#![no_std]" || t == "#![no_main]" || t == "#[panic_handler]" {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

struct Copier {
    out_src: PathBuf,
    copied: HashMap<PathBuf, String>,
    counter: usize,
}

impl Copier {
    /// Copies `file` (and, recursively, any `#[path]` modules it names) into the
    /// generated crate, returning the generated file's path.
    fn copy(&mut self, file: &Path, dest_name: &str) -> Result<PathBuf, String> {
        let text = fs::read_to_string(file)
            .map_err(|e| format!("reading {}: {e}", file.display()))?;
        let dir = file.parent().unwrap_or(Path::new("."));
        let mut out = String::new();
        for line in transform(&text).lines() {
            let t = line.trim_start();
            if let Some(rest) = t.strip_prefix("#[path = \"") {
                if let Some(end) = rest.find('"') {
                    let target = dir.join(&rest[..end]);
                    let canon = target
                        .canonicalize()
                        .map_err(|e| format!("{}: {e}", target.display()))?;
                    let generated = match self.copied.get(&canon) {
                        Some(g) => g.clone(),
                        None => {
                            self.counter += 1;
                            let stem = canon
                                .file_stem()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_default();
                            let name = format!("_path{}_{stem}.rs", self.counter);
                            self.copied.insert(canon.clone(), name.clone());
                            self.copy(&canon, &name)?;
                            name
                        }
                    };
                    let indent = &line[..line.len() - t.len()];
                    out.push_str(&format!(
                        "{indent}#[path = \"{}\"]\n",
                        slash(&self.out_src.join(generated))
                    ));
                    continue;
                }
            }
            out.push_str(line);
            out.push('\n');
        }
        let dest = self.out_src.join(dest_name);
        write_if_changed(&dest, &out)?;
        Ok(dest)
    }
}

fn write_if_changed(path: &Path, text: &str) -> Result<(), String> {
    if fs::read_to_string(path).ok().as_deref() == Some(text) {
        return Ok(()); // keeps cargo's incremental build valid
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(path, text).map_err(|e| format!("writing {}: {e}", path.display()))
}

/// Root-only additions: emulator hooks the host looks up after loading.
const ROOT_EXTRA: &str = r#"

// ---- added by the emulator's app builder (not in the radio build) ----
#[allow(dead_code)]
static mut EMU_RF: Option<(extern "C" fn(u8, u16), extern "C" fn(u8) -> u16)> = None;

#[no_mangle]
pub extern "C" fn emu_set_rf(w: extern "C" fn(u8, u16), r: extern "C" fn(u8) -> u16) {
    unsafe { EMU_RF = Some((w, r)) };
}

#[allow(dead_code)]
pub(crate) fn sim_rf_write(addr: u8, value: u16) {
    if let Some((w, _)) = unsafe { EMU_RF } {
        w(addr, value)
    }
}

#[allow(dead_code)]
pub(crate) fn sim_rf_read(addr: u8) -> u16 {
    match unsafe { EMU_RF } {
        Some((_, r)) => r(addr),
        None => 0,
    }
}

/// Routes a Rust panic inside the app to the host's `app_fault`, the same
/// thing the radio build's panic handler does.
#[no_mangle]
pub extern "C" fn emu_install_panic_hook(fault: extern "C" fn(u32) -> !) {
    std::panic::set_hook(Box::new(move |info| {
        eprintln!("app panicked: {info}");
        fault(0xEEEE)
    }));
}
"#;

/// For apps whose PC-only helpers read `crate::API` (the simulator's table):
/// a stand-in that points at the emulator's table once the host sets it.
const API_SHIM: &str = r#"

#[allow(dead_code)]
static mut EMU_API: *const bf5rh_abi::Api = core::ptr::null();

#[no_mangle]
pub extern "C" fn emu_set_api(api: &'static bf5rh_abi::Api) {
    unsafe { EMU_API = api };
}

#[allow(dead_code)]
pub(crate) struct EmuApi;

impl core::ops::Deref for EmuApi {
    type Target = bf5rh_abi::Api;
    fn deref(&self) -> &bf5rh_abi::Api {
        unsafe { &*EMU_API }
    }
}

#[allow(dead_code)]
pub(crate) static API: EmuApi = EmuApi;
"#;

/// Rewrites a segment's Cargo.toml as a cdylib with absolute dependency paths,
/// keeping its `[features]` (Settings picks its menu groups that way).
fn cargo_toml(orig_dir: &Path, orig: &str, crate_name: &str) -> String {
    let mut out = format!(
        "[package]\nname = \"{crate_name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\nautobins = false\n\n\
         [lib]\ncrate-type = [\"cdylib\"]\npath = \"src/main.rs\"\n\n"
    );
    let mut section = String::new();
    for line in orig.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            section = t.to_string();
            if section == "[features]" || section == "[dependencies]" {
                out.push_str(&format!("\n{section}\n"));
            }
            continue;
        }
        match section.as_str() {
            "[features]" => {
                out.push_str(line);
                out.push('\n');
            }
            "[dependencies]" => {
                if let Some(i) = line.find("path = \"") {
                    let rest = &line[i + 8..];
                    let end = rest.find('"').unwrap_or(rest.len());
                    let abs = orig_dir.join(&rest[..end]);
                    let abs = abs.canonicalize().unwrap_or(abs);
                    out.push_str(&format!(
                        "{}path = \"{}\"{}\n",
                        &line[..i],
                        slash(&abs),
                        rest.get(end + 1..).unwrap_or("")
                    ));
                } else {
                    out.push_str(line);
                    out.push('\n');
                }
            }
            _ => {}
        }
    }
    out
}

pub fn crate_name(seg: &str) -> String {
    format!("emuapp_{}", seg.replace('-', "_"))
}

/// Where cargo puts a segment's DLL.
pub fn dll_path(gen_root: &Path, seg: &str) -> PathBuf {
    let name = crate_name(seg);
    let file = if cfg!(windows) {
        format!("{name}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{name}.dylib")
    } else {
        format!("lib{name}.so")
    };
    gen_root.join("target").join("debug").join(file)
}

/// Generates the segment crates and runs one `cargo build` over all of them.
/// Returns cargo's error output on failure.
pub fn build_all(apps_dir: &Path, gen_root: &Path, packages: &[Package]) -> Result<(), String> {
    let mut members = Vec::new();
    for pkg in packages {
        for seg in &pkg.segments {
            let dir = find_segment_dir(apps_dir, seg)
                .ok_or_else(|| format!("{}: no app crate named {seg}", pkg.name))?;
            let name = crate_name(seg);
            let out = gen_root.join(&name);
            let orig_toml = fs::read_to_string(dir.join("Cargo.toml")).map_err(|e| e.to_string())?;
            write_if_changed(&out.join("Cargo.toml"), &cargo_toml(&dir, &orig_toml, &name))?;

            let mut copier = Copier { out_src: out.join("src"), copied: HashMap::new(), counter: 0 };
            // Copy every plain module file next to main.rs (mod logic; etc.).
            for entry in fs::read_dir(dir.join("src")).map_err(|e| e.to_string())?.flatten() {
                let p = entry.path();
                if p.extension().is_some_and(|e| e == "rs") && p.file_name().is_some_and(|n| n != "main.rs") {
                    let n = p.file_name().unwrap().to_string_lossy().to_string();
                    copier.copy(&p, &n)?;
                }
            }
            let root = copier.copy(&dir.join("src").join("main.rs"), "main.rs.tmp")?;
            let mut text = fs::read_to_string(&root).map_err(|e| e.to_string())?;
            fs::remove_file(&root).ok();
            text.insert_str(0, "#![allow(warnings)]\n");
            text.push_str(ROOT_EXTRA);
            let mentions_api = fs::read_dir(out.join("src"))
                .map_err(|e| e.to_string())?
                .flatten()
                .any(|e| fs::read_to_string(e.path()).is_ok_and(|t| t.contains("crate::API")));
            if mentions_api && !text.contains("static API") {
                text.push_str(API_SHIM);
            }
            write_if_changed(&out.join("src").join("main.rs"), &text)?;
            members.push(name);
        }
    }

    let ws = format!(
        "[workspace]\nresolver = \"2\"\nmembers = [{}]\n\n[profile.dev]\nopt-level = 1\npanic = \"unwind\"\n",
        members.iter().map(|m| format!("\"{m}\"")).collect::<Vec<_>>().join(", ")
    );
    write_if_changed(&gen_root.join("Cargo.toml"), &ws)?;

    let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .arg("build")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(gen_root.join("Cargo.toml"))
        .output()
        .map_err(|e| format!("running cargo: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}
