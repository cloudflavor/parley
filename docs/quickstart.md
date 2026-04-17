# Quickstart

## Build

```bash
cargo build --release
```

## Create and start a review

```bash
./target/release/parley review create my-review
./target/release/parley review start my-review
```

## Open the TUI

```bash
./target/release/parley tui --review my-review
```

## Essential keys

- `h/l`: previous/next file
- `j/k`: down/up line
- `m` or `c`: create comment on selected line
- `r`: reply to selected thread
- `a/o`: mark thread addressed/open
- `/query`: set search query
- `n/p`: next/previous search result
- `N/P`: next/previous thread
- `?`: open shortcuts help

## Refresh after edits

Inside TUI:

```text
R
```

This reloads review metadata and current git diff.
