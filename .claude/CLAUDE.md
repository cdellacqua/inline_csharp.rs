# inline_java project memory

## Project structure
- `/home/ubuntu/Dev/inline_java/` — workspace root
- `inline_java_core/src/lib.rs` — shared core: `JavaError`, `run_java`, `expand_java_args`, `cache_dir`
- `inline_java_macros/src/lib.rs` — proc macro implementation (`java!`, `java_fn!`, `ct_java!`)
- `inline_java/src/lib.rs` — thin re-export layer: selectively re-exports `{JavaError, expand_java_args, run_java}` from core and `{java, java_fn, ct_java}` from macros
- `inline_java_demo/src/main.rs` — demo crate
- `inline_java_demo/com/example/demo/` — Java source files (Greetings.java, HelloWorld.java)

## Architecture
- `java!` — runtime macro: zero-arg; compiles+runs Java at program runtime; expands to `Result<T, JavaError>`
- `java_fn!` — runtime macro: with parameters; expands to a Rust function value `fn(P1, ...) -> Result<T, JavaError>`; parameters are serialised via stdin
- `ct_java!` — compile-time macro: runs Java during Rust compilation (proc-macro expansion); expands to a Rust literal
- All three support `import` and `package` directives to use project Java source files
- Dependency graph: `inline_java_core` ← `inline_java_macros` and `inline_java`; `inline_java` → `inline_java_macros`; no cycles

## Key implementation details
- **Two-phase execution**: `javac [javac_extra] -d $tmpdir $src.java` then `java [java_extra] -cp $tmpdir $ClassName`
- **Optional flags**: `javac = "..."` and `java = "..."` key-value pairs before the Java body, comma-separated; values are split on whitespace into individual args
- **Naming**: `make_class_name` hashes (imports+outer+body+javac_opts+java_opts) to produce `InlineJava_<hex>` or `CtJava_<hex>`
- **Temp dir**: `cache_dir` returns `<sys_tmp>/<class_name>_<hex>/` where the second hex is a hash of (java_class+javac_raw+java_raw); two separate hashes ensure different opts and different source each get their own dir
- **Locking**: `fd-lock` file lock on `$tmpdir/.lock` (cross-process + cross-thread); `.done` sentinel marks successful compilation; optimistic pre-check before acquiring lock. Lives in `inline_java_core::run_java`.
- **Parameter passing** (`java_fn!`): Rust serialises each parameter to `_stdin_bytes` (big-endian binary via `DataOutputStream` protocol); Java reads with `DataInputStream`; return value is binary-serialised back to stdout
- **compile_run_java_now** (in macros): thin wrapper around `inline_java_core::run_java` mapping `JavaError → String` for `compile_error!` diagnostics
- **Package handling**: if user writes `package com.example;`, class runs as `com.example.InlineJava_xxx`
- **parse_package_name**: uses `find("package ")` + `find(';')` string search (NOT split_whitespace — proc_macro2 renders `com.example.demo;` without spaces around dots)
- **extract_opts / try_parse_opt**: parse `Ident("javac"|"java") Punct("=") Literal(string)` triples from the front of the token stream using a separate `&[TokenTree]`-borrowing helper (avoids borrow-then-drain conflict)

## Pitfalls encountered
- `split_whitespace` on imports string fails for `package com.example.demo;` because proc_macro2 serializes the token stream without spaces around dots/semicolons (e.g., `"package com.example.demo;"` not `"package com . example . demo ;"`)
- Old `java Foo.java` (JEP 330 single-file launcher) doesn't support multi-file/package compilation

## Java setup
- Java 21 (OpenJDK 21.0.10) at /usr/bin/java and /usr/bin/javac
