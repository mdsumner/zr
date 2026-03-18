#' Find Zarr stores in a directory tree
#'
#' Scans for Zarr V2 and V3 stores by looking for metadata marker files:
#' `.zarray`, `.zgroup`, `.zmetadata` (V2) and `zarr.json` (V3).
#' A store root is the parent directory of the shallowest marker found.
#'
#' @param path Character, root directory to scan.
#' @return Character vector of unique store root paths.
#' @export
zr_find_stores <- function(path) {
  path <- normalizePath(path, mustWork = TRUE)
  markers <- c(".zarray", ".zgroup", ".zmetadata", "zarr.json")

  all_files <- list.files(path, recursive = TRUE, all.files = TRUE,
                          full.names = TRUE, no.. = TRUE)
  marker_files <- all_files[basename(all_files) %in% markers]

  if (length(marker_files) == 0L) return(character(0))

  rel <- sub(paste0("^", gsub("([.+*?^${}()|\\[\\]])", "\\\\\\1", path), "/?"), "", marker_files)

  store_roots <- vapply(rel, function(r) {
    parts <- strsplit(r, "/", fixed = TRUE)[[1]]
    marker_pos <- which(parts %in% markers)
    if (length(marker_pos) == 0L) return(NA_character_)
    if (marker_pos[1] == 1L) return(path)
    file.path(path, paste(parts[seq_len(marker_pos[1] - 1L)], collapse = "/"))
  }, character(1), USE.NAMES = FALSE)

  sort(unique(store_roots[!is.na(store_roots)]))
}
