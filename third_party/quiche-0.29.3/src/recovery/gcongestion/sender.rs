// Copyright (c) 2026 The Usque contributors.
// SPDX-License-Identifier: BSD-2-Clause
use std::time::Instant;

use super::bbr2::BBRv2;
use super::bbr3::BBRv3;
#[cfg(feature = "qlog")]
use super::Bandwidth;
use crate::recovery::RangeSet;

/// Closed dispatch: BBRv2 keeps its original implementation and parameters.
#[enum_dispatch::enum_dispatch(CongestionControl)]
#[derive(Debug)]
pub(crate) enum BbrSender {
    V2(BBRv2),
    V3(BBRv3),
}

impl BbrSender {
    pub(super) fn time_sent_set_to_now(&self) -> bool {
        match self {
            Self::V2(sender) => sender.time_sent_set_to_now(),
            Self::V3(_) => true,
        }
    }

    pub(super) fn send_quantum(&self) -> Option<usize> {
        match self {
            Self::V2(_) => None,
            Self::V3(sender) => Some(sender.send_quantum()),
        }
    }

    pub(super) fn acknowledge_spurious_losses(
        &mut self,
        ranges: &RangeSet,
        now: Instant,
    ) {
        if let Self::V3(sender) = self {
            sender.acknowledge_spurious_losses(ranges, now);
        }
    }

    #[cfg(feature = "qlog")]
    pub(super) fn send_rate(&self) -> Option<Bandwidth> {
        match self {
            Self::V2(sender) => sender.send_rate(),
            Self::V3(sender) => sender.send_rate(),
        }
    }

    #[cfg(feature = "qlog")]
    pub(super) fn ack_rate(&self) -> Option<Bandwidth> {
        match self {
            Self::V2(sender) => sender.ack_rate(),
            Self::V3(sender) => sender.ack_rate(),
        }
    }
}
