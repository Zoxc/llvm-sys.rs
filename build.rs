extern crate glob;
extern crate semver;

use semver::{Version, VersionReq};
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::str::from_utf8_unchecked;
use std::process::Command;

fn is_whitespace(x: &u8) -> bool {
    ['\n', '\t', ' '].contains(&(*x as char))
}

fn build_wrappers() -> (PathBuf, String, &'static str) {
    // Get Cargo directories
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let mut src_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    src_dir.push("wrappers");

    // Run cmake generator
    match Command::new("cmake").arg(&src_dir).current_dir(&out_dir).status() {
        Err(e) => panic!("Failed to invoke CMake: {}", e),
        Ok(s) => {
            if !s.success() {
                panic!("CMake configuration of wrappers failed with status {}", s)
            }
        }
    }

    // Do the actual build
    match Command::new("cmake").arg("--build").arg(&out_dir).status() {
        Err(e) => panic!("Failed to invoke CMake: {}", e),
        Ok(s) => {
            if !s.success() {
                panic!("CMake build of wrappers failed with status {}", s)
            }
        }
    }

    // Get LLVM location
    let output = Command::new("cmake").args(&["-N", "-L"]).arg(&out_dir).output().unwrap().stdout;
    let dir = std::str::from_utf8(&output).unwrap().lines().filter(|l| l.starts_with("LLVM_DIR:PATH=")).next().unwrap();
    let prefix = Path::new(&dir["LLVM_DIR:PATH=".len()..]).parent().unwrap().parent().unwrap().parent().unwrap();

    (prefix.to_path_buf(), out_dir.into_os_string().into_string().unwrap(),
     "targetwrappers")
}

fn main() {
    let (llvm_prefix, wrappers_out_dir, wrappers_lib_name) = build_wrappers();
    println!("cargo:rustc-link-search=native={}", wrappers_out_dir);
    println!("cargo:rustc-link-lib=static={}", wrappers_lib_name);

    let minimum_llvm_version = VersionReq::parse(">=3.6").unwrap();
    let (llvm_config, version) = get_llvm_config(llvm_prefix);
    if minimum_llvm_version.matches(&version) {
        println!("Found LLVM version {}", version);
    } else {
        panic!("LLVM version 3.6 or higher is required. (Found {})", version);
    };

    // Are we using LLVM as a shared object or static library?
    let llvm_libtype = match std::env::var("CARGO_FEATURE_LLVM_DYLIB") {
        Ok(_) => "dylib",
        Err(_) => "static"
    };


    // llvm-config --ldflags: extract -L<dir> options
    let output = Command::new(&*llvm_config).arg("--ldflags").output().unwrap().stdout;
    for arg in output.split(is_whitespace) {
        if arg.starts_with(b"-L") {
            println!("cargo:rustc-link-search=native={}", unsafe {
                from_utf8_unchecked(&arg[2..])
            });
        }
    }

    // llvm-config --libs --system-libs: extract -l<lib> options
    let output = Command::new(&*llvm_config).args(&["--libs", "--system-libs"]).output().unwrap().stdout;
    for arg in output.split(is_whitespace) {
        if arg.starts_with(b"-l") {
            let arg = &arg[2..];
            let libtype = if arg.starts_with(b"LLVM") {
                llvm_libtype
            } else {
                "dylib"
            };
            println!("cargo:rustc-link-lib={}={}", libtype, unsafe {
                from_utf8_unchecked(arg)
            });
        }
    }

    // llvm-config --cxxflags: determine which libc++ to use: LLVM's or GCC's
    let output = String::from_utf8(
        Command::new(&*llvm_config).arg("--cxxflags").output().ok().expect("bad output from llvm-config").stdout
    ).unwrap();
    let libcpp = if output.contains("stdlib=libc++") {
        "c++"
    } else {
        "stdc++"
    };
    println!("cargo:rustc-link-lib={}", libcpp);
}

fn get_llvm_config(mut prefix: PathBuf) -> (Cow<'static, str>, Version) {
    static BAD_PATH:&'static str = "unparseable llvm-config path";
    prefix.push("bin");
    prefix.push("llvm-config*");
    let mut name = String::new();
    for path in glob::glob(prefix.to_str().unwrap()).ok().expect("could not parse glob") {
        let path:PathBuf = path.ok().expect("bad glob");
        name = path.to_str().expect(BAD_PATH).to_string()
    }
    match Command::new(&name).arg("--version").output() {
        Ok(x) => {
            (Cow::Owned(name),
             Version::parse(std::str::from_utf8(&x.stdout[..]).ok().expect("output was not utf-8")).ok().expect("could not parse version from llvm-config"))
        }
        Err(_) => {
            panic!("llvm-config not found. Install LLVM before attempting to build.");
        }
    }
}
