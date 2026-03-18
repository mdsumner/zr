use extendr_api::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;

use zarrs::array::{Array, ArrayBuilder, ArraySubset, FillValue};
use zarrs::array::data_type;
use zarrs::filesystem::FilesystemStore;
use zarrs::group::{Group, GroupBuilder};
use zarrs::storage::{
    ReadableWritableListableStorage, ReadableWritableListableStorageTraits,
    ReadableStorageTraits, ReadableListableStorageTraits,
};
use zarrs::storage::storage_adapter::async_to_sync::{
    AsyncToSyncStorageAdapter, AsyncToSyncBlockOn,
};
use zarrs_object_store::AsyncObjectStore;

// ---------------------------------------------------------------------------
// Tokio runtime
// ---------------------------------------------------------------------------

struct TokioBlockOn(tokio::runtime::Runtime);

impl AsyncToSyncBlockOn for TokioBlockOn {
    fn block_on<F: core::future::Future>(&self, future: F) -> F::Output {
        self.0.block_on(future)
    }
}

fn get_tokio_runtime() -> extendr_api::Result<tokio::runtime::Runtime> {
    tokio::runtime::Runtime::new()
        .map_err(|e| Error::Other(format!("cannot create tokio runtime: {}", e)))
}

// ---------------------------------------------------------------------------
// Store — uses ReadableWritableListableStorage for filesystem,
//         which is also readable+listable for read-only operations.
//         Remote stores get wrapped async→sync into this same type
//         where possible, or we use a read-only variant.
// ---------------------------------------------------------------------------

#[extendr]
struct ZrStore {
    inner: ReadableWritableListableStorage,
    path: String,
    writable: bool,
}

impl std::fmt::Debug for ZrStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZrStore").field("path", &self.path).finish()
    }
}

#[extendr]
impl ZrStore {
    /// Open a local filesystem store (read-write)
    fn new(path: &str) -> extendr_api::Result<Self> {
        let store_path: PathBuf = path.into();
        let store: ReadableWritableListableStorage =
            Arc::new(FilesystemStore::new(&store_path)
                .map_err(|e| Error::Other(format!("cannot open store '{}': {}", path, e)))?);
        Ok(Self { inner: store, path: path.to_string(), writable: true })
    }

    /// Open a remote HTTP store for reading zarr via simple GET requests.
    /// This works with any HTTP server — no WebDAV/PROPFIND needed.
    /// Note: listing (zr_nodes) is not supported; use zr_array() directly.
    fn new_http(url: &str) -> extendr_api::Result<Self> {
        let http_store = zarrs_http::HTTPStore::new(url)
            .map_err(|e| Error::Other(format!("cannot create HTTP store '{}': {}", url, e)))?;
        let store: ReadableWritableListableStorage = Arc::new(http_store);
        Ok(Self { inner: store, path: url.to_string(), writable: false })
    }

    /// Open an S3 store via object_store (read-only, uses env credentials)
    fn new_s3(url: &str) -> extendr_api::Result<Self> {
        let runtime = get_tokio_runtime()?;

        let os = object_store::aws::AmazonS3Builder::from_env()
            .with_url(url)
            .build()
            .map_err(|e| Error::Other(format!("cannot create S3 store '{}': {}", url, e)))?;

        let async_store = Arc::new(AsyncObjectStore::new(os));
        let block_on = TokioBlockOn(runtime);
        let sync_store: ReadableWritableListableStorage = Arc::new(
            AsyncToSyncStorageAdapter::new(async_store, block_on)
        );

        Ok(Self { inner: sync_store, path: url.to_string(), writable: false })
    }

    fn path(&self) -> &str {
        &self.path
    }

    fn is_writable(&self) -> bool {
        self.writable
    }
}

// ---------------------------------------------------------------------------
// Group
// ---------------------------------------------------------------------------

