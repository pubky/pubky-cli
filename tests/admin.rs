use std::process::Output;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::Predicate;
use serial_test::serial;
use pubky::Keypair;
use pubky_testnet::EphemeralTestnet;
use serde::Deserialize;
use tokio::time::sleep;

#[derive(Debug, Deserialize)]
struct AdminInfo {
    num_users: u64,
    num_disabled_users: u64,
    total_disk_used_mb: f64,
    num_signup_codes: u64,
    num_unused_signup_codes: u64,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn admin_info_command_returns_stats() -> Result<()> {
    let network = start_testnet().await?;
    sleep(Duration::from_millis(100)).await;

    let admin_url = format!("http://{}", network.homeserver().admin().listen_socket());

    let output = run_cli(
        &["admin", "info", "--admin-url", &admin_url],
        &[("PUBKY_ADMIN_PASSWORD", "admin")],
    )
    .await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let predicate = predicates::str::contains("Users: 0")
        .and(predicates::str::contains("Disabled users: 0"))
        .and(predicates::str::contains("Signup codes: 0"));
    assert!(
        predicate.eval(&stdout),
        "unexpected admin info output:\n{}",
        stdout
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn admin_user_disable_and_enable_flow() -> Result<()> {
    let network = start_testnet().await?;
    sleep(Duration::from_millis(100)).await;

    let admin_url = format!("http://{}", network.homeserver().admin().listen_socket());
    let homeserver_pk = network.homeserver().public_key();

    // Create a user so that disable/enable have something to act on.
    let sdk = network.sdk().context("build sdk facade")?;
    let keypair = Keypair::random();
    let signer = sdk.signer(keypair.clone());
    let session = signer
        .signup(&homeserver_pk, None)
        .await
        .context("signup test user")?;
    let user_pubkey = keypair.public_key().to_string();
    session.signout().await.map_err(|(err, _)| err)?;

    // Disable the user via CLI.
    run_cli(
        &[
            "admin",
            "user",
            "--admin-url",
            &admin_url,
            "disable",
            &user_pubkey,
        ],
        &[("PUBKY_ADMIN_PASSWORD", "admin")],
    )
    .await?;

    let info_after_disable = fetch_info(&admin_url, "admin").await?;
    assert_eq!(info_after_disable.num_users, 1);
    assert_eq!(info_after_disable.num_disabled_users, 1);

    // Re-enable the user.
    run_cli(
        &[
            "admin",
            "user",
            "--admin-url",
            &admin_url,
            "enable",
            &user_pubkey,
        ],
        &[("PUBKY_ADMIN_PASSWORD", "admin")],
    )
    .await?;

    let info_after_enable = fetch_info(&admin_url, "admin").await?;
    assert_eq!(info_after_enable.num_users, 1);
    assert_eq!(info_after_enable.num_disabled_users, 0);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn admin_generate_token_produces_invite() -> Result<()> {
    let network = start_testnet().await?;
    sleep(Duration::from_millis(100)).await;

    let admin_url = format!("http://{}", network.homeserver().admin().listen_socket());
    let before = fetch_info(&admin_url, "admin").await?;
    assert!(before.total_disk_used_mb >= 0.0);

    let output = run_cli(
        &["admin", "generate-token", "--admin-url", &admin_url],
        &[("PUBKY_ADMIN_PASSWORD", "admin")],
    )
    .await?;

    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(
        !token.is_empty(),
        "expected invitation token from CLI, got '{}'",
        token
    );

    let after = fetch_info(&admin_url, "admin").await?;
    assert!(after.total_disk_used_mb >= 0.0);
    assert_eq!(after.num_signup_codes, before.num_signup_codes + 1);
    assert_eq!(
        after.num_unused_signup_codes,
        before.num_unused_signup_codes + 1
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn admin_storage_delete_removes_entry() -> Result<()> {
    let network = start_testnet().await?;
    sleep(Duration::from_millis(100)).await;

    let admin_url = format!("http://{}", network.homeserver().admin().listen_socket());
    let homeserver_pk = network.homeserver().public_key();

    let sdk = network.sdk().context("build sdk facade")?;
    let keypair = Keypair::random();
    let signer = sdk.signer(keypair.clone());

    let session = signer
        .signup(&homeserver_pk, None)
        .await
        .context("signup test user")?;
    session
        .storage()
        .put("/pub/app/hello.txt", "hello world")
        .await
        .context("upload file")?;
    session.signout().await.map_err(|(err, _)| err)?;

    let session = signer.signin().await?;
    assert!(
        session
            .storage()
            .exists("/pub/app/hello.txt")
            .await
            .context("confirm file exists before deletion")?,
        "expected file before admin delete"
    );
    session.signout().await.map_err(|(err, _)| err)?;

    let user_pubkey = keypair.public_key().to_string();
    run_cli(
        &[
            "admin",
            "storage",
            "--admin-url",
            &admin_url,
            "delete",
            &user_pubkey,
            "/pub/app/hello.txt",
        ],
        &[("PUBKY_ADMIN_PASSWORD", "admin")],
    )
    .await?;

    let session = signer.signin().await?;
    assert!(
        !session
            .storage()
            .exists("/pub/app/hello.txt")
            .await
            .context("confirm file gone after deletion")?,
        "expected file to be removed after admin delete"
    );
    session.signout().await.map_err(|(err, _)| err)?;

    Ok(())
}

async fn run_cli(args: &[&str], envs: &[(&str, &str)]) -> Result<Output> {
    let args = args.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let envs = envs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect::<Vec<_>>();

    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::cargo_bin("pubky-homeserver-cli")?;
        for arg in args {
            cmd.arg(arg);
        }
        for (key, value) in envs {
            cmd.env(key, value);
        }

        let output = cmd.output().context("run cli command")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("CLI exited with status {:?}: {}", output.status, stderr);
        }
        Ok::<Output, anyhow::Error>(output)
    })
    .await
    .expect("spawn_blocking panicked")
}

async fn fetch_info(admin_url: &str, password: &str) -> Result<AdminInfo> {
    let url = format!("{}/info", admin_url.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .header("X-Admin-Password", password)
        .send()
        .await
        .context("send admin info request")?
        .error_for_status()
        .context("admin info response status")?;

    let info = response.json::<AdminInfo>().await.context("decode admin info")?;
    Ok(info)
}

async fn start_testnet() -> Result<EphemeralTestnet> {
    const MAX_ATTEMPTS: usize = 5;
    let mut last_err: Option<anyhow::Error> = None;

    for _attempt in 0..MAX_ATTEMPTS {
        match EphemeralTestnet::start_minimal().await {
            Ok(mut network) => {
                if let Err(err) = network.testnet.create_pkarr_relay().await {
                    last_err = Some(err.into());
                    sleep(Duration::from_millis(250)).await;
                    continue;
                }

                match network.testnet.create_homeserver().await {
                    Ok(_) => {
                        return Ok(network);
                    }
                    Err(err) => {
                        last_err = Some(err);
                        sleep(Duration::from_millis(250)).await;
                        continue;
                    }
                }
            }
            Err(err) => {
                last_err = Some(err);
                sleep(Duration::from_millis(250)).await;
                continue;
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow!("failed to start testnet")))
        .context("start homeserver")
}
