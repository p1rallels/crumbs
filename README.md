# crumbs

`crumbs` is a tiny CLI for agent context management.

Goal: make context management simple, easy, and portable.

Instead of writing giant handoff markdown files every time context runs low, keep short, structured memories:
- `what` for facts/constraints/changes
- `why` for decisions/rationale

Storage is plain CSV in `.crumbs/`, so it is easy to inspect, copy, and version.

## Quick Start

```bash
# Add memories
cr what "parser switched to csv-only storage"
cr why "keep context data portable and low-overhead"

# List / find
cr ls 20
cr find "csv" --limit 10

# Create and open a handoff checkpoint
cr handoff mark --window 10
cr handoff open
```

## Onboarding Flow

- If a `.crumbs/` store exists: run `cr handoff open`.
- If no store exists yet: start recording memories with `cr what` and `cr why`.
- When you want a checkpoint for the next agent: run `cr handoff mark --window 10`.

## What Crumbs Is Not

- Not a task tracker:
  use tools like `marbles` for todos, status, and dependencies.
- Not long-form documentation:
  avoid large narrative dumps and giant handoff markdown files.
- Not a full knowledge base:
  keep entries short and actionable, focused on immediate work context.
