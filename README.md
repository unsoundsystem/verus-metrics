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

`<PATH>` can be a single `.rs` file or a directory.

### Options

| Option | Description |
|---|---|
| `-v`, `--verbose` | Show per-file line counts in addition to the total |
| `--roots <fn,...>` | Comma-separated list of root functions; only lines reachable from these are counted in the output |
| `--whole-crate` | Follow `mod` declarations from `lib.rs`/`main.rs` and merge call graphs across all files |

### Example output (no `--roots`)

```
                                                    spec  proof   exec comment   blank  total
src/lib.rs                                           120     45    310      80      60    615

spec:     120 lines (21.6%)
  requires/ensures:            32
    reachable:                 26
    unreferenced:               6
  spec blocks:                  4
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

### Example output (with `--roots`)

When `--roots` is given the column headers change to `spec*` / `proof*` and every column
(including `total`) reflects only the lines reachable from the specified roots:

```
                                                   spec* proof*   exec comment   blank  total
TOTAL                                               1234   5678    598     763     692   8965
```

- `spec*` = reachable `requires`/`ensures` lines + `spec {}` block lines + reachable `spec fn` body lines
- `proof*` = `proof {}` block lines + reachable `proof fn` body lines
- `exec` and `comment`/`blank` are unchanged (exec reachability is not tracked)
- `total` = `spec*` + `proof*` + exec + comment + blank

## Line classification

| Category | What is counted |
|---|---|
| **spec (requires/ensures)** | `requires` / `ensures` / `decreases` / `opens_invariants` / `no_unwind` lines in function signatures, plus `invariant` / `decreases` lines inside loop bodies — always counted as spec |
| **spec (spec blocks)** | Lines inside `spec { }` override blocks within exec function bodies |
| **spec fn (reachable)** | Body lines of `spec fn`s transitively reachable from root function `requires`/`ensures`, `assert` expressions, or `proof {}` blocks |
| **spec fn (unreferenced)** | Body lines of `spec fn`s not reachable from the above |
| **proof (proof block)** | Lines inside `proof { }` blocks, `calc! { }` blocks, `assert_by { }` bodies, and `broadcast group { }` within exec function bodies |
| **proof fn (reachable)** | Body lines of `proof fn`s reachable from `proof {}` blocks |
| **proof fn (unreferenced)** | Body lines of `proof fn`s not reachable from any `proof {}` block |
| **exec** | Body lines of `exec fn`s (between `{` and `}`) and `struct` definitions inside `verus! { }` |
| **comment / blank** | Comment lines and blank lines |

Lines outside `verus! { }` (use imports, module declarations, etc.) are not counted in any metric. Exec fn signature lines (parameter types, return types, where clauses) are also not counted — they are declarations, not executable code.

## How reachability works

1. **Spec BFS seeds**: calls found in `requires`/`ensures` of root functions, plus calls inside `assert(...)` / `assert_by(...)` / `assert_forall_by(...)` argument expressions in exec function bodies
2. **Proof BFS seeds**: calls found in `proof {}` blocks of root exec functions
3. BFS follows `body_calls` of `spec fn` / `proof fn` transitively

When `--roots` is not given, every function acts as a root.

### Cross-file analysis (`--whole-crate`)

By default each file is analysed independently.

With `--whole-crate` and a directory path, the tool locates the crate root (`src/lib.rs`,
`src/main.rs`, `lib.rs`, or `main.rs`) and collects files by following `mod` declarations —
matching the Rust compiler's view of the crate. Only files reachable via `mod` are included;
commented-out `mod` declarations and inline `mod { }` blocks are skipped.
An error is reported if no crate root is found.

```sh
# Cross-file analysis of an entire crate
verus-metrics --whole-crate src/

# Restrict roots and analyse the whole crate
verus-metrics --roots foo,bar --whole-crate src/
```

## License

MIT
