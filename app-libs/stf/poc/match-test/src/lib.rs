use serde::{Deserialize, Serialize};

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
