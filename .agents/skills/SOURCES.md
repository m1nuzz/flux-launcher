# Vendored agent skills

Third-party Agent Skills vendored into this repo. Do not edit vendored files; update by re-copying from upstream at a new commit.

| Skill | Upstream | Vendored at | License |
|---|---|---|---|
| `rust-skills` | https://github.com/leonardomso/rust-skills | `fd2a861` | MIT (`rust-skills/LICENSE`) |
| `powershell-expert` | https://github.com/jorgeasaurus/agent-skills | `fde4237` | MIT (declared in upstream README; no LICENSE file upstream) |
| `conventional-commit` | https://github.com/rlespinasse/agent-skills | `22ec9d1` | MIT (declared upstream; no LICENSE file vendored) |
| `pin-github-actions` | https://github.com/rlespinasse/agent-skills | `22ec9d1` | MIT (declared upstream; no LICENSE file vendored) |
| `verify-pr-logs` | https://github.com/rlespinasse/agent-skills | `22ec9d1` | MIT (declared upstream; no LICENSE file vendored) |

Notes:

- `rust-skills` targets Rust 1.96 (2024 edition); this workspace pins MSRV 1.85 (edition 2021), so apply only compatible rules.
- No maintained agent skill was found for Inno Setup scripting, raw Win32/DWM lifecycle, or windui; those remain project knowledge in AGENTS.md.
- `microsoft/win-dev-skills` covers WinUI 3 with C# and XAML and does not apply to this Rust plus raw Win32 codebase.
