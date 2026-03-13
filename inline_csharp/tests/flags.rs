use inline_csharp::{ct_csharp, csharp};

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