#[extendr]
struct ZrGroup {
    inner: Group<dyn ReadableWritableListableStorageTraits>,
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
        let group = Group::open(store.inner.clone(), path)
            .map_err(|e| Error::Other(format!("cannot open group '{}': {}", path, e)))?;
        Ok(Self { inner: group, path: path.to_string() })
    }

    fn create(store: &ZrStore, path: &str) -> extendr_api::Result<Self> {
        let group = GroupBuilder::new()
            .build(store.inner.clone(), path)
            .map_err(|e| Error::Other(format!("cannot create group '{}': {}", path, e)))?;
        group.store_metadata()
            .map_err(|e| Error::Other(format!("cannot store group metadata: {}", e)))?;
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
    inner: Array<dyn ReadableWritableListableStorageTraits>,
    path: String,
}

impl std::fmt::Debug for ZrArray {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZrArray").field("path", &self.path).finish()
    }
}

/// Classify a zarrs DataType into read/write families for R dispatch.
/// In zarrs 0.23, DataType is a trait object (Arc<dyn DataTypeExtension>),
/// not an enum. We compare against the known factory instances.
fn dtype_family(dt: &zarrs::array::DataType) -> &'static str {
    if *dt == data_type::float64() { return "f64"; }
    if *dt == data_type::float32() { return "f32"; }
    if *dt == data_type::int32()   { return "i32"; }
    if *dt == data_type::int16()   { return "i16"; }
    if *dt == data_type::int8()    { return "i8"; }
    if *dt == data_type::uint8()   { return "u8"; }
    if *dt == data_type::uint16()  { return "u16"; }
    if *dt == data_type::uint32()  { return "u32"; }
    if *dt == data_type::int64()   { return "i64"; }
    if *dt == data_type::uint64()  { return "u64"; }
    "f64"  // fallback: attempt f64 read
}

#[extendr]
impl ZrArray {
    fn open(store: &ZrStore, path: &str) -> extendr_api::Result<Self> {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Array::open(store.inner.clone(), path)
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

    // -- metadata ----------------------------------------------------------

    /// Shape as f64 vector (preserves full u64 range in R numeric)
    fn shape(&self) -> Vec<f64> {
        self.inner.shape().iter().map(|&x| x as f64).collect()
    }

    /// Chunk shape as f64 vector (element dimensions of a chunk, not the grid shape).
    /// Returns the shape of the origin chunk, which is the regular chunk size.
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

    // -- unified read (returns Robj so R gets the right type) --------------

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
        let family = dtype_family(self.inner.data_type());
        match family {
            "f64" | "i64" | "u64" => {
                let data: Vec<f64> = self.inner.retrieve_chunk(&idx)
                    .map_err(|e| Error::Other(format!("chunk read error: {}", e)))?;
                Ok(data.into_robj())
            }
            "f32" => {
                let data: Vec<f32> = self.inner.retrieve_chunk(&idx)
                    .map_err(|e| Error::Other(format!("chunk read error: {}", e)))?;
                let doubles: Vec<f64> = data.into_iter().map(|x| x as f64).collect();
                Ok(doubles.into_robj())
            }
            "i32" | "u32" => {
                let data: Vec<i32> = self.inner.retrieve_chunk(&idx)
                    .map_err(|e| Error::Other(format!("chunk read error: {}", e)))?;
                Ok(data.into_robj())
            }
            "i16" | "u16" => {
                let data: Vec<i16> = self.inner.retrieve_chunk(&idx)
                    .map_err(|e| Error::Other(format!("chunk read error: {}", e)))?;
                let ints: Vec<i32> = data.into_iter().map(|x| x as i32).collect();
                Ok(ints.into_robj())
            }
            "i8" | "u8" => {
                let data: Vec<u8> = self.inner.retrieve_chunk(&idx)
                    .map_err(|e| Error::Other(format!("chunk read error: {}", e)))?;
                let ints: Vec<i32> = data.into_iter().map(|x| x as i32).collect();
                Ok(ints.into_robj())
            }
            _ => {
                let data: Vec<f64> = self.inner.retrieve_chunk(&idx)
                    .map_err(|e| Error::Other(format!("chunk read error: {}", e)))?;
                Ok(data.into_robj())
            }
        }
    }

