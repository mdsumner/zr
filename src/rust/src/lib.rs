use extendr_api::prelude::*;

/// Return string `"Hello world!"` to R.
/// @export
#[extendr]
fn hello_world() -> &'static str {
    "Hello world!"
}

/// Return
/// @export
#[extendr]
fn apic() -> &'static str {
  "abc"
}





/// Return
/// @export
#[extendr]
fn raex() -> &'static str {

use zarrs::group::GroupBuilder;
use zarrs::array::{ArrayBuilder, DataType, FillValue, ZARR_NAN_F32};
use zarrs::array::codec::GzipCodec; // requires gzip feature
use zarrs::array_subset::ArraySubset;
use zarrs::storage::ReadableWritableListableStorage;
use zarrs::filesystem::FilesystemStore; // requires filesystem feature
use std::sync::Arc;
use std::path::PathBuf;

// Create a filesystem store
let store_path: PathBuf = "/tmp/file24ddcc6d683865.zarr".into();
//let store: ReadableWritableListableStorage =
//    Arc::new(FilesystemStore::new(&store_path)?);

// Write the root group metadata
//GroupBuilder::new()
    //.build(store.clone(), "/")?
    //.attributes(...)
    //.store_metadata()?;

// Create a new V3 array using the array builder
//let array = ArrayBuilder::new(
//    vec![3, 4], // array shape
//    DataType::Float32,
//    vec![2, 2].try_into()?, // regular chunk shape (non-zero elements)
//    FillValue::from(ZARR_NAN_F32),
//)
//.bytes_to_bytes_codecs(vec![
//    Arc::new(GzipCodec::new(5)?),
//])
//.dimension_names(["y", "x"].into())
//.attributes(serde_json::json!({"Zarr V3": "is great"}).as_object().unwrap().clone())
//.build(store.clone(), "/array")?; // /path/to/hierarchy.zarr/array

// Store the array metadata
//array.store_metadata()?;
//println!("{}", serde_json::to_string_pretty(array.metadata())?);
// {
//     "zarr_format": 3,
//     "node_type": "array",
//     ...
// }

// Perform some operations on the chunks
//array.store_chunk_elements::<f32>(
//    &[0, 1], // chunk index
//    &[0.2, 0.3, 1.2, 1.3]
//)?;
//array.store_array_subset_ndarray::<f32, _>(
//    &[1, 1], // array index (start of subset)
//    ndarray::array![[-1.1, -1.2], [-2.1, -2.2]]
//)?;
//array.erase_chunk(&[1, 1])?;

// Retrieve all array elements as an ndarray
//let array_ndarray = array.retrieve_array_subset_ndarray::<f32>(&array.subset_all())?;
//println!("{array_ndarray:4}");
"something new!!"
}




// Macro to generate exports.
// This ensures exported functions are registered with R.
// See corresponding C code in `entrypoint.c`.
extendr_module! {
    mod zr;
    fn hello_world;
    fn apic;
    fn raex;
}
