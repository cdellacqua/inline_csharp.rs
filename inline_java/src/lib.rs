//! Embed Java directly in Rust — evaluated at program runtime ([`java!`],
//! [`java_fn!`]) or at compile time ([`ct_java!`]).
//!
//! # Runtime usage — zero-arg
//!
//! [`java!`] compiles and runs Java each time the surrounding Rust code
//! executes.  It expands to `Result<T, `[`JavaError`]`>`.
//!
//! ```rust,no_run
//! use inline_java::java;
//!
//! let x: i32 = java! {
//!     static int run() {
//!         return 42;
//!     }
//! }.unwrap();
//! ```
//!
//! # Runtime usage — with parameters
//!
//! [`java_fn!`] returns a typed Rust function.  Parameters declared in
//! `run(P1 p1, P2 p2, ...)` become the function's parameters; they are
//! serialised by Rust and piped to the Java process via stdin.
//!
//! ```rust,no_run
//! use inline_java::java_fn;
//!
//! let double_it = java_fn! {
//!     static int run(int n) {
//!         return n * 2;
//!     }
//! };
//! let result: i32 = double_it(21).unwrap();
//! assert_eq!(result, 42);
//! ```
//!
//! # Compile-time usage
//!
//! [`ct_java!`] runs Java during `rustc` macro expansion and splices the
//! result as a Rust literal at the call site.
//!
//! ```rust,no_run
//! use inline_java::ct_java;
//!
//! const PI: f64 = ct_java! {
//!     static double run() {
//!         return Math.PI;
//!     }
//! };
//! ```
//!
//! # Supported return types
//!
//! `byte`/`short`/`int`/`long`/`float`/`double`/`boolean`/`char`/`String`
//! map to the obvious Rust types.  `T[]` and `List<BoxedT>` both map to
//! `Vec<T>`.  `Optional<BoxedT>` maps to `Option<T>`.
//!
//! # Supported parameter types (`java_fn!`)
//!
//! The scalar types above plus `Optional<BoxedT>` → `Option<T>`
//! (or `Option<&str>` for `Optional<String>`).

/// Re-export the proc macros so users only need to depend on this crate.
pub use inline_java_macros::{ct_java, java, java_fn};

/// Re-export the core error type and runtime helpers.
pub use inline_java_core::{JavaError, expand_java_args, run_java};
