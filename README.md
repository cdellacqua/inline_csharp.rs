# inline_csharp

Embed C# directly in Rust — evaluated at program runtime (`csharp!`, `csharp_fn!`) or at
compile time (`ct_csharp!`).

## Prerequisites

.NET 10 SDK with `dotnet` on `PATH`.

## Quick start

```toml
# Cargo.toml
[dependencies]
inline_csharp = "0.1.0"
```

## `csharp!` — runtime, no parameters

Compiles and runs C# each time the surrounding Rust code executes.  Expands
to `Result<T, inline_csharp::CsharpError>`.

```rust
use inline_csharp::csharp;

// No type annotation needed — the macro infers `i32` from `static int Run()`
let x = csharp! {
    static int Run() {
        return 42;
    }
}.unwrap();
```

## `csharp_fn!` — runtime, with parameters

Like `csharp!`, but `Run(...)` may declare parameters.  Expands to a Rust
function value `fn(P1, P2, …) -> Result<T, CsharpError>`.  Parameters are
serialised by Rust and piped to the C# process over stdin.

```rust
use inline_csharp::csharp_fn;

// Single parameter — return type inferred from `static int Run()`
let doubled = csharp_fn! {
    static int Run(int n) {
        return n * 2;
    }
}(21).unwrap();

// Multiple parameters
let msg: String = csharp_fn! {
    static string Run(string greeting, string target) {
        return greeting + ", " + target + "!";
    }
}("Hello", "World").unwrap();

// Nullable parameter
let result: Option<i32> = csharp_fn! {
    static int? Run(int? val) {
        return val.HasValue ? val * 2 : null;
    }
}(Some(21)).unwrap();
```

## `ct_csharp!` — compile time

Runs C# during `rustc` macro expansion and splices the result as a Rust
literal at the call site.  No parameters are allowed (values must be
compile-time constants).

```rust
use inline_csharp::ct_csharp;

const PI: f64 = ct_csharp! {
    static double Run() {
        return System.Math.PI;
    }
};

// Arrays work too — result is a Rust array literal baked into the binary
const PRIMES: [i32; 5] = ct_csharp! {
    static int[] Run() {
        return new int[] { 2, 3, 5, 7, 11 };
    }
};
```

## Supported parameter types (`csharp_fn!`)

Declare parameters in the C# `Run(...)` signature; Rust receives them with
the mapped types below.

| C# parameter type      | Rust parameter type  |
|------------------------|----------------------|
| `sbyte`                | `i8`                 |
| `byte`                 | `u8`                 |
| `short`                | `i16`                |
| `ushort`               | `u16`                |
| `int`                  | `i32`                |
| `uint`                 | `u32`                |
| `long`                 | `i64`                |
| `ulong`                | `u64`                |
| `float`                | `f32`                |
| `double`               | `f64`                |
| `bool`                 | `bool`               |
| `char`                 | `char`               |
| `string`               | `&str`               |
| `T[]` / `List<T>`      | `&[T]`               |
| `T?`                   | `Option<T>`          |

## Supported return types

| C# return type         | Rust return type  |
|------------------------|-------------------|
| `sbyte`                | `i8`              |
| `byte`                 | `u8`              |
| `short`                | `i16`             |
| `ushort`               | `u16`             |
| `int`                  | `i32`             |
| `uint`                 | `u32`             |
| `long`                 | `i64`             |
| `ulong`                | `u64`             |
| `float`                | `f32`             |
| `double`               | `f64`             |
| `bool`                 | `bool`            |
| `char`                 | `char`            |
| `string`               | `String`          |
| `T[]` / `List<T>`      | `Vec<T>`          |
| `T?`                   | `Option<T>`       |

Types can be nested arbitrarily: `List<string>[]` → `Vec<Vec<String>>`,
`int?[]` → `Vec<Option<i32>>`, etc.

## Options

The following optional `key = "value"` pairs may appear before the C# body, separated by
commas:

- `build = "<args>"` — extra arguments passed to `dotnet build`.
- `run   = "<args>"` — extra arguments passed to `dotnet <dll>` at runtime.
- `reference = "<path>"` — path to a DLL to reference (repeatable).

```rust,ignore
use inline_csharp::csharp;

let result: i32 = csharp! {
    build = "--no-restore",
    reference = "../../libs/Foo.dll",
    static int Run() {
        return Foo.Value;
    }
}.unwrap();
```

## Cache directory

Compiled assemblies are cached so that unchanged C# code is not
recompiled on every run.  The cache root is resolved in this order:

| Priority | Location |
|----------|----------|
| 1 | `INLINE_CSHARP_CACHE_DIR` environment variable (if set and non-empty) |
| 2 | Platform cache directory — `~/.cache/inline_csharp` on Linux, `~/Library/Caches/inline_csharp` on macOS, `%LOCALAPPDATA%\inline_csharp` on Windows |
| 3 | `<system temp>/inline_csharp` (fallback if the platform cache dir is unavailable) |

Each compiled assembly gets its own subdirectory named
`<ClassName>_<hash>/`, where the hash covers the C# source, the
expanded `build` flags, the current working directory, the raw `run`
flags, and the reference DLL paths.  Changing any of those inputs
automatically triggers a fresh compilation.

## Using project C# source files / namespaces

Use `using` or `namespace` directives together with `reference = "..."` or
`build = "--sourcepath <path>"` to call into your own C# code:

```rust,no_run
use inline_csharp::csharp;

// using style
let s: String = csharp! {
    using MyNamespace;
    static string Run() {
        return new MyClass().Greet();
    }
}.unwrap();
```

```rust,no_run
use inline_csharp::csharp;
// namespace style — the generated class becomes part of the named namespace
let s: String = csharp! {
    namespace MyNamespace;
    static string Run() {
        return new MyClass().Greet();
    }
}.unwrap();
```

## Refactoring use case

`inline_csharp` is particularly well-suited for **incremental C# → Rust
migrations**.  The typical workflow is:

1. Keep the original C# logic intact.
2. Write the replacement in Rust.
3. Use `csharp_fn!` to call the original C# with the same inputs and assert
   that both implementations produce identical outputs.

```rust,no_run
use inline_csharp::csharp_fn;

fn my_rust_impl(n: i32) -> i32 {
    // … new Rust code …
    n * 2
}

#[test]
fn parity_with_csharp() {
    let csharp_impl = csharp_fn! {
        static int Run(int n) {
            // original C# logic, verbatim
            return n * 2;
        }
    };

    for n in [0, 1, -1, 42, i32::MAX / 2] {
        let expected = csharp_impl(n).unwrap();
        assert_eq!(my_rust_impl(n), expected, "diverged for n={n}");
    }
}
```

## Crate layout

| Crate                   | Purpose                                                          |
|-------------------------|------------------------------------------------------------------|
| `inline_csharp`         | Public API — re-exports macros and core types                    |
| `inline_csharp_macros`  | Proc-macro implementation (`csharp!`, `csharp_fn!`, `ct_csharp!`) |
| `inline_csharp_core`    | Runtime helpers (`run_csharp`, `CsharpError`)                    |
| `inline_csharp_demo`    | Demo binary                                                      |
