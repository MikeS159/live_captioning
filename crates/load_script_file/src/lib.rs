use std::fs::File;
use std::io::{self, Read};
use script_container::ScriptContainer;

fn load_from_reader<R: Read>(mut reader: R) -> io::Result<String> {
    let mut contents = String::new();
    reader.read_to_string(&mut contents)?;
    Ok(contents)
}

// For real file loading
pub fn load_file(path: &str) -> io::Result<String> {
    let file = File::open(path)?;
    load_from_reader(file)
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::io::{Write, Cursor};
    use super::*;

    #[test]
    fn test_load_from_reader() {
        let data = "mock file contents";
        let cursor = Cursor::new(data);
        let result = load_from_reader(cursor).unwrap();
        assert_eq!(result, data);
    }

        #[test]
    fn test_load_file() {
        // Create a temporary file
        let tmp_dir = env::temp_dir();
        let file_path = tmp_dir.join("test_load_file.txt");
        let test_content = "hello, world!";

        // Write test content to the file
        {
            let mut file = File::create(&file_path).unwrap();
            write!(file, "{}", test_content).unwrap();
        }

        // Test the load_file function
        let loaded = load_file(file_path.to_str().unwrap()).unwrap();
        assert_eq!(loaded, test_content);

        // Clean up
        std::fs::remove_file(file_path).unwrap();
    }
}
