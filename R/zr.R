#' Open a Zarr filesystem store
#'
#' @param path Character, path to a zarr hierarchy directory.
#' @return A ZrStore object (external pointer).
#' @export
zr_store <- function(path) {
  ZrStore$new(normalizePath(path.expand(path), mustWork = FALSE))
}

#' Open a remote Zarr store over HTTP
#'
#' Opens a read-only zarr store served over HTTP/HTTPS using simple GET
#' requests. Works with any HTTP server — no WebDAV needed. Listing
#' ([zr_nodes()]) is not supported; use [zr_array()] directly with
#' known array paths.
#'
#' @param url Character, base URL of the zarr hierarchy (e.g.
#'   `"https://example.com/path/to/store.zarr"`).
#' @return A ZrStore object (external pointer, read-only).
#' @export
zr_http_store <- function(url) {
  ZrStore$new_http(url)
}

#' Open a remote Zarr store on S3
#'
#' Opens a read-only zarr store from an S3-compatible bucket. Credentials
#' are read from the environment (`AWS_ACCESS_KEY_ID`,
#' `AWS_SECRET_ACCESS_KEY`, `AWS_DEFAULT_REGION`, etc.). For anonymous
#' access to public buckets, set `AWS_ACCESS_KEY_ID=""` and
#' `AWS_SECRET_ACCESS_KEY=""`.
#'
#' @param url Character, S3 URL (e.g. `"s3://bucket/path/to/store.zarr"`).
#' @return A ZrStore object (external pointer, read-only).
#' @export
zr_s3_store <- function(url) {
  ZrStore$new_s3(url)
}

#' Open an existing Zarr array
#'
#' @param store A ZrStore (from [zr_store()], [zr_http_store()], or
#'   [zr_s3_store()]).
#' @param path Character, path within the store (e.g. `"/temperature"`).
#' @return A ZrArray object (external pointer).
#' @export
zr_array <- function(store, path) {
  zr_open_array(store, path)
}

#' Open or create a Zarr group
#'
#' @param store A ZrStore.
#' @param path Character, group path within the store.
#' @return A ZrGroup object (external pointer).
#' @export
zr_group <- function(store, path = "/") {
  zr_open_group(store, path)
}

#' Create a Zarr group and store its metadata
#'
#' @param store A ZrStore (must be writable — filesystem only).
#' @param path Character, group path within the store.
#' @return A ZrGroup object (external pointer), invisibly.
#' @export
zr_create_group <- function(store, path = "/") {
  invisible(zr_create_group_inner(store, path))
}

#' Array shape
#'
#' @param arr A ZrArray.
#' @return Numeric vector of dimension sizes.
#' @export
zr_shape <- function(arr) {
  arr$shape()
}

#' Array chunk shape
#'
#' @param arr A ZrArray.
#' @return Numeric vector of chunk sizes.
#' @export
zr_chunks <- function(arr) {
  arr$chunk_shape()
}

#' Array data type
#'
#' @param arr A ZrArray.
#' @return Character string (zarr V3 data type name).
#' @export
zr_dtype <- function(arr) {
  arr$dtype()
}

#' Array dimensionality
#'
#' @param arr A ZrArray.
#' @return Integer scalar.
#' @export
zr_ndim <- function(arr) {
  arr$ndim()
}

#' Array dimension names
#'
#' @param arr A ZrArray.
#' @return Character vector or NULL.
#' @export
zr_dimnames <- function(arr) {
  arr$dimension_names()
}

#' Array or group attributes
#'
#' @param x A ZrArray or ZrGroup.
#' @return Named list parsed from JSON.
#' @export
zr_attrs <- function(x) {
  jsonlite::fromJSON(x$attributes_json())
}

#' Full array metadata
#'
#' @param arr A ZrArray.
#' @return Named list parsed from JSON.
#' @export
zr_metadata <- function(arr) {
  jsonlite::fromJSON(arr$metadata_json())
}

#' Array fill value
#'
#' @param arr A ZrArray.
#' @return Character representation of the fill value.
#' @export
zr_fill_value <- function(arr) {
  arr$fill_value_json()
}

# ---------------------------------------------------------------------------
# Read
# ---------------------------------------------------------------------------

#' Read array data
#'
#' Read the entire array or a subset.
#'
#' For 2D arrays, returns a matrix oriented for [ximage()] / [rasterImage()]:
#' `matrix(data, nrow, ncol, byrow = TRUE)`. This means the first
#' dimension varies along rows and the second along columns, matching
#' the C-contiguous (row-major) order in which zarr stores data.
#'
#' For 1D arrays, returns a plain vector. For 3D+ arrays, returns a
#' vector with a `"zr_shape"` attribute — use [zr_as_matrix()] or
#' construct your own array from the flat data and shape.
#'
#' **Indexing is 0-based** (matching zarr/Python convention): `offset = 0`
#' means the first element.
#'
#' @param arr A ZrArray.
#' @param offset Numeric vector of 0-based start indices, or NULL for full read.
#' @param count Numeric vector of counts per dimension, or NULL for full read.
#' @return Numeric or integer vector, matrix, or array.
#' @export
zr_read <- function(arr, offset = NULL, count = NULL) {
  if (is.null(offset) && is.null(count)) {
    shp <- arr$shape()
    vals <- arr$read_all_robj()
  } else {
    stopifnot(!is.null(offset), !is.null(count))
    offset <- as.numeric(offset)
    count <- as.numeric(count)
    shp <- count
    vals <- arr$read_subset_robj(offset, count)
  }
  .zr_shape_result(vals, shp)
}

