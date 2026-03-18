## Create a test store with various array types for manual exploration
## Usage: source this file with zr loaded
##   source(system.file("test-fixtures/make_test_store.R", package = "zr"))

make_test_store <- function(path = tempfile(fileext = ".zarr")) {
  dir.create(path, showWarnings = FALSE)
  store <- zr_store(path)
  zr_create_group(store, "/")

  ## float64 2D
  a1 <- zr_create_array(store, "/f64_2d",
                        shape = c(20, 30), chunks = c(10, 10),
                        dtype = "float64", fill_value = NaN,
                        dimension_names = c("y", "x"))
  zr_write_chunk(a1, rnorm(100), c(0, 0))

  ## float32 3D
  a2 <- zr_create_array(store, "/f32_3d",
                        shape = c(5, 10, 20), chunks = c(5, 5, 10),
                        dtype = "float32", fill_value = 0,
                        dimension_names = c("z", "y", "x"))
  zr_write(a2, as.double(1:250), offset = c(0, 0, 0), count = c(5, 5, 10))

  ## int32 1D
  a3 <- zr_create_array(store, "/counts",
                        shape = 100, chunks = 25,
                        dtype = "int32", fill_value = -999L,
                        dimension_names = "idx")
  zr_write_chunk(a3, 1:25, 0)

  ## nested group
  zr_create_group(store, "/group1")
  zr_create_array(store, "/group1/nested",
                  shape = c(4, 4), chunks = c(2, 2),
                  dtype = "float64", fill_value = 0,
                  dimension_names = c("y", "x"))

  message("Created test store: ", path)
  invisible(store)
}
