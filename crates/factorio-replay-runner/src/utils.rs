use std::{fs, path::Path};

pub type AnyErr = Box<dyn std::error::Error>;

pub async fn try_cmd(cmd: &str, args: &[&str]) -> Result<(), AnyErr> {
    use async_process::Command;
    let output = Command::new(cmd).args(args).output().await?;

    if !output.status.success() {
        return Err(format!(
            "Failed to execute {} {}: {}\nstdout: {}\nstderr: {}",
            cmd,
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}

pub async fn try_download(url: &str, path: &Path) -> Result<(), AnyErr> {
    try_cmd("wget", &["-O", path.to_str().unwrap(), url]).await
}

pub async fn try_extract(zip_file: &Path, out_path: &Path) -> Result<(), AnyErr> {
    fs::create_dir(out_path)?;
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
}
