Vendored from https://github.com/b-inary/postflop-solver at commit
`9d1509fe5077d019825f833eed04b16d342dfda1` (2023-10-01).
Copyright and license: see `LICENSE` (AGPL-3.0-or-later).

OpenFelt maintenance changes:

- Removed serialization code and its bincode/zstd dependencies because the
  upstream release-candidate bincode requirement resolves incompatibly today.
- Retained Rayon and disabled custom allocation.
- Made three raw-pointer field borrows in `src/action_tree.rs` explicit for
  current Rust's `dangerous_implicit_autorefs` check. The underlying operations
  and ownership are unchanged.
- Clarified six elided guard lifetimes for current Rust warning checks.
- OpenFelt uses this library only through `src/trainer/solver.rs`; scenario
  validation and compute limits live at that boundary.

Do not update this source without reviewing the upstream diff, license,
compiler compatibility, and the OpenFelt solver tests.
