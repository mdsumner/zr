test_that("store open and version", {
  d <- tempfile(fileext = ".zarr")
  dir.create(d)
  s <- zr_store(d)
  expect_s3_class(s, "ZrStore")
  expect_true(nchar(zr_version()) > 0)
  unlink(d, recursive = TRUE)
})

test_that("create group and list nodes", {
  d <- tempfile(fileext = ".zarr")
  dir.create(d)
  s <- zr_store(d)
  zr_create_group(s, "/")
  g <- zr_group(s, "/")
  expect_s3_class(g, "ZrGroup")
  unlink(d, recursive = TRUE)
})

test_that("create f64 array and roundtrip", {
  d <- tempfile(fileext = ".zarr")
  dir.create(d)
  s <- zr_store(d)
  zr_create_group(s, "/")

  arr <- zr_create_array(s, "/data", shape = c(4, 6), chunks = c(2, 3),
                         dtype = "float64", fill_value = NaN,
                         dimension_names = c("y", "x"))
  expect_s3_class(arr, "ZrArray")
  expect_equal(zr_shape(arr), c(4, 6))
  expect_equal(zr_chunks(arr), c(2, 3))
  expect_equal(zr_ndim(arr), 2L)
  expect_equal(zr_dimnames(arr), c("y", "x"))
  expect_true(grepl("float64", zr_dtype(arr), ignore.case = TRUE))

  unlink(d, recursive = TRUE)
})

test_that("2D write/read roundtrip with correct orientation", {
  d <- tempfile(fileext = ".zarr")
  dir.create(d)
  s <- zr_store(d)
  zr_create_group(s, "/")

  arr <- zr_create_array(s, "/mat", shape = c(3, 4), chunks = c(3, 4),
                         dtype = "float64", fill_value = 0)

  ## Write C-contiguous data: row0 = 1:4, row1 = 5:8, row2 = 9:12
  vals <- as.double(1:12)
  zr_write(s, arr, vals, offset = c(0, 0), count = c(3, 4))

  ## Read back — should get a 3x4 matrix oriented for ximage/rasterImage
  m <- zr_read(arr)
  expect_true(is.matrix(m))
  expect_equal(dim(m), c(3, 4))

  ## Row 1 should be 1, 2, 3, 4 (C-order first row)
  expect_equal(m[1, ], c(1, 2, 3, 4))
  expect_equal(m[2, ], c(5, 6, 7, 8))
  expect_equal(m[3, ], c(9, 10, 11, 12))

  ## Roundtrip: as.vector(t(m)) recovers original C-order
  expect_equal(as.vector(t(m)), vals)

  unlink(d, recursive = TRUE)
})

test_that("subset read", {
  d <- tempfile(fileext = ".zarr")
  dir.create(d)
  s <- zr_store(d)
  zr_create_group(s, "/")

  arr <- zr_create_array(s, "/sub", shape = c(4, 6), chunks = c(2, 3),
                         dtype = "float64", fill_value = 0)
  zr_write(s, arr, as.double(1:24), offset = c(0, 0), count = c(4, 6))

  ## Read a 2x3 subset starting at (1, 2)
  m <- zr_read(arr, offset = c(1, 2), count = c(2, 3))
  expect_equal(dim(m), c(2, 3))

  ## In C-order, row 1 (0-based) cols 2-4 = values 9, 10, 11
  expect_equal(m[1, ], c(9, 10, 11))

  unlink(d, recursive = TRUE)
})

test_that("1D array returns plain vector", {
  d <- tempfile(fileext = ".zarr")
  dir.create(d)
  s <- zr_store(d)
  zr_create_group(s, "/")

  arr <- zr_create_array(s, "/vec", shape = 10, chunks = 5,
                         dtype = "float64", fill_value = 0)
  zr_write(s, arr, as.double(1:10), offset = 0, count = 10)
  v <- zr_read(arr)
  expect_equal(v, as.double(1:10))
  expect_null(dim(v))

  unlink(d, recursive = TRUE)
})

test_that("int32 roundtrip", {
  d <- tempfile(fileext = ".zarr")
  dir.create(d)
  s <- zr_store(d)
  zr_create_group(s, "/")

  arr <- zr_create_array(s, "/ints", shape = c(3, 4), chunks = c(3, 4),
                         dtype = "int32", fill_value = -999)
  zr_write(s, arr, 1:12, offset = c(0, 0), count = c(3, 4))
  m <- zr_read(arr)
  expect_true(is.integer(m))
  expect_equal(m[1, ], 1:4)
  expect_equal(m[3, ], 9:12)

  unlink(d, recursive = TRUE)
})

