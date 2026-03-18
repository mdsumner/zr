# zr Package — Design & Status Summary

## What zr Is

A low-level R package wrapping the zarrs Rust crate (0.23.x) via extendr, providing direct access to Zarr V2/V3 stores, groups, arrays, and chunks. Thin wrapper philosophy — no R6/S4, plain functions with `zr_` prefix, extendr external pointers for opaque Rust objects.

**Core value proposition**: read anything, from anywhere, in R — local V2/V3, remote HTTP, S3, and icechunk virtual stores, through a single read path. Reading across the gamut of local and remote, real and virtual zarr stores in R is currently very fragmented; zr aims to unify it via zarrs.

Write-to-local is a testing and prototyping convenience. Write-to-remote is explicitly out of scope — that's a data engineering concern that Python already owns and doesn't need duplicating in R. Creating virtual stores is already easy and fast in Python; zr's job is reading them.

## Architecture

### The read-only-remote decision

Remote stores (HTTP, S3, icechunk) are **read-only** in zr. This is a deliberate design constraint that eliminates the Rust type system complexity that would otherwise arise from trying to unify readable and writable storage behind a single trait object.

The resulting architecture is clean:

```
ZrStore
├── readable: ReadableStorage              ← universal, all store types
└── writable: Option<ReadableWritableListableStorage>  ← only filesystem

ZrArray
└── inner: Array<dyn ReadableStorageTraits> ← universal, reads work on any store

ZrGroup
└── inner: Group<dyn ReadableStorageTraits> ← universal, reads work on any store
```

- **All reads** go through `ZrArray.inner` / `ZrGroup.inner` — works identically for filesystem, HTTP, S3, icechunk
- **Writes** (`zr_write`, `zr_write_chunk`, `zr_erase_chunk`) take `store` explicitly and open a temporary writable `Array` from `store.writable` using the array's path. Error immediately if `store.writable` is `None`.
- **Create** operations (`zr_create_array`, `zr_create_group`) also go through `store.writable`, error on read-only stores.
- **Node listing** (`zr_nodes`) requires listable storage — works for filesystem, errors on HTTP. S3 via object_store should support listing.

No enum, no trait gymnastics, no downcasting.

### Store constructors
```r
zr_store(path)       # FilesystemStore — sync, read-write-listable
zr_http_store(url)   # zarrs_http::HTTPStore — sync GET, any HTTP server, read-only
zr_s3_store(url)     # object_store S3 backend, env credentials, async→sync, read-only
# zr_icechunk(...)   # planned — virtual stores, async→sync, read-only
```

## What Works (compiles and tests pass)

### Local filesystem stores — full read/write
```r
s <- zr_store("/path/to/store.zarr")
zr_create_group(s, "/")
arr <- zr_create_array(s, "/data", shape = c(100, 200), chunks = c(50, 50),
                       dtype = "float64", fill_value = NaN,
                       dimension_names = c("y", "x"))
zr_write(arr, rnorm(20000), offset = c(0, 0), count = c(100, 200))
m <- zr_read(arr)                              # 100x200 matrix, byrow=TRUE
m2 <- zr_read(arr, offset = c(10, 20), count = c(5, 10))  # subset
zr_write_chunk(arr, data, chunk_index = c(0, 0))
zr_read_chunk(arr, c(0, 0))
zr_erase_chunk(arr, c(0, 0))
zr_nodes(s)                                    # data.frame of children
zr_shape(arr); zr_chunks(arr); zr_dtype(arr); zr_ndim(arr)
zr_dimnames(arr); zr_attrs(arr); zr_metadata(arr)
zr_find_stores("/path")                        # pure-R store discovery
```

### Key design decisions locked in
- **0-based indexing** (`offset` parameter name signals this)
- **ximage()/rasterImage() orientation**: 2D reads → `matrix(vals, nrow, ncol, byrow = TRUE)`, matching C-contiguous order. Roundtrip: `as.vector(t(m))` recovers C-order.
- **Unified type dispatch**: single `zr_read()` that dispatches on dtype internally in Rust. Covers float32→double, float64, int8/16/32→integer, uint8/16/32→integer, int64/uint64→double.
- **f64 shapes**: all shape/offset/count vectors are R numeric (double), preserving full u64 range. Converted to u64 inside Rust.
- **Proper error handling**: all Rust methods return `extendr_api::Result<T>`, no `.unwrap()` panics. `zr_nodes_inner` uses `throw_r_error()` instead of `Result<List>` due to extendr 0.8 bug where `Result<List>` panics on Err.
- **catch_unwind** on `Array::open` and `Node::open` to catch zarrs internal panics on malformed stores.
- **Print methods**: `print.ZrStore` shows `[rw]`/`[ro]` + path; `print.ZrArray` shows path, shape, dtype, dims, chunks; `print.ZrGroup` shows path.

### GDAL autotest zarr fixtures
131 stores found, ~112 open successfully. 20 fail (V2 compound types, bare arrays without group root, kerchunk parquet refs, deprecated V3 metadata). Errors are descriptive and caught cleanly.

## What's Next (doesn't compile yet — one bounded refactor)

The remote store constructors (`zr_http_store`, `zr_s3_store`) are written but the code doesn't compile because write methods on `ZrArray` try to call `store_chunk`/`store_array_subset` on `Array<dyn ReadableStorageTraits>`, which doesn't have those methods.

