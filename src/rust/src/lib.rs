use extendr_api::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;

use zarrs::array::{Array, ArrayBuilder, ArraySubset, FillValue};
use zarrs::array::data_type;
use zarrs::filesystem::FilesystemStore;
use zarrs::group::{Group, GroupBuilder};
use zarrs::storage::{
    ReadableWritableListableStorage,
    ReadableStorage, ReadableStorageTraits,
};

#[cfg(feature = "remote")]
use zarrs::storage::storage_adapter::async_to_sync::{
    AsyncToSyncStorageAdapter, AsyncToSyncBlockOn,
};
#[cfg(feature = "remote")]
use zarrs_object_store::AsyncObjectStore;

// ---------------------------------------------------------------------------
// Tokio runtime (only with remote feature)
// ---------------------------------------------------------------------------

#[cfg(feature = "remote")]
struct TokioBlockOn(tokio::runtime::Runtime);

#[cfg(feature = "remote")]
impl AsyncToSyncBlockOn for TokioBlockOn {
    fn block_on<F: core::future::Future>(&self, future: F) -> F::Output {
        self.0.block_on(future)
    }
}

#[cfg(feature = "remote")]
fn new_tokio_runtime() -> extendr_api::Result<tokio::runtime::Runtime> {
    tokio::runtime::Runtime::new()
        .map_err(|e| Error::Other(format!("cannot create tokio runtime: {}", e)))
}

// ---------------------------------------------------------------------------
// Store
//
//   readable  — universal, all store types (Array/Group opens + reads)
//   writable  — Option, filesystem only (writes + creates)
//   listable  — bool, true for filesystem (future: S3)
// ---------------------------------------------------------------------------

#[extendr]
struct ZrStore {
    readable: ReadableStorage,
    writable: Option<ReadableWritableListableStorage>,
    listable: bool,
    path: String,
}

impl std::fmt::Debug for ZrStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZrStore").field("path", &self.path).finish()
    }
}

#[extendr]
impl ZrStore {
    /// Open a local filesystem store (read-write-listable)
    fn new(path: &str) -> extendr_api::Result<Self> {
        let store_path: PathBuf = path.into();
        let store: ReadableWritableListableStorage =
            Arc::new(FilesystemStore::new(&store_path)
                .map_err(|e| Error::Other(format!("cannot open store '{}': {}", path, e)))?);
        Ok(Self {
            readable: store.clone() as ReadableStorage,
            listable: true,
            writable: Some(store),
            path: path.to_string(),
        })
    }

    /// Open a remote HTTP store (read-only) via object_store.
    /// Note: object_store's HTTP backend uses WebDAV PROPFIND for listing,
    /// which most servers don't support. Use zr_array() directly with known paths.
    fn new_http(url: &str) -> extendr_api::Result<Self> {
        #[cfg(feature = "remote")]
        {
            let runtime = new_tokio_runtime()?;

            let os = object_store::http::HttpBuilder::new()
                .with_url(url)
                .build()
                .map_err(|e| Error::Other(format!("cannot create HTTP store '{}': {}", url, e)))?;

            let async_store = Arc::new(AsyncObjectStore::new(os));
            let block_on = TokioBlockOn(runtime);
            let sync_store = Arc::new(
                AsyncToSyncStorageAdapter::new(async_store, block_on)
            );

            Ok(Self {
                readable: sync_store.clone() as ReadableStorage,
                writable: None,
                listable: false,
                path: url.to_string(),
            })
        }
        #[cfg(not(feature = "remote"))]
        {
            let _ = url;
            Err(Error::Other("HTTP stores require the 'remote' feature (recompile with ZR_FEATURES=remote)".to_string()))
        }
    }

    /// Open an S3 store via object_store (read-only, env credentials).
    fn new_s3(url: &str) -> extendr_api::Result<Self> {
        #[cfg(feature = "remote")]
        {
            let runtime = new_tokio_runtime()?;

            let os = object_store::aws::AmazonS3Builder::from_env()
                .with_url(url)
                .build()
                .map_err(|e| Error::Other(format!("cannot create S3 store '{}': {}", url, e)))?;

            let async_store = Arc::new(AsyncObjectStore::new(os));
            let block_on = TokioBlockOn(runtime);
            let sync_store = Arc::new(
                AsyncToSyncStorageAdapter::new(async_store, block_on)
            );

            Ok(Self {
                readable: sync_store.clone() as ReadableStorage,
                writable: None,
                listable: false,
                path: url.to_string(),
            })
        }
        #[cfg(not(feature = "remote"))]
        {
            let _ = url;
            Err(Error::Other("S3 stores require the 'remote' feature (recompile with ZR_FEATURES=remote)".to_string()))
        }
    }

