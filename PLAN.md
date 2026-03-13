# `inline_csharp` Migration Plan

The goal is to replace every Java/JVM concept with its .NET/C# equivalent while
preserving the overall architecture: a `_core` crate, a `_macros` proc-macro crate,
a thin re-export crate, and a demo crate.

---

## 1. Crate & workspace rename

| Old | New |
|---|---|
| `inline_java_core` | `inline_csharp_core` |
| `inline_java_macros` | `inline_csharp_macros` |
| `inline_java` | `inline_csharp` |
| `inline_java_demo` | `inline_csharp_demo` |

Update `Cargo.toml` workspace `members`, all `[package]` sections, and inter-crate
`[dependencies]`. Change `keywords` from `["java", "jvm", ...]` to
`["csharp", "dotnet", "macro", "interop", "ffi"]`. Update `repository`/`homepage`
to the new repo URL.

---

## 2. Public API names

| Old | New |
|---|---|
| `JavaError` | `CsharpError` |
| `run_java(...)` | `run_csharp(...)` |
| `expand_java_args(raw)` | `expand_dotnet_args(raw)` |
| `base_cache_dir()` | `base_cache_dir()` (same, change internals) |
| `cache_dir(...)` | `cache_dir(...)` (same signature) |
| `java!` | `csharp!` |
| `java_fn!` | `csharp_fn!` |
| `ct_java!` | `ct_csharp!` |
| `JavaOpts { javac_args, java_args }` | `DotnetOpts { build_args, run_args, references }` |
| option key `javac = "..."` | `build = "..."` |
| option key `java  = "..."` | `run = "..."` |
| *(no equivalent — classpath was a CLI flag)* | `reference = "path/to.dll"` (repeatable) |

Env var: `INLINE_JAVA_CACHE_DIR` → `INLINE_CSHARP_CACHE_DIR`.
Cache subdirectory: `inline_java` → `inline_csharp`.

---

## 3. Toolchain change (core crate — `run_csharp`)

Java uses a two-phase approach: `javac` then `java`. C# mirrors this with
`dotnet build` then `dotnet <dll>`:

**Phase 1 — compile** (replaces `javac -d $tmpdir $src.java`):
```
dotnet build <tmpdir>/<ClassName>.csproj -c Release -o <tmpdir>/out/ --nologo -v quiet
```
Success marker: write `.done` sentinel as before.

**Phase 2 — run** (replaces `java -cp $tmpdir $FullClassName`):
```
dotnet <tmpdir>/out/<ClassName>.dll [run_args]
```

The `inject_classpath` helper is removed — .NET references are declared in the
`.csproj`, not via a CLI flag. The `build_args` and `run_args` strings are passed
verbatim.

**Path resolution in `build_args`:** `dotnet build` is spawned without an explicit
`.current_dir()`, so it inherits the Rust process's CWD — exactly like `javac` in
the Java implementation. Relative paths inside `build_args` CLI flags are therefore
resolved against the process CWD at invocation time, and the existing `cache_dir`
logic already hashes `std::env::current_dir()` alongside the expanded args to
prevent stale-cache collisions across directories. No extra handling is needed here.

