#[derive(Debug)]
pub struct PropLists<T>(Vec<(String, T)>);

impl<T> PropLists<T> {
    pub fn new() -> Self {
        PropLists(Vec::new())
    }

    pub fn first(&self, key: &str) -> Option<&T> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn push(&mut self, key: &str, value: T) {
        self.0.push((key.to_string(), value));
    }
}

impl<T> IntoIterator for PropLists<T> {
    type Item = (String, T);
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    #[test]
    fn test_find_when_value_is_present() {
        let tags = &mut PropLists::new();
        tags.push("http.method", "GET".to_string());
        tags.push("http.version", "HTTP/1.1".to_string());
        let found = tags.first("http.method");
        assert_eq!(found, Some(&"GET".to_string()));
    }

    #[test]
    fn test_find_only_first_occurence_should_be_returned() {
        let tags = &mut PropLists::new();
        tags.push("http.method", "GET".to_string());
        tags.push("http.version", "HTTP/1.1".to_string());
        tags.push("http.method", "POST".to_string());
        tags.push("http.method", "PATCH".to_string());
        let found = tags.first("http.method");
        assert_eq!(found, Some(&"GET".to_string()));
    }

    #[test]
    fn test_find_when_value_is_not_present() {
        let tags = &mut PropLists::new();
        tags.push("http.method", "GET".to_string());
        tags.push("http.version", "HTTP/1.1".to_string());
        let found = tags.first("http.verb");
        assert_eq!(found, None);
    }

    #[test]
    fn test_iter() {
        let mut tags = PropLists::new();
        tags.push("http.method", "GET".to_string());
        tags.push("http.version", "HTTP/1.1".to_string());

        let mut iter = tags.into_iter();
        assert_eq!(
            Some(("http.method".to_string(), "GET".to_string())),
            iter.next()
        );
        assert_eq!(
            Some(("http.version".to_string(), "HTTP/1.1".to_string())),
            iter.next()
        );
        assert_eq!(None, iter.next());
    }
}
