# zr Package Reboot — Review & Architecture Plan

## 1. Version State

Current published versions on crates.io (use latest throughout):

| Crate | Latest | Notes |
|---|---|---|
| `zarrs` | **0.23.6** | Major release with breaking changes from 0.22 |
| `zarrs_icechunk` | **0.4.1** | Depends on `icechunk` crate |
| `icechunk` | **0.3.24** | Rust crate, transactional storage engine |
| `zarrs_object_store` | (check) | Wraps `object_store` crate |
| `zarrs_opendal` | **0.10.0** | |
| `zarrs_storage` | **0.2.2** | Trait definitions |
| `zarrs_filesystem` | separate crate | Re-exported as `zarrs::filesystem` with `filesystem` feature |

The old `Cargo.toml` had `zarrs = "0.23"` which will resolve to 0.23.6. That's correct for the reboot — use latest everywhere.

## 2. Key API Changes Since Your Old Code

The old `zr` code uses patterns from zarrs ~0.18–0.21 era. Significant changes in 0.22→0.23:

### Storage types
The old code uses:
```rust
use zarrs::storage::{ReadableWritableListableStorage, ReadableWritableListableStorageTraits};
```
In 0.22+, `ReadableWritableListableStorage` is an alias for `Arc<dyn ReadableWritableListableStorageTraits>`. **This still works** but note that `ReadableWritableStorageTraits` was removed (WritableStorageTraits now requires ReadableStorageTraits). In 0.23, further crate splits occurred (zarrs_codec, zarrs_chunk_grid, zarrs_chunk_key_encoding) but these are re-exported so import paths should still work. Your `ZrStore` wrapping `ReadableWritableListableStorage` is fine for the filesystem case.

### FilesystemStore moved
```rust
// Old (still works with `filesystem` feature):
use zarrs::filesystem::FilesystemStore;
// Canonical import:
use zarrs_filesystem::FilesystemStore;  // but re-exported via zarrs::filesystem
```
Your code uses `use zarrs::filesystem::FilesystemStore;` which remains correct with the `filesystem` feature enabled. No change needed.

### ArrayBuilder changes (0.22)
Your `zr_create_array` uses `data_type::float32()` factory functions and `ArrayBuilder::new(shape, chunks, dt, fv)`. In 0.23, the release notes mention renaming "inner chunk" → "subchunk" (`inner_chunk_shape()` → `subchunk_shape()`) and `ArraySubset` methods gained ergonomic indexing with `&dyn ArraySubsetTraits` (e.g. `array.retrieve_array_subset(&[0..3, 10..20])?`). The basic constructor pattern should still compile but verify.

### Node API changes  
`Node::open` and `node.children()` — in 0.22+ `Node::new_with_store` was added alongside async variants, and in 0.23 the error type changed to `NodeCreateError`. Your usage pattern should still compile but test against 0.23.6.

### The `data_type` module
Your old code uses `zarrs::array::data_type` which is the correct current pattern (factory functions like `data_type::float32()`).

### chunk_grid_shape() 
Your `chunk_shape()` method calls `self.inner.chunk_grid_shape()`. In 0.23, this returns a `ChunkGridShape` — verify the `.iter()` still works as before. The 0.23 release also renamed sharding terminology (inner_chunk → subchunk).

## 3. Old Rust Code Review — Issues & Improvements

### 3a. Panics where errors should propagate

```rust
fn read_all_f64(&self) -> Vec<f64> {
    let subset = self.inner.subset_all();
    let data: Vec<f64> = self.inner.retrieve_array_subset(&subset).unwrap();  // PANIC
    data
}
```

Every read/write method uses `.unwrap()` which means Rust panics propagate as R crashes. The reboot should return `extendr_api::Result<T>` from all methods, letting errors surface as R errors rather than segfaults. Same issue in `zr_create_array` and all write methods.

### 3b. i32 shape limitation

```rust
fn shape(&self) -> Vec<i32> {
    self.inner.shape().iter().map(|&x| x as i32).collect()
}
```

Zarr arrays can exceed 2^31 on a dimension. The old code truncates u64→i32 everywhere (shape, chunk_shape, start/count parameters). For the reboot:

