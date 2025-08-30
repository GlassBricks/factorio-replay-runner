use anyhow::{Context, Result};
use std::{fs, path::Path};

pub async fn try_cmd(cmd: &str, args: &[&str]) -> Result<()> {
    use async_process::Command;
    let output = Command::new(cmd)
        .args(args)
        .output()
        .await
        .with_context(|| format!("Failed to execute command: {}", cmd))?;

    if !output.status.success() {
        anyhow::bail!(
            "Command failed: {} {}\nStatus: {}\nStdout: {}\nStderr: {}",
            cmd,
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

pub async fn try_download(url: &str, path: &Path) -> Result<()> {
    try_cmd("wget", &["-O", path.to_str().unwrap(), url])
        .await
        .with_context(|| format!("Failed to download from {} to {}", url, path.display()))
}

pub async fn try_extract(zip_file: &Path, out_path: &Path) -> Result<()> {
    fs::create_dir_all(out_path)
        .with_context(|| format!("Failed to create directory: {}", out_path.display()))?;

    try_cmd(
        "tar",
        &[
            "-xvf",
            zip_file.to_str().unwrap(),
            "-C",
            out_path.to_str().unwrap(),
        ],
    )
    .await
    .with_context(|| {
        format!(
            "Failed to extract {} to {}",
            zip_file.display(),
            out_path.display()
        )
    })
}
