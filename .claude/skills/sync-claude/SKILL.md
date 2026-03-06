---
name: sync-claude
description: Synchronize .claude/CLAUDE.md with the current state of the codebase — update stale facts, add newly implemented features, and remove entries that no longer apply.
---

Read `.claude/CLAUDE.md` and all relevant source files, then update `CLAUDE.md` so it accurately reflects the current project:

1. **Project structure** — verify every listed file path exists and the description matches its current role; add new crates or notable files.
2. **Architecture** — update macro/crate descriptions if behaviour has changed; correct the dependency graph if crates were added or removed.
3. **Key implementation details** — revise any detail that has changed (algorithms, data structures, protocols, option parsing, locking strategy, etc.); add entries for newly implemented mechanisms.
4. **Pitfalls** — add any new gotchas discovered; remove entries that are no longer relevant.
5. **Environment / tooling** — update Java version, binary paths, or toolchain info if it has changed.

Keep entries concise and factual. Do not add speculative or aspirational content — only document what the code currently does.
