s <- zr_http_store("https://ncsa.osn.xsede.org/Pangeo/pangeo-forge/gpcp-feedstock/gpcp.zarr")
arr <- zr_array(s, "/precip")
zr_shape(arr)
## one timestep, 10x10 spatial subset
m <- zr_read(arr, offset = c(0, 0, 0), count = c(1, 10, 10))
str(m)

## check metadata
zr_dtype(arr)
zr_chunks(arr)
zr_dimnames(arr)
zr_attrs(arr)



s <- zr_http_store("https://storage.googleapis.com/gcp-public-data-arco-era5/ar/full_37-1h-0p25deg-chunk-1.zarr-v3")
arr <- zr_array(s, "/latitude")
zr_shape(arr)
zr_read(arr, offset = c(0), count = c(10))



## What arrays are available? (won't work via HTTP listing, but we know the schema)
## This is a V3 store, so common variables:
temp <- zr_array(s, "/2m_temperature")
zr_shape(temp)
zr_dtype(temp)
zr_chunks(temp)
zr_attrs(temp)

## Read a small slice — first timestep, small spatial window
## shape is likely [time, level?, lat, lon] or [time, lat, lon]
zr_read(temp, offset = c(0, 0, 0), count = c(1, 5, 5))


