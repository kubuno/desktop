//! Build script of `kubuno-ui`: gives every build of the shared library its own file name,
//! `kubuno_ui-<hash>.dll`, and makes every program linked against it import that exact name.
//!
//! `kubuno-ui` is a Rust `dylib`, and a Rust dylib has no stable ABI: rebuilding it (a new impl
//! block, another generic instantiation, a new compiler) renames or reshapes the symbols it
//! exports. Under a fixed name, a program linked against an earlier build would then load the new
//! file and die in the loader with "entry point not found" (`0xC0000139`), or worse, run on a
//! layout it was not compiled for. With a per-build name, two builds coexist in one folder or on
//! `PATH`, each program loads the one it was linked against, and a missing one is reported by the
//! loader under its own name (`kubuno_ui-<hash>.dll` was not found).
//!
//! Cargo names a path dylib `kubuno_ui.dll`, and rustc gives MSVC's `link.exe` that name as
//! `/OUT:`, which is also the name the import library records (the one every dependent then
//! imports). The only moment where the name can change is the link itself, so this script
//! installs a copy of itself as `link.exe` for this package's rustc invocations (and theirs
//! only), through the documented `rustc-env` build-script instruction:
//!
//! * `VCINSTALLDIR` set makes rustc look for `link.exe` on `PATH` (the "developer prompt" rule
//!   of its MSVC discovery), and `PATH` starts with the shim's folder;
//! * the shim finds the real `link.exe` the way rustc would have (`find-msvc-tools`, the same
//!   crate rustc uses) and forwards every link to it unchanged, except the one producing
//!   `kubuno_ui.dll`, whose `/OUT:` becomes `kubuno_ui-<hash>.dll` (hence the PDB becomes
//!   `kubuno_ui-<hash>.pdb`, and the import library `kubuno_ui.dll.lib` records the hashed name);
//! * after that link, `kubuno_ui.dll` / `kubuno_ui.pdb` are re-created as hard links to the
//!   hashed files, since Cargo and rustc keep reading the crate's metadata from the former.
//!
//! The hash covers every input of that link (arguments, and each input file's size and time, or
//! content for rustc's temporary files), so it changes whenever the DLL is relinked from anything
//! different, which is whenever its ABI can change. Where a program looks for its DLL, and why
//! this mechanism rather than another: `BUILD.md` ("Bibliothèque de composants partagée") and
//! `vskubuno/docs/DESIGNER.md` ("One file name per kubuno_ui build").

use std::env;
use std::process::ExitCode;

/// Set (to the shim's folder) in the environment of this package's rustc invocations, and hence
/// of the linker they spawn: its presence means "this process is the link shim".
const SHIM_VAR: &str = "KUBUNO_UI_LINK_SHIM";

fn main() -> ExitCode {
    match env::var_os(SHIM_VAR) {
        Some(dir) => shim::run(std::path::PathBuf::from(dir)),
        None => {
            install::run();
            ExitCode::SUCCESS
        }
    }
}

