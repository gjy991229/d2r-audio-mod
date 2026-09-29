use std::process::{Command, Output};

fn invoke(args: &[&str], hub: Option<&str>, contract: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_d2r-audio-mod"));
    command
        .args(args)
        .env_remove("D2RHUB_VERSION")
        .env_remove("D2RHUB_PROCESSING_CONTRACT");
    if let Some(hub) = hub {
        command.env("D2RHUB_VERSION", hub);
    }
    if let Some(contract) = contract {
        command.env("D2RHUB_PROCESSING_CONTRACT", contract);
    }
    command.output().expect("processor runs")
}

#[test]
fn matching_hub_receives_machine_readable_identity() {
    let output = invoke(
        &["hub-compatibility"],
        Some("0.9.111"),
        Some("d2rhub-processing-v1-r32"),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let identity: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(identity["product"], "d2r-audio-mod");
    assert_eq!(identity["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(identity["required_hub"], "0.9.111");
    assert_eq!(identity["contract"], "d2rhub-processing-v1-r32");
}

#[test]
fn old_hubs_and_unknown_callers_are_rejected_before_operation_validation() {
    for operation in ["minimal", "augment", "unpack-mpq", "recover-mpq"] {
        for hub in [None, Some("0.9.109"), Some("0.9.112")] {
            let output = invoke(
                &[operation, "--events"],
                hub,
                Some("d2rhub-processing-v1-r32"),
            );
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
            assert!(String::from_utf8_lossy(&output.stderr).contains("禁止加工"));
        }
        let output = invoke(&[operation, "--events"], None, None);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("禁止加工"));
    }
}

#[test]
fn handshake_cannot_succeed_with_missing_or_unknown_contract() {
    for contract in [None, Some("old")] {
        let output = invoke(&["hub-compatibility"], Some("0.9.111"), contract);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
}
