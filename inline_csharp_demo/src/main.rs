use inline_csharp::{ct_csharp, csharp, csharp_fn};
use std::process::Command;

fn build_demo_dll(out_dir: &str) {
	let manifest_dir = env!("CARGO_MANIFEST_DIR");
	std::fs::create_dir_all(out_dir).expect("create output dir");
	let status = Command::new("dotnet")
		.args([
			"build",
			&format!("{manifest_dir}/DemoLib/DemoLib.csproj"),
			"-o",
			out_dir,
			&format!("-p:BaseIntermediateOutputPath={out_dir}/obj/"),
			"--nologo",
		])
		.status()
		.expect("dotnet");
	assert!(status.success(), "dotnet build failed building demo DLL");
}

#[allow(clippy::too_many_lines)]
fn main() {
	// 1. csharp! runtime, no input — int return
	let x: i32 = csharp! {
		static int Run() {
			int sum = 0;
			for (int i = 1; i <= 10; i++) sum += i;
			return sum;
		}
	}
	.unwrap();
	println!("Sum 1..10 from C#: {x}");

	// 2. csharp_fn! with int parameter — double it
	let n: i32 = 21;
	let doubled: i32 = csharp_fn! {
		static int Run(int n) {
			return n * 2;
		}
	}(n)
	.unwrap();
	println!("{n} * 2 = {doubled}");

	// 3. csharp_fn! with string + string parameters — concatenate
	let greeting = "Hello";
	let target = "World";
	let msg: String = csharp_fn! {
		static string Run(string greeting, string target) {
			return greeting + ", " + target + "!";
		}
	}(greeting, target)
	.unwrap();
	println!("{msg}");

	// 4. ct_csharp! compile-time constant — double (Math.PI)
	println!("PI (baked at compile time): {PI_APPROX}");

	// 5. csharp! with build = "" (simple build flag demo)
	let flagged: i32 = csharp! {
		build = "--nologo",
		static int Run() {
			return 100;
		}
	}
	.unwrap();
	println!("build flag demo: {flagged}");

	// 6. csharp! with reference = (pre-built DLL)
	build_demo_dll("/tmp/inline_csharp_demo_dll");
	let imports_dll: String = csharp! {
		reference = "/tmp/inline_csharp_demo_dll/DemoLib.dll",
		using DemoLib;
		static string Run() {
			return new HelloWorld().Greet();
		}
	}
	.unwrap();
	println!("{imports_dll}");

	// 7. csharp! returning int[]
	let nums: Vec<i32> = csharp! {
		static int[] Run() {
			return new int[] { 10, 20, 30, 40, 50 };
		}
	}
	.unwrap();
	println!("int[] from C#: {nums:?}");

	// 8. csharp! returning List<string>
	let words: Vec<String> = csharp! {
		using System.Collections.Generic;
		static List<string> Run() {
			return new List<string> { "alpha", "beta", "gamma" };
		}
	}
	.unwrap();
	println!("List<string> from C#: {words:?}");

	// 9. ct_csharp! compile-time int array
	println!("first 5 primes (ct_csharp): {PRIMES:?}");

	// 10. ct_csharp! compile-time string array
	println!("days (ct_csharp): {DAYS:?}");

	// 11-14. ct_csharp! Optional (nullable) constants
	assert_eq!(OPT_INT_SOME, Some(99));
	assert_eq!(OPT_INT_NONE, None::<i32>);
	assert_eq!(OPT_STR_SOME, Some(true));
	assert_eq!(OPT_STR_NONE, None::<bool>);
	println!("ct_csharp int? Some: {OPT_INT_SOME:?}");
	println!("ct_csharp int? None: {OPT_INT_NONE:?}");
	println!("ct_csharp bool? Some: {OPT_STR_SOME:?}");
	println!("ct_csharp bool? None: {OPT_STR_NONE:?}");

	// 12. csharp! runtime with int? return — present
	let opt_int_some: Option<i32> = csharp! {
		static int? Run() {
			return 42;
		}
	}
	.unwrap();
	assert_eq!(opt_int_some, Some(42));
	println!("int? present: {opt_int_some:?}");

	// 13. csharp! runtime with int? return — null
	let opt_int_none: Option<i32> = csharp! {
		static int? Run() {
			return null;
		}
	}
	.unwrap();
	assert_eq!(opt_int_none, None);
	println!("int? null: {opt_int_none:?}");

	// 14. csharp! runtime with double? return — present
	let opt_dbl_some: Option<f64> = csharp! {
		static double? Run() {
			return 3.14;
		}
	}
	.unwrap();
	assert!(opt_dbl_some.is_some());
	println!("double? present: {opt_dbl_some:?}");

	// 15. csharp! runtime with double? return — null
	let opt_dbl_none: Option<f64> = csharp! {
		static double? Run() {
			return null;
		}
	}
	.unwrap();
	assert_eq!(opt_dbl_none, None);
	println!("double? null: {opt_dbl_none:?}");

	// 16. csharp_fn! with int? param — Some value
	let result_some: Option<i32> = csharp_fn! {
		static int? Run(int? val) {
			return val.HasValue ? val * 2 : null;
		}
	}(Some(21))
	.unwrap();
	assert_eq!(result_some, Some(42));
	println!("int? param Some -> {result_some:?}");

	// 17. csharp_fn! with int? param — None
	let result_none: Option<i32> = csharp_fn! {
		static int? Run(int? val) {
			return val.HasValue ? val * 2 : null;
		}
	}(None)
	.unwrap();
	assert_eq!(result_none, None);
	println!("int? param None -> {result_none:?}");

	// 18. csharp_fn! with bool? param — Some value
	let result_bool_some: Option<bool> = csharp_fn! {
		static bool? Run(bool? val) {
			return val.HasValue ? !val.Value : null;
		}
	}(Some(true))
	.unwrap();
	assert_eq!(result_bool_some, Some(false));
	println!("bool? param Some -> {result_bool_some:?}");

	// 19. csharp_fn! with bool? param — None
	let result_bool_none: Option<bool> = csharp_fn! {
		static bool? Run(bool? val) {
			return val.HasValue ? !val.Value : null;
		}
	}(None)
	.unwrap();
	assert_eq!(result_bool_none, None);
	println!("bool? param None -> {result_bool_none:?}");
}

// compile-time constant: System.Math.PI baked at compile time
#[allow(clippy::approx_constant)]
const PI_APPROX: f64 = ct_csharp! {
	static double Run() {
		return System.Math.PI;
	}
};

// compile-time int array
const PRIMES: [i32; 5] = ct_csharp! {
	static int[] Run() {
		return new int[] { 2, 3, 5, 7, 11 };
	}
};

// compile-time string array
const DAYS: [&str; 3] = ct_csharp! {
	static string[] Run() {
		return new string[] { "Mon", "Tue", "Wed" };
	}
};

// compile-time int? — present
const OPT_INT_SOME: Option<i32> = ct_csharp! {
	static int? Run() {
		return 99;
	}
};

// compile-time int? — null
const OPT_INT_NONE: Option<i32> = ct_csharp! {
	static int? Run() {
		return null;
	}
};

// compile-time bool? — present
const OPT_STR_SOME: Option<bool> = ct_csharp! {
	static bool? Run() {
		return true;
	}
};

// compile-time bool? — null
const OPT_STR_NONE: Option<bool> = ct_csharp! {
	static bool? Run() {
		return null;
	}
};
