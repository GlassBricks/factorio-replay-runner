use std::process::Command;

fn try_cmd(cmd: &mut Command, err_msg: &str) {
    if let Err(e) = cmd.status() {
        panic!("{}: {}", err_msg, e);
    }
}

fn main() {
    let replay_script_dir = "replay-script";

    println!("cargo:rerun-if-changed={replay_script_dir}/control.ts");
    println!("cargo:rerun-if-changed={replay_script_dir}/event_handler.d.ts");
    println!("cargo:rerun-if-changed={replay_script_dir}/package.json");
    println!("cargo:rerun-if-changed={replay_script_dir}/tsconfig.json");

    try_cmd(
        Command::new("bun").arg("--version"),
        "bun is not available in PATH. Please ensure you're running in a nix development shell with bun installed.",
    );
    try_cmd(
        Command::new("bun")
            .arg("install")
            .current_dir(&replay_script_dir),
        "Failed to execute bun install",
    );

    try_cmd(
        Command::new("bun")
            .arg("run")
            .arg("build")
            .current_dir(&replay_script_dir),
        "TypeScript to Lua compilation failed. Check TypeScript source files for errors.",
    );
}
