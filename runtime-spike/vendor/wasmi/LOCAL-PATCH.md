# Local Wasmi patch

This is the `wasmi` 0.40.0 crates.io source (archive SHA-256
`a19af97fcb96045dd1d6b4d23e2b4abdbbe81723dbc5c9f016eb52145b320063`).
The package records upstream revision `f384f288a149625dc7fc29bcd686f8bca3ee71c0`
with a dirty source tree. The upstream MIT license and copyright notice are
included in `LICENSE-MIT`. Upstream tests and benchmarks are not vendored.

The sole source change is in `src/memory/mod.rs`: an unrepresentable growth
request is presented to the resource limiter with a desired size of
`usize::MAX` before returning the normal invalid-growth result. If the limiter
returns an error, the operation traps. Without a limiter, or if the limiter
does not error, ordinary Wasm failure behavior remains. This permits the host's
strict limiter to trap requests that bypassed it in the published release.
Review and remove this patch when replacing or upgrading Wasmi; rerun the
growth and promotion regressions. This patch is not a security sandbox.
