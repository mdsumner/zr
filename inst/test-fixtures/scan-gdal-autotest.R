#' Scan a directory tree for Zarr stores
#'
#' Finds all Zarr V2 and V3 stores by looking for metadata marker files:
#' `.zarray`, `.zgroup`, `.zmetadata` (V2) and `zarr.json` (V3).
#' A store root is the parent directory of the shallowest marker found.
#'
#' @param path Character, root directory to scan.
#' @return Character vector of unique store root paths.
#' @export
zr_find_stores <- function(path) {
  path <- normalizePath(path, mustWork = TRUE)
  
  ## V2 markers at store root level: .zarray, .zgroup, .zmetadata
  ## V3 marker: zarr.json
  markers <- c(".zarray", ".zgroup", ".zmetadata", "zarr.json")
  
  all_files <- list.files(path, recursive = TRUE, all.files = TRUE,
                          full.names = TRUE, no.. = TRUE)
  
  ## find files matching any marker
  marker_files <- all_files[basename(all_files) %in% markers]
  
  if (length(marker_files) == 0L) return(character(0))
  
  ## the store root is the directory containing the shallowest marker
  ## relative to `path`. For a flat V2 array like array_attrs.zarr/.zarray,
  ## the store root is array_attrs.zarr/. For a V3 hierarchy like
  ## test.zr3/zarr.json the store root is test.zr3/.
  ## For nested structures like group.zarr/.zgroup with
  ## group.zarr/foo/.zarray, the store root is group.zarr/.
  
  ## Get the relative path of each marker from the scan root
  rel <- sub(paste0("^", gsub("([.+*?^${}()|\\[\\]])", "\\\\\\1", path), "/?"), "", marker_files)
  
  ## The store root is everything before the first marker in the path.
  ## Split on / and find where the marker sits
  store_roots <- vapply(rel, function(r) {
    parts <- strsplit(r, "/", fixed = TRUE)[[1]]
    marker_pos <- which(parts %in% markers)
    if (length(marker_pos) == 0L) return(NA_character_)
    ## store root = path up to (but not including) the marker
    if (marker_pos[1] == 1L) {
      ## marker is directly in scan root — the scan root itself is a store
      return(path)
    }
    file.path(path, paste(parts[seq_len(marker_pos[1] - 1L)], collapse = "/"))
  }, character(1), USE.NAMES = FALSE)
  
  sort(unique(store_roots[!is.na(store_roots)]))
}


#' Test which Zarr stores can be opened by zr
#'
#' Scans for stores, attempts to open each with `zr_store()` and
#' `zr_nodes()`, and reports results.
#'
#' @param path Character, root directory to scan.
#' @return A data.frame with columns:
#'   - `store`: path to the store
#'   - `name`: basename of the store directory
#'   - `ok`: logical, TRUE if `zr_nodes()` succeeded
#'   - `n_arrays`: number of array nodes found (NA if failed)
#'   - `n_groups`: number of group nodes found (NA if failed)
#'   - `version`: "v2", "v3", or "unknown" based on marker files
#'   - `error`: error message if failed, NA if ok
#' @export
zr_test_stores <- function(path) {
  stores <- zr_find_stores(path)
  if (length(stores) == 0L) {
    message("No zarr stores found in: ", path)
    return(data.frame(
      store = character(0), name = character(0),
      ok = logical(0), n_arrays = integer(0), n_groups = integer(0),
      version = character(0), error = character(0),
      stringsAsFactors = FALSE
    ))
  }
  
  results <- lapply(stores, function(s) {
    name <- basename(s)
    
    ## detect version
    has_v3 <- file.exists(file.path(s, "zarr.json"))
    has_v2 <- file.exists(file.path(s, ".zarray")) ||
      file.exists(file.path(s, ".zgroup")) ||
      file.exists(file.path(s, ".zmetadata"))
    version <- if (has_v3 && !has_v2) "v3"
    else if (has_v2 && !has_v3) "v2"
    else if (has_v2 && has_v3) "v2+v3"
    else "unknown"
    
    ## try opening
    result <- tryCatch({
      st <- zr_store(s)
      nodes <- zr_nodes(st)
      list(
        ok = TRUE,
        n_arrays = sum(nodes$node_type == "array"),
        n_groups = sum(nodes$node_type == "group"),
        error = NA_character_
      )
    }, error = function(e) {
      msg <- conditionMessage(e)
      ## strip the noisy panic prefix if present
      msg <- sub(".*panicked.*\n", "", msg)
      msg <- trimws(msg)
      list(
        ok = FALSE,
        n_arrays = NA_integer_,
        n_groups = NA_integer_,
        error = msg
      )
    })
    
    data.frame(
      store = s, name = name,
      ok = result$ok,
      n_arrays = result$n_arrays,
      n_groups = result$n_groups,
      version = version,
      error = result$error,
      stringsAsFactors = FALSE
    )
  })
  
  do.call(rbind, results)
}