    fn path(&self) -> &str {
        &self.path
    }

    fn is_writable(&self) -> bool {
        self.writable.is_some()
    }

    fn is_listable(&self) -> bool {
        self.listable
    }
}

// ---------------------------------------------------------------------------
// Group
// ---------------------------------------------------------------------------

#[extendr]
struct ZrGroup {
    inner: Group<dyn ReadableStorageTraits>,
    path: String,
}

impl std::fmt::Debug for ZrGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZrGroup").field("path", &self.path).finish()
    }
}

#[extendr]
impl ZrGroup {
    fn open(store: &ZrStore, path: &str) -> extendr_api::Result<Self> {
        let group = Group::open(store.readable.clone(), path)
            .map_err(|e| Error::Other(format!("cannot open group '{}': {}", path, e)))?;
        Ok(Self { inner: group, path: path.to_string() })
    }

    fn create(store: &ZrStore, path: &str) -> extendr_api::Result<Self> {
        let ws = store.writable.as_ref()
            .ok_or_else(|| Error::Other("store is read-only, cannot create group".to_string()))?;
        let group = GroupBuilder::new()
            .build(ws.clone(), path)
            .map_err(|e| Error::Other(format!("cannot create group '{}': {}", path, e)))?;
        group.store_metadata()
            .map_err(|e| Error::Other(format!("cannot store group metadata: {}", e)))?;
        let group = Group::open(store.readable.clone(), path)
            .map_err(|e| Error::Other(format!("cannot reopen group '{}': {}", path, e)))?;
        Ok(Self { inner: group, path: path.to_string() })
    }

    fn attributes_json(&self) -> String {
        serde_json::to_string(self.inner.attributes()).unwrap_or_else(|_| "{}".to_string())
    }

    fn group_path(&self) -> &str {
        &self.path
    }
}

// ---------------------------------------------------------------------------
// Array
// ---------------------------------------------------------------------------

#[extendr]
struct ZrArray {
    inner: Array<dyn ReadableStorageTraits>,
    path: String,
}

impl std::fmt::Debug for ZrArray {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZrArray").field("path", &self.path).finish()
    }
}

/// Classify a zarrs DataType for R type dispatch.
/// Returns None for unsupported types (string, vlen, compound, etc.)
fn dtype_family(dt: &zarrs::array::DataType) -> Option<&'static str> {
    if *dt == data_type::float64() { return Some("f64"); }
    if *dt == data_type::float32() { return Some("f32"); }
    if *dt == data_type::int32()   { return Some("i32"); }
    if *dt == data_type::int16()   { return Some("i16"); }
    if *dt == data_type::int8()    { return Some("i8"); }
    if *dt == data_type::uint8()   { return Some("u8"); }
    if *dt == data_type::uint16()  { return Some("u16"); }
    if *dt == data_type::uint32()  { return Some("u32"); }
    if *dt == data_type::int64()   { return Some("i64"); }
    if *dt == data_type::uint64()  { return Some("u64"); }
    None
}

