//! Build script: compile the Swift Foundation Models bridge and link it in.
//!
//! The bridge (`swift/kaseifu_fm_bridge.swift`) is the only Swift in the
//! repository. This script:
//!
//! 1. locates a Swift compiler (`xcrun --find swiftc`, then `swiftc` on PATH),
//! 2. probes whether the SDK has Foundation Models (`import FoundationModels`),
//! 3. compiles the bridge to an object file in `OUT_DIR`, archives it, and
//!    links it into the crate,
//! 4. weak-links FoundationModels so the binary still starts on macOS < 26.
//!
//! When (1) or (2) fails the build does not fail: a Rust fallback with the
//! same C ABI is compiled instead (`cfg(kaseifu_fm_stub)`), which reports the
//! feature as unavailable with an explicit reason. That is a deliberate
//! degradation, not a silent fallback to another model: nothing is inferred
//! and every call still answers with `unavailable: <reason>`.
//!
//! Set `KASEIFU_FM_FORCE_STUB=no-sdk|no-toolchain` to build the fallback on a
//! machine that has a working toolchain (used to verify the CI path locally).
//!
//! Note: the compiler is always invoked through `xcrun swiftc` when xcrun
//! exists. Calling the `swiftc` binary directly fails to find the standard
//! library outside an xcrun environment (`unable to load standard library for
//! target ...`), which would silently degrade every local build to the stub.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=swift/kaseifu_fm_bridge.swift");
    println!("cargo:rerun-if-env-changed=KASEIFU_FM_FORCE_STUB");
    println!("cargo:rustc-check-cfg=cfg(kaseifu_fm_stub, values(\"no-toolchain\", \"no-sdk\"))");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));

    if let Some(reason) = forced_stub() {
        warn(&format!(
            "KASEIFU_FM_FORCE_STUB={reason} is set: building the Foundation Models fallback"
        ));
        emit_stub(reason);
        return;
    }

    let Some(toolchain) = find_toolchain() else {
        warn(
            "no Swift compiler found (xcrun --find swiftc failed): building the Foundation Models fallback; kaseifu builds and runs, and the AI rename feature reports itself unavailable",
        );
        emit_stub("no-toolchain");
        return;
    };

    if !sdk_has_foundation_models(&toolchain, &out_dir) {
        warn(
            "this SDK cannot compile `import FoundationModels` (macOS 26 SDK required): building the Foundation Models fallback; the feature reports itself unavailable instead of failing the build",
        );
        emit_stub("no-sdk");
        return;
    }

    compile_and_link(&toolchain, &out_dir);
}

/// How the Swift compiler is invoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Compiler {
    /// `xcrun swiftc` (sets up the SDK environment correctly).
    Xcrun,
    /// The `swiftc` binary on PATH (xcrun missing).
    Direct,
}

/// A located Swift toolchain.
#[derive(Debug, Clone)]
struct Toolchain {
    compiler: Compiler,
    /// The resolved `swiftc` path (used to derive the runtime library dir).
    swiftc: PathBuf,
}

impl Toolchain {
    /// A command that runs the Swift compiler.
    ///
    /// `SDKROOT` is dropped on purpose. Homebrew exports it pointing at the
    /// Command Line Tools SDK, and under that SDK the `@Generable` macro
    /// plugin (which only Xcode ships) cannot be resolved: measured
    /// `plugin for module 'FoundationModelsMacros' not found`, exit 101, with
    /// `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk`. Without
    /// it the compiler uses its own default SDK, which for an Xcode toolchain
    /// is the Xcode one, and the same compile succeeds (measured). The probe
    /// below shares this command, so it judges the same SDK the build uses.
    fn command(&self) -> Command {
        let mut command = match self.compiler {
            Compiler::Xcrun => {
                let mut command = Command::new("xcrun");
                command.arg("swiftc");
                command
            }
            Compiler::Direct => Command::new(&self.swiftc),
        };
        command.env_remove("SDKROOT");
        command
    }
}

/// The fallback reason requested through the environment, if any.
fn forced_stub() -> Option<&'static str> {
    match env::var("KASEIFU_FM_FORCE_STUB").ok().as_deref() {
        Some("no-sdk") => Some("no-sdk"),
        Some("no-toolchain") => Some("no-toolchain"),
        Some("") | None => None,
        Some(_) => {
            warn("KASEIFU_FM_FORCE_STUB must be `no-sdk` or `no-toolchain`; ignoring the value");
            None
        }
    }
}

/// Select the Rust fallback implementation of the C ABI.
fn emit_stub(reason: &str) {
    println!("cargo:rustc-cfg=kaseifu_fm_stub={reason:?}");
}

fn warn(message: &str) {
    println!("cargo:warning={message}");
}