    // -- write (type-dispatched) -------------------------------------------

    fn write_subset_f64(&self, start: Vec<f64>, shape: Vec<f64>, data: Vec<f64>) -> extendr_api::Result<()> {
        let start_u64: Vec<u64> = start.iter().map(|&x| x as u64).collect();
        let shape_u64: Vec<u64> = shape.iter().map(|&x| x as u64).collect();
        let subset = ArraySubset::new_with_start_shape(start_u64, shape_u64)
            .map_err(|e| Error::Other(format!("invalid subset: {}", e)))?;
        let family = dtype_family(self.inner.data_type());
        match family {
            "f32" => {
                let data_f32: Vec<f32> = data.into_iter().map(|x| x as f32).collect();
                self.inner.store_array_subset(&subset, &data_f32)
                    .map_err(|e| Error::Other(format!("write error: {}", e)))?;
            }
            _ => {
                self.inner.store_array_subset(&subset, &data)
                    .map_err(|e| Error::Other(format!("write error: {}", e)))?;
            }
        }
        Ok(())
    }

    fn write_subset_i32(&self, start: Vec<f64>, shape: Vec<f64>, data: Vec<i32>) -> extendr_api::Result<()> {
        let start_u64: Vec<u64> = start.iter().map(|&x| x as u64).collect();
        let shape_u64: Vec<u64> = shape.iter().map(|&x| x as u64).collect();
        let subset = ArraySubset::new_with_start_shape(start_u64, shape_u64)
            .map_err(|e| Error::Other(format!("invalid subset: {}", e)))?;
        let family = dtype_family(self.inner.data_type());
        match family {
            "i16" | "u16" => {
                let data_i16: Vec<i16> = data.into_iter().map(|x| x as i16).collect();
                self.inner.store_array_subset(&subset, &data_i16)
                    .map_err(|e| Error::Other(format!("write error: {}", e)))?;
            }
            "i8" | "u8" => {
                let data_u8: Vec<u8> = data.into_iter().map(|x| x as u8).collect();
                self.inner.store_array_subset(&subset, &data_u8)
                    .map_err(|e| Error::Other(format!("write error: {}", e)))?;
            }
            _ => {
                self.inner.store_array_subset(&subset, &data)
                    .map_err(|e| Error::Other(format!("write error: {}", e)))?;
            }
        }
        Ok(())
    }

    fn write_chunk_f64(&self, chunk_index: Vec<f64>, data: Vec<f64>) -> extendr_api::Result<()> {
        let idx: Vec<u64> = chunk_index.iter().map(|&x| x as u64).collect();
        let family = dtype_family(self.inner.data_type());
        match family {
            "f32" => {
                let data_f32: Vec<f32> = data.into_iter().map(|x| x as f32).collect();
                self.inner.store_chunk(&idx, &data_f32)
                    .map_err(|e| Error::Other(format!("chunk write error: {}", e)))?;
            }
            _ => {
                self.inner.store_chunk(&idx, &data)
                    .map_err(|e| Error::Other(format!("chunk write error: {}", e)))?;
            }
        }
        Ok(())
    }

