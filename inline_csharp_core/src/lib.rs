//! Core runtime support for `inline_csharp`.
//!
//! This crate is an implementation detail of `inline_csharp_macros`.  End users
//! should depend on `inline_csharp` instead of this crate directly.
//!
//! Public items:
//!
//! - [`CsharpError`] — error type returned by [`run_csharp`] and by the `csharp!` /
//!   `csharp_fn!` macros at program runtime.
//! - [`run_csharp`] — compile (if needed) and run a generated C# class.
//! - [`expand_dotnet_args`] — shell-expand an option string into individual args.
//! - [`cache_dir`] — compute the deterministic temp-dir path for a C# class.

use shellexpand::full_with_context_no_errors;

/// Detect the installed .NET target framework moniker (e.g. `"net8.0"`) by
/// running `dotnet --version` and extracting the major version number.
///
/// # Errors
///
/// Returns [`CsharpError::Io`] if `dotnet` cannot be spawned or its output
/// cannot be parsed as a version string.
pub fn detect_target_framework() -> Result<String, CsharpError> {
	let output = std::process::Command::new("dotnet")
		.arg("--version")
		.output()
		.map_err(|e| CsharpError::Io(format!("failed to run `dotnet --version`: {e}")))?;
	let stdout = String::from_utf8_lossy(&output.stdout);
	let major = stdout
		.trim()
		.split('.')
		.next()
		.and_then(|s| s.parse::<u32>().ok())
		.ok_or_else(|| {
			CsharpError::Io(format!(
				"could not parse major version from `dotnet --version` output: {stdout:?}"
			))
		})?;
	Ok(format!("net{major}.0"))
}

/// All errors that `csharp!` and `csharp_fn!` can return at runtime (and that
/// `ct_csharp!` maps to `compile_error!` diagnostics at build time).
#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone)]
pub enum CsharpError {
	/// An I/O error while creating the temp directory, writing the source
	/// file, or spawning `dotnet` (e.g. the binary is not on `PATH`).
	#[error("inline_csharp: I/O error: {0}")]
	Io(String),

	/// `dotnet build` exited with a non-zero status.  The `0` field contains
	/// the compiler diagnostic output (stderr).
	#[error("inline_csharp: dotnet build failed:\n{0}")]
	CompilationFailed(String),

	/// The dotnet runtime exited with a non-zero status (e.g. an unhandled
	/// exception).  The `0` field contains the exception message and stack
	/// trace (stderr).
	#[error("inline_csharp: dotnet runtime failed:\n{0}")]
	RuntimeFailed(String),

