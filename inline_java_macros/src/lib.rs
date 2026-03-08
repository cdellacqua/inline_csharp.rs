//! Proc-macro implementation for `inline_java`.
//!
//! Provides three proc macros for embedding Java in Rust:
//!
//! | Macro        | When it runs    |
//! |--------------|-----------------|
//! | [`java!`]    | program runtime |
//! | [`java_fn!`] | program runtime |
//! | [`ct_java!`] | compile time    |
//!
//! All macros require the user to write a `static <T> run(...)` method
//! where `T` is one of: `byte`, `short`, `int`, `long`, `float`, `double`,
//! `boolean`, `char`, `String`, `T[]`, `List<BoxedT>`, or `Optional<BoxedT>`.
//!
//! # Wire format (Java → Rust, stdout)
//!
//! The macro generates a `main()` that binary-serialises `run()`'s return
//! value to stdout via `DataOutputStream` (raw UTF-8 for `String` scalars).
//! Arrays and `List`s use a length-prefixed format:
//!
//! - 4 bytes big-endian `int32`: number of elements
//! - for each element: fixed-size `DataOutputStream` encoding, or
//!   4-byte length prefix + UTF-8 bytes for `String`.
//!
//! # Wire format (Rust → Java, stdin)
//!
//! Parameters declared in `run(...)` are serialised by Rust and piped to the
//! child process's stdin. Java reads them with `DataInputStream`.
//!
//! # Options
//!
//! All three macros accept zero or more `key = "value"` pairs before the Java body,
//! comma-separated.  Recognised keys:
//!
//! - `javac = "<args>"` — extra arguments passed verbatim to `javac`
//!   (shell-quoted; single/double quotes respected).
//! - `java  = "<args>"` — extra arguments passed verbatim to `java`
//!   (shell-quoted; single/double quotes respected).

use proc_macro::TokenStream;
use proc_macro2::{Ident, TokenTree};
use quote::quote;
use std::collections::hash_map::DefaultHasher;
use std::fmt::Write as FmtWrite;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

// ScalarType — the nine primitive / String base types

#[derive(Clone, Copy, PartialEq)]
enum ScalarType {
	Byte,
	Short,
	Int,
	Long,
	Float,
	Double,
	Boolean,
	Char,
	Str,
}

impl ScalarType {
	/// Parse a Java primitive type name or "String".
	fn from_primitive_name(s: &str) -> Option<Self> {
		match s {
			"byte" => Some(Self::Byte),
			"short" => Some(Self::Short),
			"int" => Some(Self::Int),
			"long" => Some(Self::Long),
			"float" => Some(Self::Float),
			"double" => Some(Self::Double),
			"boolean" => Some(Self::Boolean),
			"char" => Some(Self::Char),
			"String" => Some(Self::Str),
			_ => None,
		}
	}

	/// Parse a Java boxed type name (used in `List<T>`).
	fn from_boxed_name(s: &str) -> Option<Self> {
		match s {
			"Byte" => Some(Self::Byte),
			"Short" => Some(Self::Short),
			"Integer" => Some(Self::Int),
			"Long" => Some(Self::Long),
			"Float" => Some(Self::Float),
			"Double" => Some(Self::Double),
			"Boolean" => Some(Self::Boolean),
			"Character" => Some(Self::Char),
			"String" => Some(Self::Str),
			_ => None,
		}
	}