    fn write_chunk_i32(&self, chunk_index: Vec<f64>, data: Vec<i32>) -> extendr_api::Result<()> {
        let idx: Vec<u64> = chunk_index.iter().map(|&x| x as u64).collect();
        let family = dtype_family(self.inner.data_type());
        match family {
            "i16" | "u16" => {
                let data_i16: Vec<i16> = data.into_iter().map(|x| x as i16).collect();
                self.inner.store_chunk(&idx, &data_i16)
                    .map_err(|e| Error::Other(format!("chunk write error: {}", e)))?;
            }
            "i8" | "u8" => {
                let data_u8: Vec<u8> = data.into_iter().map(|x| x as u8).collect();
                self.inner.store_chunk(&idx, &data_u8)
                    .map_err(|e| Error::Other(format!("chunk write error: {}", e)))?;
            }
            _ => {
                self.inner.store_chunk(&idx, &data)
                    .map_err(|e| Error::Other(format!("chunk write error: {}", e)))?;
            }
        }
        Ok(())
    }

    fn erase_chunk(&self, chunk_index: Vec<f64>) -> extendr_api::Result<()> {
        let idx: Vec<u64> = chunk_index.iter().map(|&x| x as u64).collect();
        self.inner.erase_chunk(&idx)
            .map_err(|e| Error::Other(format!("erase chunk error: {}", e)))?;
        Ok(())
    }
}

// -- private helper for unified read ---------------------------------------

impl ZrArray {
    fn retrieve_subset_as_robj(&self, subset: &ArraySubset) -> extendr_api::Result<Robj> {
        let family = dtype_family(self.inner.data_type());
        match family {
            "f64" => {
                let data: Vec<f64> = self.inner.retrieve_array_subset(subset)
                    .map_err(|e| Error::Other(format!("read error: {}", e)))?;
                Ok(data.into_robj())
            }
            "f32" => {
                let data: Vec<f32> = self.inner.retrieve_array_subset(subset)
                    .map_err(|e| Error::Other(format!("read error: {}", e)))?;
                let doubles: Vec<f64> = data.into_iter().map(|x| x as f64).collect();
                Ok(doubles.into_robj())
            }
            "i32" | "u32" => {
                let data: Vec<i32> = self.inner.retrieve_array_subset(subset)
                    .map_err(|e| Error::Other(format!("read error: {}", e)))?;
                Ok(data.into_robj())
            }
            "i64" | "u64" => {
                // int64/uint64 → f64 (precision loss for values > 2^53)
                let data: Vec<f64> = self.inner.retrieve_array_subset(subset)
                    .map_err(|e| Error::Other(format!("read error: {}", e)))?;
                Ok(data.into_robj())
            }
            "i16" | "u16" => {
                let data: Vec<i16> = self.inner.retrieve_array_subset(subset)
                    .map_err(|e| Error::Other(format!("read error: {}", e)))?;
                let ints: Vec<i32> = data.into_iter().map(|x| x as i32).collect();
                Ok(ints.into_robj())
            }
            "i8" => {
                let data: Vec<i8> = self.inner.retrieve_array_subset(subset)
                    .map_err(|e| Error::Other(format!("read error: {}", e)))?;
                let ints: Vec<i32> = data.into_iter().map(|x| x as i32).collect();
                Ok(ints.into_robj())
            }
            "u8" => {
                let data: Vec<u8> = self.inner.retrieve_array_subset(subset)
                    .map_err(|e| Error::Other(format!("read error: {}", e)))?;
                let ints: Vec<i32> = data.into_iter().map(|x| x as i32).collect();
                Ok(ints.into_robj())
            }
            _ => {
                let data: Vec<f64> = self.inner.retrieve_array_subset(subset)
                    .map_err(|e| Error::Other(format!("read error: {}", e)))?;
                Ok(data.into_robj())
            }
        }
    }
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

    let array = builder.build(store.inner.clone(), path)
        .map_err(|e| Error::Other(format!("cannot create array '{}': {}", path, e)))?;
    array.store_metadata()
        .map_err(|e| Error::Other(format!("cannot store array metadata: {}", e)))?;

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

    let node_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Node::open(&store.inner, path)
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
    // zarrs version is reported via the zarrs crate metadata at compile time
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
    fn zr_nodes_inner;
    fn zr_zarrs_version;
}
