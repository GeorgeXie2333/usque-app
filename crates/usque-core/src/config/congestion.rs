use serde::{Deserialize, Serialize};

/// Client-side HTTP/3 congestion control. HTTP/2 uses the system TCP stack.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CongestionControlAlgorithm {
    #[default]
    Cubic,
    Reno,
    /// quiche's BBRv2 implementation, matching its `bbr` name.
    Bbr,
    Bbr3,
}

impl CongestionControlAlgorithm {
    pub const ALL: [Self; 4] = [Self::Cubic, Self::Reno, Self::Bbr, Self::Bbr3];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cubic => "cubic",
            Self::Reno => "reno",
            Self::Bbr => "bbr",
            Self::Bbr3 => "bbr3",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_exact_and_round_trip() {
        for algorithm in CongestionControlAlgorithm::ALL {
            let json = serde_json::to_string(&algorithm).unwrap();
            assert_eq!(json, format!("\"{}\"", algorithm.as_str()));
            assert_eq!(
                serde_json::from_str::<CongestionControlAlgorithm>(&json).unwrap(),
                algorithm
            );
        }
        for json in ["\"bbr2\"", "\"BBR3\"", "\"unknown\"", "null", "4"] {
            assert!(serde_json::from_str::<CongestionControlAlgorithm>(json).is_err());
        }
    }
}
