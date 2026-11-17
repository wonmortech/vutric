# ADR-0002: Product name "Vutric"

- Status: accepted
- Date: 2026-11-17

## Context

The planning brief used the working title "Fling". `fling`, `flingo`, `flinq` and most short throw-related words
are taken as GitHub accounts and `.io` domains. Candidates were checked on 2026-11-17 against the GitHub users API
and the `.io` RDAP service (Identity Digital).

## Decision

The product is **Vutric**: a coined word with no prior meaning, short, easy to search, and free of other trademarks
we know of. Repository: `wonmortech/vutric`. Domain candidate: `vutric.io` (free on RDAP at decision time; not
registered yet). Rust crates use the `vutric-` prefix; the QR/deep-link scheme is `vutric://`.

The verb inside the product stays "fling" (`FlingStart`, "fling screen"): it names the gesture, not the brand.

## Consequences

The name does not explain the product, so the tagline and the first animation have to. Spelling must be shown,
not spoken ("v-u-t-r-i-c"); the landing page and store listings lead with the visual.
