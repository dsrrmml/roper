#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub fuzzy: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchMatch {
    pub start: usize,
    pub end: usize,
}

pub fn find_matches(text: &str, query: &str, options: &SearchOptions) -> Vec<SearchMatch> {
    if query.is_empty() {
        return Vec::new();
    }
    if options.fuzzy {
        fuzzy_line_matches(text, query, options.case_sensitive)
    } else {
        exact_matches(text, query, options.case_sensitive)
    }
}

fn exact_matches(text: &str, query: &str, case_sensitive: bool) -> Vec<SearchMatch> {
    let haystack = if case_sensitive {
        text.to_owned()
    } else {
        fold_case(text)
    };
    let needle = if case_sensitive {
        query.to_owned()
    } else {
        fold_case(query)
    };
    let mut matches = Vec::new();
    let mut search_start = 0;
    while let Some(byte_index) = haystack[search_start..].find(&needle) {
        let start_byte = search_start + byte_index;
        let end_byte = start_byte + needle.len();
        matches.push(SearchMatch {
            start: char_offset(&haystack, start_byte),
            end: char_offset(&haystack, end_byte),
        });
        search_start = end_byte.max(search_start + 1);
        if search_start >= haystack.len() {
            break;
        }
    }
    matches
}

fn fuzzy_line_matches(text: &str, query: &str, case_sensitive: bool) -> Vec<SearchMatch> {
    let query = if case_sensitive {
        query.to_owned()
    } else {
        fold_case(query)
    };
    let mut matches = Vec::new();
    let mut char_start = 0;
    for line in text.lines() {
        let candidate = if case_sensitive {
            line.to_owned()
        } else {
            fold_case(line)
        };
        let line_len = line.chars().count();
        if fuzzy_contains(&candidate, &query) {
            matches.push(SearchMatch {
                start: char_start,
                end: char_start + line_len,
            });
        }
        char_start += line_len + 1;
    }
    matches
}

fn fuzzy_contains(candidate: &str, query: &str) -> bool {
    let mut query_chars = query.chars();
    let Some(mut wanted) = query_chars.next() else {
        return false;
    };
    for ch in candidate.chars() {
        if ch == wanted {
            match query_chars.next() {
                Some(next) => wanted = next,
                None => return true,
            }
        }
    }
    false
}

fn fold_case(text: &str) -> String {
    text.to_uppercase().to_lowercase()
}

fn char_offset(text: &str, byte_offset: usize) -> usize {
    text[..byte_offset].chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_search_finds_all_matches() {
        let matches = find_matches(
            "bar foo\nfoo",
            "foo",
            &SearchOptions {
                case_sensitive: true,
                fuzzy: false,
            },
        );
        assert_eq!(
            matches,
            vec![
                SearchMatch { start: 4, end: 7 },
                SearchMatch { start: 8, end: 11 }
            ]
        );
    }

    #[test]
    fn case_insensitive_search_matches() {
        let matches = find_matches(
            "Straße",
            "STRASSE",
            &SearchOptions {
                case_sensitive: false,
                fuzzy: false,
            },
        );
        assert_eq!(matches, vec![SearchMatch { start: 0, end: 7 }]);
    }

    #[test]
    fn fuzzy_search_returns_line_ranges() {
        let matches = find_matches(
            "alpha beta\ncrimson flow",
            "cfl",
            &SearchOptions {
                case_sensitive: false,
                fuzzy: true,
            },
        );
        assert_eq!(matches, vec![SearchMatch { start: 11, end: 23 }]);
    }
}
