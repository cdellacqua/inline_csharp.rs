# inline_csharp project memory

## Project structure
- `/home/ubuntu/Dev/inline_csharp/` — workspace root
- `inline_csharp_core/src/lib.rs` — shared core: `CsharpError`, `run_csharp`, `expand_dotnet_args`, `cache_dir`, `detect_target_framework`, `generate_csproj`
- `inline_csharp_macros/src/lib.rs` — proc macro implementation (`csharp!`, `csharp_fn!`, `ct_csharp!`)
- `inline_csharp/src/lib.rs` — thin re-export layer: re-exports `{CsharpError, expand_dotnet_args, run_csharp}` from core and `{csharp, csharp_fn, ct_csharp}` from macros
- `inline_csharp_demo/src/main.rs` — demo crate
- `inline_csharp_demo/DemoLib/` — external C# library (DemoLib.csproj, HelloWorld.cs, Greetings.cs)

## Architecture
- `csharp!` — runtime macro: zero-arg; compiles+runs C# at program runtime; expands to `Result<T, CsharpError>`
- `csharp_fn!` — runtime macro: with parameters; expands to a Rust function value `fn(P1, ...) -> Result<T, CsharpError>`; parameters are serialised via stdin
- `ct_csharp!` — compile-time macro: runs C# during Rust compilation (proc-macro expansion); expands to a Rust literal
- All three support `using` and `namespace` directives
- Dependency graph: `inline_csharp_core` ← `inline_csharp_macros` and `inline_csharp`; `inline_csharp` → `inline_csharp_macros`; no cycles

## Key implementation details
- **Two-phase execution**: `dotnet build <class>.csproj <build_extra> -o <out>/ --nologo` then `dotnet <out>/<class>.dll <run_extra>`
- **Optional flags**: `build = "..."`, `run = "..."`, and `reference = "..."` key-value pairs before the C# body, comma-separated; `reference` is repeatable
- **Auto-detected TFM**: `detect_target_framework()` runs `dotnet --version` and derives the TFM moniker (e.g., `net8.0`)
- **Generated .csproj**: `<OutputType>Exe</OutputType>`, `<Nullable>enable</Nullable>`, `<ImplicitUsings>disable</ImplicitUsings>`, `<Optimize>true</Optimize>`; each `reference` becomes a `<Reference>` element with `<HintPath>`
- **Naming**: `make_class_name` hashes (usings+outer+body+opts) to produce `InlineCsharp_<hex>` or `CtCsharp_<hex>`
- **Cache dir**: `cache_dir` returns platform cache dir → `INLINE_CSHARP_CACHE_DIR` env var → `<temp>/inline_csharp`; subdir is `<class_name>_<hash>/` where hash covers source+build args+CWD+run args+references+TFM
- **Locking**: `fd-lock` RwLock on `$cachedir/.lock`; `.done` sentinel marks successful compilation; optimistic pre-check before acquiring lock

## Wire format (binary serialization, Rust ↔ C#)
- Rust → C# (stdin): big-endian, via `BinaryReader` in C#
- C# → Rust (stdout): little-endian, via `BinaryWriter` in C#
- Top-level `string` return: raw UTF-8, no length prefix
- `string` inside container: 4-byte LE u32 length + UTF-8 bytes
- Nullable: 1-byte tag (0=null, 1=present) + encoded value
- Array/List: 4-byte LE u32 count + N × encoded element

## Supported types
Scalars: `sbyte`, `byte`, `short`, `ushort`, `int`, `uint`, `long`, `ulong`, `float`, `double`, `bool`, `char`, `string`
Composites: `Array<T>` (→ `T[]` / `Vec<T>`), `List<T>` (→ `List<T>` / `Vec<T>`), `Nullable<T>` (→ `T?` / `Option<T>`)
Nested composites supported.

## Pitfalls encountered
- `split_whitespace` on namespace string fails for `namespace com.example;` because proc_macro2 serializes without spaces around dots/semicolons; use `find("namespace ")` + `find(';')` substring search
- Top-level `string` return must NOT have length prefix (differs from string-in-container)
- Value-type nullables (`int?`) use `.HasValue`/`.Value`; reference-type nullables (`string?`) use `!= null` check — both must be handled separately in codegen
- Compilation errors come from `dotnet build` stdout (not stderr)

## .NET setup
- .NET 8 SDK (dotnet 8.0.125) at `/usr/bin/dotnet`
