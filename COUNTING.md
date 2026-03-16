# Verus Metrics — Line Classification Spec

## Spec lines

### requires/ensures/invariant/decreases in fn signatures and loop bodies

Lines starting with any of the following keywords are counted as **spec** (`spec_req_ens`):

- `requires` / `ensures` / `opens_invariants` / `no_unwind` — fn signature spec clauses
- `decreases` — fn signature variant clause or loop variant
- `invariant` / `decreases` inside a loop body — loop invariants and variants

> Loop `invariant` / `decreases` lines are classified as spec even outside a fn signature section (`pending.is_none()`).

### spec {} blocks inside exec fn bodies

Lines inside `spec { ... }` override blocks within exec function bodies are counted as **spec** (`spec_block`). This covers ghost code and spec-mode expressions embedded in exec functions.

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

Lines inside the body of an exec fn (between `{` and `}`), excluding any lines classified as spec or proof. Exec fn signature lines (parameter types, return types, where clauses) are **not** counted as exec — they are declarations, not executable code.

### struct definitions

`struct` definitions at the top level inside `verus! { }` are counted as exec.

### proof fn / spec fn

Zero — their bodies are counted as spec or proof respectively.

---

## Not counted (NonVerus)

The following are not counted in any metric:

- Code outside the `verus! { }` macro (use imports, module declarations, etc.)
- The `verus! {` and `}` delimiter lines themselves
- `enum`, `impl`, `type`, `use` at the top level inside `verus! { }`
- Exec fn signature lines (parameter types, return types, where clauses)

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
- `spec` with `{` on the next line (e.g. `spec\n{`) is not supported.
- Exec fn reachability is not tracked; when `--roots` is specified, exec lines and proof blocks are shown unfiltered.
