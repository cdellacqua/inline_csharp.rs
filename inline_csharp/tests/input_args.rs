// Tests for passing complex types *into* csharp_fn! as input parameters.
// Each method echoes its argument back so the test verifies the full
// serialise → C# → deserialise round-trip.
//
// These are faithful C# translations of the Java nested_generics input tests
// that used Optional<T> with List and array types.

use std::f64::consts::PI;

use inline_csharp::csharp_fn;

// ── Basic value-type nullable input tests ────────────────────────────────────

// List<int?> as input → Vec<Option<i32>>
#[test]
fn csharp_fn_arg_list_of_nullable_int() {
	let input: &[Option<i32>] = &[Some(1), None, Some(3)];
	let v: Vec<Option<i32>> = csharp_fn! {
		using System.Collections.Generic;
		static List<int?> Run(List<int?> v) {
			return v;
		}
	}(input)
	.unwrap();
	assert_eq!(v, vec![Some(1), None, Some(3)]);
}

// int? as input present → Option<i32>
#[test]
fn csharp_fn_arg_nullable_int_present() {
	let v: Option<i32> = csharp_fn! {
		static int? Run(int? v) {
			return v;
		}
	}(Some(42i32))
	.unwrap();
	assert_eq!(v, Some(42));
}

// int? as input absent → Option<i32>
#[test]
fn csharp_fn_arg_nullable_int_absent() {
	let v: Option<i32> = csharp_fn! {
		static int? Run(int? v) {
			return v;
		}
	}(None::<i32>)
	.unwrap();
	assert_eq!(v, None);
}

// bool? as input present → Option<bool>
#[test]
fn csharp_fn_arg_nullable_bool_present() {
	let v: Option<bool> = csharp_fn! {
		static bool? Run(bool? v) {
			return v;
		}
	}(Some(true))
	.unwrap();
	assert_eq!(v, Some(true));
}

// bool? as input absent → Option<bool>
#[test]
fn csharp_fn_arg_nullable_bool_absent() {
	let v: Option<bool> = csharp_fn! {
		static bool? Run(bool? v) {
			return v;
		}
	}(None::<bool>)
	.unwrap();
	assert_eq!(v, None);
}

// List<int?> with transformation → double the present values
#[test]
fn csharp_fn_arg_list_of_nullable_int_transform() {
	let input: &[Option<i32>] = &[Some(10), None, Some(30)];
	let v: Vec<Option<i32>> = csharp_fn! {
		using System.Collections.Generic;
		static List<int?> Run(List<int?> vals) {
			var result = new List<int?>();
			foreach (var x in vals) {
				result.Add(x.HasValue ? x * 2 : null);
			}
			return result;
		}
	}(input)
	.unwrap();
	assert_eq!(v, vec![Some(20), None, Some(60)]);
}

// List<List<int>> as input → Vec<Vec<i32>>
#[test]
fn csharp_fn_arg_list_of_list_int() {
	let row0: &[i32] = &[1, 2];
	let row1: &[i32] = &[3, 4, 5];
	let input: &[&[i32]] = &[row0, row1];
	let v: Vec<Vec<i32>> = csharp_fn! {
		using System.Collections.Generic;
		static List<List<int>> Run(List<List<int>> v) {
			return v;
		}
	}(input)
	.unwrap();
	assert_eq!(v, vec![vec![1, 2], vec![3, 4, 5]]);
}

// List<List<int?>> as input → Vec<Vec<Option<i32>>>
#[test]
fn csharp_fn_arg_list_of_list_of_nullable_int() {
	let row0: &[Option<i32>] = &[Some(1), None];
	let row1: &[Option<i32>] = &[None, Some(4)];
	let input: &[&[Option<i32>]] = &[row0, row1];
	let v: Vec<Vec<Option<i32>>> = csharp_fn! {
		using System.Collections.Generic;
		static List<List<int?>> Run(List<List<int?>> v) {
			return v;
		}
	}(input)
	.unwrap();
	assert_eq!(v, vec![vec![Some(1), None], vec![None, Some(4)]]);
}

// double? as input present → Option<f64>
#[test]
fn csharp_fn_arg_nullable_double_present() {
	let v: Option<f64> = csharp_fn! {
		static double? Run(double? v) {
			return v;
		}
	}(Some(PI))
	.unwrap();
	assert!((v.unwrap() - PI).abs() < 1e-10);
}

// double? as input absent → Option<f64>
#[test]
fn csharp_fn_arg_nullable_double_absent() {
	let v: Option<f64> = csharp_fn! {
		static double? Run(double? v) {
			return v;
		}
	}(None::<f64>)
	.unwrap();
	assert_eq!(v, None);
}

// ── Nested reference-type nullable input tests ────────────────────────────────
// Java: csharp_fn! with Optional<T> and List<Optional<T>> types

