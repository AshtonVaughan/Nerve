//! TLS support.
//!
//! When the user binds the daemon to anything other than loopback they should
//! enable TLS. We rely on rustls + axum-tls; for local development we can
//! generate a self-signed cert on demand via `rcgen`.
//!
//! The MVP shipped only loopback HTTP/WS. This module is what gates the
//! `--bind 0.0.0.0:port` use case for sandboxes / VMs.

use std::path::PathBuf;

use anyhow::{Context, Result};
use rcgen::{CertifiedKey, generate_simple_self_signed};

use crate::config::TlsConfig;

/// Ensure that valid cert + key files exist on disk. When `auto_self_signed`
/// is true and either file is missing, generate a new pair.
pub fn ensure_certificate(cfg: &TlsConfig) -> Result<(PathBuf, PathBuf)> {
    let cert_path = cfg.cert_path.clone();
    let key_path = cfg.key_path.clone();

    if cert_path.as_os_str().is_empty() || key_path.as_os_str().is_empty() {
        anyhow::bail!("TLS enabled but cert_path / key_path are empty");
    }

    if !cert_path.exists() || !key_path.exists() {
        if !cfg.auto_self_signed {
            anyhow::bail!(
                "TLS cert {:?} / key {:?} missing and auto_self_signed = false",
                cert_path,
                key_path
            );
        }
        generate_self_signed(&cert_path, &key_path).context("auto-generating self-signed cert")?;
    }

    Ok((cert_path, key_path))
}

fn generate_self_signed(cert_path: &PathBuf, key_path: &PathBuf) -> Result<()> {
    let CertifiedKey { cert, key_pair } =
        generate_simple_self_signed(vec!["localhost".into(), "127.0.0.1".into()])?;
    if let Some(parent) = cert_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = key_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(cert_path, cert.pem())?;
    std::fs::write(key_path, key_pair.serialize_pem())?;
    Ok(())
}
