# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - Unreleased

### Added

- `csharp!` macro: compile and run inline C# at program runtime (zero-arg form)
- `csharp_fn!` macro: compile and run inline C# at program runtime, with typed parameters passed over stdin
- `ct_csharp!` macro: run C# during `rustc` macro expansion and splice the result as a Rust literal
- Support for signed integer types: `sbyte`, `short`, `int`, `long`
- Support for unsigned integer types: `byte`, `ushort`, `uint`, `ulong`
- Support for floating-point types: `float`, `double`
- Support for `bool`, `char`, `string`, `T[]`, `List<T>`, and `T?` as parameter and return types, arbitrarily nested
- `build = "..."` and `run = "..."` options for passing extra flags to `dotnet build` and `dotnet <dll>`
- `reference = "..."` option (repeatable) for referencing external DLL assemblies
- `using` and `namespace` directives for referencing project C# source files
- Little-endian binary wire format via `BinaryWriter`/`BinaryReader`
- Cross-process file locking to avoid redundant recompilation across parallel test runners
- `.done` sentinel for skip-recompile optimisation
- `INLINE_CSHARP_CACHE_DIR` environment variable to override the cache location
