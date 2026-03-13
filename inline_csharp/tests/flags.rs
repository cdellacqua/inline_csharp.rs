use inline_csharp::{ct_csharp, csharp};

/// Build a tiny C# class library DLL at `dll_path` (skipped if already present).
///
/// Creates `<parent>/_src/` with a minimal `.csproj` + `Greeter.cs`, builds it,
/// then copies the DLL from `bin/Debug/<tfm>/` to `dll_path`.
///
/// Note: `dotnet build -o` produces a 3584-byte stripped copy that the C#
/// compiler cannot use as a reference; the DLL in `bin/Debug/<tfm>/` is the
/// full managed assembly and must be copied manually.
fn build_demo_dll(dll_path: &str) {
	let dll = std::path::Path::new(dll_path);
	if dll.exists() {
		return;
	}
	let out_dir = dll.parent().expect("dll_path must have a parent directory");
	let src_dir = out_dir.join("_src");
	std::fs::create_dir_all(&src_dir).unwrap();

	// Detect installed TFM (e.g. "net8.0") by parsing `dotnet --version`.
	let ver_out = std::process::Command::new("dotnet")
		.arg("--version")
		.output()
		.expect("dotnet not found");
	let ver_str = String::from_utf8_lossy(&ver_out.stdout);
	let major = ver_str.trim().split('.').next().unwrap_or("8");
	let tfm = format!("net{major}.0");

	std::fs::write(
		src_dir.join("Greeter.csproj"),
		format!(
			"<Project Sdk=\"Microsoft.NET.Sdk\">\n  \
			 <PropertyGroup>\n    \
			 <TargetFramework>{tfm}</TargetFramework>\n    \
			 <Nullable>enable</Nullable>\n  \
			 </PropertyGroup>\n\
			 </Project>\n"
		),
	)
	.unwrap();

	std::fs::write(
		src_dir.join("Greeter.cs"),
		"namespace GreeterLib;\npublic class Greeter {\n    public string Greet() => \"Hello, World!\";\n}\n",
	)
	.unwrap();

	let status = std::process::Command::new("dotnet")
		.args(["build", src_dir.to_str().unwrap(), "--nologo", "-v", "quiet"])
		.status()
		.expect("dotnet build failed");
	assert!(status.success(), "build_demo_dll: dotnet build failed");

	// Copy the full managed DLL from bin/Debug/<tfm>/ — NOT the -o output,
	// which is a stripped apphost copy unusable as a compiler reference.
	let built_dll = src_dir
		.join("bin")
		.join("Debug")
		.join(&tfm)
		.join("Greeter.dll");
	std::fs::copy(&built_dll, dll).unwrap_or_else(|e| {
		panic!("failed to copy {built_dll:?} → {dll:?}: {e}");
	});
}

// build = "--nologo" : suppress MSBuild header output

#[test]
fn csharp_runtime_build_flag() {
	let val: Result<i32, _> = csharp! {
		build = "--nologo",
		static int Run() {
			return 42;
		}
	};
	assert_eq!(val, Ok(42i32));
}

// build = "..." : multiple build flags split on whitespace

#[test]
fn csharp_runtime_multiple_build_flags() {
	let val: Result<i32, _> = csharp! {
		build = "--nologo --verbosity quiet",
		static int Run() {
			return 7;
		}
	};
	assert_eq!(val, Ok(7i32));
}

// ct_csharp! with build = "..."

const CT_BUILD_FLAG: i32 = ct_csharp! {
	build = "--nologo",
	static int Run() {
		return 1 + 1;
	}
};

#[test]
fn ct_csharp_build_flag() {
	assert_eq!(CT_BUILD_FLAG, 2);
}

// run = "..." : single runtime arg passed to the DLL process

#[test]
fn csharp_runtime_run_flag() {
	let val: Result<String, _> = csharp! {
		run = "hello",
		static string Run() {
			return System.Environment.GetCommandLineArgs()[1];
		}
	};
	assert_eq!(val, Ok("hello".to_string()));
}

// run = "..." : multiple runtime args split on whitespace

#[test]
fn csharp_runtime_multiple_run_flags() {
	let val: Result<String, _> = csharp! {
		run = "foo bar",
		static string Run() {
			var args = System.Environment.GetCommandLineArgs();
			return args[1] + " " + args[2];
		}
	};
	assert_eq!(val, Ok("foo bar".to_string()));
}

// build = "..." + run = "..." : both flags work together

#[test]
fn csharp_runtime_build_and_run_flags() {
	let val: Result<String, _> = csharp! {
		build = "--nologo",
		run = "combined",
		static string Run() {
			return System.Environment.GetCommandLineArgs()[1];
		}
	};
	assert_eq!(val, Ok("combined".to_string()));
}

// ct_csharp! with run = "..."

const CT_RUN_FLAG: &str = ct_csharp! {
	run = "ct42",
	static string Run() {
		return System.Environment.GetCommandLineArgs()[1];
	}
};

#[test]
fn ct_csharp_run_flag() {
	assert_eq!(CT_RUN_FLAG, "ct42");
}

// reference = "..." : call into an external DLL (C# class library)

#[test]
fn csharp_runtime_reference_dll() {
	build_demo_dll("/tmp/inline_csharp_flags_ref_dll/Greeter.dll");
	let val: Result<String, _> = csharp! {
		reference = "/tmp/inline_csharp_flags_ref_dll/Greeter.dll",
		using GreeterLib;
		static string Run() {
			return new Greeter().Greet();
		}
	};
	assert_eq!(val, Ok("Hello, World!".to_string()));
}
