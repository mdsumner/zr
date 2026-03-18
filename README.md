# zr

Zarr V2/V3 arrays for R, via the [zarrs](https://zarrs.dev/) Rust crate.

## Features

- Read Zarr V2 and V3 multidimensional arrays from any source
- Filesystem, HTTP, and S3 stores through a single read path
- Local write for testing and prototyping (remote write out of scope)
- Chunk-level read/write access
- Proper error handling (no Rust panics into R)
- 2D reads oriented for `ximage()` / `rasterImage()` (`byrow = TRUE`)
- Compression: gzip, blosc, zstd, sharding

## Installation

Requires Rust toolchain (rustc >= 1.85.0, cargo):

```r
# install.packages("pak")
pak::pak("hypertidy/zr")
```

## Quick start

### Read from any store

```r
library(zr)

## Local filesystem
s <- zr_store("/path/to/store.zarr")
arr <- zr_array(s, "/temperature")
m <- zr_read(arr)

## Remote HTTP (any server, no WebDAV needed)
s <- zr_http_store("https://example.com/data.zarr")
arr <- zr_array(s, "/temperature")
m <- zr_read(arr, offset = c(0, 0, 0), count = c(1, 100, 200))

## S3 (credentials from environment)
s <- zr_s3_store("s3://bucket/store.zarr")
arr <- zr_array(s, "/temperature")
m <- zr_read(arr)
```

### Write to local stores

```r
d <- tempfile(fileext = ".zarr")
dir.create(d)
s <- zr_store(d)
zr_create_group(s, "/")

arr <- zr_create_array(s, "/data",
  shape = c(100, 200), chunks = c(50, 50),
  dtype = "float64", fill_value = NaN,
  dimension_names = c("y", "x"))

zr_write(s, arr, rnorm(100 * 200), offset = c(0, 0), count = c(100, 200))

m <- zr_read(arr)
# m is a 100x200 matrix, oriented for ximage()/rasterImage()
```

## Indexing

All indexing is **0-based** (matching zarr/Python convention).

## Design

zr is a thin wrapper — no R6/S4 classes, plain functions with `zr_` prefix,
extendr external pointers for Rust objects. Remote stores (HTTP, S3) are
read-only by design. Write-to-local is for testing and prototyping; write-to-remote
is a data engineering concern better handled in Python.

## License

Apache License (>= 2)