#[extendr]
impl ZrArray {
    fn open(store: &ZrStore, path: &str) -> extendr_api::Result<Self> {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Array::open(store.readable.clone(), path)
        }));
        let array = match result {
            Ok(Ok(a)) => a,
            Ok(Err(e)) => return Err(Error::Other(format!("cannot open array '{}': {}", path, e))),
            Err(panic) => {
                let msg = if let Some(s) = panic.downcast_ref::<String>() { s.clone() }
                    else if let Some(s) = panic.downcast_ref::<&str>() { s.to_string() }
                    else { "unknown panic".to_string() };
                return Err(Error::Other(format!("zarrs panic opening array '{}': {}", path, msg)));
            }
        };
        Ok(Self { inner: array, path: path.to_string() })
    }

    fn shape(&self) -> Vec<f64> {
        self.inner.shape().iter().map(|&x| x as f64).collect()
    }

    fn chunk_shape(&self) -> extendr_api::Result<Vec<f64>> {
        let ndim = self.inner.dimensionality();
        let origin: Vec<u64> = vec![0; ndim];
        let cs = self.inner.chunk_shape(&origin)
            .map_err(|e| Error::Other(format!("cannot get chunk shape: {}", e)))?;
        Ok(cs.iter().map(|&x| x.get() as f64).collect())
    }

    fn dtype(&self) -> String {
        format!("{}", self.inner.data_type())
    }

    fn fill_value_json(&self) -> String {
        format!("{:?}", self.inner.fill_value())
    }

    fn dimension_names(&self) -> Robj {
        match self.inner.dimension_names() {
            Some(names) => {
                let strs: Vec<String> = names
                    .iter()
                    .map(|n| n.clone().unwrap_or_default())
                    .collect();
                strs.into_robj()
            }
            None => ().into_robj(),
        }
    }

    fn attributes_json(&self) -> String {
        serde_json::to_string(self.inner.attributes()).unwrap_or_else(|_| "{}".to_string())
    }

    fn metadata_json(&self) -> String {
        serde_json::to_string_pretty(self.inner.metadata()).unwrap_or_else(|_| "{}".to_string())
    }

    fn ndim(&self) -> i32 {
        self.inner.dimensionality() as i32
    }

    fn array_path(&self) -> &str {
        &self.path
    }

    // -- read --------------------------------------------------------------

    fn read_subset_robj(&self, start: Vec<f64>, shape: Vec<f64>) -> extendr_api::Result<Robj> {
        let start_u64: Vec<u64> = start.iter().map(|&x| x as u64).collect();
        let shape_u64: Vec<u64> = shape.iter().map(|&x| x as u64).collect();
        let subset = ArraySubset::new_with_start_shape(start_u64, shape_u64)
            .map_err(|e| Error::Other(format!("invalid subset: {}", e)))?;
        self.retrieve_subset_as_robj(&subset)
    }

    fn read_all_robj(&self) -> extendr_api::Result<Robj> {
        let subset = self.inner.subset_all();
        self.retrieve_subset_as_robj(&subset)
    }

    fn read_chunk_robj(&self, chunk_index: Vec<f64>) -> extendr_api::Result<Robj> {
        let idx: Vec<u64> = chunk_index.iter().map(|&x| x as u64).collect();
        let family = dtype_family(self.inner.data_type())
            .ok_or_else(|| Error::Other(format!(
                "unsupported data type: {}", self.inner.data_type()
            )))?;

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> extendr_api::Result<Robj> {
            read_chunk_typed(&self.inner, &idx, family)
        }));

        match result {
            Ok(r) => r,
            Err(_) => Err(Error::Other(format!(
                "zarrs panic reading chunk from '{}'", self.path
            ))),
        }
    }
}

// -- private helpers for unified read --------------------------------------

fn read_chunk_typed(
    array: &Array<dyn ReadableStorageTraits>,
    idx: &[u64],
    family: &str,
) -> extendr_api::Result<Robj> {
    match family {
        "f64" => {
            let data: Vec<f64> = array.retrieve_chunk(idx)
                .map_err(|e| Error::Other(format!("chunk read error: {}", e)))?;
            Ok(data.into_robj())
        }
        "f32" => {
            let data: Vec<f32> = array.retrieve_chunk(idx)
                .map_err(|e| Error::Other(format!("chunk read error: {}", e)))?;
            Ok(data.into_iter().map(|x| x as f64).collect::<Vec<f64>>().into_robj())
        }
        "i32" => {
            let data: Vec<i32> = array.retrieve_chunk(idx)
                .map_err(|e| Error::Other(format!("chunk read error: {}", e)))?;
            Ok(data.into_robj())
        }
        "u32" => {
            let data: Vec<u32> = array.retrieve_chunk(idx)
                .map_err(|e| Error::Other(format!("chunk read error: {}", e)))?;
            Ok(data.into_iter().map(|x| x as f64).collect::<Vec<f64>>().into_robj())
        }
        "i64" | "u64" => {
            let data: Vec<f64> = array.retrieve_chunk(idx)
                .map_err(|e| Error::Other(format!("chunk read error: {}", e)))?;
            Ok(data.into_robj())
        }
        "i16" => {
            let data: Vec<i16> = array.retrieve_chunk(idx)
                .map_err(|e| Error::Other(format!("chunk read error: {}", e)))?;
            Ok(data.into_iter().map(|x| x as i32).collect::<Vec<i32>>().into_robj())
        }
        "u16" => {
            let data: Vec<u16> = array.retrieve_chunk(idx)
                .map_err(|e| Error::Other(format!("chunk read error: {}", e)))?;
            Ok(data.into_iter().map(|x| x as i32).collect::<Vec<i32>>().into_robj())
        }
        "i8" => {
            let data: Vec<i8> = array.retrieve_chunk(idx)
                .map_err(|e| Error::Other(format!("chunk read error: {}", e)))?;
            Ok(data.into_iter().map(|x| x as i32).collect::<Vec<i32>>().into_robj())
        }
        "u8" => {
            let data: Vec<u8> = array.retrieve_chunk(idx)
                .map_err(|e| Error::Other(format!("chunk read error: {}", e)))?;
            Ok(data.into_iter().map(|x| x as i32).collect::<Vec<i32>>().into_robj())
        }
        _ => unreachable!(),
    }
}