**Path resolution inside the `.csproj` (C#-specific concern):** the generated
`.csproj` lives in the cache directory. When `dotnet build` processes it, relative
paths *inside the csproj XML* are resolved relative to the **csproj file's own
directory** — not the process CWD. For the basic case (the `.cs` source sits next
to the `.csproj`) this is fine. For user-supplied reference paths (the
`reference = "..."` option below), `run_csharp` must absolutize them before writing
via `std::env::current_dir().join(path).canonicalize()`.

**New helper — `generate_csproj(class_name, references: &[PathBuf]) -> String`:**

`references` is the list of already-absolutized DLL paths. The generated csproj
includes a `<Reference>` item for each one, omitting the `<ItemGroup>` block
entirely when the list is empty:

```xml
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <AssemblyName>{class_name}</AssemblyName>
    <Nullable>enable</Nullable>
    <ImplicitUsings>disable</ImplicitUsings>
    <Optimize>true</Optimize>
  </PropertyGroup>
  <!-- only emitted when references is non-empty -->
  <ItemGroup>
    <Reference Include="/absolute/path/to/First.dll" />
    <Reference Include="/absolute/path/to/Second.dll" />
  </ItemGroup>
</Project>
```

**Cache key** — absolutized reference paths are included in the `cache_dir` hash
alongside the source and build/run args. This ensures that adding, removing, or
changing a referenced DLL path invalidates the cache. `cache_dir`'s signature gains
a `references: &[PathBuf]` parameter (already absolutized):

```rust
pub fn cache_dir(
    class_name: &str,
    csharp_source: &str,
    build_raw: &str,
    run_raw: &str,
    references: &[PathBuf],  // absolutized before hashing
) -> std::path::PathBuf
```

`run_csharp` writes both `<ClassName>.cs` and `<ClassName>.csproj` to the cache dir
before invoking `dotnet build`.

The `filename` parameter disappears — unlike Java (where the filename must match
the public class name), C# has no such constraint; the file can be named anything.

Updated `run_csharp` signature:
```rust
pub fn run_csharp(
    class_name: &str,        // e.g. "InlineCsharp_abc123"
    csharp_source: &str,     // complete .cs source
    build_raw: &str,         // raw build = "..." option string
    run_raw: &str,           // raw run = "..." option string
    references: &[&str],     // raw reference = "..." paths (relative ok; absolutized internally)
    stdin_bytes: &[u8],
) -> Result<Vec<u8>, CsharpError>
```

---

## 4. Wire format — endianness

Java's `DataOutputStream` is **big-endian**. C#'s `BinaryWriter` is
**little-endian**. Since both sides are generated by us, adopt **little-endian**
throughout:

- **C# side**: use `BinaryWriter(Console.OpenStandardOutput())` — writes LE by
  default. No change needed.
- **Rust side**: replace all `i32::from_be_bytes` / `u32::from_be_bytes` etc. with
  `from_le_bytes`, and `to_be_bytes` with `to_le_bytes` in the
  serialization/deserialization token generation. Length/count prefixes that were
  read as `i32` (signed, BE) become `u32` (unsigned, LE), eliminating any need for
  sign-checking on counts.

---

## 5. Language construct mapping (macros crate)

### 5a. Directives

| Java (in macro body) | C# (in macro body) |
|---|---|
| `import java.util.List;` | `using System.Collections.Generic;` |
| `import com.example.*;` | `using MyNamespace;` |
| `package com.example.demo;` | `namespace MyNamespace;` |

Parsing: `parse_package_name` → `parse_namespace_name`. Look for `"namespace "`
+ `";"` instead of `"package "` + `";"`. Same substring-search approach (avoids
the `split_whitespace` pitfall with proc_macro2 token serialization).

Parsing `using` directives from the token stream: same logic as `import`
detection — find leading tokens that form `using …;` lines and separate them from
the method body.

### 5b. Type system

Replace `PrimitiveType`, `BoxedType`, `JavaType` with a simpler `CsharpType`.

C# has unsigned integer types that Java lacks, so the table expands:

| C# type in macro | Rust type | Wire (LE bytes) |
|---|---|---|
| `sbyte` | `i8` | 1 |
| `byte` | `u8` | 1 |
| `short` | `i16` | 2 |
| `ushort` | `u16` | 2 |
| `int` | `i32` | 4 |
| `uint` | `u32` | 4 |
| `long` | `i64` | 8 |
| `ulong` | `u64` | 8 |
| `float` | `f32` | 4 |
| `double` | `f64` | 8 |
| `bool` | `bool` | 1 (0 or 1) |
| `char` | `char` | 2 (UTF-16 code unit as `u16`) |
| `string` | `String` (return) / `&str` (param) | `u32` length + UTF-8 bytes |
| `T[]` | `Vec<T>` (return) / `&[T]` (param) | `u32` count + N × encode(T) |
| `List<T>` | `Vec<T>` (return) / `&[T]` (param) | `u32` count + N × encode(T) |
| `T?` | `Option<T>` | 1-byte tag (0=null, 1=present) + encode(T) |

Explicitly **not supported**: `decimal` (no natural Rust equivalent without
external crates), `nint`/`nuint` (platform-dependent size).

The distinct "boxed vs primitive" split from Java (`Integer` vs `int`) disappears —
C# unifies them. The parser can drop `BoxedType` entirely; `CsharpType` is a
simpler recursive enum:

```rust
enum CsharpType {
    // Signed
    Sbyte, Short, Int, Long,
    // Unsigned
    Byte, Ushort, Uint, Ulong,
    // Float
    Float, Double,
    // Other scalars
    Bool, Char,
    Str,                        // string
    // Composites
    Array(Box<CsharpType>),     // T[]
    List(Box<CsharpType>),      // List<T>
    Nullable(Box<CsharpType>),  // T?  →  Option<T>
}
```

**Wire format — use `u32` for all length/count prefixes.** The Java implementation
used signed `i32` (big-endian) for array lengths and string byte-counts. Since
`BinaryWriter` treats counts as `uint` naturally, and Rust reads them as
`u32::from_le_bytes` then casts to `usize`, no signed-to-unsigned conversion is
ever needed. This applies to: array/list element counts, and string byte-length
prefixes when `string` appears inside a container.

Nullable (`T?`) replaces `Optional<T>`. Wire format for nullable is identical to
optional: 1-byte tag (0 = null, 1 = present) + encoded value if present.

### 5c. Generated C# source

`format_java_class` → `format_csharp_source`. Generated file structure:

```csharp
using System;
using System.Collections.Generic;
using System.IO;
// user's using directives

{namespace_decl}        // "namespace Foo;" if user wrote namespace, else omitted

class {ClassName} {
    {outer}             // user's helper methods / fields before Run()

    {body}              // user's static T Run(...) method

    static void Main() {
        {stdin_read}    // only for csharp_fn!: BinaryReader deserialization of params
        {run_call}      // var _result = Run(param1, param2, ...);
        {stdout_write}  // BinaryWriter serialization of _result
    }
}
```

`DataOutputStream` / `DataInputStream` → `BinaryWriter` / `BinaryReader`.

`System.out.write` → `Console.OpenStandardOutput()` wrapped in `BinaryWriter`.
`System.in` → `Console.OpenStandardInput()` wrapped in `BinaryReader`.

### 5d. Class name generation

`make_class_name` changes prefix strings only:
- Runtime: `InlineCsharp_<hex>`
- Compile-time: `CtCsharp_<hex>`

`qualify_class_name` can be removed — `dotnet <dll>` finds `Main` via the assembly
manifest automatically; no package-qualified class name is needed unlike
`java -cp $dir com.example.ClassName`.

### 5e. Macro option parsing

`try_parse_opt` recognises three keys: `build`, `run`, and `reference`.

`build` and `run` behave exactly as `javac`/`java` did — a single occurrence each,
last-one-wins if repeated. `reference` is **repeatable**: every occurrence appends
one path to `DotnetOpts::references`. The updated struct:

```rust
struct DotnetOpts {
    build_args: String,      // from build = "..."
    run_args: String,        // from run = "..."
    references: Vec<String>, // one entry per reference = "..." occurrence
}
```

Example macro usage:
```rust
csharp! {
    build = "--no-restore",
    reference = "../../libs/Foo.dll",
    reference = "../../libs/Bar.dll",
    static int Run() { return Foo.Value + Bar.Value; }
}
```

### 5f. Compile-time macro (`ct_csharp!`)

`compile_run_java_now` → `compile_run_csharp_now`. Same structure: calls
`run_csharp`, maps `CsharpError` to `String` for `compile_error!`.

`ct_java_tokens` / `scalar_enc_ct_lit` — only the LE byte-reading changes
(`from_le_bytes`). The token-generation logic is otherwise identical.

---

## 6. Demo crate rewrite

The demo crate (`inline_csharp_demo`) is rewritten in terms of C# equivalents:

- Replace `build_demo_jar` (javac + jar) with a helper that compiles a `.csproj`
  to a DLL using `dotnet build`.
- Demo C# source files live in `inline_csharp_demo/CSharp/`.
- Demo examples cover: `int` return, `string` return, `csharp_fn!` with parameters,
  `ct_csharp!` compile-time constants, arrays (`int[]`, `string[]`), `List<T>`,
  nullable (`int?`, `string?`).

---

## 7. Caveats & open questions

1. **`char` encoding** — C# `char` is a UTF-16 code unit (2 bytes, unsigned).
   Encode as `u16` LE on the wire via `writer.Write((ushort)value)`; decode in
   Rust as `u16::from_le_bytes` → `char::from_u32`. Same `InvalidChar` error
   variant for lone surrogates. Using `ushort` on the C# side avoids a
   sign-extension cast that would be needed with `short`.
2. **NuGet packages** — deferred to phase 2. Could be added via a
   `package = "Newtonsoft.Json:13.0.3"` option that emits `<PackageReference>`
   in the csproj (requires network access at compile time).

---

## 8. Suggested order of work

1. Rename crates and `Cargo.toml` entries.
2. Port `inline_csharp_core`: rename types/functions, replace `javac`/`java`
   subprocess logic with `dotnet build` / `dotnet exec`, add `generate_csproj`,
   update cache env var.
3. Port `inline_csharp_macros`: replace type enum, update source generator, update
   wire-format byte order, update option keys, rename macros.
4. Update `inline_csharp` re-export crate.
5. Rewrite demo crate with C# snippets.
6. Update README / docs.
