use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let proto_root = manifest.join("../../proto");
    let control = proto_root.join("usque/v1/control.proto");
    let agent = proto_root.join("usque/agent/v1/agent.proto");
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc");

    // SAFETY: Cargo executes each build script in its own process. This changes
    // only the child environment used immediately by prost-build.
    unsafe {
        std::env::set_var("PROTOC", protoc);
    }

    let mut config = prost_build::Config::new();
    config.type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]");
    // Keep control envelopes small enough to pass cheaply across async queues.
    config.boxed(".usque.v1.ControlRequest.payload.upsert_profile");
    config.boxed(".usque.v1.ControlRequest.payload.create_profile_with_identity");
    config.boxed(".usque.v1.ControlRequest.payload.reconfigure_active_profile");
    config.boxed(".usque.v1.ControlResponse.payload.status");
    config.boxed(".usque.v1.ControlResponse.payload.profile");
    config.boxed(".usque.v1.ControlResponse.payload.reconfigure");
    config.boxed(".usque.v1.ControlResponse.payload.connection_timeline");
    config.boxed(".usque.v1.ControlResponse.payload.network_quality");
    config.boxed(".usque.v1.EventEnvelope.payload.state_changed");
    config.boxed(".usque.v1.EventEnvelope.payload.exit_info_updated");
    config.boxed(".usque.v1.EventEnvelope.payload.network_quality_updated");
    config.boxed(".usque.v1.ConnectionSnapshot.network_quality");
    config.boxed(".usque.v1.NetworkQualityUpdated.snapshot");
    config
        .compile_protos(&[control, agent], &[proto_root])
        .expect("compile protobuf contracts");
    println!("cargo:rerun-if-changed=../../proto/usque/v1/control.proto");
    println!("cargo:rerun-if-changed=../../proto/usque/agent/v1/agent.proto");
}