// List<string[]?> as input → Vec<Option<Vec<String>>>
// Java: List<Optional<String[]>> as input
#[test]
fn csharp_fn_arg_list_of_nullable_string_array() {
	let arr0: &[&str] = &["a", "b"];
	let arr2: &[&str] = &["c"];
	let input: &[Option<&[&str]>] = &[Some(arr0), None, Some(arr2)];
	let v: Vec<Option<Vec<String>>> = csharp_fn! {
		using System.Collections.Generic;
		static List<string[]?> Run(List<string[]?> v) {
			return v;
		}
	}(input)
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

// List<int>? present as input → Option<Vec<i32>> present
// Java: Optional<List<Integer>> present as input
#[test]
fn csharp_fn_arg_nullable_list_int_present() {
	let inner: &[i32] = &[10, 20, 30];
	let v: Option<Vec<i32>> = csharp_fn! {
		using System.Collections.Generic;
		static List<int>? Run(List<int>? v) {
			return v;
		}
	}(Some(inner))
	.unwrap();
	assert_eq!(v, Some(vec![10i32, 20, 30]));
}

// List<int>? absent as input → Option<Vec<i32>> absent
// Java: Optional<List<Integer>> absent as input
#[test]
fn csharp_fn_arg_nullable_list_int_absent() {
	let v: Option<Vec<i32>> = csharp_fn! {
		using System.Collections.Generic;
		static List<int>? Run(List<int>? v) {
			return v;
		}
	}(None::<&[i32]>)
	.unwrap();
	assert_eq!(v, None);
}

// List<int?>? present as input → Option<Vec<Option<i32>>> present
// Java: Optional<List<Optional<Integer>>> present as input
#[test]
fn csharp_fn_arg_nullable_list_of_nullable_int_present() {
	let inner: &[Option<i32>] = &[Some(1), None, Some(3)];
	let v: Option<Vec<Option<i32>>> = csharp_fn! {
		using System.Collections.Generic;
		static List<int?>? Run(List<int?>? v) {
			return v;
		}
	}(Some(inner))
	.unwrap();
	assert_eq!(v, Some(vec![Some(1i32), None, Some(3)]));
}

// List<int?>? absent as input → Option<Vec<Option<i32>>> absent
// Java: Optional<List<Optional<Integer>>> absent as input
#[test]
fn csharp_fn_arg_nullable_list_of_nullable_int_absent() {
	let v: Option<Vec<Option<i32>>> = csharp_fn! {
		using System.Collections.Generic;
		static List<int?>? Run(List<int?>? v) {
			return v;
		}
	}(None::<&[Option<i32>]>)
	.unwrap();
	assert_eq!(v, None);
}

// List<int[]?>? present as input → Option<Vec<Option<Vec<i32>>>> present
// Java: Optional<List<Optional<Integer[]>>> present as input
#[test]
fn csharp_fn_arg_nullable_list_of_nullable_int_array_present() {
	let arr0: &[i32] = &[10, 20];
	let arr2: &[i32] = &[30];
	let inner: &[Option<&[i32]>] = &[Some(arr0), None, Some(arr2)];
	let v: Option<Vec<Option<Vec<i32>>>> = csharp_fn! {
		using System.Collections.Generic;
		static List<int[]?>? Run(List<int[]?>? v) {
			return v;
		}
	}(Some(inner))
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

// List<int[]?>? absent as input → Option<Vec<Option<Vec<i32>>>> absent
// Java: Optional<List<Optional<Integer[]>>> absent as input
#[test]
fn csharp_fn_arg_nullable_list_of_nullable_int_array_absent() {
	let v: Option<Vec<Option<Vec<i32>>>> = csharp_fn! {
		using System.Collections.Generic;
		static List<int[]?>? Run(List<int[]?>? v) {
			return v;
		}
	}(None::<&[Option<&[i32]>]>)
	.unwrap();
	assert_eq!(v, None);
}

// List<string[][]?>? present as input → Option<Vec<Option<Vec<Vec<String>>>>> present
// Java: Optional<List<Optional<String[][]>>> present as input
#[test]
fn csharp_fn_arg_nullable_list_of_nullable_2d_string_array_present() {
	let row0: &[&str] = &["a", "b"];
	let row1: &[&str] = &["c"];
	let arr0: &[&[&str]] = &[row0, row1];
	let inner: &[Option<&[&[&str]]>] = &[Some(arr0), None];
	let v: Option<Vec<Option<Vec<Vec<String>>>>> = csharp_fn! {
		using System.Collections.Generic;
		static List<string[][]?>? Run(List<string[][]?>? v) {
			return v;
		}
	}(Some(inner))
	.unwrap();
	assert_eq!(
		v,
		Some(vec![
			Some(vec![
				vec!["a".to_string(), "b".to_string()],
				vec!["c".to_string()],
			]),
			None,
		])
	);
}

// List<string[][]?>? absent as input → Option<Vec<Option<Vec<Vec<String>>>>> absent
// Java: Optional<List<Optional<String[][]>>> absent as input
#[test]
fn csharp_fn_arg_nullable_list_of_nullable_2d_string_array_absent() {
	let v: Option<Vec<Option<Vec<Vec<String>>>>> = csharp_fn! {
		using System.Collections.Generic;
		static List<string[][]?>? Run(List<string[][]?>? v) {
			return v;
		}
	}(None::<&[Option<&[&[&str]]>]>)
	.unwrap();
	assert_eq!(v, None);
}
