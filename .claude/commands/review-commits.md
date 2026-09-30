---
description: Relit les changements en cours, vérifie fmt/clippy/tests/typecheck, puis commit et push
---

Review the current changes on the codebase and perform the following instructions:

- Delete debug output (`println!`, `dbg!`, `console.log`) — never `println!` in anything reachable from `--mcp` (stdout is JSON-RPC only)
- Delete unnecessary comments (keep the ones that explain *why*)
- Review the code and suggest refactor where appropriate
- Check the dependency rules from CLAUDE.md (`core` has no infra, `src-tauri` commands hold no business logic, the front only talks to the back through `src/ipc`)
- Suggest a commit message in english following the project's conventions (Conventional Commits, see `/commit`)
- Code has to stay human readable
- No `unwrap`/`expect` outside tests, no `any` in TypeScript

Before committing, everything must be green (see "Commandes" in CLAUDE.md):

- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `pnpm typecheck` and `pnpm lint`

If any step fails, fix it or report it — do not commit on red.

Then you can commit the changes on github, you just use the following instructions:

- `git add` the relevant files (no blind `git add .`: check `git status` first)
- `git commit -m` with the message of the commit you create
- `git push origin <current branch>`

Never add a "Co-Authored-By" line or any mention of Claude in commits.
