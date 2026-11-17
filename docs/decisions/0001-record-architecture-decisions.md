# ADR-0001: Record architecture decisions

- Status: accepted
- Date: 2026-11-17

## Context

Vutric spans a Rust core, a GPU overlay with three very different host backends, and Flutter apps. Many choices
(why iroh, why a Shell extension on GNOME, why the preview is sent before the file) are non-obvious and easy to
undo by accident.

## Decision

Every non-obvious choice, and every deviation from `CLAUDE.md`, gets a short ADR in `docs/decisions/`, numbered
`NNNN-kebab-title.md`, with Status / Context / Decision / Consequences. Superseded ADRs stay in place and link to
their replacement.

## Consequences

Contributors and agents can answer "why is it like this?" from the repo alone. Changing an accepted decision costs
one new ADR, which is the point.
