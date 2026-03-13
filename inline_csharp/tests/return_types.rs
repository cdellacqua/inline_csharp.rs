use inline_csharp::{ct_csharp, csharp};

// ── Primitive arrays ──────────────────────────────────────────────────────────

#[test]
fn csharp_runtime_int_array() {
	let v: Vec<i32> = csharp! {
		static int[] Run() {
			return new int[] { 1, 2, 3, 4, 5 };
		}
	}
	.unwrap();
	assert_eq!(v, vec![1i32, 2, 3, 4, 5]);
}

#[test]
fn csharp_runtime_double_array() {
	let v: Vec<f64> = csharp! {
		static double[] Run() {
			return new double[] { 1.5, 2.5, 3.5 };
		}
	}
	.unwrap();
	assert_eq!(v, vec![1.5f64, 2.5, 3.5]);
}

#[test]
fn csharp_runtime_bool_array() {
	let v: Vec<bool> = csharp! {
		static bool[] Run() {
			return new bool[] { true, false, true };
		}
	}
	.unwrap();
	assert_eq!(v, vec![true, false, true]);
}

#[test]
fn csharp_runtime_string_array() {
	let v: Vec<String> = csharp! {
		static string[] Run() {
			return new string[] { "hello", "world" };
		}
	}
	.unwrap();
	assert_eq!(v, vec!["hello".to_string(), "world".to_string()]);
}

#[test]
fn csharp_runtime_empty_array() {
	let v: Vec<i32> = csharp! {
		static int[] Run() {
			return new int[] {};
		}
	}
	.unwrap();
	assert!(v.is_empty());
}

// ── Flat collections ──────────────────────────────────────────────────────────

#[test]
fn csharp_runtime_list_int() {
	let v: Vec<i32> = csharp! {
		using System.Collections.Generic;
		static List<int> Run() {
			return new List<int> { 10, 20, 30 };
		}
	}
	.unwrap();
	assert_eq!(v, vec![10i32, 20, 30]);
}

#[test]
fn csharp_runtime_list_string() {
	let v: Vec<String> = csharp! {
		using System.Collections.Generic;
		static List<string> Run() {
			return new List<string> { "foo", "bar", "baz" };
		}
	}
	.unwrap();
	assert_eq!(
		v,
		vec!["foo".to_string(), "bar".to_string(), "baz".to_string()]
	);
}

// ── OOP ───────────────────────────────────────────────────────────────────────

#[test]
fn csharp_runtime_abstract_class_override() {
	let sound: String = csharp! {
		abstract class Animal { public abstract string Sound(); }
		class Dog : Animal { public override string Sound() { return "woof"; } }
		static string Run() {
			return new Dog().Sound();
		}
	}
	.unwrap();
	assert_eq!(sound, "woof");
}

// ── Nested / composable container types ──────────────────────────────────────

// List<List<int>>
#[test]
fn csharp_runtime_list_of_list_int() {
	let v: Vec<Vec<i32>> = csharp! {
		using System.Collections.Generic;
		static List<List<int>> Run() {
			var a = new List<int> { 1, 2, 3 };
			var b = new List<int> { 4, 5, 6 };
			return new List<List<int>> { a, b };
		}
	}
	.unwrap();
	assert_eq!(v, vec![vec![1, 2, 3], vec![4, 5, 6]]);
}

// double? present
#[test]
fn csharp_runtime_nullable_double_present() {
	let v: Option<f64> = csharp! {
		static double? Run() {
			return 2.5;
		}
	}
	.unwrap();
	assert_eq!(v, Some(2.5f64));
}

// double? absent
#[test]
fn csharp_runtime_nullable_double_absent() {
	let v: Option<f64> = csharp! {
		static double? Run() {
			return null;
		}
	}
	.unwrap();
	assert_eq!(v, None);
}

// List<int?>
#[test]
fn csharp_runtime_list_of_nullable_int() {
	let v: Vec<Option<i32>> = csharp! {
		using System.Collections.Generic;
		static List<int?> Run() {
			return new List<int?> { 10, null, 30 };
		}
	}
	.unwrap();
	assert_eq!(v, vec![Some(10), None, Some(30)]);
}

// bool? present
#[test]
fn csharp_runtime_nullable_bool_present() {
	let v: Option<bool> = csharp! {
		static bool? Run() {
			return true;
		}
	}
	.unwrap();
	assert_eq!(v, Some(true));
}

// bool? absent
#[test]
fn csharp_runtime_nullable_bool_absent() {
	let v: Option<bool> = csharp! {
		static bool? Run() {
			return null;
		}
	}
	.unwrap();
	assert_eq!(v, None);
}

