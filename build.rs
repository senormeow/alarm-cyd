fn main() {
    // Load .env file and emit WIFI_SSID / WIFI_PASSWORD as compile-time env vars
    println!("cargo:rerun-if-changed=.env");
    load_dotenv();

    linker_be_nice();
    println!("cargo:rustc-link-arg=-Tdefmt.x");
    // make sure linkall.x is the last linker script (otherwise might cause problems with flip-link)
    println!("cargo:rustc-link-arg=-Tlinkall.x");
    slint_build::compile_with_config(
        "ui/main.slint",
        slint_build::CompilerConfiguration::new()
            .embed_resources(slint_build::EmbedResourcesKind::EmbedForSoftwareRenderer),
    )
    .unwrap();
}

fn linker_be_nice() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let kind = &args[1];
        let what = &args[2];

        match kind.as_str() {
            "undefined-symbol" => match what.as_str() {
                "_defmt_timestamp" => {
                    eprintln!();
                    eprintln!(
                        "💡 `defmt` not found - make sure `defmt.x` is added as a linker script and you have included `use defmt_rtt as _;`"
                    );
                    eprintln!();
                }
                "_stack_start" => {
                    eprintln!();
                    eprintln!("💡 Is the linker script `linkall.x` missing?");
                    eprintln!();
                }
                "esp_rtos_initialized" | "esp_rtos_yield_task" | "esp_rtos_task_create" => {
                    eprintln!();
                    eprintln!(
                        "💡 `esp-radio` has no scheduler enabled. Make sure you have initialized `esp-rtos` or provided an external scheduler."
                    );
                    eprintln!();
                }
                "embedded_test_linker_file_not_added_to_rustflags" => {
                    eprintln!();
                    eprintln!(
                        "💡 `embedded-test` not found - make sure `embedded-test.x` is added as a linker script for tests"
                    );
                    eprintln!();
                }
                _ => (),
            },
            // we don't have anything helpful for "missing-lib" yet
            _ => {
                std::process::exit(1);
            }
        }

        std::process::exit(0);
    }

    println!(
        "cargo:rustc-link-arg=-Wl,--error-handling-script={}",
        std::env::current_exe().unwrap().display()
    );
}

/// Read `.env` in the project root and emit `cargo:rustc-env=KEY=VALUE` for
/// each line so that `env!("WIFI_SSID")` / `env!("WIFI_PASSWORD")` work in
/// library and binary crates without any extra dependencies.
fn load_dotenv() {
    let env_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".env");
    let contents = match std::fs::read_to_string(&env_path) {
        Ok(c) => c,
        Err(_) => {
            eprintln!();
            eprintln!("⚠️  No .env file found at {}", env_path.display());
            eprintln!("   Copy .env.example to .env and fill in your Wi-Fi credentials:");
            eprintln!("     cp .env.example .env");
            eprintln!();
            // Let the build continue — the env!() macros will produce a clear
            // compile error pointing at the missing variable.
            return;
        }
    };

    for (lineno, line) in contents.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"');
            println!("cargo:rustc-env={}={}", key, value);
        } else {
            eprintln!("⚠️  .env:{}: ignoring malformed line: {}", lineno + 1, line);
        }
    }
}
