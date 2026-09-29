//! The processor also checks its caller before any Hub-managed mutation.
pub const CONTRACT: &str = "d2rhub-processing-v1-r32";
pub const REQUIRED_HUB: &str = "0.9.111";

fn validate(version: Option<&str>, contract: Option<&str>) -> Result<(), String> {
    if version == Some(REQUIRED_HUB) && contract == Some(CONTRACT) {
        return Ok(());
    }
    Err(format!("禁止加工：加工器 {} 需要配套 D2RHub {REQUIRED_HUB}；当前 Hub {}，配套协议{}。请更新 D2RHub 和加工器后重试。", env!("CARGO_PKG_VERSION"), version.unwrap_or("未识别"), if contract == Some(CONTRACT) { "已匹配" } else { "不匹配或缺失" }))
}

pub fn require() -> Result<(), String> {
    validate(
        std::env::var("D2RHUB_VERSION").ok().as_deref(),
        std::env::var("D2RHUB_PROCESSING_CONTRACT").ok().as_deref(),
    )
}

pub fn handshake() -> Result<(), String> {
    require()?;
    println!(
        "{}",
        serde_json::json!({
            "product": "d2r-audio-mod", "version": env!("CARGO_PKG_VERSION"),
            "contract": CONTRACT, "required_hub": REQUIRED_HUB,
        })
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_missing_old_new_or_partial_hub_identity() {
        assert!(validate(Some(REQUIRED_HUB), Some(CONTRACT)).is_ok());
        for version in [None, Some("0.9.109"), Some("0.9.112"), Some("")] {
            assert!(validate(version, Some(CONTRACT)).is_err());
        }
        for contract in [None, Some("old"), Some("")] {
            assert!(validate(Some(REQUIRED_HUB), contract).is_err());
        }
    }
}
