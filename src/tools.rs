use std::path::PathBuf;

use anyhow::Result;
use clap::Subcommand;

use crate::util::create_recovery_file_on_disk;

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Generate a recovery file with a random keypair for demos/tests.
    GenerateRecovery {
        /// Path where the recovery file should be written.
        output: PathBuf,
        /// Optional passphrase for the recovery file. Prompts if omitted.
        #[arg(long)]
        passphrase: Option<String>,
    },
}

pub async fn run(command: Command) -> Result<()> {
    match command {
        Command::GenerateRecovery { output, passphrase } => {
            generate_recovery(output, passphrase)?;
        }
    }

    Ok(())
}

fn generate_recovery(output: PathBuf, passphrase: Option<String>) -> Result<()> {
    let passphrase = match passphrase {
        Some(pass) => pass,
        None => {
            let prompt = format!(
                "Enter a passphrase to protect {} (input hidden): ",
                output.display()
            );
            rpassword::prompt_password(prompt)?
        }
    };

    let keypair = create_recovery_file_on_disk(&output, &passphrase)?;

    println!("Recovery file written to {}", output.display());
    println!("Keep this passphrase safe: {}", passphrase);
    println!("User Pubky public key: {}", keypair.public_key());

    Ok(())
}