	/// The C# program returned bytes that are not valid UTF-8.
	#[error("inline_csharp: C# output is not valid UTF-8: {0}")]
	InvalidUtf8(#[from] std::string::FromUtf8Error),

	/// The C# program returned a `char` value that is not a valid Unicode
	/// scalar (i.e. a lone surrogate half).
	#[error("inline_csharp: C# char is not a valid Unicode scalar value")]
	InvalidChar,
}

/// Shell-expand `raw` (expanding env vars and `~`), then split into individual
/// arguments (respecting quotes).
/// Returns an empty vec if `raw` is empty.
///
/// # Examples
///
/// ```rust
/// use inline_csharp_core::expand_dotnet_args;
///
/// let args = expand_dotnet_args("--configuration Release --nologo");
/// assert_eq!(args, vec!["--configuration", "Release", "--nologo"]);
///
/// let empty = expand_dotnet_args("");
/// assert!(empty.is_empty());
/// ```
#[must_use]
pub fn expand_dotnet_args(raw: &str) -> Vec<String> {
	if raw.is_empty() {
		return Vec::new();
	}
	let expanded = full_with_context_no_errors(
		raw,
		|| std::env::var("HOME").ok(),
		|var| std::env::var(var).ok(),
	);
	split_args(&expanded)
}

/// Split a shell-style argument string into individual arguments, respecting
/// single- and double-quoted spans.
fn split_args(s: &str) -> Vec<String> {
	let mut args: Vec<String> = Vec::new();
	let mut cur = String::new();
	let mut in_single = false;
	let mut in_double = false;

	for ch in s.chars() {
		match ch {
			'\'' if !in_double => in_single = !in_single,
			'"' if !in_single => in_double = !in_double,
			' ' | '\t' if !in_single && !in_double => {
				if !cur.is_empty() {
					args.push(std::mem::take(&mut cur));
				}
			}
			_ => cur.push(ch),
		}
	}
	if !cur.is_empty() {
		args.push(cur);
	}
	args
}

/// Resolve the root directory used to cache compiled C# assemblies.
///
/// Resolution order:
/// 1. `INLINE_CSHARP_CACHE_DIR` environment variable, if set and non-empty.
/// 2. The XDG / platform cache directory (`~/.cache/inline_csharp` on Linux,
///    `~/Library/Caches/inline_csharp` on macOS,
///    `%LOCALAPPDATA%\inline_csharp` on Windows) via the [`dirs`] crate.
/// 3. `<system_temp>/inline_csharp` as a final fallback.
#[must_use]
pub fn base_cache_dir() -> std::path::PathBuf {
	if let Ok(v) = std::env::var("INLINE_CSHARP_CACHE_DIR")
		&& !v.is_empty()
	{
		return std::path::PathBuf::from(v);
	}
	if let Some(cache) = dirs::cache_dir() {
		return cache.join("inline_csharp");
	}
	std::env::temp_dir().join("inline_csharp")
}

/// Compute the deterministic cache-dir path used to store compiled C# assemblies.
///
/// The path is `<base_cache_dir>/<class_name>_<hex_hash>/` where `hex_hash` is a
/// 64-bit hash of:
/// - `csharp_source` — the complete C# source text
/// - `expand_dotnet_args(build_raw)` — shell-expanded build args
/// - `std::env::current_dir()` — the process working directory at call time
/// - `run_raw` — hashed as a raw string
/// - `references` — the list of reference DLL paths
/// - `target_framework` — the TFM moniker (e.g. `"net8.0"`)
///
/// The base directory is resolved by [`base_cache_dir`].
#[must_use]
#[allow(clippy::similar_names)]
pub fn cache_dir(
	class_name: &str,
	csharp_source: &str,
	build_raw: &str,
	run_raw: &str,
	references: &[std::path::PathBuf],
	target_framework: &str,
) -> std::path::PathBuf {
	use std::collections::hash_map::DefaultHasher;
	use std::hash::{Hash, Hasher};

	let mut h = DefaultHasher::new();
	csharp_source.hash(&mut h);
	expand_dotnet_args(build_raw).hash(&mut h); // shell-expanded; CWD handles relative paths
	std::env::current_dir().ok().hash(&mut h); // anchors relative paths in build_raw
	run_raw.hash(&mut h);
	references.hash(&mut h);
	target_framework.hash(&mut h);

	let hex = format!("{:016x}", h.finish());
	base_cache_dir().join(format!("{class_name}_{hex}"))
}

/// Generate the XML content for a `.csproj` file.
///
/// `target_framework` is the TFM moniker (e.g. `"net8.0"`, `"net10.0"`)
/// returned by [`detect_target_framework`].  When `references` is non-empty,
/// an `<ItemGroup>` with `<Reference>` entries is included; `references` are
/// expected to be absolute DLL paths.
#[must_use]
pub fn generate_csproj(
	class_name: &str,
	target_framework: &str,
	references: &[std::path::PathBuf],
) -> String {
	let mut xml = format!(
		r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>{target_framework}</TargetFramework>
    <AssemblyName>{class_name}</AssemblyName>
    <Nullable>enable</Nullable>
    <ImplicitUsings>disable</ImplicitUsings>
    <Optimize>true</Optimize>
  </PropertyGroup>
"#
	);

	if !references.is_empty() {
		xml.push_str("  <ItemGroup>\n");
		for r in references {
			let path = r.to_string_lossy();
			xml.push_str(&format!("    <Reference Include=\"{path}\" />\n"));
		}
		xml.push_str("  </ItemGroup>\n");
	}

	xml.push_str("</Project>\n");
	xml
}

/// Compile (if needed) and run a generated C# class, returning raw stdout bytes.
///
/// Both the compile step (`dotnet build`) and the run step (`dotnet <dll>`) are
/// guarded by a per-class-name file lock so that concurrent invocations cooperate
/// correctly.  A `.done` sentinel and an optimistic pre-check skip recompilation
/// on subsequent calls without acquiring the lock.
///
/// - `class_name`    — bare class name; used as the project/file name.
/// - `csharp_source` — complete `.cs` source to write.
/// - `build_raw`     — raw `build = "..."` option string (shell-expanded).
/// - `run_raw`       — raw `run = "..."` option string (shell-expanded).
/// - `references`    — paths to reference DLLs (may be relative; will be absolutized).
/// - `stdin_bytes`   — bytes to pipe to the child process's stdin (may be empty).
///
/// # Errors
///
/// Returns [`CsharpError::Io`] if the temp directory, source file, or lock file
/// cannot be created, or if `dotnet` cannot be spawned.
/// Returns [`CsharpError::CompilationFailed`] if `dotnet build` exits with a non-zero status.
/// Returns [`CsharpError::RuntimeFailed`] if `dotnet <dll>` exits with a non-zero status.
#[allow(clippy::similar_names)]
pub fn run_csharp(
	class_name: &str,
	csharp_source: &str,
	build_raw: &str,
	run_raw: &str,
	references: &[&str],
	stdin_bytes: &[u8],
) -> Result<Vec<u8>, CsharpError> {
	use std::io::Write;
	use std::process::Stdio;

	// Absolutize reference paths via current_dir().join(path) — skip
	// canonicalize to avoid "file not found" for not-yet-existing paths.
	let cwd = std::env::current_dir().map_err(|e| CsharpError::Io(e.to_string()))?;
	let abs_refs: Vec<std::path::PathBuf> =
		references.iter().map(|r| cwd.join(r)).collect();

	let tfm = detect_target_framework()?;
	let tmp_dir = cache_dir(class_name, csharp_source, build_raw, run_raw, &abs_refs, &tfm);
	let build_extra = expand_dotnet_args(build_raw);
	let run_extra = expand_dotnet_args(run_raw);

	if !tmp_dir.join(".done").exists() {
		std::fs::create_dir_all(&tmp_dir).map_err(|e| CsharpError::Io(e.to_string()))?;

		let lock_file = std::fs::OpenOptions::new()
			.create(true)
			.truncate(false)
			.write(true)
			.open(tmp_dir.join(".lock"))
			.map_err(|e| CsharpError::Io(e.to_string()))?;
		let mut lock = fd_lock::RwLock::new(lock_file);
		let _guard = lock.write().map_err(|e| CsharpError::Io(e.to_string()))?;

		if !tmp_dir.join(".done").exists() {
			// Write the C# source file.
			std::fs::write(tmp_dir.join(format!("{class_name}.cs")), csharp_source)
				.map_err(|e| CsharpError::Io(e.to_string()))?;

			// Write the .csproj file.
			std::fs::write(
				tmp_dir.join(format!("{class_name}.csproj")),
				generate_csproj(class_name, &tfm, &abs_refs),
			)
			.map_err(|e| CsharpError::Io(e.to_string()))?;

			// Run: dotnet build <class_name>.csproj <build_extra> -o <tmp_dir>/out/ --nologo
			let mut cmd = std::process::Command::new("dotnet");
			cmd.arg("build")
				.arg(format!("{class_name}.csproj"))
				.args(&build_extra)
				.arg("-o")
				.arg(tmp_dir.join("out"))
				.arg("--nologo")
				.current_dir(&tmp_dir);

			let out = cmd.output().map_err(|e| CsharpError::Io(e.to_string()))?;
			if !out.status.success() {
				return Err(CsharpError::CompilationFailed(
					String::from_utf8_lossy(&out.stderr).into_owned(),
				));
			}

			std::fs::write(tmp_dir.join(".done"), b"")
				.map_err(|e| CsharpError::Io(e.to_string()))?;
		}
	}

	// Run phase: dotnet <tmp_dir>/out/<class_name>.dll <run_extra>
	let dll_path = tmp_dir.join("out").join(format!("{class_name}.dll"));
	let mut cmd = std::process::Command::new("dotnet");
	cmd.arg(&dll_path);
	for arg in &run_extra {
		cmd.arg(arg);
	}
	let mut child = cmd
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()
		.map_err(|e| CsharpError::Io(e.to_string()))?;

	// Write stdin bytes then drop the handle to signal EOF.
	if stdin_bytes.is_empty() {
		// Drop stdin handle even when empty so the process doesn't block waiting.
		drop(child.stdin.take());
	} else if let Some(mut stdin_handle) = child.stdin.take() {
		stdin_handle
			.write_all(stdin_bytes)
			.map_err(|e| CsharpError::Io(e.to_string()))?;
	}

	let out = child
		.wait_with_output()
		.map_err(|e| CsharpError::Io(e.to_string()))?;

	if !out.status.success() {
		return Err(CsharpError::RuntimeFailed(
			String::from_utf8_lossy(&out.stderr).into_owned(),
		));
	}

	Ok(out.stdout)
}

#[cfg(test)]
mod tests {
	use super::cache_dir;

