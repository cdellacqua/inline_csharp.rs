//! Proc-macro implementation for `inline_csharp`.
//!
//! Provides three proc macros for embedding C# code in Rust:
//!
//! | Macro           | When it runs    |
//! |-----------------|-----------------|
//! | [`csharp!`]     | program runtime |
//! | [`csharp_fn!`]  | program runtime |
//! | [`ct_csharp!`]  | compile time    |
//!
//! All macros require the user to write a `static <T> Run(...)` method
//! where `T` is one of: `sbyte`, `byte`, `short`, `ushort`, `int`, `uint`,
//! `long`, `ulong`, `float`, `double`, `bool`, `char`, `string`, `T[]`,
//! `List<T>`, or `T?` — including arbitrarily nested types.
//!
//! # Wire format (C# → Rust, stdout)
//!
//! The macro generates a `Main()` that binary-serialises `Run()`'s return
//! value to stdout via `BinaryWriter` (raw UTF-8 for top-level `string`).
//!
//! Encoding per type (all little-endian):
//! - `string` at top level: raw UTF-8 (no length prefix)
//! - `string` inside a container: 4-byte LE `u32` length + UTF-8 bytes
//! - scalar: fixed-width little-endian bytes via `BinaryWriter`
//! - `T[]` / `List<T>`: 4-byte LE `u32` count + N × encode(T)
//! - `T?` (Nullable): 1-byte tag (0=null, 1=present) + encode(T) if present
//!
//! # Wire format (Rust → C#, stdin)
//!
//! Parameters declared in `Run(...)` are serialised by Rust and piped to the
//! child process's stdin. C# reads them with `BinaryReader`.
//!
//! # Options
//!
//! All three macros accept zero or more `key = "value"` pairs before the C# body,
//! comma-separated.  Recognised keys:
//!
//! - `build = "<args>"` — extra arguments passed verbatim to `dotnet build`
//! - `run   = "<args>"` — extra arguments passed verbatim to `dotnet run`
//! - `reference = "<path>"` — add a reference assembly (repeatable)

use proc_macro::TokenStream;
use proc_macro2::TokenTree;
use quote::{format_ident, quote};
use std::collections::hash_map::DefaultHasher;
use std::fmt::Write as FmtWrite;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

// ── CsharpType ───────────────────────────────────────────────────────────────

/// Recursive composable C# type system.
#[derive(Clone, PartialEq)]
enum CsharpType {
	// Signed integers
	Sbyte,
	Short,
	Int,
	Long,
	// Unsigned integers
	Byte,
	Ushort,
	Uint,
	Ulong,
	// Floats
	Float,
	Double,
	// Other scalars
	Bool,
	Char,
	Str,
	// Composites
	/// `T[]` — returned as `Vec<T>`
	Array(Box<CsharpType>),
	/// `List<T>` — same wire format / Rust type as `Array`
	List(Box<CsharpType>),
	/// `T?` — returned as `Option<T>`
	Nullable(Box<CsharpType>),
}

impl CsharpType {
	/// Parse a scalar type name to a `CsharpType`.
	fn from_name(s: &str) -> Option<Self> {
		match s {
			"sbyte" | "SByte" => Some(Self::Sbyte),
			"byte" | "Byte" => Some(Self::Byte),
			"short" | "Int16" => Some(Self::Short),
			"ushort" | "UInt16" => Some(Self::Ushort),
			"int" | "Int32" => Some(Self::Int),
			"uint" | "UInt32" => Some(Self::Uint),
			"long" | "Int64" => Some(Self::Long),
			"ulong" | "UInt64" => Some(Self::Ulong),
			"float" | "Single" => Some(Self::Float),
			"double" | "Double" => Some(Self::Double),
			"bool" | "Boolean" => Some(Self::Bool),
			"char" | "Char" => Some(Self::Char),
			"string" | "String" => Some(Self::Str),
			_ => None,
		}
	}

	/// The C# type name for use in generated code.
	fn csharp_type_name(&self) -> String {
		match self {
			Self::Sbyte => "sbyte".to_string(),
			Self::Byte => "byte".to_string(),
			Self::Short => "short".to_string(),
			Self::Ushort => "ushort".to_string(),
			Self::Int => "int".to_string(),
			Self::Uint => "uint".to_string(),
			Self::Long => "long".to_string(),
			Self::Ulong => "ulong".to_string(),
			Self::Float => "float".to_string(),
			Self::Double => "double".to_string(),
			Self::Bool => "bool".to_string(),
			Self::Char => "char".to_string(),
			Self::Str => "string".to_string(),
			Self::Array(inner) => format!("{}[]", inner.csharp_type_name()),
			Self::List(inner) => {
				format!("System.Collections.Generic.List<{}>", inner.csharp_type_name())
			}
			Self::Nullable(inner) => format!("{}?", inner.csharp_type_name()),
		}
	}

	/// Returns true for C# value types (scalars).
	/// Value-type nullables (`int?`, `bool?`, …) use `.HasValue`/`.Value`.
	/// Reference-type nullables (`string?`, `T[]?`, `List<T>?`) use `!= null`.
	fn is_value_type(&self) -> bool {
		matches!(
			self,
			Self::Sbyte
				| Self::Byte
				| Self::Short
				| Self::Ushort
				| Self::Int
				| Self::Uint
				| Self::Long
				| Self::Ulong
				| Self::Float
				| Self::Double
				| Self::Bool
				| Self::Char
		)
	}