/// The build-script role: install the shim and point this package's rustc at it.
mod install {
    use std::env;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    pub fn run() {
        println!("cargo::rerun-if-changed=build.rs");
        let target_is_msvc = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
            && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
        if !cfg!(windows) || !target_is_msvc {
            return;
        }

        let Some(arch) = env::var("CARGO_CFG_TARGET_ARCH").ok().and_then(|a| vs_arch(&a).map(|vs| (a, vs))) else {
            return;
        };
        let Some(out_dir) = env::var_os("OUT_DIR").map(PathBuf::from) else {
            return;
        };
        let shim_dir = out_dir.join("link-shim");
        if let Err(error) = install_shim(&shim_dir) {
            // Not fatal: the build goes on with the plain `kubuno_ui.dll` name.
            println!("cargo::warning=kubuno_ui.dll keeps its plain name: the link shim could not be installed in {} ({error})", shim_dir.display());
            return;
        }

        let dir = shim_dir.display().to_string();
        let original_vcinstalldir = env::var("VCINSTALLDIR").unwrap_or_default();
        let original_vscmd_arch = env::var("VSCMD_ARG_TGT_ARCH").unwrap_or_default();
        let path = env::var("PATH").unwrap_or_default();
        println!("cargo::rustc-env={}={dir}", super::SHIM_VAR);
        println!("cargo::rustc-env={}_ARCH={}", super::SHIM_VAR, arch.0);
        println!("cargo::rustc-env={}_VCINSTALLDIR={original_vcinstalldir}", super::SHIM_VAR);
        println!("cargo::rustc-env={}_VSCMD_ARG_TGT_ARCH={original_vscmd_arch}", super::SHIM_VAR);
        // rustc's MSVC discovery: with VCINSTALLDIR set and VSCMD_ARG_TGT_ARCH naming the target,
        // `link.exe` is taken from PATH - where the shim comes first.
        println!("cargo::rustc-env=VCINSTALLDIR={dir}");
        println!("cargo::rustc-env=VSCMD_ARG_TGT_ARCH={}", arch.1);
        println!("cargo::rustc-env=PATH={dir};{path}");
    }

    /// Visual Studio's name of a target architecture (`VSCMD_ARG_TGT_ARCH`).
    fn vs_arch(arch: &str) -> Option<&'static str> {
        match arch {
            "x86_64" => Some("x64"),
            "x86" => Some("x86"),
            "aarch64" => Some("arm64"),
            _ => None,
        }
    }

    /// Copies this very program to `<dir>\link.exe`, with the Rust runtime it may need (a
    /// `-C prefer-dynamic` build script imports `std-<hash>.dll`).
    fn install_shim(dir: &Path) -> io::Result<()> {
        fs::create_dir_all(dir)?;
        let exe = env::current_exe()?;
        copy_if_changed(&exe, &dir.join("link.exe"))?;
        for std_dll in toolchain_std_dlls() {
            if let Some(name) = std_dll.file_name() {
                copy_if_changed(&std_dll, &dir.join(name))?;
            }
        }
        Ok(())
    }

    fn copy_if_changed(from: &Path, to: &Path) -> io::Result<()> {
        let same = match (fs::metadata(from), fs::metadata(to)) {
            (Ok(a), Ok(b)) => a.len() == b.len() && a.modified().ok() == b.modified().ok(),
            _ => false,
        };
        if !same {
            fs::copy(from, to)?;
        }
        Ok(())
    }

    /// The toolchain's `std-*.dll` (`<sysroot>\bin`), empty when rustc cannot say.
    fn toolchain_std_dlls() -> Vec<PathBuf> {
        let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let Ok(output) = Command::new(rustc).args(["--print", "sysroot"]).output() else {
            return Vec::new();
        };
        let sysroot = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let Ok(entries) = fs::read_dir(Path::new(&sysroot).join("bin")) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("std-") && n.ends_with(".dll"))
            })
            .collect()
    }
}

/// The linker role: forward to MSVC's `link.exe`, renaming `kubuno_ui.dll` on the way.
mod shim {
    use std::env;
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::hash::{DefaultHasher, Hash, Hasher};
    use std::io;
    use std::path::{Path, PathBuf};
    use std::process::{Command, ExitCode};
    use std::time::UNIX_EPOCH;

    use find_msvc_tools::{Env, EnvGetter};

    /// The library's file stem, as Cargo names it (`[lib]` name `kubuno_ui`).
    const STEM: &str = "kubuno_ui";

    /// Hashed builds kept in the output folder (the newest first): programs linked against a
    /// recent previous build still start from the build folder; older ones are deleted.
    const KEEP: usize = 3;

    /// Bumped whenever the hash's recipe changes.
    const SCHEME: &str = "kubuno-ui-link-shim/1";

    pub fn run(shim_dir: PathBuf) -> ExitCode {
        let args: Vec<OsString> = env::args_os().skip(1).collect();
        match link(&shim_dir, args) {
            Ok(code) => code,
            Err(error) => {
                eprintln!("kubuno-ui link shim: {error}");
                ExitCode::FAILURE
            }
        }
    }