impl ZrArray {
    fn retrieve_subset_as_robj(&self, subset: &ArraySubset) -> extendr_api::Result<Robj> {
        let family = dtype_family(self.inner.data_type())
            .ok_or_else(|| Error::Other(format!(
                "unsupported data type: {}", self.inner.data_type()
            )))?;

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> extendr_api::Result<Robj> {
            match family {
                "f64" => {
                    let data: Vec<f64> = self.inner.retrieve_array_subset(subset)
                        .map_err(|e| Error::Other(format!("read error: {}", e)))?;
                    Ok(data.into_robj())
                }
                "f32" => {
                    let data: Vec<f32> = self.inner.retrieve_array_subset(subset)
                        .map_err(|e| Error::Other(format!("read error: {}", e)))?;
                    Ok(data.into_iter().map(|x| x as f64).collect::<Vec<f64>>().into_robj())
                }
                "i32" => {
                    let data: Vec<i32> = self.inner.retrieve_array_subset(subset)
                        .map_err(|e| Error::Other(format!("read error: {}", e)))?;
                    Ok(data.into_robj())
                }
                "u32" => {
                    let data: Vec<u32> = self.inner.retrieve_array_subset(subset)
                        .map_err(|e| Error::Other(format!("read error: {}", e)))?;
                    Ok(data.into_iter().map(|x| x as f64).collect::<Vec<f64>>().into_robj())
                }
                "i64" | "u64" => {
                    let data: Vec<f64> = self.inner.retrieve_array_subset(subset)
                        .map_err(|e| Error::Other(format!("read error: {}", e)))?;
                    Ok(data.into_robj())
                }
                "i16" => {
                    let data: Vec<i16> = self.inner.retrieve_array_subset(subset)
                        .map_err(|e| Error::Other(format!("read error: {}", e)))?;
                    Ok(data.into_iter().map(|x| x as i32).collect::<Vec<i32>>().into_robj())
                }
                "u16" => {
                    let data: Vec<u16> = self.inner.retrieve_array_subset(subset)
                        .map_err(|e| Error::Other(format!("read error: {}", e)))?;
                    Ok(data.into_iter().map(|x| x as i32).collect::<Vec<i32>>().into_robj())
                }
                "i8" => {
                    let data: Vec<i8> = self.inner.retrieve_array_subset(subset)
                        .map_err(|e| Error::Other(format!("read error: {}", e)))?;
                    Ok(data.into_iter().map(|x| x as i32).collect::<Vec<i32>>().into_robj())
                }
                "u8" => {
                    let data: Vec<u8> = self.inner.retrieve_array_subset(subset)
                        .map_err(|e| Error::Other(format!("read error: {}", e)))?;
                    Ok(data.into_iter().map(|x| x as i32).collect::<Vec<i32>>().into_robj())
                }
                _ => unreachable!(),
            }
        }));

        match result {
            Ok(r) => r,
            Err(_) => Err(Error::Other(format!(
                "zarrs panic reading from '{}'", self.path
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Write — free functions taking &ZrStore + &ZrArray
//
// ZrArray holds Array<dyn ReadableStorageTraits> — no store_* methods.
// We open a fresh Array from store.writable which gives
// Array<dyn ReadableWritableListableStorageTraits> with write access.
// ---------------------------------------------------------------------------

/// @export
#[extendr]
fn zr_write_subset_inner(
    store: &ZrStore,
    arr: &ZrArray,
    start: Vec<f64>,
    shape: Vec<f64>,
    data_f64: Nullable<Vec<f64>>,
    data_i32: Nullable<Vec<i32>>,
) -> extendr_api::Result<()> {
    let ws = store.writable.as_ref()
        .ok_or_else(|| Error::Other("store is read-only, cannot write".to_string()))?;

    let start_u64: Vec<u64> = start.iter().map(|&x| x as u64).collect();
    let shape_u64: Vec<u64> = shape.iter().map(|&x| x as u64).collect();
    let subset = ArraySubset::new_with_start_shape(start_u64, shape_u64)
        .map_err(|e| Error::Other(format!("invalid subset: {}", e)))?;

    let array = Array::open(ws.clone(), &arr.path)
        .map_err(|e| Error::Other(format!("cannot open array '{}' for writing: {}", arr.path, e)))?;

    let family = dtype_family(array.data_type())
        .ok_or_else(|| Error::Other(format!(
            "unsupported data type for writing: {}", array.data_type()
        )))?;

    match (data_f64, data_i32) {
        (NotNull(data), _) => {
            match family {
                "f32" => {
                    let d: Vec<f32> = data.into_iter().map(|x| x as f32).collect();
                    array.store_array_subset(&subset, &d)
                        .map_err(|e| Error::Other(format!("write error: {}", e)))?;
                }
                "i32" => {
                    let d: Vec<i32> = data.into_iter().map(|x| x as i32).collect();
                    array.store_array_subset(&subset, &d)
                        .map_err(|e| Error::Other(format!("write error: {}", e)))?;
                }
                "i16" => {
                    let d: Vec<i16> = data.into_iter().map(|x| x as i16).collect();
                    array.store_array_subset(&subset, &d)
                        .map_err(|e| Error::Other(format!("write error: {}", e)))?;
                }
                "i8" => {
                    let d: Vec<i8> = data.into_iter().map(|x| x as i8).collect();
                    array.store_array_subset(&subset, &d)
                        .map_err(|e| Error::Other(format!("write error: {}", e)))?;
                }
                "u8" => {
                    let d: Vec<u8> = data.into_iter().map(|x| x as u8).collect();
                    array.store_array_subset(&subset, &d)
                        .map_err(|e| Error::Other(format!("write error: {}", e)))?;
                }
                "u16" => {
                    let d: Vec<u16> = data.into_iter().map(|x| x as u16).collect();
                    array.store_array_subset(&subset, &d)
                        .map_err(|e| Error::Other(format!("write error: {}", e)))?;
                }
                "u32" => {
                    let d: Vec<u32> = data.into_iter().map(|x| x as u32).collect();
                    array.store_array_subset(&subset, &d)
                        .map_err(|e| Error::Other(format!("write error: {}", e)))?;
                }
                "i64" => {
                    let d: Vec<i64> = data.into_iter().map(|x| x as i64).collect();
                    array.store_array_subset(&subset, &d)
                        .map_err(|e| Error::Other(format!("write error: {}", e)))?;
                }
                "u64" => {
                    let d: Vec<u64> = data.into_iter().map(|x| x as u64).collect();
                    array.store_array_subset(&subset, &d)
                        .map_err(|e| Error::Other(format!("write error: {}", e)))?;
                }
                _ => {
                    // f64 default
                    array.store_array_subset(&subset, &data)
                        .map_err(|e| Error::Other(format!("write error: {}", e)))?;
                }
            }
        }
        (_, NotNull(data)) => {
            match family {
                "i16" | "u16" => {
                    let d: Vec<i16> = data.into_iter().map(|x| x as i16).collect();
                    array.store_array_subset(&subset, &d)
                        .map_err(|e| Error::Other(format!("write error: {}", e)))?;
                }
                "i8" => {
                    let d: Vec<i8> = data.into_iter().map(|x| x as i8).collect();
                    array.store_array_subset(&subset, &d)
                        .map_err(|e| Error::Other(format!("write error: {}", e)))?;
                }
                "u8" => {
                    let d: Vec<u8> = data.into_iter().map(|x| x as u8).collect();
                    array.store_array_subset(&subset, &d)
                        .map_err(|e| Error::Other(format!("write error: {}", e)))?;
                }
                _ => {
                    array.store_array_subset(&subset, &data)
                        .map_err(|e| Error::Other(format!("write error: {}", e)))?;
                }
            }
        }
        _ => {
            return Err(Error::Other("zr_write: no data provided".to_string()));
        }
    }
    Ok(())
}

/// @export
#[extendr]
fn zr_write_chunk_inner(
    store: &ZrStore,
    arr: &ZrArray,
    chunk_index: Vec<f64>,
    data_f64: Nullable<Vec<f64>>,
    data_i32: Nullable<Vec<i32>>,
) -> extendr_api::Result<()> {
    let ws = store.writable.as_ref()
        .ok_or_else(|| Error::Other("store is read-only, cannot write chunk".to_string()))?;

    let idx: Vec<u64> = chunk_index.iter().map(|&x| x as u64).collect();

    let array = Array::open(ws.clone(), &arr.path)
        .map_err(|e| Error::Other(format!("cannot open array '{}' for writing: {}", arr.path, e)))?;

    let family = dtype_family(array.data_type())
        .ok_or_else(|| Error::Other(format!(
            "unsupported data type for writing: {}", array.data_type()
        )))?;

    match (data_f64, data_i32) {
        (NotNull(data), _) => {
            match family {
                "f32" => {
                    let d: Vec<f32> = data.into_iter().map(|x| x as f32).collect();
                    array.store_chunk(&idx, &d)
                        .map_err(|e| Error::Other(format!("chunk write error: {}", e)))?;
                }
                _ => {
                    array.store_chunk(&idx, &data)
                        .map_err(|e| Error::Other(format!("chunk write error: {}", e)))?;
                }
            }
        }
        (_, NotNull(data)) => {
            match family {
                "i16" | "u16" => {
                    let d: Vec<i16> = data.into_iter().map(|x| x as i16).collect();
                    array.store_chunk(&idx, &d)
                        .map_err(|e| Error::Other(format!("chunk write error: {}", e)))?;
                }
                "i8" => {
                    let d: Vec<i8> = data.into_iter().map(|x| x as i8).collect();
                    array.store_chunk(&idx, &d)
                        .map_err(|e| Error::Other(format!("chunk write error: {}", e)))?;
                }
                "u8" => {
                    let d: Vec<u8> = data.into_iter().map(|x| x as u8).collect();
                    array.store_chunk(&idx, &d)
                        .map_err(|e| Error::Other(format!("chunk write error: {}", e)))?;
                }
                _ => {
                    array.store_chunk(&idx, &data)
                        .map_err(|e| Error::Other(format!("chunk write error: {}", e)))?;
                }
            }
        }
        _ => {
            return Err(Error::Other("zr_write_chunk: no data provided".to_string()));
        }
    }
    Ok(())
}

/// @export
#[extendr]
fn zr_erase_chunk_inner(
    store: &ZrStore,
    arr: &ZrArray,
    chunk_index: Vec<f64>,
) -> extendr_api::Result<()> {
    let ws = store.writable.as_ref()
        .ok_or_else(|| Error::Other("store is read-only, cannot erase chunk".to_string()))?;

    let idx: Vec<u64> = chunk_index.iter().map(|&x| x as u64).collect();

    let array = Array::open(ws.clone(), &arr.path)
        .map_err(|e| Error::Other(format!("cannot open array '{}' for erase: {}", arr.path, e)))?;

    array.erase_chunk(&idx)
        .map_err(|e| Error::Other(format!("erase chunk error: {}", e)))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Array creation
// ---------------------------------------------------------------------------

/// @export
#[extendr]
fn zr_create_array_inner(
    store: &ZrStore,
    path: &str,
    shape: Vec<f64>,
    chunks: Vec<f64>,
    dtype: &str,
    fill_value: f64,
    dimension_names: Nullable<Vec<String>>,
    attributes_json: Nullable<String>,
) -> extendr_api::Result<ZrArray> {
    let shape_u64: Vec<u64> = shape.iter().map(|&x| x as u64).collect();
    let chunks_u64: Vec<u64> = chunks.iter().map(|&x| x as u64).collect();

    let (dt, fv): (zarrs::array::DataType, FillValue) = match dtype {
        "float32" => (data_type::float32(), (fill_value as f32).into()),
        "float64" => (data_type::float64(), fill_value.into()),
        "int8"    => (data_type::int8(),    (fill_value as i8).into()),
        "int16"   => (data_type::int16(),   (fill_value as i16).into()),
        "int32"   => (data_type::int32(),   (fill_value as i32).into()),
        "int64"   => (data_type::int64(),   (fill_value as i64).into()),
        "uint8"   => (data_type::uint8(),   (fill_value as u8).into()),
        "uint16"  => (data_type::uint16(),  (fill_value as u16).into()),
        "uint32"  => (data_type::uint32(),  (fill_value as u32).into()),
        "uint64"  => (data_type::uint64(),  (fill_value as u64).into()),
        _ => return Err(Error::Other(format!("unsupported dtype: {dtype}"))),
    };

    let mut builder = ArrayBuilder::new(shape_u64, chunks_u64, dt, fv);

    if let NotNull(names) = dimension_names {
        let dim_names: Vec<zarrs::array::DimensionName> =
            names.into_iter().map(|n| Some(n)).collect();
        builder.dimension_names(Some(dim_names));
    }

    if let NotNull(json) = attributes_json {
        if let Ok(attrs) =
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&json)
        {
            builder.attributes(attrs);
        }
    }

    let ws = store.writable.as_ref()
        .ok_or_else(|| Error::Other("store is read-only, cannot create array".to_string()))?;
    let array = builder.build(ws.clone(), path)
        .map_err(|e| Error::Other(format!("cannot create array '{}': {}", path, e)))?;
    array.store_metadata()
        .map_err(|e| Error::Other(format!("cannot store array metadata: {}", e)))?;
    let array = Array::open(store.readable.clone(), path)
        .map_err(|e| Error::Other(format!("cannot reopen array '{}': {}", path, e)))?;

    Ok(ZrArray { inner: array, path: path.to_string() })
}

// ---------------------------------------------------------------------------
// Node listing
// ---------------------------------------------------------------------------

/// @export
#[extendr]
fn zr_nodes_inner(store: &ZrStore, path: &str) -> List {
    use zarrs::node::Node;
    use zarrs::node::NodeMetadata;

    let ws = match store.writable.as_ref() {
        Some(s) => s,
        None => {
            throw_r_error("store does not support listing (remote stores cannot list, use zr_array() directly)");
        }
    };

    let node_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Node::open(ws, path)
    }));

    let node = match node_result {
        Ok(Ok(n)) => n,
        Ok(Err(e)) => {
            throw_r_error(format!("cannot open node '{}': {}", path, e));
        }
        Err(_) => {
            throw_r_error(format!("zarrs internal error opening '{}'", path));
        }
    };

    let children_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        node.children()
    }));

    let children = match children_result {
        Ok(c) => c,
        Err(_) => {
            throw_r_error(format!("zarrs internal error listing children of '{}'", path));
        }
    };

    let mut paths: Vec<String> = Vec::new();
    let mut types: Vec<String> = Vec::new();

    for child in children.iter() {
        paths.push(child.path().as_str().to_string());
        let ntype = match child.metadata() {
            NodeMetadata::Array(_) => "array",
            NodeMetadata::Group(_) => "group",
        };
        types.push(ntype.to_string());
    }

    list!(path = paths, node_type = types)
}

// ---------------------------------------------------------------------------
// Version
// ---------------------------------------------------------------------------

/// @export
#[extendr]
fn zr_zarrs_version() -> String {
    format!("zr {}", env!("CARGO_PKG_VERSION"))
}

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

extendr_module! {
    mod zr;
    impl ZrStore;
    impl ZrGroup;
    impl ZrArray;
    fn zr_create_array_inner;
    fn zr_write_subset_inner;
    fn zr_write_chunk_inner;
    fn zr_erase_chunk_inner;
    fn zr_nodes_inner;
    fn zr_zarrs_version;
}