	/// Returns the Rust return type token stream for this C# type.
	fn rust_return_type_ts(&self) -> proc_macro2::TokenStream {
		match self {
			Self::Sbyte => quote! { i8 },
			Self::Byte => quote! { u8 },
			Self::Short => quote! { i16 },
			Self::Ushort => quote! { u16 },
			Self::Int => quote! { i32 },
			Self::Uint => quote! { u32 },
			Self::Long => quote! { i64 },
			Self::Ulong => quote! { u64 },
			Self::Float => quote! { f32 },
			Self::Double => quote! { f64 },
			Self::Bool => quote! { bool },
			Self::Char => quote! { char },
			Self::Str => quote! { ::std::string::String },
			Self::Array(inner) | Self::List(inner) => {
				let inner_ts = inner.rust_return_type_ts();
				quote! { ::std::vec::Vec<#inner_ts> }
			}
			Self::Nullable(inner) => {
				let inner_ts = inner.rust_return_type_ts();
				quote! { ::std::option::Option<#inner_ts> }
			}
		}
	}

	/// Returns the Rust parameter type token stream.
	/// `Str` leaf → `&str`; `Array`/`List` → `&[T]` (slice reference).
	fn rust_param_type_ts(&self) -> proc_macro2::TokenStream {
		match self {
			Self::Str => quote! { &str },
			Self::Array(inner) | Self::List(inner) => {
				let inner_ts = inner.rust_param_type_ts();
				quote! { &[#inner_ts] }
			}
			Self::Nullable(inner) => {
				let inner_ts = inner.rust_param_type_ts();
				quote! { ::std::option::Option<#inner_ts> }
			}
			// All scalar types: same as return type
			_ => self.rust_return_type_ts(),
		}
	}

	/// Generates Rust code to serialize a parameter value into `_stdin_bytes`.
	fn rust_ser_ts(
		&self,
		param_ident: &proc_macro2::TokenStream,
		depth: usize,
	) -> proc_macro2::TokenStream {
		match self {
			Self::Sbyte => quote! {
				_stdin_bytes.extend_from_slice(&(#param_ident as i8).to_le_bytes());
			},
			Self::Byte => quote! {
				_stdin_bytes.push(#param_ident as u8);
			},
			Self::Short => quote! {
				_stdin_bytes.extend_from_slice(&(#param_ident as i16).to_le_bytes());
			},
			Self::Ushort => quote! {
				_stdin_bytes.extend_from_slice(&(#param_ident as u16).to_le_bytes());
			},
			Self::Int => quote! {
				_stdin_bytes.extend_from_slice(&(#param_ident as i32).to_le_bytes());
			},
			Self::Uint => quote! {
				_stdin_bytes.extend_from_slice(&(#param_ident as u32).to_le_bytes());
			},
			Self::Long => quote! {
				_stdin_bytes.extend_from_slice(&(#param_ident as i64).to_le_bytes());
			},
			Self::Ulong => quote! {
				_stdin_bytes.extend_from_slice(&(#param_ident as u64).to_le_bytes());
			},
			Self::Float => quote! {
				_stdin_bytes.extend_from_slice(&(#param_ident as f32).to_bits().to_le_bytes());
			},
			Self::Double => quote! {
				_stdin_bytes.extend_from_slice(&(#param_ident as f64).to_bits().to_le_bytes());
			},
			Self::Bool => quote! {
				_stdin_bytes.push(#param_ident as u8);
			},
			Self::Char => quote! {
				{
					let _c = #param_ident as u32;
					assert!(_c <= 0xFFFF, "inline_csharp: char value exceeds u16 range");
					_stdin_bytes.extend_from_slice(&(_c as u16).to_le_bytes());
				}
			},
			Self::Str => quote! {
				{
					let _b = #param_ident.as_bytes();
					let _len = _b.len() as u32;
					_stdin_bytes.extend_from_slice(&_len.to_le_bytes());
					_stdin_bytes.extend_from_slice(_b);
				}
			},
			Self::Array(inner) | Self::List(inner) => {
				let item_var = format_ident!("_item{}", depth);
				let item_expr = quote! { #item_var };
				let inner_ser = inner.rust_ser_ts(&item_expr, depth + 1);
				quote! {
					{
						_stdin_bytes.extend_from_slice(&(#param_ident.len() as u32).to_le_bytes());
						for &#item_var in #param_ident {
							#inner_ser
						}
					}
				}
			}
			Self::Nullable(inner) => {
				let inner_var = format_ident!("_inner{}", depth);
				let inner_expr = quote! { #inner_var };
				let inner_ser = inner.rust_ser_ts(&inner_expr, depth + 1);
				quote! {
					match #param_ident {
						::std::option::Option::None => _stdin_bytes.push(0u8),
						::std::option::Option::Some(#inner_var) => {
							_stdin_bytes.push(1u8);
							#inner_ser
						}
					}
				}
			}
		}
	}

	/// Returns a Rust expression that deserialises raw stdout bytes `_raw: Vec<u8>`
	/// into the corresponding Rust type. Used by `csharp!` and `csharp_fn!` at runtime.
	fn rust_deser(&self) -> proc_macro2::TokenStream {
		match self {
			Self::Str => {
				// Top-level string: raw UTF-8, no length prefix
				quote! { ::std::string::String::from_utf8(_raw)? }
			}
			Self::Sbyte => quote! { i8::from_le_bytes([_raw[0]]) },
			Self::Byte => quote! { _raw[0] },
			Self::Short => quote! { i16::from_le_bytes([_raw[0], _raw[1]]) },
			Self::Ushort => quote! { u16::from_le_bytes([_raw[0], _raw[1]]) },
			Self::Int => quote! { i32::from_le_bytes([_raw[0], _raw[1], _raw[2], _raw[3]]) },
			Self::Uint => quote! { u32::from_le_bytes([_raw[0], _raw[1], _raw[2], _raw[3]]) },
			Self::Long => {
				quote! {
					i64::from_le_bytes([
						_raw[0], _raw[1], _raw[2], _raw[3],
						_raw[4], _raw[5], _raw[6], _raw[7],
					])
				}
			}
			Self::Ulong => {
				quote! {
					u64::from_le_bytes([
						_raw[0], _raw[1], _raw[2], _raw[3],
						_raw[4], _raw[5], _raw[6], _raw[7],
					])
				}
			}
			Self::Float => {
				quote! { f32::from_bits(u32::from_le_bytes([_raw[0], _raw[1], _raw[2], _raw[3]])) }
			}
			Self::Double => {
				quote! {
					f64::from_bits(u64::from_le_bytes([
						_raw[0], _raw[1], _raw[2], _raw[3],
						_raw[4], _raw[5], _raw[6], _raw[7],
					]))
				}
			}
			Self::Bool => quote! { _raw[0] != 0 },
			Self::Char => {
				quote! {
					::std::char::from_u32(u16::from_le_bytes([_raw[0], _raw[1]]) as u32)
						.ok_or(::inline_csharp::CsharpError::InvalidChar)?
				}
			}
			_ => {
				// Container types: set up shared _cur and call recursive reader
				let rust_type = self.rust_return_type_ts();
				let read_expr = rust_read_element(self, 0);
				quote! {
					{
						let mut _cur = 0usize;
						let _result: #rust_type = #read_expr;
						_result
					}
				}
			}
		}
	}

	/// Converts raw stdout bytes produced by the generated `Main()` into a
	/// Rust literal / expression token stream to splice at the `ct_csharp!` call site.
	fn ct_csharp_tokens(&self, bytes: Vec<u8>) -> Result<proc_macro2::TokenStream, String> {
		match self {
			Self::Str => {
				// Top-level string: raw UTF-8, no length prefix
				let s = String::from_utf8(bytes)
					.map_err(|_| "ct_csharp: C# string is not valid UTF-8".to_string())?;
				let lit = format!("{s:?}");
				proc_macro2::TokenStream::from_str(&lit)
					.map_err(|e| format!("ct_csharp: produced invalid Rust token: {e}"))
			}
			Self::Sbyte
			| Self::Byte
			| Self::Short
			| Self::Ushort
			| Self::Int
			| Self::Uint
			| Self::Long
			| Self::Ulong
			| Self::Float
			| Self::Double
			| Self::Bool
			| Self::Char => {
				let (lit, _) = scalar_ct_lit(self, &bytes, 0)?;
				proc_macro2::TokenStream::from_str(&lit)
					.map_err(|e| format!("ct_csharp: produced invalid Rust token: {e}"))
			}
			_ => {
				let mut cur = 0usize;
				let ts = ct_csharp_tokens_recursive(self, &bytes, &mut cur)?;
				Ok(ts)
			}
		}
	}

	/// Generates the complete `static void Main()` method that binary-serialises
	/// `Run()`'s return value to stdout. `params` lists the parameters declared
	/// in `Run(...)` so the generated `Main` can read them from stdin and forward
	/// them to `Run`.
	fn csharp_main(&self, params: &[(CsharpType, String)]) -> String {
		let param_reads = if params.is_empty() {
			String::new()
		} else {
			let mut s = String::from(
				"\t\tBinaryReader _br = new BinaryReader(Console.OpenStandardInput());\n",
			);
			for (ty, name) in params {
				writeln!(s, "\t\t{}", csharp_br_read(ty, name, 0)).unwrap();
			}
			s
		};

		let run_args: String = params
			.iter()
			.map(|(_, name)| name.as_str())
			.collect::<Vec<_>>()
			.join(", ");

		let result_ty = self.csharp_type_name();
		let serialize = csharp_bw_write(self, "_result", 0);

		format!(
			"\tstatic void Main() {{\n\
			 {param_reads}\t\t{result_ty} _result = Run({run_args});\n\
			 \t\t{serialize}\n\
			 \t}}"
		)
	}
}

// ── C# BinaryReader param reading ────────────────────────────────────────────

/// Generates C# statement(s) to read a parameter from `BinaryReader _br`.
fn csharp_br_read(ty: &CsharpType, name: &str, depth: usize) -> String {
	match ty {
		CsharpType::Sbyte => format!("sbyte {name} = _br.ReadSByte();"),
		CsharpType::Byte => format!("byte {name} = _br.ReadByte();"),
		CsharpType::Short => format!("short {name} = _br.ReadInt16();"),
		CsharpType::Ushort => format!("ushort {name} = _br.ReadUInt16();"),
		CsharpType::Int => format!("int {name} = _br.ReadInt32();"),
		CsharpType::Uint => format!("uint {name} = _br.ReadUInt32();"),
		CsharpType::Long => format!("long {name} = _br.ReadInt64();"),
		CsharpType::Ulong => format!("ulong {name} = _br.ReadUInt64();"),
		CsharpType::Float => format!("float {name} = _br.ReadSingle();"),
		CsharpType::Double => format!("double {name} = _br.ReadDouble();"),
		CsharpType::Bool => format!("bool {name} = _br.ReadBoolean();"),
		CsharpType::Char => format!("char {name} = (char)_br.ReadUInt16();"),
		CsharpType::Str => {
			format!(
				"uint _len_{name} = _br.ReadUInt32();\n\
				 \t\tbyte[] _b_{name} = _br.ReadBytes((int)_len_{name});\n\
				 \t\tstring {name} = System.Text.Encoding.UTF8.GetString(_b_{name});"
			)
		}
		CsharpType::Array(inner) => {
			let count_var = format!("_count_{name}_{depth}");
			let i_var = format!("_i_{name}_{depth}");
			let elem_var = format!("_elem_{name}_{depth}");
			let inner_cs_type = inner.csharp_type_name();
			let inner_read = csharp_br_read(inner, &elem_var, depth + 1);
			format!(
				"uint {count_var} = _br.ReadUInt32();\n\
				 \t\t{inner_cs_type}[] {name} = new {inner_cs_type}[{count_var}];\n\
				 \t\tfor (int {i_var} = 0; {i_var} < {count_var}; {i_var}++) {{\n\
				 \t\t\t{inner_read}\n\
				 \t\t\t{name}[{i_var}] = {elem_var};\n\
				 \t\t}}"
			)
		}
		CsharpType::List(inner) => {
			let count_var = format!("_count_{name}_{depth}");
			let i_var = format!("_i_{name}_{depth}");
			let elem_var = format!("_elem_{name}_{depth}");
			let inner_cs_type = inner.csharp_type_name();
			let inner_read = csharp_br_read(inner, &elem_var, depth + 1);
			format!(
				"uint {count_var} = _br.ReadUInt32();\n\
				 \t\tSystem.Collections.Generic.List<{inner_cs_type}> {name} = new();\n\
				 \t\tfor (int {i_var} = 0; {i_var} < {count_var}; {i_var}++) {{\n\
				 \t\t\t{inner_read}\n\
				 \t\t\t{name}.Add({elem_var});\n\
				 \t\t}}"
			)
		}
		CsharpType::Nullable(inner) => {
			let tag_var = format!("_tag_{name}_{depth}");
			let inner_var = format!("_inner_{name}_{depth}");
			let inner_cs_type = inner.csharp_type_name();
			let inner_read = csharp_br_read(inner, &inner_var, depth + 1);
			format!(
				"byte {tag_var} = _br.ReadByte();\n\
				 \t\t{inner_cs_type}? {name};\n\
				 \t\tif ({tag_var} != 0) {{\n\
				 \t\t\t{inner_read}\n\
				 \t\t\t{name} = {inner_var};\n\
				 \t\t}} else {{\n\
				 \t\t\t{name} = null;\n\
				 \t\t}}"
			)
		}
	}
}

// ── C# BinaryWriter result writing ───────────────────────────────────────────

/// Generates C# statement(s) to write `var` of type `ty` to stdout.
/// For the top-level `_result` this is called from `csharp_main`.
fn csharp_bw_write(ty: &CsharpType, var: &str, _depth: usize) -> String {
	match ty {
		CsharpType::Str => {
			// Top-level string: raw UTF-8, no length prefix
			format!(
				"byte[] _b = System.Text.Encoding.UTF8.GetBytes({var}); \
				 Console.OpenStandardOutput().Write(_b, 0, _b.Length);"
			)
		}
		CsharpType::Char => {
			format!(
				"BinaryWriter _bw = new BinaryWriter(Console.OpenStandardOutput());\n\
				 \t\t_bw.Write((ushort){var});\n\
				 \t\t_bw.Flush();"
			)
		}
		CsharpType::Array(inner) => {
			let inner_cs_type = inner.csharp_type_name();
			let ser_body = csharp_ser_element(inner, "_e0", "_bw", 1);
			format!(
				"BinaryWriter _bw = new BinaryWriter(Console.OpenStandardOutput());\n\
				 \t\t_bw.Write((uint){var}.Length);\n\
				 \t\tforeach ({inner_cs_type} _e0 in {var}) {{\n\
				 \t\t\t{ser_body}\n\
				 \t\t}}\n\
				 \t\t_bw.Flush();"
			)
		}
		CsharpType::List(inner) => {
			let inner_cs_type = inner.csharp_type_name();
			let ser_body = csharp_ser_element(inner, "_e0", "_bw", 1);
			format!(
				"BinaryWriter _bw = new BinaryWriter(Console.OpenStandardOutput());\n\
				 \t\t_bw.Write((uint){var}.Count);\n\
				 \t\tforeach ({inner_cs_type} _e0 in {var}) {{\n\
				 \t\t\t{ser_body}\n\
				 \t\t}}\n\
				 \t\t_bw.Flush();"
			)
		}
		CsharpType::Nullable(inner) => {
			let inner_cs_type = inner.csharp_type_name();
			if inner.is_value_type() {
				// Nullable<T> value type: .HasValue / .Value
				let ser_body = csharp_ser_element(inner, &format!("{var}.Value"), "_bw", 1);
				format!(
					"BinaryWriter _bw = new BinaryWriter(Console.OpenStandardOutput());\n\
					 \t\tif ({var}.HasValue) {{\n\
					 \t\t\t_bw.Write((byte)1);\n\
					 \t\t\t{inner_cs_type} _opt_val = {var}.Value;\n\
					 \t\t\t{ser_body}\n\
					 \t\t}} else {{\n\
					 \t\t\t_bw.Write((byte)0);\n\
					 \t\t}}\n\
					 \t\t_bw.Flush();"
				)
			} else {
				// Nullable reference type (string?, T[]?, List<T>?): != null check
				let ser_body = csharp_ser_element(inner, "_opt_val", "_bw", 1);
				format!(
					"BinaryWriter _bw = new BinaryWriter(Console.OpenStandardOutput());\n\
					 \t\tif ({var} != null) {{\n\
					 \t\t\t_bw.Write((byte)1);\n\
					 \t\t\t{inner_cs_type} _opt_val = {var};\n\
					 \t\t\t{ser_body}\n\
					 \t\t}} else {{\n\
					 \t\t\t_bw.Write((byte)0);\n\
					 \t\t}}\n\
					 \t\t_bw.Flush();"
				)
			}
		}
		// All other scalars: BinaryWriter.Write with cast
		_ => {
			let cs_type = ty.csharp_type_name();
			format!(
				"BinaryWriter _bw = new BinaryWriter(Console.OpenStandardOutput());\n\
				 \t\t_bw.Write(({cs_type}){var});\n\
				 \t\t_bw.Flush();"
			)
		}
	}
}

/// Generates C# code to serialize `var` of type `ty` to `BinaryWriter` named `bw_name`.
/// Used for element serialization inside containers.
fn csharp_ser_element(ty: &CsharpType, var: &str, bw_name: &str, depth: usize) -> String {
	match ty {
		CsharpType::Char => {
			format!("{bw_name}.Write((ushort){var});")
		}
		CsharpType::Str => {
			format!(
				"{{ byte[] _b{depth} = System.Text.Encoding.UTF8.GetBytes({var}); \
				 {bw_name}.Write((uint)_b{depth}.Length); \
				 {bw_name}.Write(_b{depth}); }}"
			)
		}
		CsharpType::Array(inner) => {
			let inner_cs_type = inner.csharp_type_name();
			let elem_var = format!("_e{depth}");
			let inner_ser = csharp_ser_element(inner, &elem_var, bw_name, depth + 1);
			format!(
				"{bw_name}.Write((uint)({var}).Length);\n\
				 \t\t\tforeach ({inner_cs_type} {elem_var} in ({var})) {{\n\
				 \t\t\t\t{inner_ser}\n\
				 \t\t\t}}"
			)
		}
		CsharpType::List(inner) => {
			let inner_cs_type = inner.csharp_type_name();
			let elem_var = format!("_e{depth}");
			let inner_ser = csharp_ser_element(inner, &elem_var, bw_name, depth + 1);
			format!(
				"{bw_name}.Write((uint)({var}).Count);\n\
				 \t\t\tforeach ({inner_cs_type} {elem_var} in ({var})) {{\n\
				 \t\t\t\t{inner_ser}\n\
				 \t\t\t}}"
			)
		}
		CsharpType::Nullable(inner) => {
			let inner_cs_type = inner.csharp_type_name();
			let opt_inner_var = format!("_opt_inner{depth}");
			let inner_ser = csharp_ser_element(inner, &opt_inner_var, bw_name, depth + 1);
			if inner.is_value_type() {
				format!(
					"if (({var}).HasValue) {{\n\
					 \t\t\t\t{bw_name}.Write((byte)1);\n\
					 \t\t\t\t{inner_cs_type} {opt_inner_var} = ({var}).Value;\n\
					 \t\t\t\t{inner_ser}\n\
					 \t\t\t}} else {{\n\
					 \t\t\t\t{bw_name}.Write((byte)0);\n\
					 \t\t\t}}"
				)
			} else {
				format!(
					"if (({var}) != null) {{\n\
					 \t\t\t\t{bw_name}.Write((byte)1);\n\
					 \t\t\t\t{inner_cs_type} {opt_inner_var} = {var};\n\
					 \t\t\t\t{inner_ser}\n\
					 \t\t\t}} else {{\n\
					 \t\t\t\t{bw_name}.Write((byte)0);\n\
					 \t\t\t}}"
				)
			}
		}
		// Non-string scalars
		_ => {
			let cs_type = ty.csharp_type_name();
			format!("{bw_name}.Write(({cs_type}){var});")
		}
	}
}

// ── Recursive Rust deserialization helper ─────────────────────────────────────

/// Generates a Rust expression block that reads one value of type `ty` from `_raw`
/// using the shared mutable cursor `_cur`. All levels share the same `_cur` and `_raw`.
fn rust_read_element(ty: &CsharpType, depth: usize) -> proc_macro2::TokenStream {
	match ty {
		CsharpType::Sbyte => quote! {{
			let _val = i8::from_le_bytes([_raw[_cur]]);
			_cur += 1;
			_val
		}},
		CsharpType::Byte => quote! {{
			let _val = _raw[_cur];
			_cur += 1;
			_val
		}},
		CsharpType::Short => quote! {{
			let _val = i16::from_le_bytes([_raw[_cur], _raw[_cur + 1]]);
			_cur += 2;
			_val
		}},
		CsharpType::Ushort => quote! {{
			let _val = u16::from_le_bytes([_raw[_cur], _raw[_cur + 1]]);
			_cur += 2;
			_val
		}},
		CsharpType::Int => quote! {{
			let _val = i32::from_le_bytes([_raw[_cur], _raw[_cur + 1], _raw[_cur + 2], _raw[_cur + 3]]);
			_cur += 4;
			_val
		}},
		CsharpType::Uint => quote! {{
			let _val = u32::from_le_bytes([_raw[_cur], _raw[_cur + 1], _raw[_cur + 2], _raw[_cur + 3]]);
			_cur += 4;
			_val
		}},
		CsharpType::Long => quote! {{
			let _val = i64::from_le_bytes([
				_raw[_cur], _raw[_cur + 1], _raw[_cur + 2], _raw[_cur + 3],
				_raw[_cur + 4], _raw[_cur + 5], _raw[_cur + 6], _raw[_cur + 7],
			]);
			_cur += 8;
			_val
		}},
		CsharpType::Ulong => quote! {{
			let _val = u64::from_le_bytes([
				_raw[_cur], _raw[_cur + 1], _raw[_cur + 2], _raw[_cur + 3],
				_raw[_cur + 4], _raw[_cur + 5], _raw[_cur + 6], _raw[_cur + 7],
			]);
			_cur += 8;
			_val
		}},
		CsharpType::Float => quote! {{
			let _val = f32::from_bits(u32::from_le_bytes([_raw[_cur], _raw[_cur + 1], _raw[_cur + 2], _raw[_cur + 3]]));
			_cur += 4;
			_val
		}},
		CsharpType::Double => quote! {{
			let _val = f64::from_bits(u64::from_le_bytes([
				_raw[_cur], _raw[_cur + 1], _raw[_cur + 2], _raw[_cur + 3],
				_raw[_cur + 4], _raw[_cur + 5], _raw[_cur + 6], _raw[_cur + 7],
			]));
			_cur += 8;
			_val
		}},
		CsharpType::Bool => quote! {{
			let _val = _raw[_cur] != 0;
			_cur += 1;
			_val
		}},
		CsharpType::Char => quote! {{
			let _val = ::std::char::from_u32(u16::from_le_bytes([_raw[_cur], _raw[_cur + 1]]) as u32)
				.ok_or(::inline_csharp::CsharpError::InvalidChar)?;
			_cur += 2;
			_val
		}},
		// String inside container: u32 length prefix + UTF-8 bytes
		CsharpType::Str => quote! {{
			let _slen = u32::from_le_bytes([_raw[_cur], _raw[_cur + 1], _raw[_cur + 2], _raw[_cur + 3]]) as usize;
			_cur += 4;
			let _val = ::std::string::String::from_utf8(_raw[_cur.._cur + _slen].to_vec())?;
			_cur += _slen;
			_val
		}},
		CsharpType::Array(inner) | CsharpType::List(inner) => {
			let n_var = format_ident!("_n{}", depth);
			let v_var = format_ident!("_v{}", depth);
			let inner_rust_type = inner.rust_return_type_ts();
			let inner_read = rust_read_element(inner, depth + 1);
			quote! {{
				let #n_var = u32::from_le_bytes([_raw[_cur], _raw[_cur + 1], _raw[_cur + 2], _raw[_cur + 3]]) as usize;
				_cur += 4;
				let mut #v_var: ::std::vec::Vec<#inner_rust_type> = ::std::vec::Vec::with_capacity(#n_var);
				for _ in 0..#n_var {
					let _item = #inner_read;
					#v_var.push(_item);
				}
				#v_var
			}}
		}
		CsharpType::Nullable(inner) => {
			let inner_rust_type = inner.rust_return_type_ts();
			let inner_read = rust_read_element(inner, depth + 1);
			quote! {{
				let _tag = _raw[_cur];
				_cur += 1;
				if _tag == 0 {
					::std::option::Option::None::<#inner_rust_type>
				} else {
					::std::option::Option::Some(#inner_read)
				}
			}}
		}
	}
}

// ── Compile-time literal generation ──────────────────────────────────────────

/// Deserialise one scalar element from `bytes[offset..]` and return a
/// `(rust_literal_string, bytes_consumed)` pair for `ct_csharp_tokens`.
fn scalar_ct_lit(
	ty: &CsharpType,
	bytes: &[u8],
	offset: usize,
) -> Result<(String, usize), String> {
	let b = &bytes[offset..];
	match ty {
		CsharpType::Sbyte => {
			if b.is_empty() {
				return Err("ct_csharp: truncated sbyte element".to_string());
			}
			Ok((format!("{}", i8::from_le_bytes([b[0]])), 1))
		}
		CsharpType::Byte => {
			if b.is_empty() {
				return Err("ct_csharp: truncated byte element".to_string());
			}
			Ok((format!("{}", b[0]), 1))
		}
		CsharpType::Short => {
			if b.len() < 2 {
				return Err("ct_csharp: truncated short element".to_string());
			}
			Ok((format!("{}", i16::from_le_bytes([b[0], b[1]])), 2))
		}
		CsharpType::Ushort => {
			if b.len() < 2 {
				return Err("ct_csharp: truncated ushort element".to_string());
			}
			Ok((format!("{}", u16::from_le_bytes([b[0], b[1]])), 2))
		}
		CsharpType::Int => {
			let arr: [u8; 4] = b[..4]
				.try_into()
				.map_err(|_| "ct_csharp: truncated int element")?;
			Ok((format!("{}", i32::from_le_bytes(arr)), 4))
		}
		CsharpType::Uint => {
			let arr: [u8; 4] = b[..4]
				.try_into()
				.map_err(|_| "ct_csharp: truncated uint element")?;
			Ok((format!("{}", u32::from_le_bytes(arr)), 4))
		}
		CsharpType::Long => {
			let arr: [u8; 8] = b[..8]
				.try_into()
				.map_err(|_| "ct_csharp: truncated long element")?;
			Ok((format!("{}", i64::from_le_bytes(arr)), 8))
		}
		CsharpType::Ulong => {
			let arr: [u8; 8] = b[..8]
				.try_into()
				.map_err(|_| "ct_csharp: truncated ulong element")?;
			Ok((format!("{}", u64::from_le_bytes(arr)), 8))
		}
		CsharpType::Float => {
			let arr: [u8; 4] = b[..4]
				.try_into()
				.map_err(|_| "ct_csharp: truncated float element")?;
			let bits = u32::from_le_bytes(arr);
			Ok((format!("f32::from_bits(0x{bits:08x}_u32)"), 4))
		}
		CsharpType::Double => {
			let arr: [u8; 8] = b[..8]
				.try_into()
				.map_err(|_| "ct_csharp: truncated double element")?;
			let bits = u64::from_le_bytes(arr);
			Ok((format!("f64::from_bits(0x{bits:016x}_u64)"), 8))
		}
		CsharpType::Bool => {
			if b.is_empty() {
				return Err("ct_csharp: truncated bool element".to_string());
			}
			Ok((if b[0] != 0 { "true".to_string() } else { "false".to_string() }, 1))
		}
		CsharpType::Char => {
			if b.len() < 2 {
				return Err("ct_csharp: truncated char element".to_string());
			}
			let code_unit = u16::from_le_bytes([b[0], b[1]]);
			let c = char::from_u32(u32::from(code_unit))
				.ok_or("ct_csharp: C# char is not a valid Unicode scalar value")?;
			Ok((format!("{c:?}"), 2))
		}
		CsharpType::Str => {
			// String inside container: u32 length prefix
			if b.len() < 4 {
				return Err("ct_csharp: truncated String length prefix".to_string());
			}
			let len = u32::from_le_bytes(b[..4].try_into().unwrap()) as usize;
			if b.len() < 4 + len {
				return Err(format!(
					"ct_csharp: truncated String element (expected {len} bytes)"
				));
			}
			let s = String::from_utf8(b[4..4 + len].to_vec())
				.map_err(|_| "ct_csharp: String element is not valid UTF-8".to_string())?;
			Ok((format!("{s:?}"), 4 + len))
		}
		_ => Err("ct_csharp: scalar_ct_lit called on non-scalar type".to_string()),
	}
}

/// Recursively decode one value of `ty` from `bytes[*cur..]`, advance `*cur`,
/// and return a Rust literal/expression token stream.
fn ct_csharp_tokens_recursive(
	ty: &CsharpType,
	bytes: &[u8],
	cur: &mut usize,
) -> Result<proc_macro2::TokenStream, String> {
	match ty {
		CsharpType::Array(inner) | CsharpType::List(inner) => {
			if bytes[*cur..].len() < 4 {
				return Err("ct_csharp: array/list output too short (missing length)".to_string());
			}
			let n = u32::from_le_bytes(bytes[*cur..*cur + 4].try_into().unwrap()) as usize;
			*cur += 4;
			let mut lits: Vec<proc_macro2::TokenStream> = Vec::with_capacity(n);
			for _ in 0..n {
				lits.push(ct_csharp_tokens_recursive(inner, bytes, cur)?);
			}
			Ok(quote! { [#(#lits),*] })
		}
		CsharpType::Nullable(inner) => {
			if bytes[*cur..].is_empty() {
				return Err("ct_csharp: nullable output is empty".to_string());
			}
			let tag = bytes[*cur];
			*cur += 1;
			if tag == 0 {
				proc_macro2::TokenStream::from_str("::std::option::Option::None")
					.map_err(|e| format!("ct_csharp: produced invalid Rust token: {e}"))
			} else {
				let inner_ts = ct_csharp_tokens_recursive(inner, bytes, cur)?;
				Ok(quote! { ::std::option::Option::Some(#inner_ts) })
			}
		}
		CsharpType::Str => {
			// String inside container: u32 length prefix
			let (lit, consumed) = scalar_ct_lit(ty, bytes, *cur)?;
			*cur += consumed;
			proc_macro2::TokenStream::from_str(&lit)
				.map_err(|e| format!("ct_csharp: produced invalid Rust token: {e}"))
		}
		_ => {
			// Scalar types
			let (lit, consumed) = scalar_ct_lit(ty, bytes, *cur)?;
			*cur += consumed;
			proc_macro2::TokenStream::from_str(&lit)
				.map_err(|e| format!("ct_csharp: produced invalid Rust token: {e}"))
		}
	}
}

// ── ParsedCsharp + source parser ──────────────────────────────────────────────

/// Output of the unified C# source parser.
struct ParsedCsharp {
	/// The `using` directives verbatim from the original source.
	usings: String,
	/// The `namespace` declaration (e.g. `"namespace MyNs;"`) or empty string.
	namespace_decl: String,
	/// Any class/interface/enum declarations written before `Run()`.
	outer: String,
	/// The `Run()` method and everything after it, verbatim from the original source.
	body: String,
	/// Parameters declared in `Run(...)`, in order.
	params: Vec<(CsharpType, String)>,
	/// Return type of the `static T Run(...)` method.
	csharp_type: CsharpType,
}

/// Recursively parse a `CsharpType` from `tts` starting at index 0.
/// Returns `(csharp_type, tokens_consumed)` on success.
///
/// Recognises:
/// - Scalar: `T` where T is a C# type name
/// - Array: `T[]`, `T[][]`, … (Ident + one or more empty Bracket groups)
/// - List: `List<T>`
/// - Nullable suffix: `T?` wraps the whole type in `Nullable`
fn parse_csharp_type(tts: &[TokenTree]) -> Result<(CsharpType, usize), String> {
	if tts.is_empty() {
		return Err(
			"inline_csharp: unexpected end of tokens while parsing C# type".to_string(),
		);
	}

	match tts.first() {
		Some(TokenTree::Ident(id)) => {
			let name = id.to_string();
			let (base_ty, consumed) = if name == "List" {
				// Expect `<` inner_type `>`
				if !matches!(tts.get(1), Some(TokenTree::Punct(p)) if p.as_char() == '<') {
					return Err("inline_csharp: expected `<` after `List`".to_string());
				}
				let (inner_ty, inner_consumed) = parse_csharp_type_inner(&tts[2..])?;
				let close_idx = 2 + inner_consumed;
				if !matches!(tts.get(close_idx), Some(TokenTree::Punct(p)) if p.as_char() == '>')
				{
					return Err("inline_csharp: expected `>` to close `List<...>`".to_string());
				}
				(CsharpType::List(Box::new(inner_ty)), close_idx + 1)
			} else if let Some(scalar) = CsharpType::from_name(&name) {
				(scalar, 1)
			} else {
				return Err(format!(
					"inline_csharp: `{name}` is not a supported C# type; \
					 scalar types: sbyte byte short ushort int uint long ulong float double bool char string"
				));
			};

			// Consume trailing `[]` bracket groups, each wraps in Array.
			let mut ty = base_ty;
			let mut total_consumed = consumed;
			while matches!(
				tts.get(total_consumed),
				Some(TokenTree::Group(g))
					if g.delimiter() == proc_macro2::Delimiter::Bracket
					   && g.stream().is_empty()
			) {
				ty = CsharpType::Array(Box::new(ty));
				total_consumed += 1;
			}

			// Consume optional `?` suffix — wraps the outermost type in Nullable.
			if matches!(tts.get(total_consumed), Some(TokenTree::Punct(p)) if p.as_char() == '?')
			{
				ty = CsharpType::Nullable(Box::new(ty));
				total_consumed += 1;
			}

			Ok((ty, total_consumed))
		}
		_ => Err("inline_csharp: expected a C# type name".to_string()),
	}
}

/// Like `parse_csharp_type` but for use inside `<>` generics.
fn parse_csharp_type_inner(tts: &[TokenTree]) -> Result<(CsharpType, usize), String> {
	if tts.is_empty() {
		return Err(
			"inline_csharp: unexpected end of tokens while parsing C# type argument".to_string(),
		);
	}

	match tts.first() {
		Some(TokenTree::Ident(id)) => {
			let name = id.to_string();
			let (base_ty, consumed) = if name == "List" {
				if !matches!(tts.get(1), Some(TokenTree::Punct(p)) if p.as_char() == '<') {
					return Err("inline_csharp: expected `<` after `List`".to_string());
				}
				let (inner_ty, inner_consumed) = parse_csharp_type_inner(&tts[2..])?;
				let close_idx = 2 + inner_consumed;
				if !matches!(tts.get(close_idx), Some(TokenTree::Punct(p)) if p.as_char() == '>')
				{
					return Err("inline_csharp: expected `>` to close `List<...>`".to_string());
				}
				(CsharpType::List(Box::new(inner_ty)), close_idx + 1)
			} else if let Some(scalar) = CsharpType::from_name(&name) {
				(scalar, 1)
			} else {
				return Err(format!(
					"inline_csharp: `{name}` is not a supported C# type argument; \
					 supported: sbyte byte short ushort int uint long ulong float double bool char string"
				));
			};

			// Consume trailing `[]` bracket groups.
			let mut ty = base_ty;
			let mut total_consumed = consumed;
			while matches!(
				tts.get(total_consumed),
				Some(TokenTree::Group(g))
					if g.delimiter() == proc_macro2::Delimiter::Bracket
					   && g.stream().is_empty()
			) {
				ty = CsharpType::Array(Box::new(ty));
				total_consumed += 1;
			}

			// Consume optional `?` suffix.
			if matches!(tts.get(total_consumed), Some(TokenTree::Punct(p)) if p.as_char() == '?')
			{
				ty = CsharpType::Nullable(Box::new(ty));
				total_consumed += 1;
			}

			Ok((ty, total_consumed))
		}
		_ => Err("inline_csharp: expected a C# type name inside `<>`".to_string()),
	}
}

/// Scan `tts` for the first `[visibility] static <T> Run` pattern and return the
/// corresponding `CsharpType` together with the index of the method declaration
/// start and the index of the `Run` identifier token.
///
/// Returns `(csharp_type, method_start_idx, run_idx)`.
fn parse_run_return_type(tts: &[TokenTree]) -> Result<(CsharpType, usize, usize), String> {
	for i in 0..tts.len().saturating_sub(2) {
		if !matches!(&tts[i], TokenTree::Ident(id) if id == "static") {
			continue;
		}

		// Include an optional preceding visibility modifier in the returned start index.
		let start = if i > 0
			&& matches!(&tts[i - 1], TokenTree::Ident(id)
				if matches!(id.to_string().as_str(), "public" | "private" | "protected"))
		{
			i - 1
		} else {
			i
		};

		let type_start = i + 1;
		if type_start >= tts.len() {
			continue;
		}

		if let Ok((csharp_type, consumed)) = parse_csharp_type(&tts[type_start..]) {
			let run_idx = type_start + consumed;
			if matches!(tts.get(run_idx), Some(TokenTree::Ident(id)) if id == "Run") {
				return Ok((csharp_type, start, run_idx));
			}
		}
	}
	Err("inline_csharp: could not find `static <type> Run()` in C# body".to_string())
}

/// Parse the parameter list from the `Group(Parenthesis)` token immediately
/// after the `Run` identifier. Returns `Vec<(CsharpType, param_name)>`.
fn parse_run_params(tts: &[TokenTree]) -> Result<Vec<(CsharpType, String)>, String> {
	let group = match tts.first() {
		Some(TokenTree::Group(g)) if g.delimiter() == proc_macro2::Delimiter::Parenthesis => g,
		_ => return Ok(vec![]),
	};

	let inner: Vec<TokenTree> = group.stream().into_iter().collect();
	if inner.is_empty() {
		return Ok(vec![]);
	}

	let mut params = Vec::new();
	let mut segments: Vec<Vec<TokenTree>> = Vec::new();
	let mut current: Vec<TokenTree> = Vec::new();
	let mut angle_depth = 0i32;
	for tt in inner {
		if matches!(&tt, TokenTree::Punct(p) if p.as_char() == '<') {
			angle_depth += 1;
			current.push(tt);
		} else if matches!(&tt, TokenTree::Punct(p) if p.as_char() == '>') {
			angle_depth -= 1;
			current.push(tt);
		} else if matches!(&tt, TokenTree::Punct(p) if p.as_char() == ',') && angle_depth == 0 {
			segments.push(std::mem::take(&mut current));
		} else {
			current.push(tt);
		}
	}
	if !current.is_empty() {
		segments.push(current);
	}

	for seg in segments {
		if seg.is_empty() {
			continue;
		}

		let param_name = match seg.last() {
			Some(TokenTree::Ident(id)) => id.to_string(),
			_ => {
				return Err(
					"inline_csharp: unexpected token in Run() parameter list: expected a parameter name"
						.to_string(),
				);
			}
		};

		let type_tts = &seg[..seg.len() - 1];
		if type_tts.is_empty() {
			return Err(format!(
				"inline_csharp: missing type for parameter `{param_name}`"
			));
		}

		let (csharp_type, consumed) = parse_csharp_type(type_tts).map_err(|e| {
			format!("inline_csharp: error parsing type of parameter `{param_name}`: {e}")
		})?;

		if consumed != type_tts.len() {
			return Err(format!(
				"inline_csharp: unexpected tokens after type of parameter `{param_name}`"
			));
		}

		params.push((csharp_type, param_name));
	}

	Ok(params)
}

/// Unified parser: walks the token stream once to separate `using` directives
/// from the method body, identify the `Run()` return type and parameters.
fn parse_csharp_source(stream: proc_macro2::TokenStream) -> Result<ParsedCsharp, String> {
	let tts: Vec<TokenTree> = stream.into_iter().collect();

	// Separate usings from body: collect leading `using ... ;` sequences.
	// Skip `using static` and `using var` (treated as non-using).
	let mut first_using_idx: Option<usize> = None;
	let mut last_using_end_idx: Option<usize> = None;
	let mut first_body_idx: Option<usize> = None;
	let mut in_usings = true;
	let mut i = 0usize;

	while i < tts.len() && in_usings {
		match &tts[i] {
			TokenTree::Ident(id) if id == "using" => {
				// Check for `using static` or `using var` — not a namespace using
				let is_namespace_using =
					!matches!(tts.get(i + 1), Some(TokenTree::Ident(next))
						if next == "static" || next == "var");
				if is_namespace_using {
					first_using_idx.get_or_insert(i);
					// Scan forward for the terminating ';'.
					let semi = tts[i + 1..]
						.iter()
						.position(|t| matches!(t, TokenTree::Punct(p) if p.as_char() == ';'))
						.map(|rel| i + 1 + rel);
					if let Some(semi_idx) = semi {
						last_using_end_idx = Some(semi_idx);
						i = semi_idx + 1;
					} else {
						in_usings = false;
						first_body_idx = Some(i);
					}
				} else {
					in_usings = false;
					first_body_idx = Some(i);
				}
			}
			_ => {
				in_usings = false;
				first_body_idx = Some(i);
			}
		}
	}
	if first_body_idx.is_none() && i < tts.len() {
		first_body_idx = Some(i);
	}
	let body_start = first_body_idx.unwrap_or(tts.len());

	// Parse return type and Run index from body tokens.
	let (csharp_type, run_rel_idx, run_rel_run_idx) =
		parse_run_return_type(&tts[body_start..])?;
	let run_abs_idx = body_start + run_rel_idx;
	let run_token_abs_idx = body_start + run_rel_run_idx;

	// Parse Run() parameters.
	let params = parse_run_params(&tts[run_token_abs_idx + 1..])?;

	// Helper: get source text for a contiguous slice of tts, with fallback.
	let slice_text = |lo: usize, hi: usize| -> String {
		if lo >= hi {
			return String::new();
		}
		tts[lo]
			.span()
			.join(tts[hi - 1].span())
			.and_then(|s| s.source_text())
			.unwrap_or_else(|| {
				tts[lo..hi]
					.iter()
					.map(std::string::ToString::to_string)
					.collect::<Vec<_>>()
					.join(" ")
			})
	};

	// usings: span from first using keyword to last ';'
	let usings = match (first_using_idx, last_using_end_idx) {
		(Some(fi), Some(le)) => slice_text(fi, le + 1),
		_ => String::new(),
	};

	// outer: any tokens between end of usings and the `Run` method declaration
	let outer = slice_text(body_start, run_abs_idx);

	// body: from the `Run` method declaration to end (verbatim).
	let body = if run_abs_idx < tts.len() {
		let start_span = tts[run_abs_idx].span();
		let end_span = tts.last().unwrap().span();
		match start_span.join(end_span).and_then(|s| s.source_text()) {
			Some(raw) => raw,
			None => tts[run_abs_idx..]
				.iter()
				.map(std::string::ToString::to_string)
				.collect::<Vec<_>>()
				.join(" "),
		}
	} else {
		String::new()
	};

	// Extract namespace declaration from outer section (substring search).
	let namespace_decl = parse_namespace_name(&outer)
		.map(|ns| format!("namespace {ns};"))
		.unwrap_or_default();

	Ok(ParsedCsharp {
		usings,
		namespace_decl,
		outer,
		body,
		params,
		csharp_type,
	})
}

// ── Option extraction ─────────────────────────────────────────────────────────

struct DotnetOpts {
	build_args: String,
	run_args: String,
	references: Vec<String>,
}

impl Default for DotnetOpts {
	fn default() -> Self {
		Self {
			build_args: String::new(),
			run_args: String::new(),
			references: Vec::new(),
		}
	}
}

/// Consume leading `build = "…"` / `run = "…"` / `reference = "…"` option pairs
/// (comma-separated, trailing comma optional) and return the remaining token stream.
fn extract_opts(input: proc_macro2::TokenStream) -> (DotnetOpts, proc_macro2::TokenStream) {
	let mut tts: Vec<TokenTree> = input.into_iter().collect();
	let mut opts = DotnetOpts::default();
	let mut cursor = 0;

	loop {
		match try_parse_opt(&tts[cursor..]) {
			None => break,
			Some((key, val, consumed)) => {
				match key.as_str() {
					"build" => opts.build_args = val,
					"run" => opts.run_args = val,
					"reference" => opts.references.push(val),
					_ => break,
				}
				cursor += consumed;
				if let Some(TokenTree::Punct(p)) = tts.get(cursor)
					&& p.as_char() == ','
				{
					cursor += 1;
				}
			}
		}
	}

	let rest = tts.drain(cursor..).collect();
	(opts, rest)
}

/// Try to parse `Ident("build"|"run"|"reference") Punct("=") Literal(string)` at the
/// start of `tts`. Returns `(key, unquoted_value, tokens_consumed)` or `None`.
fn try_parse_opt(tts: &[TokenTree]) -> Option<(String, String, usize)> {
	let key = match tts.first() {
		Some(TokenTree::Ident(id)) => id.to_string(),
		_ => return None,
	};
	let Some(TokenTree::Punct(eq)) = tts.get(1) else {
		return None;
	};
	if eq.as_char() != '=' {
		return None;
	}
	let Some(TokenTree::Literal(lit)) = tts.get(2) else {
		return None;
	};
	let value = litrs::StringLit::try_from(lit).ok()?.value().to_owned();
	Some((key, value, 3))
}

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Compute a deterministic class name by hashing the source and options.
fn make_class_name(
	prefix: &str,
	usings: &str,
	outer: &str,
	body: &str,
	opts: &DotnetOpts,
) -> String {
	let mut h = DefaultHasher::new();
	usings.hash(&mut h);
	outer.hash(&mut h);
	body.hash(&mut h);
	opts.build_args.hash(&mut h);
	opts.run_args.hash(&mut h);
	opts.references.hash(&mut h);
	format!("{prefix}_{:016x}", h.finish())
}

/// Extract the namespace name from a source string using substring search.
fn parse_namespace_name(source: &str) -> Option<String> {
	let marker = "namespace ";
	let i = source.find(marker)?;
	if i > 0 && !source[..i].ends_with(|c: char| c.is_whitespace()) {
		return None;
	}
	let rest = source[i + marker.len()..].trim_start();
	let semi = rest.find(';')?;
	let ns = rest[..semi].trim().replace(|c: char| c.is_whitespace(), "");
	if ns.is_empty() { None } else { Some(ns) }
}

/// Render the complete C# source file.
fn format_csharp_source(
	usings: &str,
	namespace_decl: &str,
	class_name: &str,
	outer: &str,
	body: &str,
	main_method: &str,
) -> String {
	format!(
		"using System;\nusing System.Collections.Generic;\nusing System.IO;\n{usings}\n{namespace_decl}\nclass {class_name} {{\n\n{outer}\n\n{body}\n\n{main_method}\n}}\n"
	)
}

/// Compile and run C# at compile time, returning raw stdout bytes.
fn compile_run_csharp_now(
	class_name: &str,
	csharp_source: &str,
	build_raw: Option<&str>,
	run_raw: Option<&str>,
	references: &[&str],
) -> Result<Vec<u8>, String> {
	inline_csharp_core::run_csharp(
		class_name,
		csharp_source,
		build_raw.unwrap_or(""),
		run_raw.unwrap_or(""),
		references,
		&[],
	)
	.map_err(|e| e.to_string())
}

// ── make_runner_fn ─────────────────────────────────────────────────────────────

/// Generate a `fn __csharp_runner(...) -> Result<T, CsharpError>` token stream
/// used by both `csharp!` and `csharp_fn!`.
#[allow(clippy::similar_names)]
fn make_runner_fn(
	parsed: ParsedCsharp,
	opts: DotnetOpts,
	prefix: &str,
) -> proc_macro2::TokenStream {
	let ParsedCsharp {
		usings,
		namespace_decl,
		outer,
		body,
		params,
		csharp_type,
	} = parsed;

	let class_name = make_class_name(prefix, &usings, &outer, &body, &opts);
	let main_method = csharp_type.csharp_main(&params);
	let csharp_source =
		format_csharp_source(&usings, &namespace_decl, &class_name, &outer, &body, &main_method);

	let build_raw = opts.build_args;
	let run_raw = opts.run_args;
	let reference_strs: Vec<proc_macro2::TokenStream> = opts
		.references
		.iter()
		.map(|r| {
			let lit = proc_macro2::Literal::string(r);
			quote! { #lit }
		})
		.collect();

	let deser = csharp_type.rust_deser();
	let ret_ty = csharp_type.rust_return_type_ts();

	let fn_params: Vec<proc_macro2::TokenStream> = params
		.iter()
		.map(|(ty, name)| {
			let ident = proc_macro2::Ident::new(name, proc_macro2::Span::call_site());
			let param_ty = ty.rust_param_type_ts();
			quote! { #ident: #param_ty }
		})
		.collect();

	let ser_stmts: Vec<proc_macro2::TokenStream> = params
		.iter()
		.map(|(ty, name)| {
			let ident = proc_macro2::Ident::new(name, proc_macro2::Span::call_site());
			let ident_ts = quote! { #ident };
			ty.rust_ser_ts(&ident_ts, 0)
		})
		.collect();

	quote! {
		fn __csharp_runner(#(#fn_params),*) -> ::std::result::Result<#ret_ty, ::inline_csharp::CsharpError> {
			let mut _stdin_bytes: ::std::vec::Vec<u8> = ::std::vec::Vec::new();
			#(#ser_stmts)*
			let _raw = ::inline_csharp::run_csharp(
				#class_name,
				#csharp_source,
				#build_raw,
				#run_raw,
				&[#(#reference_strs),*],
				&_stdin_bytes,
			)?;
			::std::result::Result::Ok(#deser)
		}
	}
}

// ── ct_csharp_impl ────────────────────────────────────────────────────────────

fn ct_csharp_impl(
	input: proc_macro2::TokenStream,
) -> Result<proc_macro2::TokenStream, String> {
	let (opts, input) = extract_opts(input);

	let ParsedCsharp {
		usings,
		namespace_decl,
		outer,
		body,
		csharp_type,
		..
	} = parse_csharp_source(input)?;

	let class_name = make_class_name("CtCsharp", &usings, &outer, &body, &opts);
	let main_method = csharp_type.csharp_main(&[]);
	let csharp_source =
		format_csharp_source(&usings, &namespace_decl, &class_name, &outer, &body, &main_method);

	let refs: Vec<&str> = opts.references.iter().map(String::as_str).collect();
	let bytes = compile_run_csharp_now(
		&class_name,
		&csharp_source,
		Some(&opts.build_args),
		Some(&opts.run_args),
		&refs,
	)?;
	csharp_type.ct_csharp_tokens(bytes)
}

// ── Public proc macros ────────────────────────────────────────────────────────

/// Compile and run zero-argument C# code at *program runtime*.
///
/// Wraps the provided C# body in a generated class, compiles it with `dotnet build`,
/// and executes it with `dotnet run`. The return value of `static T Run()` is
/// binary-serialised by the generated `Main()` and deserialised to the inferred
/// Rust type.
///
/// Expands to `Result<T, inline_csharp::CsharpError>`.
///
/// For `Run()` methods that take parameters, use [`csharp_fn!`] instead.
///
/// # Options
///
/// Optional `key = "value"` pairs may appear before the C# body, separated by commas:
///
/// - `build = "<args>"` — extra arguments for `dotnet build`.
/// - `run   = "<args>"` — extra arguments for `dotnet run`.
/// - `reference = "<path>"` — add a reference assembly (repeatable).
///
/// # Examples
///
/// ```text
/// use inline_csharp::csharp;
///
/// let x: i32 = csharp! {
///     static int Run() {
///         return 42;
///     }
/// }.unwrap();
/// ```
#[proc_macro]
#[allow(clippy::similar_names)]
pub fn csharp(input: TokenStream) -> TokenStream {
	let input2 = proc_macro2::TokenStream::from(input);
	let (opts, input2) = extract_opts(input2);

	let parsed = match parse_csharp_source(input2) {
		Ok(p) => p,
		Err(msg) => return quote! { compile_error!(#msg) }.into(),
	};

	let runner_fn = make_runner_fn(parsed, opts, "InlineCsharp");

	let generated = quote! {
		{
			#runner_fn
			__csharp_runner()
		}
	};

	generated.into()
}

/// Return a typed Rust function that compiles and runs C# at *program runtime*.
///
/// Like [`csharp!`], but supports parameters. The parameters declared in the
/// C# `Run(P1 p1, P2 p2, ...)` method become the Rust function's parameters.
/// Arguments are serialised by Rust and piped to the C# process via stdin;
/// C# reads them with `BinaryReader`.
///
/// Expands to a function value of type `fn(P1, P2, ...) -> Result<T, CsharpError>`.
///
/// # Examples
///
/// ```text
/// use inline_csharp::csharp_fn;
///
/// let double_it = csharp_fn! {
///     static int Run(int n) {
///         return n * 2;
///     }
/// };
/// let result: i32 = double_it(21).unwrap();
/// assert_eq!(result, 42);
/// ```
#[proc_macro]
#[allow(clippy::similar_names)]
pub fn csharp_fn(input: TokenStream) -> TokenStream {
	let input2 = proc_macro2::TokenStream::from(input);
	let (opts, input2) = extract_opts(input2);

	let parsed = match parse_csharp_source(input2) {
		Ok(p) => p,
		Err(msg) => return quote! { compile_error!(#msg) }.into(),
	};

	let runner_fn = make_runner_fn(parsed, opts, "InlineCsharp");

	let generated = quote! {
		{
			#runner_fn
			__csharp_runner
		}
	};

	generated.into()
}

/// Run C# at *compile time* and splice its return value as a Rust literal.
///
/// Accepts optional `build = "..."`, `run = "..."`, and `reference = "..."` key-value
/// pairs before the C# body. The user provides a `static <T> Run()` method; its
/// binary-serialised return value is decoded and emitted as a Rust literal at
/// the call site.
///
/// C# compilation/runtime errors become Rust `compile_error!` diagnostics.
///
/// # Examples
///
/// ```text
/// use inline_csharp::ct_csharp;
///
/// const PI_APPROX: f64 = ct_csharp! {
///     static double Run() {
///         return Math.PI;
///     }
/// };
/// ```
#[proc_macro]
pub fn ct_csharp(input: TokenStream) -> TokenStream {
	match ct_csharp_impl(proc_macro2::TokenStream::from(input)) {
		Ok(ts) => ts.into(),
		Err(msg) => quote! { compile_error!(#msg) }.into(),
	}
}
