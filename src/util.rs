use std::path::Path;

use anyhow::{Context, Result};
use pubky::{Keypair, Pubky, PubkySigner};

pub fn build_pubky(testnet: bool) -> Result<Pubky> {
    let facade = if testnet {
        Pubky::testnet()?
    } else {
        Pubky::new()?
    };

    Ok(facade)
}

pub fn build_signer(testnet: bool, keypair: Keypair) -> Result<PubkySigner> {
    let facade = build_pubky(testnet)?;
    Ok(facade.signer(keypair))
}

pub fn load_keypair_from_recovery_file(path: &Path) -> Result<Keypair> {
    let recovery_bytes = std::fs::read(path)
        .with_context(|| format!("Failed to read recovery file {}", path.display()))?;

    let passphrase = match std::env::var("PUBKY_CLI_RECOVERY_PASSPHRASE") {
        Ok(value) => value,
        Err(_) => {
            let prompt = format!(
                "Enter the recovery file passphrase for {} (input hidden): ",
                path.display()
            );
            rpassword::prompt_password(prompt)?
        }
    };

    let keypair = pubky::recovery_file::decrypt_recovery_file(&recovery_bytes, &passphrase)
        .with_context(|| "Failed to decrypt recovery file with provided passphrase")?;

    Ok(keypair)
}
