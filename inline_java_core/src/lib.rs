//! Core runtime support for `inline_java`.
//!
//! This crate is an implementation detail of `inline_java_macros`.  End users
//! should depend on `inline_java` instead of this crate directly.
//!
//! Public items:
//!
//! - [`JavaError`] — error type returned by [`run_java`] and by the `java!` /
//!   `java_fn!` macros at program runtime.
//! - [`run_java`] — compile (if needed) and run a generated Java class.
//! - [`expand_java_args`] — shell-expand an option string into individual args.

/// All errors that `java!` and `java_fn!` can return at runtime (and that
/// `ct_java!` maps to `compile_error!` diagnostics at build time).
#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone)]
pub enum JavaError {
	/// An I/O error while creating the temp directory, writing the source
	/// file, or spawning `javac`/`java` (e.g. the binary is not on `PATH`).
	#[error("inline_java: I/O error: {0}")]
	Io(String),

	/// `javac` exited with a non-zero status.  The `0` field contains the
	/// compiler diagnostic output (stderr).
	#[error("inline_java: javac compilation failed:\n{0}")]
	CompilationFailed(String),

	/// The JVM exited with a non-zero status (e.g. an unhandled exception).
	/// The `0` field contains the exception message and stack trace (stderr).
	#[error("inline_java: java runtime failed:\n{0}")]
	RuntimeFailed(String),

	/// The Java `run()` method returned a `String` whose bytes are not valid
	/// UTF-8.
	#[error("inline_java: Java String output is not valid UTF-8: {0}")]
	InvalidUtf8(#[from] std::string::FromUtf8Error),

	/// The Java `run()` method returned a `char` value that is not a valid
	/// Unicode scalar (i.e. a lone surrogate half).
	#[error("inline_java: Java char is not a valid Unicode scalar value")]
	InvalidChar,
}

/// Shell-expand `raw` with `INLINE_JAVA_CP` resolved to `inline_java_cp`,
/// then split into individual arguments (respecting quotes).
/// Returns an empty vec if `raw` is empty.
///
/// # Examples
///
/// ```rust
/// use inline_java_core::expand_java_args;
///
/// let args = expand_java_args("-verbose:class -cp $INLINE_JAVA_CP", "/tmp/MyClass");
/// assert_eq!(args, vec!["-verbose:class", "-cp", "/tmp/MyClass"]);
///
/// let empty = expand_java_args("", "/tmp/MyClass");
/// assert!(empty.is_empty());
/// ```
#[must_use]
pub fn expand_java_args(raw: &str, inline_java_cp: &str) -> Vec<String> {
	if raw.is_empty() {
		return Vec::new();
	}
	let cp = inline_java_cp.to_owned();
	let expanded = shellexpand::full_with_context_no_errors(
		raw,
		|| std::env::var("HOME").ok(),
		move |var| match var {
			"INLINE_JAVA_CP" => Some(cp.clone()),
			other => std::env::var(other).ok(),
		},
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

/// Compile (if needed) and run a generated Java class, returning raw stdout bytes.
///
/// Both the compile step (javac) and the run step (java) are guarded by a
/// per-class-name file lock so that concurrent invocations cooperate correctly.
/// A `.done` sentinel and an optimistic pre-check skip recompilation on
/// subsequent calls without acquiring the lock.
///
/// - `class_name`      — bare class name; used as the temp-dir name.
/// - `filename`        — `"<class_name>.java"`, written inside the temp dir.
/// - `java_class`      — complete `.java` source to write.
/// - `full_class_name` — package-qualified class name passed to `java`.
/// - `javac_raw`       — raw `javac = "..."` option string (shell-expanded).
/// - `java_raw`        — raw `java  = "..."` option string (shell-expanded).
/// - `stdin_bytes`     — bytes to pipe to the child process's stdin (may be empty).
///
/// # Errors
///
/// Returns [`JavaError::Io`] if the temp directory, source file, or lock file
/// cannot be created, or if `javac`/`java` cannot be spawned.
/// Returns [`JavaError::CompilationFailed`] if `javac` exits with a non-zero status.
/// Returns [`JavaError::RuntimeFailed`] if `java` exits with a non-zero status.
///
/// # Examples
///
/// ```rust,no_run
/// use inline_java_core::run_java;
///
/// let src = "public class Greet {
///     public static void main(String[] args) {
///         System.out.print(\"hi\");
///     }
/// }";
/// let output = run_java("Greet", "Greet.java", src, "Greet", "", "", &[]).unwrap();
/// assert_eq!(output, b"hi");
/// ```
#[allow(clippy::similar_names)]
pub fn run_java(
	class_name: &str,
	filename: &str,
	java_class: &str,
	full_class_name: &str,
	javac_raw: &str,
	java_raw: &str,
	stdin_bytes: &[u8],
) -> Result<Vec<u8>, JavaError> {
	use std::io::Write;
	use std::process::Stdio;

	let tmp_dir = std::env::temp_dir().join(class_name);
	let cp = tmp_dir.to_string_lossy().into_owned();
	let javac_extra = expand_java_args(javac_raw, &cp);
	let java_extra = expand_java_args(java_raw, &cp);

	if !tmp_dir.join(".done").exists() {
		std::fs::create_dir_all(&tmp_dir).map_err(|e| JavaError::Io(e.to_string()))?;

		let lock_file = std::fs::OpenOptions::new()
			.create(true)
			.truncate(false)
			.write(true)
			.open(tmp_dir.join(".lock"))
			.map_err(|e| JavaError::Io(e.to_string()))?;
		let mut lock = fd_lock::RwLock::new(lock_file);
		let _guard = lock.write().map_err(|e| JavaError::Io(e.to_string()))?;

		if !tmp_dir.join(".done").exists() {
			let src = tmp_dir.join(filename);
			std::fs::write(&src, java_class).map_err(|e| JavaError::Io(e.to_string()))?;

			let mut cmd = std::process::Command::new("javac");
			for arg in &javac_extra {
				cmd.arg(arg);
			}
			let out = cmd
				.arg("-d")
				.arg(&tmp_dir)
				.arg(&src)
				.output()
				.map_err(|e| JavaError::Io(e.to_string()))?;
			if !out.status.success() {
				return Err(JavaError::CompilationFailed(
					String::from_utf8_lossy(&out.stderr).into_owned(),
				));
			}

			std::fs::write(tmp_dir.join(".done"), b"")
				.map_err(|e| JavaError::Io(e.to_string()))?;
		}
	}

	let mut cmd = std::process::Command::new("java");
	cmd.arg("-cp").arg(&tmp_dir);
	for arg in &java_extra {
		cmd.arg(arg);
	}
	let mut child = cmd
		.arg(full_class_name)
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()
		.map_err(|e| JavaError::Io(e.to_string()))?;

	// Write stdin bytes then drop the handle to signal EOF.
	if !stdin_bytes.is_empty() {
		if let Some(mut stdin_handle) = child.stdin.take() {
			stdin_handle
				.write_all(stdin_bytes)
				.map_err(|e| JavaError::Io(e.to_string()))?;
		}
	} else {
		// Drop stdin handle even when empty so Java doesn't block waiting.
		drop(child.stdin.take());
	}

	let out = child
		.wait_with_output()
		.map_err(|e| JavaError::Io(e.to_string()))?;

	if !out.status.success() {
		return Err(JavaError::RuntimeFailed(
			String::from_utf8_lossy(&out.stderr).into_owned(),
		));
	}

	Ok(out.stdout)
}
