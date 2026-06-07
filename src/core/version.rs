use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VersionOrdering {
    Less,
    Equal,
    Greater,
}

#[must_use]
pub fn normalize_version_string(value: &str) -> Option<String> {
    let compact = value
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    let bytes = compact.as_bytes();
    let mut index = 0usize;

    while index < bytes.len() {
        if !bytes[index].is_ascii_digit() {
            index += 1;
            continue;
        }

        let start = index;
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }

        let mut end = index;
        while index < bytes.len() && bytes[index] == b'.' {
            index += 1;
            let component_start = index;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            if component_start == index {
                break;
            }
            end = index;
        }

        return Some(compact[start..end].to_string());
    }

    None
}

#[must_use]
pub fn compare_versions(lhs: &str, rhs: &str) -> VersionOrdering {
    match compare_segments(lhs, rhs) {
        Ordering::Less => VersionOrdering::Less,
        Ordering::Equal => VersionOrdering::Equal,
        Ordering::Greater => VersionOrdering::Greater,
    }
}

pub(crate) fn compare_segments(lhs: &str, rhs: &str) -> Ordering {
    let mut lhs_segments = VersionSegments::new(lhs);
    let mut rhs_segments = VersionSegments::new(rhs);

    loop {
        let lhs_segment = lhs_segments.next();
        let rhs_segment = rhs_segments.next();
        match compare_segment(lhs_segment, rhs_segment) {
            Ordering::Equal if lhs_segment.is_some() || rhs_segment.is_some() => continue,
            ordering => return ordering,
        }
    }
}

fn compare_segment(lhs: Option<VersionSegment<'_>>, rhs: Option<VersionSegment<'_>>) -> Ordering {
    match (lhs, rhs) {
        (None, None) => Ordering::Equal,
        (Some(VersionSegment::Number(0)), None) | (None, Some(VersionSegment::Number(0))) => {
            Ordering::Equal
        }
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (Some(VersionSegment::Number(lhs)), Some(VersionSegment::Number(rhs))) => lhs.cmp(&rhs),
        (Some(VersionSegment::Text(lhs)), Some(VersionSegment::Text(rhs))) => {
            compare_ascii_case_insensitive(lhs, rhs)
        }
        (Some(VersionSegment::Number(_)), Some(VersionSegment::Text(_))) => Ordering::Greater,
        (Some(VersionSegment::Text(_)), Some(VersionSegment::Number(_))) => Ordering::Less,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VersionSegment<'a> {
    Number(u64),
    Text(&'a str),
}

struct VersionSegments<'a> {
    value: &'a str,
    cursor: usize,
}

impl<'a> VersionSegments<'a> {
    fn new(value: &'a str) -> Self {
        Self { value, cursor: 0 }
    }
}

impl<'a> Iterator for VersionSegments<'a> {
    type Item = VersionSegment<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let bytes = self.value.as_bytes();
        while self.cursor < bytes.len() && !bytes[self.cursor].is_ascii_alphanumeric() {
            self.cursor += 1;
        }

        if self.cursor >= bytes.len() {
            return None;
        }

        let start = self.cursor;
        let is_numeric = bytes[start].is_ascii_digit();
        self.cursor += 1;
        while self.cursor < bytes.len()
            && bytes[self.cursor].is_ascii_alphanumeric()
            && bytes[self.cursor].is_ascii_digit() == is_numeric
        {
            self.cursor += 1;
        }

        let segment = &self.value[start..self.cursor];
        Some(if is_numeric {
            VersionSegment::Number(parse_u64_lossy(segment))
        } else {
            VersionSegment::Text(segment)
        })
    }
}

fn parse_u64_lossy(value: &str) -> u64 {
    value
        .bytes()
        .try_fold(0u64, |accumulator, byte| {
            accumulator
                .checked_mul(10)?
                .checked_add(u64::from(byte - b'0'))
        })
        .unwrap_or(0)
}

fn compare_ascii_case_insensitive(lhs: &str, rhs: &str) -> Ordering {
    let mut lhs_bytes = lhs.bytes().map(to_ascii_lowercase);
    let mut rhs_bytes = rhs.bytes().map(to_ascii_lowercase);

    loop {
        match (lhs_bytes.next(), rhs_bytes.next()) {
            (Some(lhs), Some(rhs)) => match lhs.cmp(&rhs) {
                Ordering::Equal => continue,
                ordering => return ordering,
            },
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (None, None) => return Ordering::Equal,
        }
    }
}

const fn to_ascii_lowercase(byte: u8) -> u8 {
    if byte >= b'A' && byte <= b'Z' {
        byte + 32
    } else {
        byte
    }
}

#[cfg(test)]
mod tests {
    use super::{VersionOrdering, compare_versions, normalize_version_string};

    #[test]
    fn normalizes_scripta_style_versions() {
        assert_eq!(
            normalize_version_string("v1.2.3"),
            Some("1.2.3".to_string())
        );
        assert_eq!(
            normalize_version_string(" 1. 2 .3 "),
            Some("1.2.3".to_string())
        );
        assert_eq!(
            normalize_version_string("build 12 beta"),
            Some("12".to_string())
        );
        assert_eq!(normalize_version_string("version x"), None);
    }

    #[test]
    fn compares_versions_without_segment_allocation() {
        assert_eq!(compare_versions("1.2", "1.2.0"), VersionOrdering::Equal);
        assert_eq!(compare_versions("1.10", "1.2"), VersionOrdering::Greater);
        assert_eq!(compare_versions("1.0b", "1.0A"), VersionOrdering::Greater);
        assert_eq!(compare_versions("2.0", "2.0.1"), VersionOrdering::Less);
    }
}
