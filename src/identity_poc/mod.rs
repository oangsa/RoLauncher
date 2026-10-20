//! Experimental example only: deliberately absent from lib.rs and production startup.
use serde::{Deserialize, Serialize};

pub mod provisioning;

#[cfg(windows)]
pub mod windows;

pub const PURPOSE: &str = "rolauncher.identity-poc.proof.v1";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Challenge {
    pub schema_version: u16,
    pub nonce: [u8; 32],
    pub installation_key: [u8; 32],
    pub user_key: [u8; 32],
    pub windows_sid: [u8; 32],
    pub installation_evidence: [u8; 32],
    pub synthetic_customer: String,
    pub purpose: String,
}

impl Challenge {
    pub fn canonical(&self) -> Result<Vec<u8>, String> {
        if self.schema_version != 1
            || self.purpose != PURPOSE
            || !self.synthetic_customer.starts_with("synthetic:")
            || !(11..=128).contains(&self.synthetic_customer.len())
            || !self
                .synthetic_customer
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b":-_".contains(&b))
            || [
                self.nonce,
                self.installation_key,
                self.user_key,
                self.windows_sid,
                self.installation_evidence,
            ]
            .contains(&[0; 32])
        {
            return Err("invalid synthetic context".into());
        }
        let mut bytes = b"RoLauncher-Identity-PoC\0".to_vec();
        bytes.extend(self.schema_version.to_be_bytes());
        for field in [
            self.nonce,
            self.installation_key,
            self.user_key,
            self.windows_sid,
            self.installation_evidence,
        ] {
            bytes.extend(field);
        }
        for field in [&self.synthetic_customer, &self.purpose] {
            let length = u32::try_from(field.len()).map_err(|_| "context too large")?;
            bytes.extend(length.to_be_bytes());
            bytes.extend(field.as_bytes());
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests;