	/// Java primitive / String type name used in `T[]` declarations.
	fn java_prim_name(self) -> &'static str {
		match self {
			Self::Byte => "byte",
			Self::Short => "short",
			Self::Int => "int",
			Self::Long => "long",
			Self::Float => "float",
			Self::Double => "double",
			Self::Boolean => "boolean",
			Self::Char => "char",
			Self::Str => "String",
		}
	}

	/// Java boxed type name used in `List<T>` declarations.
	fn java_boxed_name(self) -> &'static str {
		match self {
			Self::Byte => "Byte",
			Self::Short => "Short",
			Self::Int => "Integer",
			Self::Long => "Long",
			Self::Float => "Float",
			Self::Double => "Double",
			Self::Boolean => "Boolean",
			Self::Char => "Character",
			Self::Str => "String",
		}
	}

	/// `DataOutputStream` write method name; `None` for String (special case).
	fn dos_write_method(self) -> Option<&'static str> {
		match self {
			Self::Byte => Some("writeByte"),
			Self::Short => Some("writeShort"),
			Self::Int => Some("writeInt"),
			Self::Long => Some("writeLong"),
			Self::Float => Some("writeFloat"),
			Self::Double => Some("writeDouble"),
			Self::Boolean => Some("writeBoolean"),
			Self::Char => Some("writeChar"),
			Self::Str => None,
		}
	}

	/// Rust type token stream corresponding to this scalar.
	/// For `String`, returns `::std::string::String` (owned).
	fn rust_type_ts(self) -> proc_macro2::TokenStream {
		match self {
			Self::Byte => quote! { i8 },
			Self::Short => quote! { i16 },
			Self::Int => quote! { i32 },
			Self::Long => quote! { i64 },
			Self::Float => quote! { f32 },
			Self::Double => quote! { f64 },
			Self::Boolean => quote! { bool },
			Self::Char => quote! { char },
			Self::Str => quote! { ::std::string::String },
		}
	}

	/// Rust parameter type for `java_fn!` function signatures.
	/// `String` params use `&str` so both `&str` and `&String` work at call sites.
	fn rust_param_type_ts(self) -> proc_macro2::TokenStream {
		if self == Self::Str {
			quote! { &str }
		} else {
			self.rust_type_ts()
		}
	}

	/// Generates Rust code to serialize one parameter value into `_stdin_bytes: Vec<u8>`.
	/// `param_ident` is the Rust identifier holding the value.
	fn rust_ser_ts(self, param_ident: &Ident) -> proc_macro2::TokenStream {
		match self {
			Self::Byte => quote! {
				_stdin_bytes.extend_from_slice(&(#param_ident as i8).to_be_bytes());
			},
			Self::Short => quote! {
				_stdin_bytes.extend_from_slice(&(#param_ident as i16).to_be_bytes());
			},
			Self::Int => quote! {
				_stdin_bytes.extend_from_slice(&(#param_ident as i32).to_be_bytes());
			},
			Self::Long => quote! {
				_stdin_bytes.extend_from_slice(&(#param_ident as i64).to_be_bytes());
			},
			Self::Float => quote! {
				_stdin_bytes.extend_from_slice(&(#param_ident as f32).to_bits().to_be_bytes());
			},
			Self::Double => quote! {
				_stdin_bytes.extend_from_slice(&(#param_ident as f64).to_bits().to_be_bytes());
			},
			Self::Boolean => quote! {
				_stdin_bytes.push(#param_ident as u8);
			},
			Self::Char => quote! {
				{
					let _c = #param_ident as u32;
					assert!(_c <= 0xFFFF, "inline_java: char value exceeds u16 range");
					_stdin_bytes.extend_from_slice(&(_c as u16).to_be_bytes());
				}
			},
			Self::Str => quote! {
				{
					let _b = #param_ident.as_bytes();
					let _len = _b.len() as i32;
					_stdin_bytes.extend_from_slice(&_len.to_be_bytes());
					_stdin_bytes.extend_from_slice(_b);
				}
			},
		}
	}

	/// Generates Java statement(s) to read this type from a `DataInputStream` named `_dis`.
	fn java_dis_read(self, param_name: &str) -> String {
		match self {
			Self::Byte => format!("byte {param_name} = _dis.readByte();"),
			Self::Short => format!("short {param_name} = _dis.readShort();"),
			Self::Int => format!("int {param_name} = _dis.readInt();"),
			Self::Long => format!("long {param_name} = _dis.readLong();"),
			Self::Float => format!("float {param_name} = _dis.readFloat();"),
			Self::Double => format!("double {param_name} = _dis.readDouble();"),
			Self::Boolean => format!("boolean {param_name} = _dis.readBoolean();"),
			Self::Char => format!("char {param_name} = _dis.readChar();"),
			Self::Str => format!(
				"int _len_{param_name} = _dis.readInt();\n\
				 \t\tbyte[] _b_{param_name} = new byte[_len_{param_name}];\n\
				 \t\t_dis.readFully(_b_{param_name});\n\
				 \t\tString {param_name} = new String(_b_{param_name}, java.nio.charset.StandardCharsets.UTF_8);"
			),
		}
	}

	/// Generates code for the body of the element-deserialization loop inside
	/// `rust_deser_array`.  Produces a statement that pushes one element onto
	/// `_v` and advances `_cur` by the element's byte width.
	fn rust_deser_one_ts(self) -> proc_macro2::TokenStream {
		match self {
			Self::Byte => quote! {
				_v.push(i8::from_be_bytes([_raw[_cur]]));
				_cur += 1;
			},
			Self::Short => quote! {
				_v.push(i16::from_be_bytes([_raw[_cur], _raw[_cur + 1]]));
				_cur += 2;
			},
			Self::Int => quote! {
				_v.push(i32::from_be_bytes([_raw[_cur], _raw[_cur + 1], _raw[_cur + 2], _raw[_cur + 3]]));
				_cur += 4;
			},
			Self::Long => quote! {
				_v.push(i64::from_be_bytes([
					_raw[_cur], _raw[_cur + 1], _raw[_cur + 2], _raw[_cur + 3],
					_raw[_cur + 4], _raw[_cur + 5], _raw[_cur + 6], _raw[_cur + 7],
				]));
				_cur += 8;
			},
			Self::Float => quote! {
				_v.push(f32::from_bits(u32::from_be_bytes([_raw[_cur], _raw[_cur + 1], _raw[_cur + 2], _raw[_cur + 3]])));
				_cur += 4;
			},
			Self::Double => quote! {
				_v.push(f64::from_bits(u64::from_be_bytes([
					_raw[_cur], _raw[_cur + 1], _raw[_cur + 2], _raw[_cur + 3],
					_raw[_cur + 4], _raw[_cur + 5], _raw[_cur + 6], _raw[_cur + 7],
				])));
				_cur += 8;
			},
			Self::Boolean => quote! {
				_v.push(_raw[_cur] != 0);
				_cur += 1;
			},
			Self::Char => quote! {
				_v.push(
					::std::char::from_u32(u16::from_be_bytes([_raw[_cur], _raw[_cur + 1]]) as u32)
						.ok_or(::inline_java::JavaError::InvalidChar)?
				);
				_cur += 2;
			},
			Self::Str => quote! {
				let _slen = i32::from_be_bytes([_raw[_cur], _raw[_cur + 1], _raw[_cur + 2], _raw[_cur + 3]]) as usize;
				_cur += 4;
				_v.push(::std::string::String::from_utf8(_raw[_cur.._cur + _slen].to_vec())?);
				_cur += _slen;
			},
		}
	}
}

// ParamType — wraps ScalarType with an Optional variant for run() parameters

#[derive(Clone, Copy, PartialEq)]
enum ParamType {
	Scalar(ScalarType),
	Optional(ScalarType),
}

impl ParamType {
	/// Rust parameter type for `java_fn!` function signatures.
	/// For `Optional(String)` the inner type is `&str`.
	fn rust_param_type_ts(self) -> proc_macro2::TokenStream {
		match self {
			Self::Scalar(s) => s.rust_param_type_ts(),
			Self::Optional(s) => {
				let inner = s.rust_param_type_ts(); // uses &str for String
				quote! { ::std::option::Option<#inner> }
			}
		}
	}

	/// Generates Rust code to serialize one parameter value into `_stdin_bytes: Vec<u8>`.
	fn rust_ser_ts(self, param_ident: &Ident) -> proc_macro2::TokenStream {
		match self {
			Self::Scalar(s) => s.rust_ser_ts(param_ident),
			Self::Optional(s) => {
				let inner_ident = Ident::new("_inner", proc_macro2::Span::call_site());
				let inner_ser = s.rust_ser_ts(&inner_ident);
				quote! {
					match #param_ident {
						::std::option::Option::None => _stdin_bytes.push(0u8),
						::std::option::Option::Some(_inner) => {
							_stdin_bytes.push(1u8);
							#inner_ser
						}
					}
				}
			}
		}
	}

	/// Generates Java statement(s) to read this type from a `DataInputStream` named `_dis`.
	fn java_dis_read(self, param_name: &str) -> String {
		match self {
			Self::Scalar(s) => s.java_dis_read(param_name),
			Self::Optional(s) => {
				let boxed = s.java_boxed_name();
				if s == ScalarType::Str {
					format!(
						"int _tag_{param_name} = _dis.readUnsignedByte();\n\
						 \t\tjava.util.Optional<String> {param_name};\n\
						 \t\tif (_tag_{param_name} != 0) {{\n\
						 \t\t\tint _len_{param_name} = _dis.readInt();\n\
						 \t\t\tbyte[] _b_{param_name} = new byte[_len_{param_name}];\n\
						 \t\t\t_dis.readFully(_b_{param_name});\n\
						 \t\t\t{param_name} = java.util.Optional.of(new String(_b_{param_name}, java.nio.charset.StandardCharsets.UTF_8));\n\
						 \t\t}} else {{\n\
						 \t\t\t{param_name} = java.util.Optional.empty();\n\
						 \t\t}}"
					)
				} else {
					let write_method = s.dos_write_method().unwrap();
					let read_method = write_method.replacen("write", "read", 1);
					format!(
						"int _tag_{param_name} = _dis.readUnsignedByte();\n\
						 \t\tjava.util.Optional<{boxed}> {param_name};\n\
						 \t\tif (_tag_{param_name} != 0) {{\n\
						 \t\t\t{param_name} = java.util.Optional.of(_dis.{read_method}());\n\
						 \t\t}} else {{\n\
						 \t\t\t{param_name} = java.util.Optional.empty();\n\
						 \t\t}}"
					)
				}
			}
		}
	}
}

// JavaType — allowed return types for run(), with serialisation/deserialisation

#[derive(Clone, Copy, PartialEq)]
enum JavaType {
	Scalar(ScalarType),
	/// Java `T[]` — returned as `Vec<T>` at runtime, `[T; N]` at compile time.
	Array(ScalarType),
	/// Java `List<BoxedT>` — same wire format / Rust type as `Array`.
	List(ScalarType),
	/// Java `Optional<BoxedT>` — returned as `Option<T>`.
	Optional(ScalarType),
}

impl JavaType {
	/// Generates the complete `main(String[] args)` method that binary-serialises
	/// `run()`'s return value to stdout.  `params` lists the parameters declared
	/// in `run(...)` so the generated `main` can read them from stdin and forward
	/// them to `run`.
	fn java_main(self, params: &[(ParamType, String)]) -> String {
		// Build DataInputStream setup + parameter reads (only if there are params).
		let param_reads = if params.is_empty() {
			String::new()
		} else {
			let mut s = String::from(
				"\t\tjava.io.DataInputStream _dis = new java.io.DataInputStream(System.in);\n",
			);
			for (ty, name) in params {
				writeln!(s, "\t\t{}", ty.java_dis_read(name)).unwrap();
			}
			s
		};

		// Build the run() call argument list.
		let run_args: String = params
			.iter()
			.map(|(_, name)| name.as_str())
			.collect::<Vec<_>>()
			.join(", ");

		match self {
			Self::Scalar(s) => {
				let serialize = if s == ScalarType::Str {
					format!(
						"byte[] _b = run({run_args}).getBytes(java.nio.charset.StandardCharsets.UTF_8);\n\
  				 \t\tSystem.out.write(_b);\n\
  				 \t\tSystem.out.flush();"
					)
				} else {
					let method = s.dos_write_method().unwrap();
					format!(
						"java.io.DataOutputStream _dos = \
  					 new java.io.DataOutputStream(System.out);\n\
  					 \t\t_dos.{method}(run({run_args}));\n\
  					 \t\t_dos.flush();"
					)
				};
				format!(
					"\tpublic static void main(String[] args) throws Exception {{\n\
					 {param_reads}\t\t{serialize}\n\
					 \t}}"
				)
			}
			Self::Array(s) => {
				let prim = s.java_prim_name();
				let loop_body = array_serialize_loop(s, prim);
				format!(
					"\tpublic static void main(String[] args) throws Exception {{\n\
					 {param_reads}\t\t{prim}[] _arr = run({run_args});\n\
					 \t\tjava.io.DataOutputStream _dos = new java.io.DataOutputStream(System.out);\n\
					 \t\t_dos.writeInt(_arr.length);\n\
					 \t\t{loop_body}\n\
					 \t\t_dos.flush();\n\
					 \t}}"
				)
			}
			Self::List(s) => {
				let boxed = s.java_boxed_name();
				let iter_type = if s == ScalarType::Str {
					"String"
				} else {
					boxed
				};
				let loop_body = array_serialize_loop(s, iter_type);
				format!(
					"\tpublic static void main(String[] args) throws Exception {{\n\
					 {param_reads}\t\tjava.util.List<{boxed}> _arr = run({run_args});\n\
					 \t\tjava.io.DataOutputStream _dos = new java.io.DataOutputStream(System.out);\n\
					 \t\t_dos.writeInt(_arr.size());\n\
					 \t\t{loop_body}\n\
					 \t\t_dos.flush();\n\
					 \t}}"
				)
			}
			Self::Optional(s) => {
				let boxed = s.java_boxed_name();
				let present_body = if s == ScalarType::Str {
					format!(
						"byte[] _b = _opt.get().getBytes(java.nio.charset.StandardCharsets.UTF_8);\n\
						 \t\t\t_dos.writeInt(_b.length);\n\
						 \t\t\t_dos.write(_b, 0, _b.length);"
					)
				} else {
					let method = s.dos_write_method().unwrap();
					format!("_dos.{method}(_opt.get());")
				};
				format!(
					"\tpublic static void main(String[] args) throws Exception {{\n\
					 {param_reads}\t\tjava.util.Optional<{boxed}> _opt = run({run_args});\n\
					 \t\tjava.io.DataOutputStream _dos = new java.io.DataOutputStream(System.out);\n\
					 \t\tif (_opt.isPresent()) {{\n\
					 \t\t\t_dos.writeByte(1);\n\
					 \t\t\t{present_body}\n\
					 \t\t}} else {{\n\
					 \t\t\t_dos.writeByte(0);\n\
					 \t\t}}\n\
					 \t\t_dos.flush();\n\
					 \t}}"
				)
			}
		}
	}

	/// Returns the Rust return type token stream for this Java type.
	fn rust_return_type_ts(self) -> proc_macro2::TokenStream {
		match self {
			Self::Scalar(s) => s.rust_type_ts(),
			Self::Array(s) | Self::List(s) => {
				let inner = s.rust_type_ts();
				quote! { ::std::vec::Vec<#inner> }
			}
			Self::Optional(s) => {
				let inner = s.rust_type_ts();
				quote! { ::std::option::Option<#inner> }
			}
		}
	}

	/// Returns a Rust expression (as a token stream) that deserialises the raw
	/// stdout bytes `_raw: Vec<u8>` into the corresponding Rust type.
	/// Used by `java!` and `java_fn!` at program runtime.
	fn rust_deser(self) -> proc_macro2::TokenStream {
		match self {
			Self::Scalar(s) => match s {
				ScalarType::Byte => quote! { i8::from_be_bytes([_raw[0]]) },
				ScalarType::Short => quote! { i16::from_be_bytes([_raw[0], _raw[1]]) },
				ScalarType::Int => {
					quote! { i32::from_be_bytes([_raw[0], _raw[1], _raw[2], _raw[3]]) }
				}
				ScalarType::Long => {
					quote! {
						i64::from_be_bytes([
							_raw[0], _raw[1], _raw[2], _raw[3],
							_raw[4], _raw[5], _raw[6], _raw[7],
						])
					}
				}
				ScalarType::Float => {
					quote! { f32::from_bits(u32::from_be_bytes([_raw[0], _raw[1], _raw[2], _raw[3]])) }
				}
				ScalarType::Double => {
					quote! {
						f64::from_bits(u64::from_be_bytes([
							_raw[0], _raw[1], _raw[2], _raw[3],
							_raw[4], _raw[5], _raw[6], _raw[7],
						]))
					}
				}
				ScalarType::Boolean => quote! { _raw[0] != 0 },
				ScalarType::Char => {
					quote! {
						::std::char::from_u32(u16::from_be_bytes([_raw[0], _raw[1]]) as u32)
							.ok_or(::inline_java::JavaError::InvalidChar)?
					}
				}
				ScalarType::Str => {
					quote! {
						::std::string::String::from_utf8(_raw)?
					}
				}
			},
			Self::Array(s) | Self::List(s) => {
				let rust_type = s.rust_type_ts();
				let deser_one = s.rust_deser_one_ts();
				quote! {
					{
						let _n = i32::from_be_bytes([_raw[0], _raw[1], _raw[2], _raw[3]]) as usize;
						let mut _cur = 4usize;
						let mut _v: Vec<#rust_type> = ::std::vec::Vec::with_capacity(_n);
						for _ in 0.._n {
							#deser_one
						}
						_v
					}
				}
			}
			Self::Optional(s) => match s {
				ScalarType::Byte => quote! {
					if _raw[0] == 0 { ::std::option::Option::None } else { ::std::option::Option::Some(i8::from_be_bytes([_raw[1]])) }
				},
				ScalarType::Short => quote! {
					if _raw[0] == 0 { ::std::option::Option::None } else { ::std::option::Option::Some(i16::from_be_bytes([_raw[1], _raw[2]])) }
				},
				ScalarType::Int => quote! {
					if _raw[0] == 0 { ::std::option::Option::None } else { ::std::option::Option::Some(i32::from_be_bytes([_raw[1], _raw[2], _raw[3], _raw[4]])) }
				},
				ScalarType::Long => quote! {
					if _raw[0] == 0 { ::std::option::Option::None } else { ::std::option::Option::Some(i64::from_be_bytes([_raw[1],_raw[2],_raw[3],_raw[4],_raw[5],_raw[6],_raw[7],_raw[8]])) }
				},
				ScalarType::Float => quote! {
					if _raw[0] == 0 { ::std::option::Option::None } else { ::std::option::Option::Some(f32::from_bits(u32::from_be_bytes([_raw[1],_raw[2],_raw[3],_raw[4]]))) }
				},
				ScalarType::Double => quote! {
					if _raw[0] == 0 { ::std::option::Option::None } else { ::std::option::Option::Some(f64::from_bits(u64::from_be_bytes([_raw[1],_raw[2],_raw[3],_raw[4],_raw[5],_raw[6],_raw[7],_raw[8]]))) }
				},
				ScalarType::Boolean => quote! {
					if _raw[0] == 0 { ::std::option::Option::None } else { ::std::option::Option::Some(_raw[1] != 0) }
				},
				ScalarType::Char => quote! {
					if _raw[0] == 0 {
						::std::option::Option::None
					} else {
						::std::option::Option::Some(
							::std::char::from_u32(u16::from_be_bytes([_raw[1], _raw[2]]) as u32)
								.ok_or(::inline_java::JavaError::InvalidChar)?
						)
					}
				},
				ScalarType::Str => quote! {
					if _raw[0] == 0 {
						::std::option::Option::None
					} else {
						let _slen = i32::from_be_bytes([_raw[1], _raw[2], _raw[3], _raw[4]]) as usize;
						::std::option::Option::Some(::std::string::String::from_utf8(_raw[5..5 + _slen].to_vec())?)
					}
				},
			},
		}
	}

	/// Converts the raw stdout bytes produced by the generated `main()` into a
	/// Rust literal / expression token stream to splice at the `ct_java!` call site.
	/// Scalars produce literals (42, 3.14, true, 'x', "hello").
	/// Arrays/Lists produce array expressions ([e0, e1, e2]).
	fn ct_java_tokens(self, bytes: Vec<u8>) -> Result<proc_macro2::TokenStream, String> {
		match self {
			Self::Scalar(s) => {
				// Scalar String is serialised as raw UTF-8 (no length prefix) — special case.
				let lit = if s == ScalarType::Str {
					let s = String::from_utf8(bytes)
						.map_err(|_| "ct_java: Java String is not valid UTF-8".to_string())?;
					format!("{s:?}")
				} else {
					let (l, _) = scalar_ct_lit(s, &bytes)?;
					l
				};
				proc_macro2::TokenStream::from_str(&lit)
					.map_err(|e| format!("ct_java: produced invalid Rust token: {e}"))
			}
			Self::Array(s) | Self::List(s) => {
				if bytes.len() < 4 {
					return Err("ct_java: array output too short (missing length)".to_string());
				}
				#[allow(clippy::cast_sign_loss)]
				let n = i32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize;
				let mut cur = 4;
				let mut lits: Vec<String> = Vec::with_capacity(n);
				for _ in 0..n {
					let (lit, consumed) = scalar_ct_lit(s, &bytes[cur..])?;
					lits.push(lit);
					cur += consumed;
				}
				let array_expr = format!("[{}]", lits.join(", "));
				proc_macro2::TokenStream::from_str(&array_expr)
					.map_err(|e| format!("ct_java: produced invalid Rust tokens: {e}"))
			}
			Self::Optional(s) => {
				if bytes.is_empty() {
					return Err("ct_java: optional output is empty".to_string());
				}
				if bytes[0] == 0 {
					proc_macro2::TokenStream::from_str("::std::option::Option::None")
						.map_err(|e| format!("ct_java: produced invalid Rust token: {e}"))
				} else {
					// For Scalar String ct_java_tokens special-cases raw UTF-8.
					// For Optional String the Java side writes writeByte(1) + writeInt(len) + bytes,
					// so bytes[1..] is length-prefixed — scalar_ct_lit(Str, …) handles that correctly.
					let (lit, _) = scalar_ct_lit(s, &bytes[1..])?;
					proc_macro2::TokenStream::from_str(&format!("::std::option::Option::Some({lit})"))
						.map_err(|e| format!("ct_java: produced invalid Rust token: {e}"))
				}
			}
		}
	}
}

// scalar_ct_lit — convert raw bytes to a Rust literal string for one element

/// Deserialise one element of type `s` from `bytes` and return a
/// `(rust_literal_string, bytes_consumed)` pair for use in `ct_java_tokens`.
fn scalar_ct_lit(s: ScalarType, bytes: &[u8]) -> Result<(String, usize), String> {
	match s {
		ScalarType::Byte => {
			if bytes.is_empty() {
				return Err("ct_java: truncated byte element".to_string());
			}
			Ok((format!("{}", i8::from_be_bytes([bytes[0]])), 1))
		}
		ScalarType::Short => {
			if bytes.len() < 2 {
				return Err("ct_java: truncated short element".to_string());
			}
			Ok((format!("{}", i16::from_be_bytes([bytes[0], bytes[1]])), 2))
		}
		ScalarType::Int => {
			let arr: [u8; 4] = bytes[..4]
				.try_into()
				.map_err(|_| "ct_java: truncated int element")?;
			Ok((format!("{}", i32::from_be_bytes(arr)), 4))
		}
		ScalarType::Long => {
			let arr: [u8; 8] = bytes[..8]
				.try_into()
				.map_err(|_| "ct_java: truncated long element")?;
			Ok((format!("{}", i64::from_be_bytes(arr)), 8))
		}
		ScalarType::Float => {
			let arr: [u8; 4] = bytes[..4]
				.try_into()
				.map_err(|_| "ct_java: truncated float element")?;
			let bits = u32::from_be_bytes(arr);
			Ok((format!("f32::from_bits(0x{bits:08x}_u32)"), 4))
		}
		ScalarType::Double => {
			let arr: [u8; 8] = bytes[..8]
				.try_into()
				.map_err(|_| "ct_java: truncated double element")?;
			let bits = u64::from_be_bytes(arr);
			Ok((format!("f64::from_bits(0x{bits:016x}_u64)"), 8))
		}
		ScalarType::Boolean => {
			if bytes.is_empty() {
				return Err("ct_java: truncated boolean element".to_string());
			}
			Ok((
				if bytes[0] != 0 {
					"true".to_string()
				} else {
					"false".to_string()
				},
				1,
			))
		}
		ScalarType::Char => {
			if bytes.len() < 2 {
				return Err("ct_java: truncated char element".to_string());
			}
			let code_unit = u16::from_be_bytes([bytes[0], bytes[1]]);
			let c = char::from_u32(u32::from(code_unit))
				.ok_or("ct_java: Java char is not a valid Unicode scalar value")?;
			Ok((format!("{c:?}"), 2))
		}
		ScalarType::Str => {
			if bytes.len() < 4 {
				return Err("ct_java: truncated String length prefix".to_string());
			}
			#[allow(clippy::cast_sign_loss)]
			let len = i32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize;
			if bytes.len() < 4 + len {
				return Err(format!(
					"ct_java: truncated String element (expected {len} bytes)"
				));
			}
			let s = String::from_utf8(bytes[4..4 + len].to_vec())
				.map_err(|_| "ct_java: String element is not valid UTF-8".to_string())?;
			Ok((format!("{s:?}"), 4 + len))
		}
	}
}

// array_serialize_loop — Java loop body for array/List serialisation

/// Returns the Java `for` loop that serialises the elements of `_arr` using
/// `_dos`.  `iter_type` is the element type used in the `for` declaration
/// (primitive for T[], boxed for List<T>).
fn array_serialize_loop(s: ScalarType, iter_type: &str) -> String {
	if s == ScalarType::Str {
		"for (String _e : _arr) {\n\
			 \t\t\tbyte[] _b = _e.getBytes(java.nio.charset.StandardCharsets.UTF_8);\n\
			 \t\t\t_dos.writeInt(_b.length);\n\
			 \t\t\t_dos.write(_b, 0, _b.length);\n\
			 \t\t}"
			.to_string()
	} else {
		let method = s.dos_write_method().unwrap();
		format!("for ({iter_type} _e : _arr) {{ _dos.{method}(_e); }}")
	}
}

// parse_java_source — merged import split + return-type parse

/// Output of the unified Java source parser.
struct ParsedJava {
	/// The import/package section verbatim from the original source.
	imports: String,
	/// Any class/interface/enum declarations written before `run()`.
	/// Emitted as top-level Java types, outside the generated wrapper class.
	outer: String,
	/// The `run()` method and everything after it, verbatim from the original source.
	/// Placed inside the generated wrapper class.
	body: String,
	/// Parameters declared in `run(...)`, in order.
	params: Vec<(ParamType, String)>,
	/// Return type of the `static T run(...)` method.
	java_type: JavaType,
}

/// Scan `tts` for the first `[visibility] static <T> run` pattern and return the
/// corresponding `JavaType` together with the index of the first token of the
/// method declaration within `tts` (the visibility modifier if present, otherwise
/// `static`), and the index of the `run` identifier token.
///
/// Returns `(java_type, method_start_idx, run_idx)`.
///
/// The visibility modifier (`public`, `private`, `protected`) is optional; plain
/// `static <T> run()` is accepted in addition to `public static <T> run()`.
fn parse_run_return_type(tts: &[TokenTree]) -> Result<(JavaType, usize, usize), String> {
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

		// Pattern 1: [vis] static T run  (scalar)
		if let Some(TokenTree::Ident(type_id)) = tts.get(i + 1) {
			let type_name = type_id.to_string();

			if matches!(tts.get(i + 2), Some(TokenTree::Ident(id)) if id == "run") {
				let run_idx = i + 2;
				return ScalarType::from_primitive_name(&type_name)
					.map(|s| (JavaType::Scalar(s), start, run_idx))
					.ok_or_else(|| {
						format!(
							"inline_java: `run()` return type `{type_name}` is not supported; \
							 scalar types: byte short int long float double boolean char String; \
							 array types: T[] or List<T> for any of those T"
						)
					});
			}

			// Pattern 2: [vis] static T[] run  (array)
			let is_empty_bracket = matches!(
				tts.get(i + 2),
				Some(TokenTree::Group(g))
					if g.delimiter() == proc_macro2::Delimiter::Bracket
					   && g.stream().is_empty()
			);
			if is_empty_bracket
				&& matches!(tts.get(i + 3), Some(TokenTree::Ident(id)) if id == "run")
			{
				let run_idx = i + 3;
				return ScalarType::from_primitive_name(&type_name)
					.map(|s| (JavaType::Array(s), start, run_idx))
					.ok_or_else(|| {
						format!(
							"inline_java: `run()` array element type `{type_name}` is not supported; \
								 supported types: byte short int long float double boolean char String"
						)
					});
			}
		}

		// Pattern 3: [vis] static List < BoxedT > run  (List<T>)
		if matches!(tts.get(i + 1), Some(TokenTree::Ident(id)) if id == "List")
			&& matches!(tts.get(i + 2), Some(TokenTree::Punct(p)) if p.as_char() == '<')
			&& let Some(TokenTree::Ident(inner_id)) = tts.get(i + 3)
		{
			let inner_name = inner_id.to_string();
			if matches!(tts.get(i + 4), Some(TokenTree::Punct(p)) if p.as_char() == '>')
				&& matches!(tts.get(i + 5), Some(TokenTree::Ident(id)) if id == "run")
			{
				let run_idx = i + 5;
				return ScalarType::from_boxed_name(&inner_name)
					.map(|s| (JavaType::List(s), start, run_idx))
					.ok_or_else(|| {
						format!(
							"inline_java: `run()` List element type `{inner_name}` is not supported; \
										 supported types: Byte Short Integer Long Float Double Boolean Character String"
						)
					});
			}
		}

		// Pattern 4: [vis] static java.util.List < BoxedT > run
		if matches!(tts.get(i + 1), Some(TokenTree::Ident(id)) if id == "java")
			&& matches!(tts.get(i + 2), Some(TokenTree::Punct(p)) if p.as_char() == '.')
			&& matches!(tts.get(i + 3), Some(TokenTree::Ident(id)) if id == "util")
			&& matches!(tts.get(i + 4), Some(TokenTree::Punct(p)) if p.as_char() == '.')
			&& matches!(tts.get(i + 5), Some(TokenTree::Ident(id)) if id == "List")
			&& matches!(tts.get(i + 6), Some(TokenTree::Punct(p)) if p.as_char() == '<')
			&& let Some(TokenTree::Ident(inner_id)) = tts.get(i + 7)
			&& matches!(tts.get(i + 8), Some(TokenTree::Punct(p)) if p.as_char() == '>')
			&& matches!(tts.get(i + 9), Some(TokenTree::Ident(id)) if id == "run")
		{
			let inner_name = inner_id.to_string();
			let run_idx = i + 9;
			return ScalarType::from_boxed_name(&inner_name)
				.map(|s| (JavaType::List(s), start, run_idx))
				.ok_or_else(|| {
					format!(
						"inline_java: `run()` List element type `{inner_name}` is not supported; \
						 supported types: Byte Short Integer Long Float Double Boolean Character String"
					)
				});
		}

		// Pattern 5: [vis] static Optional < BoxedT > run
		if matches!(tts.get(i + 1), Some(TokenTree::Ident(id)) if id == "Optional")
			&& matches!(tts.get(i + 2), Some(TokenTree::Punct(p)) if p.as_char() == '<')
			&& let Some(TokenTree::Ident(inner_id)) = tts.get(i + 3)
			&& matches!(tts.get(i + 4), Some(TokenTree::Punct(p)) if p.as_char() == '>')
			&& matches!(tts.get(i + 5), Some(TokenTree::Ident(id)) if id == "run")
		{
			let inner_name = inner_id.to_string();
			let run_idx = i + 5;
			return ScalarType::from_boxed_name(&inner_name)
				.map(|s| (JavaType::Optional(s), start, run_idx))
				.ok_or_else(|| {
					format!(
						"inline_java: `run()` Optional element type `{inner_name}` is not supported; \
						 supported types: Byte Short Integer Long Float Double Boolean Character String"
					)
				});
		}

		// Pattern 6: [vis] static java.util.Optional < BoxedT > run
		if matches!(tts.get(i + 1), Some(TokenTree::Ident(id)) if id == "java")
			&& matches!(tts.get(i + 2), Some(TokenTree::Punct(p)) if p.as_char() == '.')
			&& matches!(tts.get(i + 3), Some(TokenTree::Ident(id)) if id == "util")
			&& matches!(tts.get(i + 4), Some(TokenTree::Punct(p)) if p.as_char() == '.')
			&& matches!(tts.get(i + 5), Some(TokenTree::Ident(id)) if id == "Optional")
			&& matches!(tts.get(i + 6), Some(TokenTree::Punct(p)) if p.as_char() == '<')
			&& let Some(TokenTree::Ident(inner_id)) = tts.get(i + 7)
			&& matches!(tts.get(i + 8), Some(TokenTree::Punct(p)) if p.as_char() == '>')
			&& matches!(tts.get(i + 9), Some(TokenTree::Ident(id)) if id == "run")
		{
			let inner_name = inner_id.to_string();
			let run_idx = i + 9;
			return ScalarType::from_boxed_name(&inner_name)
				.map(|s| (JavaType::Optional(s), start, run_idx))
				.ok_or_else(|| {
					format!(
						"inline_java: `run()` Optional element type `{inner_name}` is not supported; \
						 supported types: Byte Short Integer Long Float Double Boolean Character String"
					)
				});
		}
	}
	Err("inline_java: could not find `static <type> run()` in Java body".to_string())
}

/// Parse the parameter list from the `Group(Parenthesis)` token immediately
/// after the `run` identifier.  Returns `Vec<(ParamType, param_name)>`.
///
/// Empty group → `Ok(vec![])`.
/// Unknown/unsupported type → `Err(...)` with a helpful message.
fn parse_run_params(tts: &[TokenTree]) -> Result<Vec<(ParamType, String)>, String> {
	// tts[0] must be the parenthesis group immediately after `run`.
	let group = match tts.first() {
		Some(TokenTree::Group(g)) if g.delimiter() == proc_macro2::Delimiter::Parenthesis => g,
		_ => return Ok(vec![]),
	};

	let inner: Vec<TokenTree> = group.stream().into_iter().collect();
	if inner.is_empty() {
		return Ok(vec![]);
	}

	// Split on ',' to get segments, one per parameter.
	// Note: commas inside angle brackets (e.g. Map<K,V>) would need special
	// handling, but we don't support those types so simple splitting is fine.
	let mut params = Vec::new();
	let mut segments: Vec<Vec<TokenTree>> = Vec::new();
	let mut current: Vec<TokenTree> = Vec::new();
	for tt in inner {
		if matches!(&tt, TokenTree::Punct(p) if p.as_char() == ',') {
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

		// Pattern: Optional < BoxedT > name  (5 tokens)
		if seg.len() == 5
			&& matches!(&seg[0], TokenTree::Ident(id) if id == "Optional")
			&& matches!(&seg[1], TokenTree::Punct(p) if p.as_char() == '<')
			&& matches!(&seg[3], TokenTree::Punct(p) if p.as_char() == '>')
		{
			let inner_name = match &seg[2] {
				TokenTree::Ident(id) => id.to_string(),
				_ => return Err("inline_java: expected boxed type inside Optional<>".to_string()),
			};
			let param_name = match &seg[4] {
				TokenTree::Ident(id) => id.to_string(),
				_ => return Err("inline_java: expected parameter name after Optional<T>".to_string()),
			};
			let s = ScalarType::from_boxed_name(&inner_name).ok_or_else(|| {
				format!(
					"inline_java: unsupported Optional element type `{inner_name}`; \
					 supported types: Byte Short Integer Long Float Double Boolean Character String"
				)
			})?;
			params.push((ParamType::Optional(s), param_name));
			continue;
		}

		// Pattern: java . util . Optional < BoxedT > name  (9 tokens)
		if seg.len() == 9
			&& matches!(&seg[0], TokenTree::Ident(id) if id == "java")
			&& matches!(&seg[1], TokenTree::Punct(p) if p.as_char() == '.')
			&& matches!(&seg[2], TokenTree::Ident(id) if id == "util")
			&& matches!(&seg[3], TokenTree::Punct(p) if p.as_char() == '.')
			&& matches!(&seg[4], TokenTree::Ident(id) if id == "Optional")
			&& matches!(&seg[5], TokenTree::Punct(p) if p.as_char() == '<')
			&& matches!(&seg[7], TokenTree::Punct(p) if p.as_char() == '>')
		{
			let inner_name = match &seg[6] {
				TokenTree::Ident(id) => id.to_string(),
				_ => return Err("inline_java: expected boxed type inside java.util.Optional<>".to_string()),
			};
			let param_name = match &seg[8] {
				TokenTree::Ident(id) => id.to_string(),
				_ => return Err("inline_java: expected parameter name after java.util.Optional<T>".to_string()),
			};
			let s = ScalarType::from_boxed_name(&inner_name).ok_or_else(|| {
				format!(
					"inline_java: unsupported Optional element type `{inner_name}`; \
					 supported types: Byte Short Integer Long Float Double Boolean Character String"
				)
			})?;
			params.push((ParamType::Optional(s), param_name));
			continue;
		}

		// Fallback: primitive/String scalar — first Ident = type, last Ident = name.
		let type_name = match seg.first() {
			Some(TokenTree::Ident(id)) => id.to_string(),
			_ => {
				return Err(
					"inline_java: unexpected token in run() parameter list: expected a type name"
						.to_string(),
				);
			}
		};
		let param_name = match seg.last() {
			Some(TokenTree::Ident(id)) => id.to_string(),
			_ => {
				return Err(
					"inline_java: unexpected token in run() parameter list: expected a parameter name"
						.to_string(),
				);
			}
		};
		let scalar_type = ScalarType::from_primitive_name(&type_name).ok_or_else(|| {
			format!(
				"inline_java: unsupported run() parameter type `{type_name}`; \
				 supported types: byte short int long float double boolean char String"
			)
		})?;
		params.push((ParamType::Scalar(scalar_type), param_name));
	}

	Ok(params)
}

/// Unified parser: walks the token stream once to separate `import`/`package`
/// directives from the method body, identify the `run()` return type and
/// parameters.
///
/// Rather than reconstructing strings from the token tree (which loses
/// whitespace), it uses `Span::join` + `Span::source_text` to slice the
/// original source text directly.
fn parse_java_source(stream: proc_macro2::TokenStream) -> Result<ParsedJava, String> {
	let tts: Vec<TokenTree> = stream.into_iter().collect();

	// Separate imports from body
	let mut first_import_idx: Option<usize> = None;
	let mut last_import_end_idx: Option<usize> = None; // index of the last ';' in imports
	let mut first_body_idx: Option<usize> = None;
	let mut in_imports = true;
	let mut i = 0usize;

	while i < tts.len() && in_imports {
		match &tts[i] {
			TokenTree::Ident(id) if id == "import" || id == "package" => {
				first_import_idx.get_or_insert(i);
				// Scan forward for the terminating ';'.
				let semi = tts[i + 1..]
					.iter()
					.position(|t| matches!(t, TokenTree::Punct(p) if p.as_char() == ';'))
					.map(|rel| i + 1 + rel);
				if let Some(semi_idx) = semi {
					last_import_end_idx = Some(semi_idx);
					i = semi_idx + 1;
				} else {
					// Malformed: no semicolon; treat remainder as body.
					in_imports = false;
					first_body_idx = Some(i);
				}
			}
			_ => {
				in_imports = false;
				first_body_idx = Some(i);
			}
		}
	}
	// If the loop ended because all tokens were imports, body starts at i.
	if first_body_idx.is_none() && i < tts.len() {
		first_body_idx = Some(i);
	}
	let body_start = first_body_idx.unwrap_or(tts.len());

	// Parse return type and run index from body tokens.
	let (java_type, run_rel_idx, run_rel_run_idx) =
		parse_run_return_type(&tts[body_start..])?;
	let run_abs_idx = body_start + run_rel_idx;
	let run_token_abs_idx = body_start + run_rel_run_idx;

	// Parse run() parameters from the token immediately after `run`.
	let params = parse_run_params(&tts[run_token_abs_idx + 1..])?;

	// Extract text via source_text()

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

	// imports: span from first import keyword to last ';'
	let imports = match (first_import_idx, last_import_end_idx) {
		(Some(fi), Some(le)) => slice_text(fi, le + 1),
		_ => String::new(),
	};

	// outer: any tokens between the end of imports and the `run` method declaration
	let outer = slice_text(body_start, run_abs_idx);

	// body: from the `run` method declaration to end (verbatim, no substitution).
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

	Ok(ParsedJava {
		imports,
		outer,
		body,
		params,
		java_type,
	})
}

// Shared code-generation helper used by both java! and java_fn!

/// Generate a `fn __java_runner(...) -> Result<T, JavaError>` token stream
/// used by both `java!` and `java_fn!`.  The caller decides whether to emit
/// `__java_runner()` (immediate call, `java!`) or `__java_runner` (return
/// the function, `java_fn!`).
fn make_runner_fn(
	parsed: ParsedJava,
	opts: JavaOpts,
	prefix: &str,
) -> proc_macro2::TokenStream {
	let ParsedJava {
		imports,
		outer,
		body,
		params,
		java_type,
	} = parsed;

	let class_name = make_class_name(prefix, &imports, &outer, &body, &opts);
	let filename = format!("{class_name}.java");
	let full_class_name = qualify_class_name(&class_name, &imports);

	let main_method = java_type.java_main(&params);
	let java_class = format_java_class(&imports, &outer, &class_name, &body, &main_method);

	let javac_raw = opts.javac_args.unwrap_or_default();
	let java_raw = opts.java_args.unwrap_or_default();
	let deser = java_type.rust_deser();
	let ret_ty = java_type.rust_return_type_ts();

	// Build Rust parameter list for the generated function signature.
	// String params use `&str`.
	let fn_params: Vec<proc_macro2::TokenStream> = params
		.iter()
		.map(|(ty, name)| {
			let ident = proc_macro2::Ident::new(name, proc_macro2::Span::call_site());
			let param_ty = ty.rust_param_type_ts();
			quote! { #ident: #param_ty }
		})
		.collect();

	// Build serialization statements for each parameter.
	let ser_stmts: Vec<proc_macro2::TokenStream> = params
		.iter()
		.map(|(ty, name)| {
			let ident = proc_macro2::Ident::new(name, proc_macro2::Span::call_site());
			ty.rust_ser_ts(&ident)
		})
		.collect();

	quote! {
		fn __java_runner(#(#fn_params),*) -> ::std::result::Result<#ret_ty, ::inline_java::JavaError> {
			let mut _stdin_bytes: ::std::vec::Vec<u8> = ::std::vec::Vec::new();
			#(#ser_stmts)*
			let _raw = ::inline_java::run_java(
				#class_name,
				#filename,
				#java_class,
				#full_class_name,
				#javac_raw,
				#java_raw,
				&_stdin_bytes,
			)?;
			::std::result::Result::Ok(#deser)
		}
	}
}

/// Compile and run zero-argument Java code at *program runtime*.
///
/// Wraps the provided Java body in a generated class, compiles it with `javac`,
/// and executes it with `java`.  The return value of `static T run()` is
/// binary-serialised by the generated `main()` and deserialised to the inferred
/// Rust type.
///
/// Expands to `Result<T, inline_java::JavaError>`, so callers can propagate
/// errors with `?` or surface them with `.unwrap()`.
///
/// For `run()` methods that take parameters, use [`java_fn!`] instead.
///
/// # Options
///
/// Optional `key = "value"` pairs may appear before the Java body, separated by
/// commas:
///
/// - `javac = "<args>"` — extra arguments for `javac` (shell-quoted).
/// - `java  = "<args>"` — extra arguments for `java` (shell-quoted).
///
/// `$INLINE_JAVA_CP` in either option expands to the class-output directory.
///
/// # Examples
///
/// ```text
/// use inline_java::java;
///
/// // Scalar return value
/// let x: i32 = java! {
///     static int run() {
///         return 42;
///     }
/// }.unwrap();
///
/// // Array return
/// let primes: Vec<i32> = java! {
///     static int[] run() {
///         return new int[]{2, 3, 5, 7, 11};
///     }
/// }.unwrap();
///
/// // Extra javac flags
/// let greeting: String = java! {
///     javac = "-sourcepath .",
///     import com.example.demo.*;
///     static String run() {
///         return new HelloWorld().greet();
///     }
/// }.unwrap();
///
/// // Visibility modifiers are accepted — `public`, `private`, `protected` all work
/// let v: i32 = java! {
///     public static int run() { return 99; }
/// }.unwrap();
/// ```
#[proc_macro]
#[allow(clippy::similar_names)]
pub fn java(input: TokenStream) -> TokenStream {
	let input2 = proc_macro2::TokenStream::from(input);

	// Consume any leading `key = "value",` option pairs.
	let (opts, input2) = extract_opts(input2);

	let parsed = match parse_java_source(input2) {
		Ok(p) => p,
		Err(msg) => return quote! { compile_error!(#msg) }.into(),
	};

	let runner_fn = make_runner_fn(parsed, opts, "InlineJava");

	let generated = quote! {
		{
			#runner_fn
			__java_runner()
		}
	};

	generated.into()
}

/// Return a typed Rust function that compiles and runs Java at *program runtime*.
///
/// Like [`java!`], but supports parameters.  The parameters declared in the
/// Java `run(P1 p1, P2 p2, ...)` method become the Rust function's parameters.
/// Arguments are serialised by Rust and piped to the Java process via stdin;
/// Java reads them with `DataInputStream`.
///
/// Expands to a function value of type `fn(P1, P2, ...) -> Result<T, JavaError>`.
/// Call it immediately or store it in a variable.
///
/// # Supported parameter types
///
/// | Java type              | Rust type           |
/// |------------------------|---------------------|
/// | `byte`                 | `i8`                |
/// | `short`                | `i16`               |
/// | `int`                  | `i32`               |
/// | `long`                 | `i64`               |
/// | `float`                | `f32`               |
/// | `double`               | `f64`               |
/// | `boolean`              | `bool`              |
/// | `char`                 | `char`              |
/// | `String`               | `&str`              |
/// | `Optional<BoxedT>`     | `Option<T>`         |
/// | `Optional<String>`     | `Option<&str>`      |
///
/// # Options
///
/// Same `javac = "..."` / `java = "..."` key-value pairs as [`java!`].
///
/// # Examples
///
/// ```text
/// use inline_java::java_fn;
///
/// // Single int parameter
/// let double_it = java_fn! {
///     static int run(int n) {
///         return n * 2;
///     }
/// };
/// let result: i32 = double_it(21).unwrap();
/// assert_eq!(result, 42);
///
/// // Multiple parameters including String
/// let greet = java_fn! {
///     static String run(String greeting, String target) {
///         return greeting + ", " + target + "!";
///     }
/// };
/// let msg: String = greet("Hello", "World").unwrap();
/// assert_eq!(msg, "Hello, World!");
/// ```
#[proc_macro]
#[allow(clippy::similar_names)]
pub fn java_fn(input: TokenStream) -> TokenStream {
	let input2 = proc_macro2::TokenStream::from(input);

	// Consume any leading `key = "value",` option pairs.
	let (opts, input2) = extract_opts(input2);

	let parsed = match parse_java_source(input2) {
		Ok(p) => p,
		Err(msg) => return quote! { compile_error!(#msg) }.into(),
	};

	let runner_fn = make_runner_fn(parsed, opts, "InlineJava");

	let generated = quote! {
		{
			#runner_fn
			__java_runner
		}
	};

	generated.into()
}

// ct_java! — compile-time Java evaluation

/// Run Java at *compile time* and splice its return value as a Rust literal.
///
/// Accepts optional `javac = "..."` and `java = "..."` key-value pairs before
/// the Java body.  The user provides a `static <T> run()` method; its
/// binary-serialised return value is decoded and emitted as a Rust literal at
/// the call site (`42`, `3.14`, `true`, `'x'`, `"hello"`, `[1, 2, 3]`, …).
///
/// Java compilation/runtime errors become Rust `compile_error!` diagnostics.
///
/// # Examples
///
/// ```text
/// use inline_java::ct_java;
///
/// // Numeric constant computed at compile time
/// const PI_APPROX: f64 = ct_java! {
///     static double run() {
///         return Math.PI;
///     }
/// };
///
/// // String constant
/// const GREETING: &str = ct_java! {
///     static String run() {
///         return "Hello, World!";
///     }
/// };
///
/// // Array constant
/// const PRIMES: [i32; 5] = ct_java! {
///     static int[] run() {
///         return new int[]{2, 3, 5, 7, 11};
///     }
/// };
/// ```
#[proc_macro]
pub fn ct_java(input: TokenStream) -> TokenStream {
	match ct_java_impl(proc_macro2::TokenStream::from(input)) {
		Ok(ts) => ts.into(),
		Err(msg) => quote! { compile_error!(#msg) }.into(),
	}
}

fn ct_java_impl(input: proc_macro2::TokenStream) -> Result<proc_macro2::TokenStream, String> {
	let (opts, input) = extract_opts(input);

	let ParsedJava {
		imports,
		outer,
		body,
		java_type,
		..
	} = parse_java_source(input)?;

	let class_name = make_class_name("CtJava", &imports, &outer, &body, &opts);
	let filename = format!("{class_name}.java");
	let full_class_name = qualify_class_name(&class_name, &imports);

	let main_method = java_type.java_main(&[]);
	let java_class = format_java_class(&imports, &outer, &class_name, &body, &main_method);

	let bytes = compile_run_java_now(
		&class_name,
		&filename,
		&java_class,
		&full_class_name,
		opts.javac_args.as_deref(),
		opts.java_args.as_deref(),
	)?;
	java_type.ct_java_tokens(bytes)
}

// Option extraction: `javac = "…"` / `java = "…"` before the Java body

struct JavaOpts {
	/// Extra args for `javac`, shell-split at use-site.  `None` → no extra args.
	javac_args: Option<String>,
	/// Extra args for `java`, shell-split at use-site.  `None` → no extra args.
	java_args: Option<String>,
}


/// Consume leading `javac = "…"` / `java = "…"` option pairs (comma-separated,
/// trailing comma optional) and return the remaining token stream as the Java
/// body.  Unrecognised leading tokens are left untouched.
fn extract_opts(input: proc_macro2::TokenStream) -> (JavaOpts, proc_macro2::TokenStream) {
	let mut tts: Vec<TokenTree> = input.into_iter().collect();
	let mut opts = JavaOpts {
		javac_args: None,
		java_args: None,
	};
	let mut cursor = 0;

	loop {
		match try_parse_opt(&tts[cursor..]) {
			None => break,
			Some((key, val, consumed)) => {
				match key.as_str() {
					"javac" => opts.javac_args = Some(val),
					"java" => opts.java_args = Some(val),
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

/// Try to parse `Ident("javac"|"java") Punct("=") Literal(string)` at the
/// start of `tts`.  Returns `(key, unquoted_value, tokens_consumed)` or
/// `None` if the pattern doesn't match.
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

// Shared helpers used by java!, java_fn!, and ct_java!

/// Compute a deterministic class name by hashing the source and options.
/// `prefix` distinguishes runtime ("`InlineJava`") from compile-time ("`CtJava`").
fn make_class_name(prefix: &str, imports: &str, outer: &str, body: &str, opts: &JavaOpts) -> String {
	let mut h = DefaultHasher::new();
	imports.hash(&mut h);
	outer.hash(&mut h);
	body.hash(&mut h);
	opts.javac_args.hash(&mut h);
	opts.java_args.hash(&mut h);
	format!("{prefix}_{:016x}", h.finish())
}

/// Qualify `class_name` with its package if `imports` contains a `package`
/// declaration (e.g. `"com.example.InlineJava_xxx"`).
fn qualify_class_name(class_name: &str, imports: &str) -> String {
	match parse_package_name(imports) {
		Some(pkg) => format!("{pkg}.{class_name}"),
		None => class_name.to_owned(),
	}
}

/// Compile (if needed) and run a Java class at *compile time*, returning raw
/// stdout bytes.  Delegates to `inline_java_core::run_java` and maps
/// `JavaError` to `String` for use as a `compile_error!` diagnostic.
#[allow(clippy::similar_names)]
fn compile_run_java_now(
	class_name: &str,
	filename: &str,
	java_class: &str,
	full_class_name: &str,
	javac_raw: Option<&str>,
	java_raw: Option<&str>,
) -> Result<Vec<u8>, String> {
	inline_java_core::run_java(
		class_name,
		filename,
		java_class,
		full_class_name,
		javac_raw.unwrap_or(""),
		java_raw.unwrap_or(""),
		&[],
	)
	.map_err(|e| e.to_string())
}

/// Render the complete `.java` source file.
fn format_java_class(
	imports: &str,
	outer: &str,
	class_name: &str,
	body: &str,
	main_method: &str,
) -> String {
	format!(
		"{imports}\n{outer}\npublic class {class_name} {{\n\n{body}\n\n{main_method}\n}}\n"
	)
}

// Package name extraction

/// Extract the package name from the string representation of the imports
/// token stream.  `proc_macro2` serialises `package com.example.demo;` as a
/// compact string (dots and semicolon not separated by spaces), so we use
/// substring search rather than splitting on whitespace.
fn parse_package_name(imports: &str) -> Option<String> {
	let marker = "package ";
	let i = imports.find(marker)?;
	if i > 0 && !imports[..i].ends_with(|c: char| c.is_whitespace()) {
		return None;
	}
	let rest = imports[i + marker.len()..].trim_start();
	let semi = rest.find(';')?;
	let pkg = rest[..semi].trim().replace(|c: char| c.is_whitespace(), "");
	if pkg.is_empty() { None } else { Some(pkg) }
}
