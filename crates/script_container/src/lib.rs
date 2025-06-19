#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptLine {
    pub line_number: usize,
    pub name: String,
    pub time: String,
    pub text: String,
}

impl ScriptLine {
    pub fn new(line_number: usize, name: String, time: String, text: String) -> Self {
        Self {
            line_number,
            name,
            time,
            text,
        }
    }

    pub fn get_line_number(&self) -> usize {
        self.line_number
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_time(&self) -> &str {
        &self.time
    }

    pub fn get_text(&self) -> &str {
        &self.text
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptContainer {
    pub lines: Vec<ScriptLine>,
}

impl ScriptContainer {
    pub fn new() -> Self {
        Self { lines: Vec::new() }
    }

    pub fn add_line(&mut self, line: ScriptLine) {
        self.lines.push(line);
    }

    pub fn get_line(&self, index: usize) -> Option<&ScriptLine> {
        self.lines.get(index)
    }

    pub fn iter(&self) -> std::slice::Iter<'_, ScriptLine> {
        self.lines.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_script_line_creation() {
        let line = ScriptLine::new(1, "Alice".to_string(), "00:00:01".to_string(), "Hello, world!".to_string());
        assert_eq!(line.get_line_number(), 1);
        assert_eq!(line.get_name(), "Alice");
        assert_eq!(line.get_time(), "00:00:01");
        assert_eq!(line.get_text(), "Hello, world!");
    }
    #[test]
    fn test_script_container() {
        let mut container = ScriptContainer::new();
        let line1 = ScriptLine::new(1, "Alice".to_string(), "00:00:01".to_string(), "Hello, world!".to_string());
        let line2 = ScriptLine::new(2, "Bob".to_string(), "00:00:02".to_string(), "Goodbye, world!".to_string());
        container.add_line(line1);
        container.add_line(line2);
        assert_eq!(container.get_line(0).unwrap().get_text(), "Hello, world!");
        assert_eq!(container.get_line(0).unwrap().get_name(), "Alice");
        assert_eq!(container.get_line(0).unwrap().get_time(), "00:00:01");
        assert_eq!(container.get_line(1).unwrap().get_text(), "Goodbye, world!");
        assert_eq!(container.get_line(1).unwrap().get_name(), "Bob");
        assert_eq!(container.get_line(1).unwrap().get_time(), "00:00:02");
        // Test out of bounds
        assert!(container.get_line(2).is_none());
        let mut iter = container.iter();
        let first = iter.next().unwrap();
        assert_eq!(first.get_text(), "Hello, world!");
        assert_eq!(first.get_name(), "Alice");
        assert_eq!(first.get_time(), "00:00:01");
        let second = iter.next().unwrap();
        assert_eq!(second.get_text(), "Goodbye, world!");
        assert_eq!(second.get_name(), "Bob");
        assert_eq!(second.get_time(), "00:00:02");
        // Ensure we reach the end of the iterator
        assert!(iter.next().is_none());
    }
    #[test]
    fn test_script_container_empty() {
        let container = ScriptContainer::new();
        assert!(container.get_line(0).is_none());
        let mut iter = container.iter();
        assert!(iter.next().is_none()); 
    }
    #[test]
    fn test_script_line_equality() {
        let line1 = ScriptLine::new(1, "Alice".to_string(), "00:00:01".to_string(), "Hello, world!".to_string());
        let line2 = ScriptLine::new(1, "Alice".to_string(), "00:00:01".to_string(), "Hello, world!".to_string());
        let line3 = ScriptLine::new(2, "Bob".to_string(), "00:00:02".to_string(), "Goodbye, world!".to_string());
        assert_eq!(line1, line2);
        assert_ne!(line1, line3);
    }
    #[test]
    fn test_script_container_equality() {
        let mut container1 = ScriptContainer::new();
        let mut container2 = ScriptContainer::new();
        let line1 = ScriptLine::new(1, "Alice".to_string(), "00:00:01".to_string(), "Hello, world!".to_string());
        let line2 = ScriptLine::new(2, "Bob".to_string(), "00:00:02".to_string(), "Goodbye, world!".to_string());
        container1.add_line(line1.clone());
        container1.add_line(line2.clone());
        container2.add_line(line1);
        container2.add_line(line2);
        assert_eq!(container1, container2);
        let line3 = ScriptLine::new(3, "Charlie".to_string(), "00:00:03".to_string(), "Hello again!".to_string());
        container2.add_line(line3);
        assert_ne!(container1, container2);
    }
    #[test]
    fn test_script_line_empty_fields() {
        let line = ScriptLine::new(0, "".to_string(), "".to_string(), "".to_string());
        assert_eq!(line.get_line_number(), 0);
        assert_eq!(line.get_name(), "");
        assert_eq!(line.get_time(), "");
        assert_eq!(line.get_text(), "");
    }
}