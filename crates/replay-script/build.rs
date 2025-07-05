use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=tstl_src/tstl_src");
    println!("cargo:rerun-if-changed=tstl_src/package.json");
    println!("cargo:rerun-if-changed=tstl_src/bun.lock");
    println!("cargo:rerun-if-changed=tstl_src/tsconfig.json");

    let out_dir = env::var("OUT_DIR").unwrap();
    run_bun_command("tstl_src", "bun", &["i"]);
    run_bun_command("tstl_src", "bunx", &["tstl", "--outDir", &out_dir]);
}

fn run_bun_command(working_dir: &str, cmd: &str, args: &[&str]) {
    let mut cmd = Command::new(cmd);
    cmd.current_dir(working_dir);
    cmd.args(args);

    let output = cmd.output().unwrap_or_else(|e| {
        panic!("Failed to execute bun command {:?}: {}", args, e);
    });

    if !output.status.success() {
        eprintln!("bun command failed: bun {}", args.join(" "));
        eprintln!("stdout: {}", String::from_utf8_lossy(&output.stdout));
        eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));
        eprintln!("Working directory: {}", working_dir);
        panic!(
            "bun command failed with exit code: {:?}",
            output.status.code()
        );
    }

    // Print stdout for visibility during build
    if !output.stdout.is_empty() {
        println!("{}", String::from_utf8_lossy(&output.stdout));
    }
}
