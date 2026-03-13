// Tests for nested generic return types that involve reference-type nullables.
// These are faithful C# translations of the Java tests that used Optional<T>
// with List and array types.
//
// C# nullable mapping:
//   Java Optional<T>          → C# T?  (nullable)
//   Java List<T>              → C# List<T>  → Rust Vec<T>
//   Java T[]                  → C# T[]      → Rust Vec<T>
//   Java Optional<List<T>>    → C# List<T>? → Rust Option<Vec<T>>
//   Java List<Optional<T>>    → C# List<T?> → Rust Vec<Option<T>>

use inline_csharp::csharp;

// ── List<string[]?> — list of nullable string arrays ─────────────────────────
// Java: List<Optional<String[]>>

#[test]
fn csharp_runtime_list_of_nullable_string_array() {
	let v: Vec<Option<Vec<String>>> = csharp! {
		using System.Collections.Generic;
		static List<string[]?> Run() {
			return new List<string[]?> {
				new string[] { "a", "b" },
				null,
				new string[] { "c" }
			};
		}
	}
	.unwrap();
	assert_eq!(
		v,
		vec![
			Some(vec!["a".to_string(), "b".to_string()]),
			None,
			Some(vec!["c".to_string()]),
		]
	);
}

// ── List<int>? present — nullable list of ints present ───────────────────────
// Java: Optional<List<Integer>> present

#[test]
fn csharp_runtime_nullable_list_int_present() {
	let v: Option<Vec<i32>> = csharp! {
		using System.Collections.Generic;
		static List<int>? Run() {
			return new List<int> { 1, 2, 3 };
		}
	}
	.unwrap();
	assert_eq!(v, Some(vec![1i32, 2, 3]));
}

// ── List<int>? absent — nullable list of ints absent ─────────────────────────
// Java: Optional<List<Integer>> absent

#[test]
fn csharp_runtime_nullable_list_int_absent() {
	let v: Option<Vec<i32>> = csharp! {
		using System.Collections.Generic;
		static List<int>? Run() {
			return null;
		}
	}
	.unwrap();
	assert_eq!(v, None);
}

// ── List<int?>? present — nullable list of nullable ints present ──────────────
// Java: Optional<List<Optional<Integer>>> present

#[test]
fn csharp_runtime_nullable_list_of_nullable_int_present() {
	let v: Option<Vec<Option<i32>>> = csharp! {
		using System.Collections.Generic;
		static List<int?>? Run() {
			return new List<int?> { 1, null, 3 };
		}
	}
	.unwrap();
	assert_eq!(v, Some(vec![Some(1i32), None, Some(3)]));
}

// ── List<int?>? absent — nullable list of nullable ints absent ────────────────
// Java: Optional<List<Optional<Integer>>> absent

#[test]
fn csharp_runtime_nullable_list_of_nullable_int_absent() {
	let v: Option<Vec<Option<i32>>> = csharp! {
		using System.Collections.Generic;
		static List<int?>? Run() {
			return null;
		}
	}
	.unwrap();
	assert_eq!(v, None);
}

// ── List<int[]?>? present — nullable list of nullable int arrays present ──────
// Java: Optional<List<Optional<Integer[]>>> present

#[test]
fn csharp_runtime_nullable_list_of_nullable_int_array_present() {
	let v: Option<Vec<Option<Vec<i32>>>> = csharp! {
		using System.Collections.Generic;
		static List<int[]?>? Run() {
			return new List<int[]?> {
				new int[] { 10, 20 },
				null,
				new int[] { 30 }
			};
		}
	}
	.unwrap();
	assert_eq!(
		v,
		Some(vec![
			Some(vec![10i32, 20]),
			None,
			Some(vec![30i32]),
		])
	);
}

// ── List<int[]?>? absent — nullable list of nullable int arrays absent ────────
// Java: Optional<List<Optional<Integer[]>>> absent

#[test]
fn csharp_runtime_nullable_list_of_nullable_int_array_absent() {
	let v: Option<Vec<Option<Vec<i32>>>> = csharp! {
		using System.Collections.Generic;
		static List<int[]?>? Run() {
			return null;
		}
	}
	.unwrap();
	assert_eq!(v, None);
}

// ── List<string[][]?>? present — nullable list of nullable 2D string arrays ───
// Java: Optional<List<Optional<String[][]>>> present

#[test]
fn csharp_runtime_nullable_list_of_nullable_2d_string_array_present() {
	let v: Option<Vec<Option<Vec<Vec<String>>>>> = csharp! {
		using System.Collections.Generic;
		static List<string[][]?>? Run() {
			return new List<string[][]?> {
				new string[][] {
					new string[] { "foo", "bar" },
					new string[] { "baz" }
				},
				null
			};
		}
	}
	.unwrap();
	assert_eq!(
		v,
		Some(vec![
			Some(vec![
				vec!["foo".to_string(), "bar".to_string()],
				vec!["baz".to_string()],
			]),
			None,
		])
	);
}

// ── List<string[][]?>? absent — nullable list of nullable 2D string arrays ────
// Java: Optional<List<Optional<String[][]>>> absent

#[test]
fn csharp_runtime_nullable_list_of_nullable_2d_string_array_absent() {
	let v: Option<Vec<Option<Vec<Vec<String>>>>> = csharp! {
		using System.Collections.Generic;
		static List<string[][]?>? Run() {
			return null;
		}
	}
	.unwrap();
	assert_eq!(v, None);
}
