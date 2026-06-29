# OpenSpec Conventions (LLMeter)

This repo uses OpenSpec for spec-driven development. Read this before proposing or
implementing changes.

## Layout
- `openspec/project.md` — durable project context.
- `openspec/specs/<capability>/spec.md` — the **current**, deployed truth for a capability.
  Created/updated by *archiving* a completed change. Empty until the first change ships.
- `openspec/changes/<change-id>/` — an in-flight proposal:
  - `proposal.md` — Why / What changes / Impact.
  - `design.md` — technical decisions and trade-offs (optional but expected for non-trivial work).
  - `tasks.md` — ordered, checkbox implementation plan.
  - `specs/<capability>/spec.md` — the **delta**: requirements this change ADDs / MODIFIEs / REMOVEs.

## Workflow
1. Author the change folder (proposal + design + tasks + spec deltas). **Do not write code first.**
2. Get the proposal reviewed/approved.
3. Implement against `tasks.md`, checking boxes as you go.
4. On ship, fold the deltas into `openspec/specs/<capability>/spec.md` and archive the change.

## Spec delta format
Group requirements under `## ADDED Requirements`, `## MODIFIED Requirements`, or
`## REMOVED Requirements`. Each requirement:

```
### Requirement: <name>
The system SHALL <normative statement>.

#### Scenario: <name>
- **WHEN** <condition>
- **THEN** <observable outcome>
```

Every requirement needs at least one scenario. Use SHALL/MUST for normative text.

## Validation
The `openspec` CLI (`npm i -g @openspec/cli` or equivalent) can validate/diff/archive,
but is optional — these files are plain Markdown and are the source of truth regardless.
