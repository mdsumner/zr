#' @export
format.ZrStore <- function(x, ...) {
  paste0("<ZrStore> ", x$path())
}

#' @export
print.ZrStore <- function(x, ...) {
  cat(format(x, ...), "\n")
  invisible(x)
}

#' @export
format.ZrArray <- function(x, ...) {
  shp <- paste(x$shape(), collapse = " x ")
  paste0("<ZrArray> ", x$array_path(), "  [", shp, "]  ", x$dtype())
}

#' @export
print.ZrArray <- function(x, ...) {
  cat(format(x, ...), "\n")
  dn <- x$dimension_names()
  if (!is.null(dn) && length(dn) > 0) {
    cat("  dims:", paste(dn, collapse = ", "), "\n")
  }
  cat("  chunks:", paste(x$chunk_shape(), collapse = " x "), "\n")
  invisible(x)
}

#' @export
format.ZrGroup <- function(x, ...) {
  paste0("<ZrGroup> ", x$group_path())
}

#' @export
print.ZrGroup <- function(x, ...) {
  cat(format(x, ...), "\n")
  invisible(x)
}
