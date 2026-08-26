use std::fmt;

use crate::error::GeoError;

/// ISO 3166-1 alpha-2 country code, stored uppercase.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CountryCode(String);

impl CountryCode {
    pub fn parse(value: &str) -> Result<Self, GeoError> {
        let trimmed = value.trim();
        let upper = trimmed.to_ascii_uppercase();
        if upper.len() != 2 || !upper.bytes().all(|byte| byte.is_ascii_uppercase()) {
            return Err(GeoError::InvalidCountryCode(value.to_owned()));
        }
        Ok(Self(upper))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_lower(&self) -> String {
        self.0.to_ascii_lowercase()
    }
}

impl fmt::Display for CountryCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for CountryCode {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl TryFrom<&str> for CountryCode {
    type Error = GeoError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

#[cfg(test)]
mod tests {
    use super::CountryCode;

    #[test]
    fn normalizes_trim_and_case() {
        assert_eq!(CountryCode::parse(" cn ").unwrap().as_str(), "CN");
        assert_eq!(CountryCode::parse("Us").unwrap().as_lower(), "us");
    }

    #[test]
    fn rejects_non_alpha2() {
        for value in ["", "C", "CN1", "c n", "C1", "中N", "cn-"] {
            assert!(
                CountryCode::parse(value).is_err(),
                "expected {value:?} to be rejected"
            );
        }
    }
}
