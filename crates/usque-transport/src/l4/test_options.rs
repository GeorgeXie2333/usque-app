//! Offline/internal controls only; this module is absent from production builds.
use usque_core::Profile;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum TestOptions {
    #[default]
    Baseline,
    TunMtu(u16),
    UdpReceiveMiB(u32),
    InitialStreamMiB(u32),
}
impl TestOptions {
    pub(crate) fn profile(self, mut profile: Profile) -> Profile {
        if let Self::TunMtu(mtu) = self {
            assert!([1280, 1500, 4096, 9000].contains(&mtu));
            profile.mtu = mtu;
        }
        profile
    }
    pub(crate) fn apply(
        self,
        config: &mut quiche::Config,
        socket: &tokio::net::UdpSocket,
    ) -> std::io::Result<()> {
        match self {
            Self::UdpReceiveMiB(mib) => {
                assert!([2, 4, 8].contains(&mib));
                socket2::SockRef::from(socket).set_recv_buffer_size((mib as usize) << 20)?;
                // Verify the actual OS result, never treat a requested value as measured.
                let actual = socket2::SockRef::from(socket).recv_buffer_size()?;
                assert!(actual != 0);
            }
            Self::InitialStreamMiB(mib) => {
                assert!([1, 2].contains(&mib));
                config.set_initial_max_stream_data_bidi_local(u64::from(mib) << 20);
            }
            Self::Baseline | Self::TunMtu(_) => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn one_factor_controls_do_not_mutate_production_defaults_or_other_knobs() {
        let original = Profile::default();
        for option in [
            TestOptions::Baseline,
            TestOptions::TunMtu(1280),
            TestOptions::TunMtu(1500),
            TestOptions::TunMtu(4096),
            TestOptions::TunMtu(9000),
            TestOptions::UdpReceiveMiB(2),
            TestOptions::UdpReceiveMiB(4),
            TestOptions::UdpReceiveMiB(8),
            TestOptions::InitialStreamMiB(1),
            TestOptions::InitialStreamMiB(2),
        ] {
            let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let mut profile = option.profile(original.clone());
            let _pair = crate::h3::tests::test_quic_pair_with_config(
                "127.0.0.1:13340".parse().unwrap(),
                "127.0.0.1:13341".parse().unwrap(),
                |config, _| {
                    super::super::Limits::platform().configure(config);
                    option.apply(config, &socket).unwrap();
                },
            );
            if let TestOptions::TunMtu(_) = option {
                profile.mtu = original.mtu;
            }
            assert_eq!(profile, original);
            assert!(socket2::SockRef::from(&socket).recv_buffer_size().unwrap() > 0);
        }
        assert_eq!(Profile::default().mtu, 1280);
    }
}
