use std::env;
use std::path::PathBuf;
use std::process::Command;

fn required_env(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| {
        panic!("{name} is not set; build this workspace through build-plm")
    })
}

fn toolchain_library_dir(gcc: &str, library: &str) -> PathBuf {
    let output = Command::new(gcc)
        .args([
            "-mcpu=cortex-r5",
            "-mfloat-abi=hard",
            "-mfpu=vfpv3-d16",
            &format!("-print-file-name={library}"),
        ])
        .output()
        .unwrap_or_else(|error| panic!("Failed to query {library}: {error}"));
    if !output.status.success() {
        panic!("Could not query {library} from {gcc}");
    }

    let text = String::from_utf8(output.stdout)
        .unwrap_or_else(|error| panic!("Toolchain returned invalid UTF-8: {error}"));
    PathBuf::from(text.trim())
        .parent()
        .unwrap_or_else(|| panic!("Could not derive the directory for {library}"))
        .to_path_buf()
}

fn main() {
    println!("cargo:rerun-if-env-changed=ARM_R5_GCC");
    println!("cargo:rerun-if-env-changed=BSP_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=RPU_LINKER_SCRIPT");

    let gcc = required_env("ARM_R5_GCC");
    let bsp_include = PathBuf::from(required_env("BSP_INCLUDE_DIR"));
    let bsp_lib = bsp_include
        .parent()
        .expect("BSP include directory has no parent")
        .join("lib");
    let linker_script = PathBuf::from(required_env("RPU_LINKER_SCRIPT"));

    if !linker_script.is_file() {
        panic!(
            "Vitis-generated RPU linker script does not exist: {}",
            linker_script.display()
        );
    }

    println!("cargo:rustc-link-search=native={}", bsp_lib.display());
    println!(
        "cargo:rustc-link-search=native={}",
        toolchain_library_dir(&gcc, "libc.a").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        toolchain_library_dir(&gcc, "libgcc.a").display()
    );
    println!("cargo:rustc-link-arg=-T{}", linker_script.display());
    // Vitis standalone archives contain circular references. Supplying them
    // as final linker arguments keeps them after Rust objects, while the
    // group makes GNU ld rescan archives until all BSP symbols are resolved.
    println!("cargo:rustc-link-arg=-Wl,--start-group");
    println!("cargo:rustc-link-arg=-lxilstandalone");
    println!("cargo:rustc-link-arg=-lxiltimer");
    println!("cargo:rustc-link-arg=-lxil");
    println!("cargo:rustc-link-arg=-lcfupmc");
    println!("cargo:rustc-link-arg=-lgcc");
    println!("cargo:rustc-link-arg=-lc");
    println!("cargo:rustc-link-arg=-lnosys");
    println!("cargo:rustc-link-arg=-Wl,--end-group");
}