	// -----------------------------------------------------------------------
	// cache_dir is idempotent: two calls with identical arguments return the
	// same path.
	// -----------------------------------------------------------------------
	#[test]
	fn cache_dir_idempotent() {
		let a = cache_dir("MyClass", "class body", "-v quiet", "", &[], "net8.0");
		let b = cache_dir("MyClass", "class body", "-v quiet", "", &[], "net8.0");
		assert_eq!(
			a, b,
			"cache_dir must return the same path for identical args"
		);
	}

	// -----------------------------------------------------------------------
	// cache_dir produces different paths for build_raw strings that expand to
	// different argument lists.
	// -----------------------------------------------------------------------
	#[test]
	fn cache_dir_differs_for_different_build_raw() {
		let a = cache_dir("MyClass", "class body", "--configuration Debug", "", &[], "net8.0");
		let b = cache_dir("MyClass", "class body", "--configuration Release", "", &[], "net8.0");
		assert_ne!(
			a, b,
			"cache_dir must differ when build_raw expands to different args"
		);
	}

	// -----------------------------------------------------------------------
	// cache_dir produces different paths when csharp_source differs.
	// -----------------------------------------------------------------------
	#[test]
	fn cache_dir_differs_for_different_csharp_source() {
		let a = cache_dir("MyClass", "class body A", "", "", &[], "net8.0");
		let b = cache_dir("MyClass", "class body B", "", "", &[], "net8.0");
		assert_ne!(a, b, "cache_dir must differ when csharp_source differs");
	}