// List<List<string>>
#[test]
fn csharp_runtime_list_of_list_string() {
	let v: Vec<Vec<String>> = csharp! {
		using System.Collections.Generic;
		static List<List<string>> Run() {
			var a = new List<string> { "foo", "bar" };
			var b = new List<string> { "baz" };
			return new List<List<string>> { a, b };
		}
	}
	.unwrap();
	assert_eq!(
		v,
		vec![
			vec!["foo".to_string(), "bar".to_string()],
			vec!["baz".to_string()],
		]
	);
}

// List<string>? present — Java Optional<List<String>> present
#[test]
fn csharp_runtime_nullable_list_string_present() {
	let v: Option<Vec<String>> = csharp! {
		using System.Collections.Generic;
		static List<string>? Run() {
			return new List<string> { "hello", "world" };
		}
	}
	.unwrap();
	assert_eq!(
		v,
		Some(vec!["hello".to_string(), "world".to_string()])
	);
}

// List<string>? absent — Java Optional<List<String>> absent
#[test]
fn csharp_runtime_nullable_list_string_absent() {
	let v: Option<Vec<String>> = csharp! {
		using System.Collections.Generic;
		static List<string>? Run() {
			return null;
		}
	}
	.unwrap();
	assert_eq!(v, None);
}

// List<bool?> present — covers nesting where inner element is nullable
#[test]
fn csharp_runtime_list_of_nullable_bool_present() {
	let v: Vec<Option<bool>> = csharp! {
		using System.Collections.Generic;
		static List<bool?> Run() {
			return new List<bool?> { true, null, false };
		}
	}
	.unwrap();
	assert_eq!(v, vec![Some(true), None, Some(false)]);
}

// List<bool?> absent via nullable wrapper
#[test]
fn csharp_runtime_nullable_list_of_bool_absent() {
	let v: Option<Vec<Option<bool>>> = csharp! {
		using System.Collections.Generic;
		static List<bool?>? Run() {
			return null;
		}
	}
	.unwrap();
	assert_eq!(v, None);
}

// string? present — nullable string return
#[test]
fn csharp_runtime_nullable_string_present() {
	let v: Option<String> = csharp! {
		static string? Run() {
			return "hello nullable";
		}
	}
	.unwrap();
	assert_eq!(v, Some("hello nullable".to_string()));
}

// string? absent — null string return
#[test]
fn csharp_runtime_nullable_string_absent() {
	let v: Option<String> = csharp! {
		static string? Run() {
			return null;
		}
	}
	.unwrap();
	assert_eq!(v, None);
}

// ── ct_csharp! return types ───────────────────────────────────────────────────

const CT_INT_ARRAY: [i32; 3] = ct_csharp! {
	static int[] Run() {
		return new int[] { 100, 200, 300 };
	}
};

#[test]
fn ct_csharp_int_array() {
	assert_eq!(CT_INT_ARRAY, [100i32, 200, 300]);
}

const CT_STRING_ARRAY: [&str; 2] = ct_csharp! {
	static string[] Run() {
		return new string[] { "compile", "time" };
	}
};

#[test]
fn ct_csharp_string_array() {
	assert_eq!(CT_STRING_ARRAY, ["compile", "time"]);
}

// ct_csharp! with List<List<int>>
const CT_NESTED_LIST: [[i32; 2]; 2] = ct_csharp! {
	using System.Collections.Generic;
	static List<List<int>> Run() {
		var a = new List<int> { 10, 20 };
		var b = new List<int> { 30, 40 };
		return new List<List<int>> { a, b };
	}
};

#[test]
fn ct_csharp_nested_list() {
	assert_eq!(CT_NESTED_LIST, [[10, 20], [30, 40]]);
}

// ct_csharp! with int? — Some value at compile time
const CT_NULLABLE_INT: Option<i32> = ct_csharp! {
	static int? Run() {
		return 42;
	}
};

#[test]
fn ct_csharp_nullable_int() {
	assert_eq!(CT_NULLABLE_INT, Some(42i32));
}

// ct_csharp! with List<int>? — Some value at compile time
// Java: Optional<List<Integer>> present at compile time
const CT_NULLABLE_LIST: Option<[i32; 3]> = ct_csharp! {
	using System.Collections.Generic;
	static List<int>? Run() {
		return new List<int> { 7, 8, 9 };
	}
};

#[test]
fn ct_csharp_nullable_list() {
	assert_eq!(CT_NULLABLE_LIST, Some([7i32, 8, 9]));
}

// ── Runtime error ─────────────────────────────────────────────────────────────

#[test]
fn csharp_runtime_divide_by_zero() {
	let result: Result<i32, _> = csharp! {
		static int Run() {
			int zero = 0;
			return 1 / zero;
		}
	};
	assert!(
		result.is_err(),
		"expected Err for divide-by-zero, got {result:?}"
	);
}