    fn link(shim_dir: &Path, args: Vec<OsString>) -> io::Result<ExitCode> {
        let path = cleaned_path(shim_dir);
        let arch = env::var(format!("{}_ARCH", super::SHIM_VAR)).unwrap_or_else(|_| "x86_64".into());
        let getter = OriginalEnv { path: path.clone() };
        let tool = find_msvc_tools::find_tool_with_env(&arch, "link.exe", &getter)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "MSVC's link.exe was not found (is the C++ build tools workload installed?)"))?;

        let mut command = Command::new(tool.path());
        let mut tool_path = None;
        for (key, value) in tool.env() {
            if key.eq_ignore_ascii_case("PATH") {
                tool_path = Some(value.clone());
            } else {
                command.env(key, value);
            }
        }
        command.env("PATH", tool_path.unwrap_or(path));
        for var in ["VCINSTALLDIR", "VSCMD_ARG_TGT_ARCH"] {
            match env::var_os(format!("{}_{var}", super::SHIM_VAR)).filter(|v| !v.is_empty()) {
                Some(original) => command.env(var, original),
                None => command.env_remove(var),
            };
        }
        for suffix in ["", "_ARCH", "_VCINSTALLDIR", "_VSCMD_ARG_TGT_ARCH"] {
            command.env_remove(format!("{}{suffix}", super::SHIM_VAR));
        }

        let Some(mut line) = CommandLine::read(&args)? else {
            // Not a rustc-shaped command line: forwarded as is.
            return status(command.args(&args));
        };
        let Some(out_index) = line.output_of_our_dll() else {
            return status(command.args(&args));
        };

        let out = PathBuf::from(&line.args[out_index][5..]);
        let dir = out.parent().map(Path::to_path_buf).unwrap_or_default();
        let hashed_name = format!("{STEM}-{}.dll", line.input_hash(out_index));
        let hashed = dir.join(&hashed_name);
        line.args[out_index] = format!("/OUT:{}", hashed.display());
        let code = status(line.apply(&mut command)?)?;
        if code != ExitCode::SUCCESS {
            return Ok(code);
        }

        // Cargo and rustc go on reading the crate's metadata from `kubuno_ui.dll`: the plain names
        // become aliases of this build's files (hard links, so no copy of a 20+ MB DLL).
        for extension in ["dll", "pdb"] {
            let source = hashed.with_extension(extension);
            if source.exists() {
                alias(&source, &dir.join(format!("{STEM}.{extension}")))?;
            }
        }
        prune(&dir, &hashed_name);
        publish_runtime(&dir, shim_dir, &hashed_name);
        Ok(ExitCode::SUCCESS)
    }

    /// Makes the programs of the profile folder runnable by double-click: Cargo leaves the exes in
    /// `target\<profile>\` (and `examples\`) but the DLL in `deps\`, and Windows only looks beside
    /// the exe. The hashed DLL (and its PDB) is hard-linked next to them, along with the
    /// toolchain's `std-*.dll` (which the install step copied into the shim's folder).
    ///
    /// The coexistence guarantee is the one of `deps\`: other hashed builds are never touched
    /// here, and the same [`prune`] keeps the newest [`KEEP`] of them in each folder. Best
    /// effort: a failure only means the programs need `stage-runtime.ps1` again.
    fn publish_runtime(deps: &Path, shim_dir: &Path, hashed_name: &str) {
        if !deps.file_name().and_then(OsStr::to_str).is_some_and(|n| n.eq_ignore_ascii_case("deps")) {
            return;
        }
        let Some(profile) = deps.parent() else {
            return;
        };
        let std_dlls: Vec<PathBuf> = fs::read_dir(shim_dir)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.file_name().and_then(OsStr::to_str).is_some_and(|n| n.starts_with("std-") && n.ends_with(".dll")))
                    .collect()
            })
            .unwrap_or_default();
        for folder in [profile.to_path_buf(), profile.join("examples")] {
            if fs::create_dir_all(&folder).is_err() {
                continue;
            }
            for extension in ["dll", "pdb"] {
                let source = deps.join(hashed_name).with_extension(extension);
                if source.exists() {
                    if let Some(name) = source.file_name() {
                        let _ = place(&source, &folder.join(name));
                    }
                }
            }
            for std_dll in &std_dlls {
                if let Some(name) = std_dll.file_name() {
                    let _ = place(std_dll, &folder.join(name));
                }
            }
            prune(&folder, hashed_name);
        }
    }

    /// Puts a hard link (or, failing that, a copy) of `source` at `dest`, unless a file of the
    /// same size is already there (the same build staged by `stage-runtime.ps1`). Created under a
    /// temporary name and renamed, so `dest` is never seen half-written.
    fn place(source: &Path, dest: &Path) -> io::Result<()> {
        let same_size = |a: &Path, b: &Path| matches!((fs::metadata(a), fs::metadata(b)), (Ok(x), Ok(y)) if x.len() == y.len());
        if same_size(source, dest) {
            return Ok(());
        }
        let pid = std::process::id();
        let mut name = dest.as_os_str().to_os_string();
        name.push(format!(".new-{pid}"));
        let fresh = PathBuf::from(name);
        let _ = fs::remove_file(&fresh);
        if fs::hard_link(source, &fresh).is_err() {
            fs::copy(source, &fresh)?;
        }
        let renamed = fs::rename(&fresh, dest);
        if renamed.is_err() {
            let _ = fs::remove_file(&fresh);
        }
        renamed
    }

    fn status(command: &mut Command) -> io::Result<ExitCode> {
        let status = command.status()?;
        Ok(match status.code() {
            Some(0) => ExitCode::SUCCESS,
            Some(code) => ExitCode::from(u8::try_from(code & 0xff).unwrap_or(1).max(1)),
            None => ExitCode::FAILURE,
        })
    }

    /// PATH without the shim's own folder.
    fn cleaned_path(shim_dir: &Path) -> OsString {
        let path = env::var_os("PATH").unwrap_or_default();
        let kept: Vec<PathBuf> = env::split_paths(&path).filter(|p| !same_path(p, shim_dir)).collect();
        env::join_paths(kept).unwrap_or(path)
    }

    fn same_path(a: &Path, b: &Path) -> bool {
        let norm = |p: &Path| p.to_string_lossy().trim_end_matches(['\\', '/']).to_ascii_lowercase();
        norm(a) == norm(b)
    }

    /// The environment rustc's MSVC discovery would have seen without the shim.
    struct OriginalEnv {
        path: OsString,
    }

    impl EnvGetter for OriginalEnv {
        fn get_env(&self, name: &'static str) -> Option<Env> {
            let value = match name {
                "PATH" => Some(self.path.clone()),
                "VCINSTALLDIR" | "VSCMD_ARG_TGT_ARCH" => {
                    env::var_os(format!("{}_{name}", super::SHIM_VAR)).filter(|v| !v.is_empty())
                }
                _ => env::var_os(name),
            };
            value.map(Env::Owned)
        }
    }

    /// Replaces `link` by a hard link (or, failing that, a copy) of `source`, in one rename so that
    /// `link` never goes missing for a rustc reading it meanwhile. A `link` still held by a running
    /// process cannot be replaced, but can be renamed out of the way first.
    fn alias(source: &Path, link: &Path) -> io::Result<()> {
        let extension = link.extension().and_then(OsStr::to_str).unwrap_or_default().to_string();
        let pid = std::process::id();
        let fresh = link.with_extension(format!("{extension}.new-{pid}"));
        let _ = fs::remove_file(&fresh);
        if fs::hard_link(source, &fresh).is_err() {
            fs::copy(source, &fresh)?;
        }
        if fs::rename(&fresh, link).is_ok() {
            return Ok(());
        }
        let stale = link.with_extension(format!("{extension}.stale-{pid}"));
        let moved = fs::rename(link, &stale).and_then(|()| fs::rename(&fresh, link));
        if moved.is_err() {
            let _ = fs::remove_file(&fresh);
        }
        moved
    }

    /// Deletes the hashed builds beyond the newest [`KEEP`] (and stale aliases), best effort:
    /// a file a running program holds is left for the next link.
    fn prune(dir: &Path, current: &str) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut builds = Vec::new();
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(&format!("{STEM}.")) && name.contains(".stale-") {
                let _ = fs::remove_file(entry.path());
            } else if name != current && is_hashed_dll(&name) {
                let time = entry.metadata().and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH);
                builds.push((time, entry.path()));
            }
        }
        builds.sort_by_key(|build| std::cmp::Reverse(build.0));
        for (_, dll) in builds.into_iter().skip(KEEP.saturating_sub(1)) {
            if fs::remove_file(&dll).is_ok() {
                let _ = fs::remove_file(dll.with_extension("pdb"));
            }
        }
    }

    /// `kubuno_ui-<16 hex digits>.dll`.
    pub fn is_hashed_dll(name: &str) -> bool {
        name.strip_prefix(STEM)
            .and_then(|rest| rest.strip_prefix('-'))
            .and_then(|rest| rest.strip_suffix(".dll"))
            .is_some_and(|hash| hash.len() == 16 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
    }

    /// A linker command line as rustc writes it: inline arguments, or one `@response-file`.
    struct CommandLine {
        args: Vec<String>,
        response: Option<Response>,
    }

    struct Response {
        path: PathBuf,
        utf16: bool,
    }

    impl CommandLine {
        /// `None` when the arguments are not all Unicode (then forwarded untouched).
        fn read(args: &[OsString]) -> io::Result<Option<Self>> {
            let Some(strings) = args.iter().map(|a| a.to_str().map(str::to_string)).collect::<Option<Vec<_>>>() else {
                return Ok(None);
            };
            if let [single] = strings.as_slice() {
                if let Some(path) = single.strip_prefix('@') {
                    let bytes = fs::read(path)?;
                    let (text, utf16) = decode(&bytes);
                    return Ok(Some(Self {
                        args: parse_response(&text),
                        response: Some(Response { path: PathBuf::from(path), utf16 }),
                    }));
                }
            }
            Ok(Some(Self { args: strings, response: None }))
        }

        /// The index of `/OUT:<...>\kubuno_ui.dll` when this links our DLL.
        fn output_of_our_dll(&self) -> Option<usize> {
            let is_dll = self.args.iter().any(|a| a.eq_ignore_ascii_case("/DLL") || a.eq_ignore_ascii_case("-DLL"));
            if !is_dll {
                return None;
            }
            self.args.iter().position(|a| {
                option_value(a, "OUT").is_some_and(|out| {
                    Path::new(out)
                        .file_name()
                        .and_then(OsStr::to_str)
                        .is_some_and(|n| n.eq_ignore_ascii_case(&format!("{STEM}.dll")))
                })
            })
        }

        /// A hash of everything the link reads: the arguments (rustc's temporary folder left
        /// out of them), and every input file - its content when it lives in that temporary
        /// folder (rewritten on every run), its size and modification time otherwise.
        fn input_hash(&self, out_index: usize) -> String {
            let temp = self
                .args
                .iter()
                .find_map(|a| option_value(a, "DEF"))
                .and_then(|def| Path::new(def).parent())
                .map(|p| p.display().to_string());
            let mut hasher = NameHasher::default();
            SCHEME.hash(&mut hasher);
            for (index, arg) in self.args.iter().enumerate() {
                if index == out_index {
                    continue;
                }
                let is_output = ["IMPLIB", "PDB", "PDBALTPATH", "ILK", "MAP"].iter().any(|o| option_value(arg, o).is_some());
                if is_output {
                    continue;
                }
                let normalized = match &temp {
                    Some(t) => arg.replace(t.as_str(), "<rustc-temp>"),
                    None => arg.clone(),
                };
                normalized.hash(&mut hasher);
                let file = if arg.starts_with('/') || arg.starts_with('-') {
                    arg.split_once(':').map(|(_, v)| v)
                } else {
                    Some(arg.as_str())
                };
                let Some(file) = file.filter(|f| !f.is_empty()) else {
                    continue;
                };
                let Ok(meta) = fs::metadata(file) else {
                    continue;
                };
                if !meta.is_file() {
                    continue;
                }
                let in_temp = temp.as_deref().is_some_and(|t| file.starts_with(t));
                if in_temp {
                    if let Ok(content) = fs::read(file) {
                        content.hash(&mut hasher);
                    }
                } else {
                    meta.len().hash(&mut hasher);
                    let time = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos());
                    time.hash(&mut hasher);
                }
            }
            hasher.hex()
        }

        /// The command running this line: inline arguments, or a rewritten response file next
        /// to the original, in the original's encoding.
        fn apply<'a>(&self, command: &'a mut Command) -> io::Result<&'a mut Command> {
            match &self.response {
                None => Ok(command.args(&self.args)),
                Some(response) => {
                    let text: String = self.args.iter().map(|a| format!("\"{}\"\n", a.replace('"', "\\\""))).collect();
                    let bytes = encode(&text, response.utf16);
                    let mut name = response.path.as_os_str().to_os_string();
                    name.push(".kubuno-ui");
                    let path = PathBuf::from(name);
                    fs::write(&path, bytes)?;
                    let mut arg = OsString::from("@");
                    arg.push(&path);
                    Ok(command.arg(arg))
                }
            }
        }
    }

    /// The value of `/NAME:value` (or `-NAME:value`), case-insensitively.
    fn option_value<'a>(arg: &'a str, name: &str) -> Option<&'a str> {
        let rest = arg.strip_prefix('/').or_else(|| arg.strip_prefix('-'))?;
        let (option, value) = rest.split_once(':')?;
        option.eq_ignore_ascii_case(name).then_some(value)
    }

    fn decode(bytes: &[u8]) -> (String, bool) {
        if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
            let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
            (String::from_utf16_lossy(&units), true)
        } else {
            let rest = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
            (String::from_utf8_lossy(rest).into_owned(), false)
        }
    }

    fn encode(text: &str, utf16: bool) -> Vec<u8> {
        if utf16 {
            let mut bytes = vec![0xFF, 0xFE];
            for unit in text.encode_utf16() {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
            bytes
        } else {
            text.as_bytes().to_vec()
        }
    }

    /// Splits a response file the way rustc writes one for `link.exe`: arguments separated by
    /// white space, double quotes grouping, `\"` a literal quote.
    fn parse_response(text: &str) -> Vec<String> {
        let mut args = Vec::new();
        let mut current = String::new();
        let mut in_arg = false;
        let mut quoted = false;
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\\' if chars.peek() == Some(&'"') => {
                    chars.next();
                    current.push('"');
                    in_arg = true;
                }
                '"' => {
                    quoted = !quoted;
                    in_arg = true;
                }
                c if c.is_whitespace() && !quoted => {
                    if in_arg {
                        args.push(std::mem::take(&mut current));
                        in_arg = false;
                    }
                }
                c => {
                    current.push(c);
                    in_arg = true;
                }
            }
        }
        if in_arg {
            args.push(current);
        }
        args
    }

    /// A 64-bit hash, as 16 hex digits (the standard library's SipHash with fixed keys:
    /// deterministic for a given build of this program, which is all a file name needs).
    #[derive(Default)]
    struct NameHasher(DefaultHasher);

    impl Hasher for NameHasher {
        fn write(&mut self, bytes: &[u8]) {
            self.0.write(bytes);
        }

        fn finish(&self) -> u64 {
            self.0.finish()
        }
    }

    impl NameHasher {
        fn hex(&self) -> String {
            format!("{:016x}", self.finish())
        }
    }
}
