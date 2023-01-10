#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

use serde::{Deserialize, Serialize};

#[cfg(not(feature = "std"))]
use alloc::string::String;

#[derive(Serialize, Deserialize, Debug)]
pub struct Person {
	pub name: String,
	pub age: u8,
}

pub fn from_json_string(json_string: &str) -> Person {
	serde_json::from_str(json_string).unwrap()
}

pub fn to_json_string(person: &Person) -> String {
	serde_json::to_string(person).unwrap()
}
