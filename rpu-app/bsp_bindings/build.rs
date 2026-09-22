use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn required_env(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| {
        panic!("{name} is not set; build this workspace through build-plm")
    })
}

fn run(command: &mut Command, description: &str) {
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("Failed to {description}: {error}"));
    if !status.success() {
        panic!("{description} failed with {status}");
    }
}

fn path_string(path: &Path) -> String {
    path.to_str()
        .unwrap_or_else(|| panic!("Path is not valid UTF-8: {}", path.display()))
        .to_owned()
}

fn main() {
    println!("cargo:rerun-if-changed=bsp_wrapper.c");
    println!("cargo:rerun-if-changed=bsp_wrapper.h");
    println!("cargo:rerun-if-env-changed=ARM_R5_GCC");
    println!("cargo:rerun-if-env-changed=ARM_R5_INCLUDE_DIRS");
    println!("cargo:rerun-if-env-changed=ARM_R5_SYSROOT");
    println!("cargo:rerun-if-env-changed=BSP_INCLUDE_DIR");

    let bsp_include = PathBuf::from(required_env("BSP_INCLUDE_DIR"));
    let gcc = PathBuf::from(required_env("ARM_R5_GCC"));
    let include_dirs: Vec<PathBuf> =
        env::split_paths(&required_env("ARM_R5_INCLUDE_DIRS")).collect();
    let manifest_dir = PathBuf::from(required_env("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(required_env("OUT_DIR"));
    let wrapper = manifest_dir.join("bsp_wrapper.h");

    let mut builder = bindgen::Builder::default()
        .header(path_string(&wrapper))
        .use_core()
        .layout_tests(false)
        .clang_arg("-nostdinc")
        .clang_arg("--target=arm-none-eabi")
        .clang_arg("-mcpu=cortex-r5")
        .clang_arg("-mfloat-abi=hard")
        .clang_arg("-mfpu=vfpv3-d16")
        .clang_arg("-DSDT")
        .clang_arg("-DXIL_INTERRUPT")
        .clang_arg("-Dversal")
        .clang_arg("-U__clang__")
        .clang_arg(format!("-I{}", bsp_include.display()))
        .allowlist_type("XIpiPsu.*")
        .allowlist_function("XIpiPsu_(ReadMessage|WriteMessage|TriggerIpi)")
        .allowlist_function("RpuIpi.*")
        .allowlist_function("sleep")
        .allowlist_function("xil_printf")
        .allowlist_var("XIPIPSU_BUF_TYPE_.*")
        .allowlist_var("XST_.*");

    for include_dir in &include_dirs {
        builder = builder
            .clang_arg("-isystem")
            .clang_arg(path_string(include_dir));
    }

    if let Ok(sysroot) = env::var("ARM_R5_SYSROOT") {
        if !sysroot.is_empty() {
            builder = builder.clang_arg(format!("--sysroot={sysroot}"));
        }
    }

    builder
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .unwrap_or_else(|error| panic!("Unable to generate RPU BSP bindings: {error}"))
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("Unable to write generated RPU BSP bindings");

    let object = out_dir.join("bsp_wrapper.o");
    let archive = out_dir.join("libbsp_wrapper.a");
    run(
        Command::new(&gcc)
            .arg("-c")
            .arg(manifest_dir.join("bsp_wrapper.c"))
            .arg("-o")
            .arg(&object)
            .args([
                "-mcpu=cortex-r5",
                "-mfloat-abi=hard",
                "-mfpu=vfpv3-d16",
                "-O0",
                "-g3",
                "-DSDT",
                "-DXIL_INTERRUPT",
                "-Dversal",
            ])
            .arg(format!("-I{}", bsp_include.display())),
        "compile the BSP macro wrapper",
    );

    let ar = gcc.with_file_name("armr5-none-eabi-ar");
    run(
        Command::new(&ar).args(["crs"]).arg(&archive).arg(&object),
        "archive the BSP macro wrapper",
    );

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=bsp_wrapper");
}