/// Locate `swiftc` through xcrun first (Xcode toolchains), then PATH.
fn find_toolchain() -> Option<Toolchain> {
    if let Ok(output) = Command::new("xcrun").args(["--find", "swiftc"]).output()
        && output.status.success()
    {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Some(Toolchain {
                compiler: Compiler::Xcrun,
                swiftc: PathBuf::from(path),
            });
        }
    }
    let output = Command::new("sh")
        .args(["-c", "command -v swiftc"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return None;
    }
    warn(
        "xcrun is missing: invoking swiftc directly; if the SDK is not found the Foundation Models fallback is built",
    );
    Some(Toolchain {
        compiler: Compiler::Direct,
        swiftc: PathBuf::from(path),
    })
}

/// True when the active SDK can compile `import FoundationModels`.
fn sdk_has_foundation_models(toolchain: &Toolchain, out_dir: &Path) -> bool {
    let probe = out_dir.join("kaseifu_fm_probe.swift");
    if fs::write(&probe, "import FoundationModels\n").is_err() {
        return false;
    }
    toolchain
        .command()
        .arg("-typecheck")
        .arg(&probe)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Compile the bridge, archive it, and emit the link directives.
fn compile_and_link(toolchain: &Toolchain, out_dir: &Path) {
    let source = Path::new("swift/kaseifu_fm_bridge.swift");
    if !source.exists() {
        panic!("swift/kaseifu_fm_bridge.swift is missing; the repository is incomplete");
    }
    let object = out_dir.join("kaseifu_fm_bridge.o");
    let archive = out_dir.join("libkaseifu_fm_bridge.a");

    let mut compile = toolchain.command();
    compile
        .arg("-parse-as-library")
        .arg("-emit-object")
        .arg("-module-name")
        .arg("kaseifu_fm_bridge")
        // The compiler autolinks FoundationModels strongly; the weak link is
        // added at the Rust link step instead (macOS < 26 must still start).
        .arg("-Xfrontend")
        .arg("-disable-autolink-framework")
        .arg("-Xfrontend")
        .arg("FoundationModels")
        .arg("-o")
        .arg(&object);
    if let Some(target) = swift_target() {
        compile.arg("-target").arg(target);
    }
    if env::var("PROFILE").as_deref() != Ok("debug") {
        compile.arg("-O");
    }
    compile.arg(source);

    let output = compile.output().expect("swiftc runs");
    if !output.status.success() {
        panic!(
            "the Swift bridge failed to compile:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let status = Command::new("ar")
        .arg("crs")
        .arg(&archive)
        .arg(&object)
        .status()
        .expect("ar runs");
    assert!(status.success(), "ar failed to archive the Swift bridge");

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=kaseifu_fm_bridge");
    // Swift back-deployment libraries (libswiftCompatibility*) live next to
    // the toolchain; without this search path the link fails on older targets.
    if let Some(runtime) = swift_runtime_dir(&toolchain.swiftc) {
        println!("cargo:rustc-link-search=native={}", runtime.display());
    }
    // The Swift concurrency runtime (used by `Task` for the async bridge) is
    // part of macOS from 12.0 on. With an older deployment target the linker
    // rewrites the load to `@rpath/libswift_Concurrency.dylib`, which no
    // machine without Xcode can resolve: dyld aborts on the first `Task`
    // (measured). Raising the link deployment target to 12.0 makes the load
    // use the OS path instead. Consequence: the binary requires macOS 12.0
    // (Big Sur is EOL); the AI rename feature still needs macOS 26.
    println!("cargo:rustc-link-arg=-mmacosx-version-min=12.0");
    println!("cargo:rustc-link-arg=-weak_framework");
    println!("cargo:rustc-link-arg=FoundationModels");
}

/// The Swift target triple for the crate's target, with a deployment target
/// old enough that macOS < 26 can still launch the binary, and new enough that
/// the Swift concurrency runtime comes from the OS (see the link args above).
fn swift_target() -> Option<&'static str> {
    match env::var("TARGET").as_deref() {
        Ok("aarch64-apple-darwin") => Some("arm64-apple-macosx12.0"),
        Ok("x86_64-apple-darwin") => Some("x86_64-apple-macosx12.0"),
        Ok(other) => {
            warn(&format!(
                "no Swift deployment target mapping for {other}: using the toolchain default"
            ));
            None
        }
        Err(_) => None,
    }
}

/// `<toolchain>/usr/lib/swift/macosx`, derived from the compiler location.
fn swift_runtime_dir(swiftc: &Path) -> Option<PathBuf> {
    let dir = swiftc.parent()?.parent()?.join("lib/swift/macosx");
    dir.is_dir().then_some(dir)
}
