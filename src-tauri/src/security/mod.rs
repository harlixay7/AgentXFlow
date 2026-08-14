use crate::error::CoordinatorError;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use parking_lot::RwLock;
use tracing::info;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SecurityManager {
    token: Arc<RwLock<String>>,
    token_file: Option<PathBuf>,
}

impl SecurityManager {
    pub fn new_with_token(token: String) -> Self {
        Self {
            token: Arc::new(RwLock::new(token)),
            token_file: None,
        }
    }

    pub fn init_or_load(data_dir: &Path) -> Result<Self, CoordinatorError> {
        let auth_dir = data_dir.join(".agentxflow");
        let _ = fs::create_dir_all(&auth_dir);
        let token_path = auth_dir.join("auth.token");

        let token = if token_path.exists() {
            let existing = fs::read_to_string(&token_path)
                .map_err(|e| CoordinatorError::Io(format!("Failed to read auth.token: {}", e)))?
                .trim()
                .to_string();
            if !existing.is_empty() {
                info!("Loaded existing secure MCP token");
                existing
            } else {
                Self::generate_and_save_token(&token_path)?
            }
        } else {
            Self::generate_and_save_token(&token_path)?
        };

        Ok(Self {
            token: Arc::new(RwLock::new(token)),
            token_file: Some(token_path),
        })
    }

    fn generate_and_save_token(path: &Path) -> Result<String, CoordinatorError> {
        let raw = format!("{}-{}-{}", Uuid::new_v4(), Uuid::new_v4(), chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
        let mut hasher = Sha256::new();
        hasher.update(raw.as_bytes());
        let token = format!("axf_live_{}", hex::encode(hasher.finalize()));

        fs::write(path, &token)
            .map_err(|e| CoordinatorError::Io(format!("Failed to persist auth.token: {}", e)))?;
        info!("Generated new cryptographically secure per-install MCP token");
        Ok(token)
    }

    pub fn get_token(&self) -> String {
        self.token.read().clone()
    }

    pub fn rotate_token(&self) -> Result<String, CoordinatorError> {
        let raw = format!("{}-{}-{}", Uuid::new_v4(), Uuid::new_v4(), chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
        let mut hasher = Sha256::new();
        hasher.update(raw.as_bytes());
        let new_token = format!("axf_live_{}", hex::encode(hasher.finalize()));

        if let Some(ref path) = self.token_file {
            fs::write(path, &new_token)
                .map_err(|e| CoordinatorError::Io(format!("Failed to persist rotated auth.token: {}", e)))?;
        }

        *self.token.write() = new_token.clone();
        info!("Rotated MCP authentication token");
        Ok(new_token)
    }

    pub fn validate_token(&self, incoming: &str) -> bool {
        let current = self.token.read();
        incoming == *current
    }
}
