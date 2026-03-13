# verus-metrics

A CLI tool that counts spec / proof / exec lines in [Verus](https://github.com/verus-lang/verus) code.
It tracks which `spec fn` and `proof fn` are reachable from `requires`/`ensures` clauses, `proof {}` blocks, and `assert` expressions via call-graph reachability analysis.

## Installation

```
cargo install --path .
```

## Usage

```
verus-metrics [OPTIONS] <PATH>
```

`<PATH>` can be a single `.rs` file or a directory. When a directory is given, all `.rs` files underneath it are scanned.

### Options

| Option | Description |
|---|---|
| `-v`, `--verbose` | Show per-file line counts |
| `--roots <fn,...>` | Comma-separated list of root functions for reachability analysis |
| `--whole-crate` | Merge call graphs across all files for cross-file reachability analysis |

### Example output

```
                                                    spec  proof   exec comment   blank  total
src/lib.rs                                           120     45    310      80      60    615

spec:     120 lines (21.6%)
  requires/ensures:            32
  spec fn bodies:
    reachable:                 74
    unreferenced:              14

proof:     45 lines (8.1%)
  proof blocks:                12
  proof fn bodies:
    reachable:                 28
    unreferenced:               5

exec:     310 lines (55.8%)

assert calls:                  18
```

## Line classification

| Category | What is counted |
|---|---|
| **spec (requires/ensures)** | `requires` / `ensures` / `decreases` / `opens_invariants` / `no_unwind` lines — always counted as spec regardless of the enclosing function's mode |
| **spec fn (reachable)** | Body lines of `spec fn`s transitively reachable from any root function's `requires`/`ensures`, `assert` expressions, or `proof {}` blocks |
| **spec fn (unreferenced)** | Body lines of `spec fn`s not reachable from the above |
| **proof (proof block)** | Lines inside `proof { }` blocks within exec functions |
| **proof fn (reachable)** | Body lines of `proof fn`s reachable from `proof {}` blocks |
| **proof fn (unreferenced)** | Body lines of `proof fn`s not reachable from any `proof {}` block |
| **exec** | All other lines inside `verus! { }` |
| **comment / blank** | Comment lines and blank lines |

## How reachability works

1. **Spec BFS seeds**: calls found in `requires`/`ensures` of root functions, plus calls inside `assert(...)` / `assert_by(...)` / `assert_forall_by(...)` argument expressions in exec function bodies
2. **Proof BFS seeds**: calls found in `proof {}` blocks of root exec functions
3. BFS follows `body_calls` of `spec fn` / `proof fn` transitively

When `--roots` is not given, every function acts as a root.

### Cross-file analysis (`--whole-crate`)

By default each file is analysed independently.
With `--whole-crate`, the `FnInfo` tables from all files are merged into a single global BFS, so call chains that cross file boundaries are tracked correctly.

```sh
# Cross-file analysis of an entire directory
verus-metrics --whole-crate src/

# Restrict roots and analyse the whole crate
verus-metrics --roots foo,bar --whole-crate src/
```

## License

MIT
