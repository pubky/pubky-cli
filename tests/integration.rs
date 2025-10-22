use std::io::Write;
use std::path::Path;
use std::process::Output;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use assert_cmd::Command;
use predicates::Predicate;
use predicates::prelude::PredicateBooleanExt;
use pubky::Keypair;
use pubky_common::recovery_file::create_recovery_file;
use pubky_testnet::EphemeralTestnet;
use serde::Deserialize;
use serial_test::serial;
use tempfile::NamedTempFile;
use tokio::time::sleep;

#[derive(Debug, Deserialize)]
struct AdminInfo {
    num_users: u64,
    num_disabled_users: u64,
    num_signup_codes: u64,
    num_unused_signup_codes: u64,
    total_disk_used_mb: f64,
}

struct RecoveryFixture {
    file: NamedTempFile,
    passphrase: String,
    keypair: Keypair,
}

impl RecoveryFixture {
    fn path(&self) -> &Path {
        self.file.path()
    }

    fn pass(&self) -> &str {
        &self.passphrase
    }

    fn keypair(&self) -> &Keypair {
        &self.keypair
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn admin_info_command_returns_stats() -> Result<()> {
    let network = start_testnet().await?;
    sleep(Duration::from_millis(100)).await;

    let admin_url = admin_base_url(&network);

    let output = run_cli(
        &["admin", "info", "--admin-url", &admin_url],
        &[(PASS_ENV, "admin"), (RECOVERY_PASS_ENV, "ignored")],
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

    let admin_url = admin_base_url(&network);
    let homeserver_pk = network.homeserver().public_key();

    let fixture = create_recovery_fixture()?;
    let env = cli_env(fixture.pass());

    // Create the user via CLI signup.
    run_cli(
        &[
            "user",
            "signup",
            &homeserver_pk.to_string(),
            fixture.path().to_str().unwrap(),
            "--testnet",
        ],
        &env,
    )
    .await?;

    let user_pubkey = fixture.keypair().public_key().to_string();

    run_cli(
        &[
            "admin",
            "user",
            "--admin-url",
            &admin_url,
            "disable",
            &user_pubkey,
        ],
        &env,
    )
    .await?;

    let info_after_disable = fetch_info(&admin_url, "admin").await?;
    assert_eq!(info_after_disable.num_users, 1);
    assert_eq!(info_after_disable.num_disabled_users, 1);

    run_cli(
        &[
            "admin",
            "user",
            "--admin-url",
            &admin_url,
            "enable",
            &user_pubkey,
        ],
        &env,
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

    let admin_url = admin_base_url(&network);
    let before = fetch_info(&admin_url, "admin").await?;
    assert!(before.total_disk_used_mb >= 0.0);

    let env = [(PASS_ENV, "admin"), (RECOVERY_PASS_ENV, "ignored")];
    let output = run_cli(
        &["admin", "generate-token", "--admin-url", &admin_url],
        &env,
    )
    .await?;

    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(
        !token.is_empty(),
        "expected invitation token from CLI, got '{}'",
        token
    );

    let after = fetch_info(&admin_url, "admin").await?;
    assert_eq!(after.num_signup_codes, before.num_signup_codes + 1);
    assert_eq!(
        after.num_unused_signup_codes,
        before.num_unused_signup_codes + 1
    );
    assert!(after.total_disk_used_mb >= 0.0);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn admin_storage_delete_removes_entry() -> Result<()> {
    let network = start_testnet().await?;
    sleep(Duration::from_millis(100)).await;

    let admin_url = admin_base_url(&network);
    let homeserver_pk = network.homeserver().public_key();

    let fixture = create_recovery_fixture()?;
    let env = cli_env(fixture.pass());

    run_cli(
        &[
            "user",
            "signup",
            &homeserver_pk.to_string(),
            fixture.path().to_str().unwrap(),
            "--testnet",
        ],
        &env,
    )
    .await?;

    let sdk = network.sdk().context("build sdk facade")?;
    let signer = sdk.signer(fixture.keypair().clone());
    let session = signer.signin().await?;
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

    run_cli(
        &[
            "admin",
            "storage",
            "--admin-url",
            &admin_url,
            "delete",
            &fixture.keypair().public_key().to_string(),
            "/pub/app/hello.txt",
        ],
        &env,
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn user_signup_signin_session_signout_flow() -> Result<()> {
    let network = start_testnet().await?;
    sleep(Duration::from_millis(100)).await;

    let homeserver_pk = network.homeserver().public_key();
    let fixture = create_recovery_fixture()?;
    let env = cli_env(fixture.pass());

    let signup_output = run_cli(
        &[
            "user",
            "signup",
            &homeserver_pk.to_string(),
            fixture.path().to_str().unwrap(),
            "--testnet",
        ],
        &env,
    )
    .await?;
    let signup_stdout = String::from_utf8_lossy(&signup_output.stdout);
    assert!(
        signup_stdout.contains("Signup successful"),
        "unexpected signup output: {}",
        signup_stdout
    );

    let signin_output = run_cli(
        &[
            "user",
            "signin",
            fixture.path().to_str().unwrap(),
            "--testnet",
        ],
        &env,
    )
    .await?;
    let signin_stdout = String::from_utf8_lossy(&signin_output.stdout);
    assert!(
        signin_stdout.contains("Signin successful"),
        "unexpected signin output: {}",
        signin_stdout
    );

    let session_output = run_cli(
        &[
            "user",
            "session",
            fixture.path().to_str().unwrap(),
            "--testnet",
        ],
        &env,
    )
    .await?;
    let session_stdout = String::from_utf8_lossy(&session_output.stdout);
    assert!(
        session_stdout.contains("Session information"),
        "unexpected session output: {}",
        session_stdout
    );

    let signout_output = run_cli(
        &[
            "user",
            "signout",
            fixture.path().to_str().unwrap(),
            "--testnet",
        ],
        &env,
    )
    .await?;
    let signout_stdout = String::from_utf8_lossy(&signout_output.stdout);
    assert!(
        signout_stdout.contains("Signed out of homeserver"),
        "unexpected signout output: {}",
        signout_stdout
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn user_list_includes_uploaded_file() -> Result<()> {
    let network = start_testnet().await?;
    sleep(Duration::from_millis(100)).await;

    let homeserver_pk = network.homeserver().public_key();
    let fixture = create_recovery_fixture()?;
    let env = cli_env(fixture.pass());

    run_cli(
        &[
            "user",
            "signup",
            &homeserver_pk.to_string(),
            fixture.path().to_str().unwrap(),
            "--testnet",
        ],
        &env,
    )
    .await?;

    let sdk = network.sdk().context("build sdk facade")?;
    let signer = sdk.signer(fixture.keypair().clone());
    let session = signer.signin().await?;
    session
        .storage()
        .put("/pub/app/data.txt", "contents")
        .await
        .context("upload file for user list")?;
    session.signout().await.map_err(|(err, _)| err)?;

    let list_url = format!(
        "pubky://{}/pub/app/",
        fixture.keypair().public_key().to_string()
    );

    let list_output = run_cli(&["user", "list", &list_url, "--testnet", "--shallow"], &env).await?;

    let out = String::from_utf8_lossy(&list_output.stdout);
    assert!(
        out.contains("pubky://") && out.contains("data.txt"),
        "expected list to contain uploaded file, got: {}",
        out
    );

    Ok(())
}

const PASS_ENV: &str = "PUBKY_ADMIN_PASSWORD";
const RECOVERY_PASS_ENV: &str = "PUBKY_CLI_RECOVERY_PASSPHRASE";

fn cli_env<'a>(passphrase: &'a str) -> Vec<(&'static str, &'a str)> {
    vec![(PASS_ENV, "admin"), (RECOVERY_PASS_ENV, passphrase)]
}

async fn run_cli(args: &[&str], envs: &[(&str, &str)]) -> Result<Output> {
    let args_vec = args.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let env_vec = envs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect::<Vec<_>>();

    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::cargo_bin("pubky-homeserver-cli")?;
        for arg in args_vec {
            cmd.arg(arg);
        }
        for (key, value) in env_vec {
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

    let info = response
        .json::<AdminInfo>()
        .await
        .context("decode admin info")?;
    Ok(info)
}

fn admin_base_url(network: &EphemeralTestnet) -> String {
    format!("http://{}", network.homeserver().admin().listen_socket())
}

fn create_recovery_fixture() -> Result<RecoveryFixture> {
    let keypair = Keypair::random();
    let passphrase = "test-passphrase".to_string();
    let recovery_bytes = create_recovery_file(&keypair, &passphrase);
    let mut file = NamedTempFile::new().context("create temp recovery file")?;
    file.write_all(&recovery_bytes)
        .context("write recovery file")?;
    Ok(RecoveryFixture {
        file,
        passphrase,
        keypair,
    })
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

    Err(last_err.unwrap_or_else(|| anyhow!("failed to start testnet"))).context("start homeserver")
}
