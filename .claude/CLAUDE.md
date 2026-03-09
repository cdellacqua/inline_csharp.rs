# inline_java project memory

## Project structure
- `/home/ubuntu/Dev/inline_java/` — workspace root
- `inline_java_core/src/lib.rs` — shared core: `JavaError`, `run_java`, `expand_java_args`
- `inline_java_macros/src/lib.rs` — proc macro implementation (java! and ct_java!)
- `inline_java/src/lib.rs` — thin re-export layer (`pub use inline_java_core::*` + macros)
- `inline_java_demo/src/main.rs` — demo crate
- `inline_java_demo/com/example/demo/` — Java source files (Greetings.java, HelloWorld.java)

## Architecture
- `java!` — runtime macro: generates Rust code that compiles+runs Java at program runtime
- `ct_java!` — compile-time macro: runs Java during Rust compilation (proc-macro expansion)
- Both support `import` and `package` directives to use project Java source files
- Dependency graph: `inline_java_core` ← `inline_java_macros` and `inline_java`; `inline_java` → `inline_java_macros`; no cycles

## Key implementation details
- **Two-phase execution**: `javac [javac_extra] -d $tmpdir $src.java` then `java [java_extra] -cp $tmpdir $ClassName`
- **Optional flags**: `javac = "..."` and `java = "..."` key-value pairs before the Java body, comma-separated; values are split on whitespace into individual args
- **Temp dir**: deterministic per class (`/tmp/<ClassName>/`), named by hash of imports+outer+body+javac_opts+java_opts (opts included so different compilation strategies get separate dirs)
- **Locking**: `fd-lock` file lock on `$tmpdir/.lock` (cross-process + cross-thread); `.done` sentinel marks successful compilation; optimistic pre-check before acquiring lock. Lives in `inline_java_core::run_java`.
- **compile_run_java_now** (in macros): thin wrapper around `inline_java_core::run_java` mapping `JavaError → String` for `compile_error!` diagnostics
- **Package handling**: if user writes `package com.example;`, class runs as `com.example.InlineJava_xxx`
- **parse_package_name**: uses `find("package ")` + `find(';')` string search (NOT split_whitespace — proc_macro2 renders `com.example.demo;` without spaces around dots)
- **extract_opts / try_parse_opt**: parse `Ident("javac"|"java") Punct("=") Literal(string)` triples from the front of the token stream using a separate `&[TokenTree]`-borrowing helper (avoids borrow-then-drain conflict)

## Pitfalls encountered
- `split_whitespace` on imports string fails for `package com.example.demo;` because proc_macro2 serializes the token stream without spaces around dots/semicolons (e.g., `"package com.example.demo;"` not `"package com . example . demo ;"`)
- Old `java Foo.java` (JEP 330 single-file launcher) doesn't support multi-file/package compilation

## Java setup
- Java 21 (OpenJDK 21.0.10) at /usr/bin/java and /usr/bin/javac