test_that("float32 roundtrip (promoted to double)", {
  d <- tempfile(fileext = ".zarr")
  dir.create(d)
  s <- zr_store(d)
  zr_create_group(s, "/")

  arr <- zr_create_array(s, "/f32", shape = c(2, 3), chunks = c(2, 3),
                         dtype = "float32", fill_value = 0)
  zr_write(s, arr, c(1.5, 2.5, 3.5, 4.5, 5.5, 6.5), offset = c(0, 0), count = c(2, 3))
  m <- zr_read(arr)
  expect_true(is.double(m))
  expect_equal(m[1, 1], 1.5, tolerance = 1e-6)

  unlink(d, recursive = TRUE)
})

test_that("chunk read/write roundtrip", {
  d <- tempfile(fileext = ".zarr")
  dir.create(d)
  s <- zr_store(d)
  zr_create_group(s, "/")

  arr <- zr_create_array(s, "/chunked", shape = c(4, 6), chunks = c(2, 3),
                         dtype = "float64", fill_value = NaN)
  chunk_data <- as.double(101:106)
  zr_write_chunk(s, arr, chunk_data, chunk_index = c(0, 0))
  got <- zr_read_chunk(arr, c(0, 0))
  expect_equal(got, chunk_data)

  unlink(d, recursive = TRUE)
})

test_that("erase chunk", {
  d <- tempfile(fileext = ".zarr")
  dir.create(d)
  s <- zr_store(d)
  zr_create_group(s, "/")

  arr <- zr_create_array(s, "/eraseme", shape = c(4, 4), chunks = c(2, 2),
                         dtype = "float64", fill_value = -1)
  zr_write_chunk(s, arr, rep(99, 4), chunk_index = c(0, 0))
  zr_erase_chunk(s, arr, c(0, 0))
  ## After erase, reading should give fill values
  m <- zr_read(arr, offset = c(0, 0), count = c(2, 2))
  expect_true(all(m == -1))

  unlink(d, recursive = TRUE)
})

test_that("nodes listing", {
  d <- tempfile(fileext = ".zarr")
  dir.create(d)
  s <- zr_store(d)
  zr_create_group(s, "/")
  zr_create_array(s, "/a1", shape = c(2, 2), chunks = c(2, 2),
                  dtype = "float64", fill_value = 0)
  zr_create_group(s, "/grp")
  zr_create_array(s, "/grp/a2", shape = 10, chunks = 5,
                  dtype = "int32", fill_value = 0)

  nodes <- zr_nodes(s)
  expect_true(is.data.frame(nodes))
  expect_true("/a1" %in% nodes$path)
  expect_true("/grp" %in% nodes$path)

  unlink(d, recursive = TRUE)
})

test_that("print methods produce output", {
  d <- tempfile(fileext = ".zarr")
  dir.create(d)
  s <- zr_store(d)
  zr_create_group(s, "/")
  arr <- zr_create_array(s, "/pr", shape = c(3, 4), chunks = c(3, 4),
                         dtype = "float32", fill_value = 0,
                         dimension_names = c("lat", "lon"))

  expect_output(print(s), "ZrStore")
  expect_output(print(arr), "ZrArray")
  expect_output(print(arr), "float32")
  expect_output(print(arr), "lat, lon")

  g <- zr_group(s, "/")
  expect_output(print(g), "ZrGroup")

  unlink(d, recursive = TRUE)
})

test_that("attributes roundtrip", {
  d <- tempfile(fileext = ".zarr")
  dir.create(d)
  s <- zr_store(d)
  zr_create_group(s, "/")
  arr <- zr_create_array(s, "/withattrs", shape = c(2, 2), chunks = c(2, 2),
                         dtype = "float64", fill_value = 0,
                         attributes = list(units = "kelvin", source = "test"))
  attrs <- zr_attrs(arr)
  expect_equal(attrs$units, "kelvin")
  expect_equal(attrs$source, "test")

  unlink(d, recursive = TRUE)
})

test_that("find_stores discovers V3 stores", {
  d <- tempfile()
  dir.create(d)
  ## Create a V3 store manually
  store_path <- file.path(d, "test.zarr")
  dir.create(store_path)
  writeLines('{"zarr_format":3,"node_type":"group"}', file.path(store_path, "zarr.json"))

  stores <- zr_find_stores(d)
  expect_true(length(stores) >= 1)
  expect_true(any(grepl("test.zarr", stores)))

  unlink(d, recursive = TRUE)
})
