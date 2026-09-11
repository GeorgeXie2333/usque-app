use usque_core::CongestionControlAlgorithm;
use usque_ipc::v1;

use crate::ControlServiceError;

pub(crate) fn to_proto(algorithm: CongestionControlAlgorithm) -> i32 {
    match algorithm {
        CongestionControlAlgorithm::Cubic => v1::CongestionControlAlgorithm::Cubic as i32,
        CongestionControlAlgorithm::Reno => v1::CongestionControlAlgorithm::Reno as i32,
        CongestionControlAlgorithm::Bbr => v1::CongestionControlAlgorithm::Bbr as i32,
        CongestionControlAlgorithm::Bbr3 => v1::CongestionControlAlgorithm::Bbr3 as i32,
    }
}

pub(crate) fn from_proto(value: i32) -> Result<CongestionControlAlgorithm, ControlServiceError> {
    match v1::CongestionControlAlgorithm::try_from(value) {
        Ok(v1::CongestionControlAlgorithm::Unspecified | v1::CongestionControlAlgorithm::Cubic) => {
            Ok(CongestionControlAlgorithm::Cubic)
        }
        Ok(v1::CongestionControlAlgorithm::Reno) => Ok(CongestionControlAlgorithm::Reno),
        Ok(v1::CongestionControlAlgorithm::Bbr) => Ok(CongestionControlAlgorithm::Bbr),
        Ok(v1::CongestionControlAlgorithm::Bbr3) => Ok(CongestionControlAlgorithm::Bbr3),
        Err(_) => Err(ControlServiceError::InvalidRequest(
            "unknown congestion control algorithm".to_owned(),
        )),
    }
}
