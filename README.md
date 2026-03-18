# zr

Zarr V2/V3 arrays for R, via the [zarrs](https://zarrs.dev/) Rust crate.

## Features

- Read and write Zarr V2 and V3 multidimensional arrays
- Filesystem store (Phase 1), object_store and icechunk planned
- Chunk-level read/write access
- Proper error handling (no Rust panics into R)
- 2D reads oriented for `ximage()` / `rasterImage()` (`byrow = TRUE`)

## Installation

Requires Rust toolchain (rustc >= 1.82.0, cargo):

```r
# install.packages("pak")
pak::pak("mdsumner/zr")
```

## Quick start

```r
library(zr)

d <- tempfile(fileext = ".zarr")
dir.create(d)
s <- zr_store(d)
zr_create_group(s, "/")

arr <- zr_create_array(s, "/data",
  shape = c(100, 200), chunks = c(50, 50),
  dtype = "float64", fill_value = NaN,
  dimension_names = c("y", "x"))

zr_write(arr, mm <- rnorm(100 * 200), offset = c(0, 0), count = c(100, 200))

m <- zr_read(arr)
# m is a 100x200 matrix, oriented for ximage()/rasterImage()
```

## Indexing

All indexing is **0-based** (matching zarr/Python convention).

## License

Apache License (>= 2)