- **Shape/metadata queries**: Return `Vec<f64>` (R's native numeric) to preserve full u64 range. R `double` has 53-bit mantissa, enough for any realistic array dimension.
- **Read start/count**: Accept `Vec<f64>` from R and convert to u64 internally.
- **Integer arrays**: Keep `Vec<i32>` for actual i32 data values (R's `integer`), but document the limitation for reading int64/uint64 arrays.

### 3c. Type dispatch is entirely R-side

The `.zr_dtype_family()` function in R classifies types into f32/f64/i32 families, then dispatches to type-specific Rust methods. This works but is fragile and duplicates knowledge. Better: a single `read_subset()` Rust method that inspects `self.inner.data_type()` internally and returns the appropriate R vector, using `Robj` as the return type so it can be `numeric` or `integer`.

### 3d. Missing types

No support for: int64 (→ R double with precision loss warning), uint64, int8, int16, uint8, uint16, float16/bfloat16. The reboot should at minimum handle int8/int16/uint8/uint16 → i32, and int64/uint64 → f64 with a warning.

### 3e. No string/vlen support

Zarr V3 supports string and variable-length data types. Not urgent but note the gap.

### 3f. `zr_nodes` is defined twice

In `R/extendr-wrappers.R` it's generated as `zr_nodes <- function(store, path) .Call(...)` and in `R/zr.R` it's redefined as a wrapper that calls `.Call(wrap__zr_nodes, ...)` and converts to data.frame. The extendr-generated wrapper gets shadowed. This works but is untidy — the reboot should have the R wrapper call the extendr wrapper cleanly (or use `#[extendr]` on an impl method instead of a free function).

## 4. Old R Code Review — Idiom & Hypertidy Alignment

### 4a. jsonlite dependency

`zr_attrs()` and `zr_metadata()` use `jsonlite::fromJSON()`. This is the correct choice — jsonlite is lightweight, widely depended-on, and fast. Keep it.

### 4b. `path.expand()` in `zr_store()`

Good — handles `~/` paths. But also consider `normalizePath(path, mustWork = TRUE)` or at least `mustWork = FALSE` for stores that will be created. The reboot should distinguish "open existing" (normalizePath, mustWork=TRUE) from "create new" (just expand).

### 4c. Function naming convention

`zr_store()`, `zr_array()`, `zr_read()`, `zr_write()` — clean, consistent prefix. This follows the hypertidy pattern (cf. `vapour::vapour_read_geometry()`, `grout::grout_index()`). Keep the `zr_` prefix.

### 4d. 0-based indexing exposed to R

`zr_read(arr, start = NULL, count = NULL)` documents start as "0-based". This is a usability trap — R users expect 1-based. Two options:

1. **Keep 0-based** with prominent documentation (matches Zarr/Python convention, less surprising for the target audience who are also Python/Zarr users).
2. **Accept 1-based**, subtract 1 internally.

Given hypertidy philosophy (thin wrapper, no magic), option 1 is probably right. But document it loudly. Consider naming the parameter `offset` instead of `start` to signal it's not R-native indexing.

### 4e. `stringsAsFactors = FALSE`

In `zr_nodes()`:
```r
data.frame(path = result$path, node_type = result$node_type, stringsAsFactors = FALSE)
```
Since R 4.0, `stringsAsFactors` defaults to FALSE. You can drop this — the reboot should target R >= 4.1 minimum anyway. Minor but de-clutters.

### 4f. Return values

- `zr_read()` sets `dim(vals) <- shp` when ndim > 1. Zarr's default chunk order is C-contiguous (row-major). R's `matrix()`/`array()` storage is Fortran-contiguous (column-major). The correct interpretation depends on what convention we align to.

  **We align to `ximage()` / `rasterImage()` convention.** For a 2D array with shape `[nrow, ncol]`, the zarr data comes flattened in row-major order. To get the correct matrix for `rasterImage()`:
  
  ```r
  m <- matrix(vals, nrow = shp[1], ncol = shp[2], byrow = TRUE)
  ```
  
  This is the correct orientation for `ximage()` and `rasterImage()` (not `image()`, which transposes and flips). The round-trip caveat: `as.vector(m)` does NOT recover the original flat order — you need `as.vector(t(m))` to get back to C-contiguous. This is an inherent R/C mismatch and should be documented clearly.
  
  For the Rust side, the simplest approach: return the flat vector and shape from Rust, and in R do `matrix(vals, nrow = shp[1], ncol = shp[2], byrow = TRUE)` for 2D, or for nD arrays use a helper that permutes appropriately. The key invariant is: **the data from zarrs is C-order, and we present it in R as if `byrow = TRUE`**, matching ximage/rasterImage orientation.
  
  For nD arrays (3+ dimensions), the equivalent is `array(vals, dim = rev(shp))` followed by `aperm(arr, rev(seq_along(shp)))` — but this is complex and we should think carefully about whether to do it automatically or leave it to the user with good documentation. For the common 2D raster case, `byrow = TRUE` is the right default.

### 4g. No print/format methods

The extendr objects (`ZrStore`, `ZrArray`, `ZrGroup`) have no R-side print methods. `print(store)` shows an opaque external pointer. The reboot should add `print.ZrStore`, `print.ZrArray` etc. that show path, shape, dtype — like ndr's dataset printing.

## 5. Hypertidy Ecosystem — No Overlaps

zr is the only hypertidy package touching Zarr directly. No overlap with vapour/gdalraster (GDAL 2D raster/vector), vaster (grid geometry), align (alignment invariants), or grout (tile indexing). Future bridges are possible (e.g. a zr array + coordinate variables → `align` object, or grout tilescheme concepts mapping to zarr chunk layouts) but those are downstream concerns, not zr's scope.

## 6. Recommended Architecture for the Reboot

### Cargo.toml (start here, filesystem only)

```toml
[package]
name = "zr"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["staticlib"]
name = "zr"

[dependencies]
extendr-api = "0.8"
zarrs = { version = "0.23", features = ["ndarray", "filesystem", "gzip", "blosc", "zstd", "sharding"] }
serde_json = "1"
```

**Phase 1**: Ship with just filesystem. Get all the R API right, tests passing, CRAN-compatible.

### Cargo.toml (phase 2, add object_store + async)

```toml
[dependencies]
# ... phase 1 deps ...
zarrs_object_store = "0.x"  # check exact version compatible with zarrs 0.23
tokio = { version = "1", features = ["rt-multi-thread"] }
```

Add `zr_object_store(url, ...)` returning async-to-sync wrapped store.

### Cargo.toml (phase 3, add icechunk)

```toml
[dependencies]
# ... phase 2 deps ...
zarrs_icechunk = "0.4"
icechunk = "0.3"
```

Add `zr_icechunk_store(path, ...)` → Session → AsyncToSync.

### Feature gating for CRAN

Consider Cargo features so the base package (filesystem only) compiles without tokio/openssl/ring, which are the hard CRAN dependencies:

```toml
[features]
default = ["filesystem"]
filesystem = ["zarrs/filesystem"]
object_store = ["dep:zarrs_object_store", "dep:tokio"]
icechunk = ["dep:zarrs_icechunk", "dep:icechunk", "dep:tokio"]
```

Then `Makevars` can set `--features filesystem` for CRAN and `--all-features` for r-universe/dev.

### R API shape (reboot)

```r
# --- stores ---
zr_store(path)                     # FilesystemStore (sync, no tokio)
zr_object_store(url, ...)          # object_store (async→sync, needs tokio)
zr_icechunk(repo_path, branch = "main")  # icechunk Session (async→sync)

# --- open ---
zr_group(store, path = "/")        # open group
zr_array(store, path)              # open array

# --- metadata ---
zr_shape(arr)                      # numeric vector (f64 to preserve u64 range)
zr_chunks(arr)                     # numeric vector
zr_dtype(arr)                      # character
zr_ndim(arr)                       # integer
zr_dimnames(arr)                   # character vector or NULL
zr_attrs(arr)                      # named list (via jsonlite)
zr_metadata(arr)                   # full metadata list (via jsonlite)
zr_fill_value(arr)                 # R scalar matching dtype

# --- read ---
zr_read(arr, offset = NULL, count = NULL)   # full or subset read
zr_read_chunk(arr, chunk_index)             # single chunk

# --- write ---
zr_write(arr, data, offset, count)          # subset write
zr_write_chunk(arr, data, chunk_index)      # chunk write

# --- create ---
zr_create_array(store, path, shape, chunks, dtype,
                fill_value = 0, dimension_names = NULL,
                attributes = NULL, codecs = NULL)

# --- hierarchy ---
zr_nodes(store, path = "/")        # data.frame(path, node_type)
zr_create_group(store, path)       # create group + store metadata

# --- utilities ---
zr_find_stores(path)               # scan directory for zarr stores
zr_version()                       # package + zarrs version string
```

### Key design decisions

1. **`offset` not `start`** — signals 0-based, avoids confusion with R's 1-based `start` in `seq()` etc.

2. **Unified read method** — single `zr_read()` that dispatches on dtype internally in Rust, returns appropriate R type (`numeric` for float/int64, `integer` for int8–int32, `raw` for uint8).

3. **C→ximage orientation** — zarrs returns flat data in C-contiguous (row-major) order. For 2D, `zr_read()` should return a matrix via `matrix(vals, nrow = shp[1], ncol = shp[2], byrow = TRUE)`, which is correct for `ximage()` / `rasterImage()`. Document that `as.vector(t(m))` recovers the original C-order flat vector (not `as.vector(m)`). For nD arrays, return the flat vector + shape attribute and let the user decide — automatic nD permutation is too opinionated for a thin wrapper.

4. **Error handling** — all Rust methods return `extendr_api::Result<T>`, no `.unwrap()` anywhere.

5. **Print methods** — `format.ZrStore`, `format.ZrArray`, `format.ZrGroup` showing useful summary info.

6. **No R6/S4** — plain functions + extendr external pointers, consistent with hypertidy philosophy.

## 7. Comparison: zr_find_stores / zr_test_stores

These pure-R utilities in `inst/test-fixtures/scan_gdal_autotest.R` are useful testing tools but shouldn't be exported from the package. They scan for `.zarray`/`.zgroup`/`zarr.json` markers — this is fine for V2/V3 discovery. Keep them as internal helpers or in `inst/` for development.

## 8. Build Infrastructure

The `Makevars`, `Makevars.win`, `Makevars.ucrt`, and `entrypoint.c` are standard extendr boilerplate and look correct. No changes needed for the reboot except:

- `entrypoint.c` references `R_init_zr_extendr` — this must match the Rust `extendr_module!` name.
- For CRAN, ensure `CARGO_HOME` isolation works (the `NOT_CRAN` check is already there).

## 9. Immediate Action Items

1. **Create fresh repo** `mdsumner/zr` (or `hypertidy/zr`).
2. **Start with `zarrs = "0.23"`** — current on crates.io.
3. **Phase 1 scope**: filesystem store, read/write, proper error handling, ximage()-aligned orientation, print methods. No async, no icechunk, no object_store.
4. **Test against GDAL autotest zarr fixtures** (your `scan_gdal_autotest.R` is perfect for this).
5. **Test orientation** — write a known array in Python zarr, read with zr, verify `matrix(vals, nrow, ncol, byrow = TRUE)` gives the correct image for `ximage()`.
6. **Write tests using testthat** — create stores in `setup()`, test read/write roundtrips, verify `as.vector(t(m))` recovers original flat C-order.
7. **Adapt to 0.23 breaking changes** — the changelog mentions subchunk renaming, ArraySubset ergonomics (`&[0..3, 10..20]` syntax), and crate splits (zarrs_codec, zarrs_chunk_grid, zarrs_chunk_key_encoding — re-exported so may be transparent).
8. **Phase 2**: Add `zarrs_object_store` behind a feature flag, implement `zr_object_store()`.
9. **Phase 3**: Add `zarrs_icechunk`, implement `zr_icechunk()`.
