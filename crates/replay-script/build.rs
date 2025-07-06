use glob::glob;
use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=package.json");
    println!("cargo:rerun-if-changed=bun.lock");
    println!("cargo:rerun-if-changed=tsconfig.json");
    println!("cargo:rerun-if-changed=tstl_src/");

    let out_dir = env::var("OUT_DIR").unwrap();
    run_bun_command("tstl_src", "bun", &["i"]);
    run_bun_command("tstl_src", "bunx", &["tstl", "--outDir", &out_dir]);

    generate_file_list_for_replay_scripts(&out_dir);
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

fn generate_file_list_for_replay_scripts(out_dir: &str) {
    let mut file_names = Vec::new();

    for entry in glob("tstl_src/rules/*.ts").expect("Failed to read glob pattern") {
        if let Ok(path) = entry {
            if let Some(stem) = path.file_stem() {
                if let Some(name) = stem.to_str() {
                    file_names.push(name.to_string());
                }
            }
        }
    }

    file_names.sort();

    let macro_call = format!(
        "generate_replay_scripts!(\n    {}\n);",
        file_names.join(",\n    ")
    );

    let output_path = Path::new(out_dir).join("replay_scripts.rs");
    let mut file = fs::File::create(output_path).unwrap();
    writeln!(file, "{}", macro_call).unwrap();
}
