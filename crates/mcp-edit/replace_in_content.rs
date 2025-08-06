use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum ReplacementError {
    NotFound,
    UnexpectedCount { expected: usize, found: usize },
}

impl fmt::Display for ReplacementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            ReplacementError::NotFound => write!(f, "no occurrences of the target string were found"),
            ReplacementError::UnexpectedCount { expected, found } => write!(
                f,
                "expected to replace {} occurrence(s), but found {}",
                expected, found
            ),
        }
    }
}

impl Error for ReplacementError {}

pub fn replace_in_content(
    content: &str,
    old: &str,
    new: &str,
    expected_replacements: Option<usize>,
) -> Result<String, ReplacementError> {
    let expected = expected_replacements.unwrap_or(1);
    let found = content.matches(old).count();

    if found == 0 {
        return Err(ReplacementError::NotFound);
    }
    if found != expected {
        return Err(ReplacementError::UnexpectedCount { expected, found });
    }

    let updated = if expected == 1 {
        content.replacen(old, new, 1)
    } else {
        content.replace(old, new)
    };

    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_replace_ok() {
        let text = "foo bar baz";
        let result = replace_in_content(text, "bar", "qux", None).unwrap();
        assert_eq!(result, "foo qux baz");
    }

    #[test]
    fn single_replace_not_found() {
        let text = "foo bar baz";
        assert!(matches!(
            replace_in_content(text, "xyz", "qux", None),
            Err(ReplacementError::NotFound)
        ));
    }

    #[test]
    fn single_replace_multiple_found() {
        let text = "repeat repeat";
        assert!(matches!(
            replace_in_content(text, "repeat", "once", None),
            Err(ReplacementError::UnexpectedCount { expected: 1, found: 2 })
        ));
    }

    #[test]
    fn multi_replace_ok() {
        let text = "a a a";
        let result = replace_in_content(text, "a", "b", Some(3)).unwrap();
        assert_eq!(result, "b b b");
    }

    #[test]
    fn multi_replace_wrong_count() {
        let text = "x x";
        assert!(matches!(
            replace_in_content(text, "x", "y", Some(3)),
            Err(ReplacementError::UnexpectedCount { expected: 3, found: 2 })
        ));
    }
}