#' Read a single chunk
#'
#' @param arr A ZrArray.
#' @param chunk_index Numeric vector of chunk indices (0-based).
#' @return Numeric or integer vector (flat, no dimensions set).
#' @export
zr_read_chunk <- function(arr, chunk_index) {
  arr$read_chunk_robj(as.numeric(chunk_index))
}

# ---------------------------------------------------------------------------
# Write (filesystem stores only)
# ---------------------------------------------------------------------------

#' Write data to an array subset
#'
#' Writes to local filesystem stores only. Remote stores are read-only.
#'
#' Data should be in C-contiguous (row-major) order. For a matrix created
#' with `byrow = TRUE` (as returned by [zr_read()]), flatten with
#' `as.vector(t(m))` before writing.
#'
#' @param store A ZrStore (must be writable — filesystem only).
#' @param arr A ZrArray.
#' @param data Numeric or integer vector.
#' @param offset Numeric vector of 0-based start indices.
#' @param count Numeric vector of counts per dimension.
#' @export
zr_write <- function(store, arr, data, offset, count) {
  offset <- as.numeric(offset)
  count <- as.numeric(count)
  if (is.integer(data)) {
    zr_write_subset_inner(store, arr, offset, count,
                          data_f64 = NULL, data_i32 = data)
  } else {
    zr_write_subset_inner(store, arr, offset, count,
                          data_f64 = as.double(data), data_i32 = NULL)
  }
  invisible(NULL)
}

#' Write data to a chunk
#'
#' Writes to local filesystem stores only. Remote stores are read-only.
#'
#' @param store A ZrStore (must be writable — filesystem only).
#' @param arr A ZrArray.
#' @param data Numeric or integer vector.
#' @param chunk_index Numeric vector of chunk indices (0-based).
#' @export
zr_write_chunk <- function(store, arr, data, chunk_index) {
  chunk_index <- as.numeric(chunk_index)
  if (is.integer(data)) {
    zr_write_chunk_inner(store, arr, chunk_index,
                         data_f64 = NULL, data_i32 = data)
  } else {
    zr_write_chunk_inner(store, arr, chunk_index,
                         data_f64 = as.double(data), data_i32 = NULL)
  }
  invisible(NULL)
}

#' Erase a chunk
#'
#' Erases on local filesystem stores only. Remote stores are read-only.
#'
#' @param store A ZrStore (must be writable — filesystem only).
#' @param arr A ZrArray.
#' @param chunk_index Numeric vector of chunk indices (0-based).
#' @export
zr_erase_chunk <- function(store, arr, chunk_index) {
  zr_erase_chunk_inner(store, arr, as.numeric(chunk_index))
  invisible(NULL)
}

# ---------------------------------------------------------------------------
# Create
# ---------------------------------------------------------------------------

#' Create a new Zarr array
#'
#' Creates on local filesystem stores only. Remote stores are read-only.
#'
#' @param store A ZrStore (must be writable — filesystem only).
#' @param path Character, array path within the store.
#' @param shape Numeric vector of dimension sizes.
#' @param chunks Numeric vector of chunk sizes.
#' @param dtype Character, data type name. One of: "float32", "float64",
#'   "int8", "int16", "int32", "int64", "uint8", "uint16", "uint32", "uint64".
#' @param fill_value Numeric scalar, fill value for uninitialised chunks.
#' @param dimension_names Character vector of dimension names, or NULL.
#' @param attributes Named list of attributes (will be serialised to JSON), or NULL.
#' @return A ZrArray object (external pointer).
#' @export
zr_create_array <- function(store, path, shape, chunks, dtype,
                            fill_value = 0, dimension_names = NULL,
                            attributes = NULL) {
  shape <- as.numeric(shape)
  chunks <- as.numeric(chunks)
  attrs_json <- if (!is.null(attributes)) jsonlite::toJSON(attributes, auto_unbox = TRUE) else NULL
  zr_create_array_inner(store, path, shape, chunks, dtype,
                        as.double(fill_value),
                        dimension_names, attrs_json)
}

# ---------------------------------------------------------------------------
# Hierarchy
# ---------------------------------------------------------------------------

#' List nodes in a Zarr store
#'
#' Lists child nodes (arrays and groups) under a path. Requires a
#' listable store (filesystem). HTTP stores do not support listing.
#'
#' @param store A ZrStore.
#' @param path Character, root path to list from.
#' @return A data.frame with columns `path` and `node_type`.
#' @export
zr_nodes <- function(store, path = "/") {
  result <- zr_nodes_inner(store, path)
  data.frame(path = result$path, node_type = result$node_type)
}

# ---------------------------------------------------------------------------
# Utilities
# ---------------------------------------------------------------------------

#' Package and zarrs version
#'
#' @return Character string with version info.
#' @export
zr_version <- function() {
  zr_zarrs_version()
}

# ---------------------------------------------------------------------------
# Internal: shape result into appropriate R object
# ---------------------------------------------------------------------------

.zr_shape_result <- function(vals, shp) {
  nd <- length(shp)
  if (nd <= 1L) {
    return(vals)
  }
  if (nd == 2L) {
    return(matrix(vals, nrow = shp[1], ncol = shp[2], byrow = TRUE))
  }
  ## nD: return flat vector with shape attribute; let user decide layout
  attr(vals, "zr_shape") <- shp
  vals
}
