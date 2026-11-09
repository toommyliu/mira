use std::{env, path::PathBuf, process::Command};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    for input in [
        "scripts/build-runtime.ts",
        "packages/mira/src",
        "packages/mira/package.json",
        "pnpm-lock.yaml",
    ] {
        println!("cargo::rerun-if-changed={}", manifest.join(input).display());
    }
    if !manifest.join("packages/mira/node_modules/effect").exists() {
        panic!(
            "The script runtime needs its JavaScript dependencies. Run `pnpm install`, then build again."
        );
    }
    let status = Command::new("node")
        .arg(manifest.join("scripts/build-runtime.ts"))
        .arg("--outdir")
        .arg(out.join("mira"))
        .arg("--table")
        .arg(out.join("runtime_assets.rs"))
        .status()
        .unwrap_or_else(|error| panic!("couldn't run node to build the script runtime: {error}"));
    if !status.success() {
        panic!("building the script runtime failed ({status})");
    }
}