### The fix (concrete, no type battles)

Move write methods from being methods on `ZrArray` to **free functions** that take both `ZrStore` and `ZrArray` (or just store + path). On the Rust side:

```rust
// Instead of:  self.inner.store_array_subset(...)
// Do:
fn zr_write_inner(store: &ZrStore, path: &str, ...) -> Result<()> {
    let ws = store.writable.as_ref()
        .ok_or("store is read-only")?;
    let array = Array::open(ws.clone(), path)?;
    array.store_array_subset(&subset, &data)?;
    Ok(())
}
```

On the R side, `zr_write()` gains a `store` parameter:
```r
zr_write <- function(store, arr, data, offset, count)
# or: store is retrieved from arr if we stash it there
```

This is the only change needed. Reads already work for all store types through `ZrArray.inner`.

## Dependency Versions (resolved)

```toml
zarrs = { version = "0.23", features = ["ndarray", "filesystem", "async", "gzip", "blosc", "zstd", "sharding"] }
zarrs_object_store = "0.6"       # must match zarrs_storage 0.4.x
zarrs_http = "0.3"               # simple sync HTTP store (GET only, no WebDAV)
object_store = { version = "0.13", features = ["http", "aws"] }
tokio = { version = "1", features = ["rt-multi-thread"] }
serde_json = "1"
extendr-api = "0.8"
```

**Critical version constraints**:
- zarrs_object_store 0.4.x targets zarrs_storage 0.3.x — **incompatible** with zarrs 0.23 (which uses zarrs_storage 0.4.x). Must use zarrs_object_store **0.6.x**.
- Same issue will apply to zarrs_icechunk — check crates.io for a version targeting zarrs_storage 0.4.x before adding.
- object_store HTTP backend uses WebDAV PROPFIND for listing — most HTTP servers and S3-over-HTTP don't support this. That's why `zr_http_store` uses `zarrs_http::HTTPStore` (simple GET) instead.
- Don't pin `object_store` to match `zarrs_object_store` — let Cargo resolve. They're in the same workspace.

## Build Requirements

- **Rust**: >= 1.85 (edition 2024 support required by zarrs 0.23 ecosystem). Rustup's cargo must be on PATH before system cargo.
- **Makevars**: prepends `$(HOME)/.cargo/bin` to PATH (not append) so rustup cargo wins over system cargo
- **R**: >= 4.1, jsonlite
- **extendr**: 0.8 (known issue: `Result<List>` panics on Err, workaround is `throw_r_error()`)
- Delete `src/rust/Cargo.lock` when switching between git and crates.io dependencies

## zarrs 0.23 API Adaptations

- **DataType is a trait object** (`Arc<dyn DataTypeExtension>`), not an enum. Can't pattern match — compare against factory instances: `if *dt == data_type::float32() { ... }`
- **ArrayBuilder fill value**: typed directly (e.g. `0.0f32.into()`), `FillValue::from()` still works
- **Array methods are generic**: `let data: Vec<f32> = array.retrieve_array_subset(&subset)?` — return type inferred from annotation
- **chunk_grid_shape()** returns number of chunks per dimension, NOT chunk element size. Use `array.chunk_shape(&[0,0,...])` for element dimensions of the origin chunk.
- **Subchunk terminology**: `inner_chunk_shape()` → `subchunk_shape()`
- **Ergonomic indexing**: `array.retrieve_array_subset(&[0..3, 10..20])?` works directly

## Files

```
zr/
├── DESCRIPTION
├── NAMESPACE
├── README.md
├── .Rbuildignore
├── .gitignore
├── R/
│   ├── zr.R          # main API (283 lines)
│   ├── print.R       # format/print methods
│   ├── find.R        # zr_find_stores (pure R)
│   └── zzz.R         # useDynLib
├── src/
│   ├── entrypoint.c
│   ├── Makevars      # prepend cargo to PATH
│   ├── Makevars.win
│   ├── Makevars.ucrt
│   ├── zr-win.def
│   ├── .gitignore
│   └── rust/
│       ├── Cargo.toml
│       └── src/
│           └── lib.rs  # (~615 lines)
├── inst/
│   └── test-fixtures/
│       └── make_test_store.R
└── tests/
    ├── testthat.R
    └── testthat/
        └── test-zr.R  # 13 tests (237 lines)
```

`R/extendr-wrappers.R` is auto-generated by `rextendr::document()` — not in the scaffold.

## Next Steps

1. **Fix the write path** — move write methods to free functions taking `store` + `path`, open temporary writable array from `store.writable`. One bounded refactor, no type battles.
2. **Test HTTP reads** — `zr_http_store(url)` → `zr_array(s, "/varname")` → `zr_read(arr, offset, count)` against a known public zarr
3. **Test S3 reads** — Pangeo OSN stores with anonymous S3 credentials (`AWS_ENDPOINT_URL`, empty key/secret)
4. **Add icechunk** — check zarrs_icechunk version targeting zarrs_storage 0.4.x. This unlocks VirtualiZarr parquet-backed virtual stores (the BRAN2023 use case).
5. **Error handling cleanup** — audit for other extendr `Result<List>` patterns; consider `throw_r_error` consistently for non-struct returns
6. **CRAN readiness** — feature-gate tokio/object_store/icechunk deps behind Cargo features so the minimal filesystem-only build compiles without them
