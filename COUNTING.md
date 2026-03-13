# Verus Metrics — Line Classification Spec

## Spec lines

### requires/ensures/invariant/decreases in exec fn

Lines starting with any of the following keywords are counted as **spec** (`spec_req_ens`):

- `requires` / `ensures` / `opens_invariants` / `no_unwind` — fn signature spec clauses
- `decreases` — fn signature variant clause or loop variant
- `invariant` / `decreases` inside a loop body — loop invariants and variants

> Loop `invariant` / `decreases` lines are classified as spec even outside a fn signature section (`pending.is_none()`).

### spec fn / proof fn bodies

- All lines of a spec fn body → `spec_fn_reachable` or `spec_fn_unreferenced`
- All lines of a proof fn body → `proof_fn_reachable` or `proof_fn_unreferenced`

Reachable vs. unreferenced is determined by call-graph reachability analysis.

---

## Proof lines

### Proof constructs inside exec fn bodies

All of the following are counted as `proof_block`:

| Syntax | Counted range |
|--------|---------------|
| `proof { ... }` | All lines from `{` to `}` |
| `assert(...);` | 1 line |
| `assert(...) by (...);` | 1 line |
| `assert(...) by { ... };` | All lines from `assert` to `}` |
| `assume(...);` | 1 line |
| `admit();` | 1 line |
| `calc! { ... }` | All lines from `{` to `}` |

Multi-line `assert(` conditions (where `)` appears on a later line) are also fully counted as proof.

---

## Exec lines

### exec fn bodies

All lines of an exec fn definition minus any spec or proof lines.

### proof fn / spec fn

Zero — their bodies are counted as spec or proof respectively.

---

## Supported fn modifiers

All of the following modifier patterns are supported. Tokens before `spec fn` / `proof fn` are skipped character-by-character until the mode keyword is reached.

| Pattern | Classification |
|---------|---------------|
| `pub spec fn` | Spec |
| `pub open spec fn` | Spec |
| `pub closed spec fn` | Spec |
| `pub(crate) spec fn` | Spec |
| `broadcast proof fn` | Proof |
| `uninterp spec fn foo();` | Spec (bodyless `;` form handled correctly) |
| `proof fn` inside `impl` block | Proof |
| `fn` with no mode keyword | Exec |

---

## Known limitations

- `calc!` or `proof` with `{` on the next line (e.g. `calc!\n{`) is not supported.
- Code outside the `verus!` macro is always classified as Exec.
