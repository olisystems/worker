#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

use log::info;
pub use match_test::Person;
use match_test::{self, from_json_string, to_json_string};

#[cfg(not(feature = "std"))]
use alloc::string::String;

pub fn main() -> Person {
	let json_string = r#"
        {
            "name": "John Doe",
            "age": 30
        }
    "#;

	let person = from_json_string(json_string);
	info!("{:?}", person);

	let json = to_json_string(&person);
	info!("{}", json);

	person
}