	// -----------------------------------------------------------------------
	// cache_dir produces different paths when run_raw differs.
	// -----------------------------------------------------------------------
	#[test]
	fn cache_dir_differs_for_different_run_raw() {
		let a = cache_dir("MyClass", "class body", "", "--rollForward Major", &[], "net8.0");
		let b = cache_dir("MyClass", "class body", "", "--rollForward Minor", &[], "net8.0");
		assert_ne!(a, b, "cache_dir must differ when run_raw differs");
	}

	// -----------------------------------------------------------------------
	// cache_dir produces different paths when references differ.
	// -----------------------------------------------------------------------
	#[test]
	fn cache_dir_differs_for_different_references() {
		let refs_a = vec![std::path::PathBuf::from("/path/to/Foo.dll")];
		let refs_b = vec![std::path::PathBuf::from("/path/to/Bar.dll")];
		let a = cache_dir("MyClass", "class body", "", "", &refs_a, "net8.0");
		let b = cache_dir("MyClass", "class body", "", "", &refs_b, "net8.0");
		assert_ne!(a, b, "cache_dir must differ when references differ");
	}

	// -----------------------------------------------------------------------
	// cache_dir result is inside base_cache_dir and uses the class_name as a
	// prefix.
	// -----------------------------------------------------------------------
	#[test]
	fn cache_dir_path_structure() {
		let result = cache_dir("InlineCsharp_abc123", "src", "", "", &[], "net8.0");
		let base = super::base_cache_dir();
		assert!(
			result.starts_with(&base),
			"cache_dir result must be under base_cache_dir ({}); got: {}",
			base.display(),
			result.display()
		);
		let file_name = result.file_name().unwrap().to_string_lossy();
		assert!(
			file_name.starts_with("InlineCsharp_abc123_"),
			"cache_dir result filename must start with the class name; got: {file_name}"
		);
	}

	// -----------------------------------------------------------------------
	// cache_dir produces different paths when target_framework differs.
	// -----------------------------------------------------------------------
	#[test]
	fn cache_dir_differs_for_different_target_framework() {
		let a = cache_dir("MyClass", "class body", "", "", &[], "net8.0");
		let b = cache_dir("MyClass", "class body", "", "", &[], "net10.0");
		assert_ne!(a, b, "cache_dir must differ when target_framework differs");
	}

	// -----------------------------------------------------------------------
	// INLINE_CSHARP_CACHE_DIR env var overrides the base cache directory.
	// -----------------------------------------------------------------------
	#[test]
	fn base_cache_dir_respects_env_var() {
		unsafe { std::env::set_var("INLINE_CSHARP_CACHE_DIR", "/custom/cache") };
		let base = super::base_cache_dir();
		unsafe { std::env::remove_var("INLINE_CSHARP_CACHE_DIR") };
		assert_eq!(base, std::path::PathBuf::from("/custom/cache"));
	}
}
