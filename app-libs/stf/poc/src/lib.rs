use match_test::{from_json_string, to_json_string};

pub fn main() {
	let json_string = r#"
        {
            "name": "John Doe",
            "age": 30
        }
    "#;

	let person = from_json_string(json_string);
	println!("{:?}", person);

	let json = to_json_string(&person);
	println!("{}", json);
}
