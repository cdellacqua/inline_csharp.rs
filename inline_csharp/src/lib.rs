#![doc = include_str!("../../README.md")]

/// Re-export the proc macros so users only need to depend on this crate.
pub use inline_csharp_macros::{csharp, csharp_fn, ct_csharp};

/// Re-export the core error type and runtime helpers.
pub use inline_csharp_core::{CsharpError, expand_dotnet_args, run_csharp};
