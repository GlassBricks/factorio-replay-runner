use glob::glob;
use itertools::Itertools;
use serde::Deserialize;
use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Deserialize)]
struct ScriptMetadataParse {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    param_type: Option<String>,
    #[serde(default)]
    default: Option<String>,
    #[serde(default)]
    enable_if: Option<String>,
    #[serde(default)]
    enable_value: Option<String>,
}

#[derive(Debug)]
struct ScriptMetadata {
    file_name: String,
    name: String,
    param_type: String,
    default_value: String,
    enable_if: String,
    enable_value: String,
}

impl ScriptMetadata {
    fn from(parse: ScriptMetadataParse, file_name: &str) -> Self {
        let file_name = file_name.to_string();
        let is_no_prefixed = file_name.starts_with("no_");
        let name = parse.name.unwrap_or_else(|| {
            if is_no_prefixed {
                file_name[3..].to_string()
            } else {
                file_name.to_string()
            }
        });
        let param_type = parse.param_type.unwrap_or_else(|| "bool".to_string());
        let default_value = parse.default.unwrap_or_else(|| {
            if param_type == "bool" {
                if is_no_prefixed { "false" } else { "true" }
            } else if param_type.contains("Option") {
                "None"
            } else {
                panic!(
                    "Default value not provided for parameter type: {}",
                    param_type
                );
            }
            .to_string()
        });
        let enable_if = parse.enable_if.unwrap_or_else(|| {
            if param_type == "bool" {
                if is_no_prefixed { "!param" } else { "param" }
            } else if param_type.contains("Option") {
                "param.is_some()"
            } else {
                panic!(
                    "Enable condition not provided for parameter type: {}",
                    param_type
                );
            }
            .to_string()
        });

        let enable_value = parse.enable_value.unwrap_or_else(|| {
            match enable_if.as_str() {
                "param" => "true",
                "!param" => "false",
                "param.is_some()" => default_value.as_str(),
                _ => panic!(
                    "A value needs to be provided for \"default enable\" condition: {}",
                    enable_if
                ),
            }
            .to_string()
        });

        Self {
            file_name,
            name,
            param_type,
            default_value,
            enable_if,
            enable_value,
        }
    }
}

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

fn parse_script_metadata(file_name: &str, file_path: &Path) -> ScriptMetadata {
    let content = fs::read_to_string(file_path)
        .unwrap_or_else(|e| panic!("Failed to read file {:?}: {}", file_path, e));

    let yaml_content = content
        .lines()
        .take_while(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("//") || trimmed.is_empty()
        })
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("//") && !trimmed.is_empty() {
                Some(trimmed[2..].trim().to_string())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let parse: ScriptMetadataParse = serde_yaml::from_str(&yaml_content)
        .unwrap_or_else(|e| panic!("Failed to parse YAML metadata from {:?}: {}", file_path, e));

    ScriptMetadata::from(parse, file_name)
}

fn formatter_for_type(type_name: &str) -> &str {
    match type_name {
        "bool" => "value.to_string()",
        "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64" => "value.to_string()",
        "String" => "format!(\"\\\"{{}}\\\"\", value)",
        _ => panic!("Unsupported param_type: {}", type_name),
    }
}

fn generate_file_list_for_replay_scripts(out_dir: &str) {
    let mut scripts = Vec::new();

    for entry in glob("tstl_src/rules/*.ts").expect("Failed to read glob pattern") {
        if let Ok(path) = entry {
            if let Some(stem) = path.file_stem() {
                if let Some(name) = stem.to_str() {
                    scripts.push(parse_script_metadata(name, &path));
                }
            }
        }
    }

    scripts.sort_by(|a, b| a.name.cmp(&b.name));

    let default_functions = scripts
        .iter()
        .map(|metadata| {
            format!(
                "fn default_{}() -> {} {{ {} }}",
                metadata.name, metadata.param_type, metadata.default_value
            )
        })
        .join("\n");

    let struct_fields = scripts
        .iter()
        .map(|metadata| {
            format!(
                "    #[serde(default = \"default_{}\")]\n    pub {}: {},",
                metadata.name, metadata.name, metadata.param_type
            )
        })
        .join("\n");

    let display_logic = scripts
        .iter()
        .map(
            |ScriptMetadata {
                 file_name,
                 name,
                 param_type,
                 enable_if,
                 ..
             }| {
                let param_formatter =
                    if param_type.starts_with("Option<") && param_type.ends_with(">") {
                        let inner_type = &param_type[7..param_type.len() - 1];
                        let inner_formatter = formatter_for_type(inner_type);
                        format!(
                            "self.{}.map(|value| {}).unwrap_or_else(|| \"undefined\".to_string())",
                            name, inner_formatter
                        )
                    } else {
                        formatter_for_type(param_type).replace("value", &format!("self.{}", name))
                    };

                let should_borrow = param_type.contains("String");
                let borrow_str = if should_borrow { "&" } else { "" };
                format!(
                    r#"        let param = {borrow_str}self.{name};
        if {enable_if} {{
            writeln!(fmt, "-- Script: {file_name}")?;
            let script_content = include_str!(concat!(env!("OUT_DIR"), "/rules/{file_name}.lua"));
            let param_value = {param_formatter};
            let substituted = script_content.replace("PARAM_VALUE", &param_value);
            fmt.write_str(&substituted)?;
        }}"#,
                )
            },
        )
        .join("\n");

    let defaults = scripts
        .iter()
        .map(|metadata| format!("    {}: {},", metadata.name, metadata.default_value))
        .join("\n");

    let all_enabled = scripts
        .iter()
        .map(|metadata| format!("    {}: {},", metadata.name, metadata.enable_value))
        .join("\n");

    let all_scripts = scripts
        .iter()
        .map(|metadata| format!("\"{}\"", metadata.file_name))
        .join(",");

    let generated_code = format!(
        r#"// Generated by build.rs

{default_functions}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayScripts {{
{struct_fields}
}}

impl Default for ReplayScripts {{
    fn default() -> Self {{
        Self {{
{defaults}
        }}
    }}
}}

impl ReplayScripts {{
    pub fn all_enabled() -> Self {{
        Self {{
{all_enabled}
        }}
    }}

    pub fn all_scripts() -> &'static [&'static str] {{
        &[ {all_scripts} ]
    }}
}}

impl std::fmt::Display for ReplayScripts {{
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {{
        fmt.write_str(include_str!(concat!(env!("OUT_DIR"), "/main.lua")))?;
{display_logic}
        Ok(())
    }}
}}
"#
    );

    let output_path = Path::new(out_dir).join("replay_scripts.rs");
    let mut file = fs::File::create(output_path).unwrap();
    writeln!(file, "{}", generated_code).unwrap();
}
