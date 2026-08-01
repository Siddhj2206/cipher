use std::process::Command;

fn cipher_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cipher"))
}

fn temp_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("Failed to create temp dir")
}

/// Command with an isolated global config: XDG_CONFIG_HOME and HOME point at
/// the temp dir so tests never touch the real `~/.config/cipher/config.toml`.
fn isolated_command(home: &tempfile::TempDir) -> Command {
    let xdg = home.path().join("config");
    std::fs::create_dir_all(&xdg).expect("Failed to create isolated config dir");
    let mut cmd = cipher_binary();
    cmd.env("XDG_CONFIG_HOME", &xdg).env("HOME", home.path());
    cmd
}

fn init_book(cmd: &mut Command, book_dir: &std::path::Path) {
    let output = cmd.arg("init").arg(book_dir).output().expect("init failed");
    assert!(
        output.status.success(),
        "init failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_chapter(book_dir: &std::path::Path) {
    std::fs::write(book_dir.join("raw").join("001.md"), "# Chapter 1\n\nText\n")
        .expect("write chapter");
}

fn write_global_config(home: &tempfile::TempDir, content: &str) {
    let config_path = home
        .path()
        .join("config")
        .join("cipher")
        .join("config.toml");
    std::fs::create_dir_all(config_path.parent().unwrap()).expect("create config dir");
    std::fs::write(&config_path, content).expect("write global config");
}

/// Isolated book with one raw chapter, ready to translate.
fn book_with_chapter(home: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
    let book_path = home.path().join(name);
    init_book(&mut isolated_command(home), &book_path);
    write_chapter(&book_path);
    book_path
}

#[test]
fn init_creates_book_structure() {
    let dir = temp_dir();
    let book_path = dir.path().join("test-book");

    let output = cipher_binary()
        .arg("init")
        .arg(&book_path)
        .output()
        .expect("Failed to run cipher init");

    assert!(
        output.status.success(),
        "init failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        book_path.join("cipher.toml").exists(),
        "cipher.toml missing"
    );
    assert!(book_path.join("raw").exists(), "raw/ missing");
    assert!(book_path.join("tl").exists(), "tl/ missing");
    assert!(
        book_path.join("glossary.json").exists(),
        "glossary.json missing"
    );
    assert!(book_path.join("style.md").exists(), "style.md missing");
}

#[test]
fn init_on_existing_dir_does_not_fail() {
    let dir = temp_dir();
    let book_path = dir.path().join("another-book");

    let first = cipher_binary()
        .arg("init")
        .arg(&book_path)
        .output()
        .expect("First init failed");
    assert!(first.status.success());

    let second = cipher_binary()
        .arg("init")
        .arg(&book_path)
        .output()
        .expect("Second init failed");
    assert!(
        second.status.success(),
        "re-init should succeed: {:?}",
        String::from_utf8_lossy(&second.stderr)
    );
}

#[test]
fn doctor_without_book_succeeds() {
    let output = cipher_binary()
        .arg("doctor")
        .output()
        .expect("Failed to run cipher doctor");

    assert!(
        output.status.success(),
        "doctor failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn status_on_nonexistent_dir_does_not_crash() {
    let dir = temp_dir();
    let book_path = dir.path().join("nonexistent-book");

    let output = cipher_binary()
        .arg("status")
        .arg(&book_path)
        .output()
        .expect("Failed to run cipher status");

    assert!(
        output.status.success(),
        "status should not crash on nonexistent dir"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No translation runs recorded yet"),
        "status should indicate no runs, got: {stdout}"
    );
}

#[test]
fn doctor_on_initialized_book_succeeds() {
    let dir = temp_dir();
    let book_path = dir.path().join("doctor-book");

    let init = cipher_binary()
        .arg("init")
        .arg(&book_path)
        .output()
        .expect("init failed");
    assert!(init.status.success());

    let output = cipher_binary()
        .arg("doctor")
        .arg(&book_path)
        .output()
        .expect("doctor failed");

    assert!(
        output.status.success(),
        "doctor on book should succeed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Book layout looks valid"), "got: {stdout}");
}

#[test]
fn glossary_list_on_initialized_book_is_empty() {
    let dir = temp_dir();
    let book_path = dir.path().join("glossary-book");

    let init = cipher_binary()
        .arg("init")
        .arg(&book_path)
        .output()
        .expect("init failed");
    assert!(init.status.success());

    let output = cipher_binary()
        .arg("glossary")
        .arg("list")
        .arg(&book_path)
        .output()
        .expect("glossary list failed");

    assert!(output.status.success(), "glossary list: {:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Glossary entries"),
        "expected 'Glossary entries' in stdout, got: {stdout}"
    );
    assert!(
        stdout.contains("No entries found"),
        "expected 'No entries found' in stdout, got: {stdout}"
    );
}

#[test]
fn glossary_import_adds_entries() {
    let dir = temp_dir();
    let book_path = dir.path().join("import-book");

    let init = cipher_binary()
        .arg("init")
        .arg(&book_path)
        .output()
        .expect("init failed");
    assert!(init.status.success());

    let import_file = dir.path().join("import.json");
    let import_data = r#"[
        {"term": "foo", "og_term": null, "definition": "the foo", "notes": null},
        {"term": "bar", "og_term": "bar", "definition": "the bar", "notes": "a note"}
    ]"#;
    std::fs::write(&import_file, import_data).expect("write import file");

    let output = cipher_binary()
        .arg("glossary")
        .arg("import")
        .arg("--file")
        .arg(&import_file)
        .arg(&book_path)
        .output()
        .expect("glossary import failed");
    assert!(
        output.status.success(),
        "import failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Added"), "got: {stderr}");

    let list = cipher_binary()
        .arg("glossary")
        .arg("list")
        .arg(&book_path)
        .output()
        .expect("glossary list failed");
    assert!(list.status.success());
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(
        stdout.contains("foo"),
        "list should show foo, got: {stdout}"
    );
    assert!(
        stdout.contains("bar"),
        "list should show bar, got: {stdout}"
    );
}

#[test]
fn glossary_export_writes_file() {
    let dir = temp_dir();
    let book_path = dir.path().join("export-book");

    let init = cipher_binary()
        .arg("init")
        .arg(&book_path)
        .output()
        .expect("init failed");
    assert!(init.status.success());

    let import_file = dir.path().join("import.json");
    let import_data = r#"[
        {"term": "foo", "og_term": null, "definition": "the foo", "notes": null}
    ]"#;
    std::fs::write(&import_file, import_data).expect("write import file");

    cipher_binary()
        .arg("glossary")
        .arg("import")
        .arg("--file")
        .arg(&import_file)
        .arg(&book_path)
        .output()
        .expect("import failed");

    let export_file = dir.path().join("exported.json");
    let output = cipher_binary()
        .arg("glossary")
        .arg("export")
        .arg("--output")
        .arg(&export_file)
        .arg(&book_path)
        .output()
        .expect("glossary export failed");

    assert!(
        output.status.success(),
        "export failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(export_file.exists(), "export file should exist");
    let content = std::fs::read_to_string(&export_file).expect("read export");
    assert!(content.contains("foo"), "export should contain foo");
}

#[test]
fn translate_quiet_and_verbose_flags_parse() {
    let dir = temp_dir();
    let book_path = dir.path().join("quiet-book");

    let init = cipher_binary()
        .arg("init")
        .arg(&book_path)
        .output()
        .expect("init failed");
    assert!(init.status.success());

    for flag in &["--quiet", "--verbose"] {
        let output = cipher_binary()
            .arg("translate")
            .arg(flag)
            .arg("--dry-run")
            .arg(&book_path)
            .output()
            .expect("translate command failed");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("unexpected argument"),
            "flag {flag} was not recognized: {stderr}"
        );
    }
}

#[test]
fn status_json_output_is_valid() {
    let dir = temp_dir();
    let book_path = dir.path().join("json-book");

    let init = cipher_binary()
        .arg("init")
        .arg(&book_path)
        .output()
        .expect("init failed");
    assert!(init.status.success());

    let output = cipher_binary()
        .arg("status")
        .arg("--json")
        .arg(&book_path)
        .output()
        .expect("status --json failed");

    assert!(
        output.status.success(),
        "status --json: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should be parseable JSON with expected keys
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("status --json should be valid JSON");
    assert!(
        parsed.get("book").is_some() && parsed.get("chapters").is_some(),
        "json output should contain 'book' and 'chapters', got keys: {:?}",
        parsed.as_object().map(|o| o.keys().collect::<Vec<_>>())
    );
}

#[test]
fn translate_json_output_is_valid_report() {
    let dir = temp_dir();
    let book_path = dir.path().join("translate-json-book");

    let init = cipher_binary()
        .arg("init")
        .arg(&book_path)
        .output()
        .expect("init failed");
    assert!(init.status.success());
    std::fs::write(
        book_path.join("raw").join("001.md"),
        "# Chapter 1\n\nText\n",
    )
    .expect("write chapter");

    let output = cipher_binary()
        .arg("translate")
        .arg("--json")
        .arg("--dry-run")
        .arg(&book_path)
        .output()
        .expect("translate --json failed");

    assert!(
        output.status.success(),
        "translate --json: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("translate --json should be valid JSON");
    assert_eq!(parsed["dry_run"], true, "got keys: {parsed}");
    for key in ["book", "chapters", "summary", "exit_code"] {
        assert!(
            parsed.get(key).is_some(),
            "json report should contain '{key}', got keys: {:?}",
            parsed.as_object().map(|o| o.keys().collect::<Vec<_>>())
        );
    }
    assert_eq!(parsed["chapters"].as_array().map(Vec::len), Some(1));
    assert_eq!(parsed["chapters"][0]["action"], "translate");
    assert_eq!(parsed["summary"]["translate"], 1);
}

#[test]
fn translate_json_emits_typed_error_envelope_on_failure() {
    let dir = temp_dir();
    let book_path = dir.path().join("not-a-book");

    let output = cipher_binary()
        .arg("translate")
        .arg("--json")
        .arg(&book_path)
        .output()
        .expect("translate --json failed");

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("translate --json error should be valid JSON");
    assert_eq!(parsed["error"]["code"], "E006");
    assert_eq!(parsed["error"]["exit_code"], 1);
    assert!(
        parsed["error"]["message"].is_string(),
        "error envelope should carry a message"
    );
}

#[test]
fn translate_on_invalid_book_layout_fails_with_e006() {
    let home = temp_dir();
    let book_path = home.path().join("not-a-book");

    let output = isolated_command(&home)
        .arg("translate")
        .arg(&book_path)
        .output()
        .expect("translate failed");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Invalid book layout"), "got: {stderr}");
}

#[test]
fn translate_without_global_config_fails_with_e006() {
    let home = temp_dir();
    let book_path = book_with_chapter(&home, "book");

    let output = isolated_command(&home)
        .arg("translate")
        .arg(&book_path)
        .output()
        .expect("translate failed");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("No profile configured"), "got: {stderr}");
}

#[test]
fn translate_json_without_global_config_emits_e006_envelope() {
    let home = temp_dir();
    let book_path = book_with_chapter(&home, "book");

    let output = isolated_command(&home)
        .arg("translate")
        .arg("--json")
        .arg(&book_path)
        .output()
        .expect("translate --json failed");

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("error envelope should be valid JSON");
    assert_eq!(parsed["error"]["code"], "E006");
    assert_eq!(parsed["error"]["exit_code"], 1);
    assert!(
        parsed["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("No profile configured"),
        "got: {parsed}"
    );
}

#[test]
fn translate_with_profile_missing_api_key_fails_with_e006() {
    let home = temp_dir();
    let book_path = book_with_chapter(&home, "book");
    write_global_config(
        &home,
        r#"
        default_profile = "nokey"

        [providers.gemini]
        kind = "gemini"

        [profiles.nokey]
        provider = "gemini"
        model = "gemini-2.5-flash"
        "#,
    );

    let output = isolated_command(&home)
        .arg("translate")
        .arg(&book_path)
        .output()
        .expect("translate failed");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No API key configured for provider 'gemini'"),
        "got: {stderr}"
    );
    assert!(
        stderr.contains("Cannot translate with invalid translation profile"),
        "got: {stderr}"
    );
}

#[test]
fn translate_with_provider_failure_marks_chapter_failed_and_exits_2() {
    let home = temp_dir();
    let book_path = book_with_chapter(&home, "book");
    write_global_config(
        &home,
        r#"
        default_profile = "local"

        [providers.local]
        kind = "openai_compatible"
        base_url = "http://127.0.0.1:1/v1"
        keys = [{ value = "fake-key" }]

        [profiles.local]
        provider = "local"
        model = "test-model"
        "#,
    );

    let output = isolated_command(&home)
        .arg("translate")
        .arg(&book_path)
        .output()
        .expect("translate failed");

    // Provider errors surface as typed chapter failures; the run then exits
    // with the chapters-failed code.
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("API error") && stderr.contains("request failed"),
        "provider error should surface in chapter failure, got: {stderr}"
    );
    assert!(stderr.contains("1 failed"), "got: {stderr}");
}

#[test]
fn glossary_import_missing_file_fails_with_e002() {
    let home = temp_dir();
    let book_path = home.path().join("book");
    init_book(&mut isolated_command(&home), &book_path);

    let output = isolated_command(&home)
        .arg("glossary")
        .arg("import")
        .arg("--file")
        .arg(home.path().join("missing.json"))
        .arg(&book_path)
        .output()
        .expect("glossary import failed");

    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("[E002]"), "got: {stderr}");
    assert!(
        stderr.contains("missing.json"),
        "error should name the missing import file: {stderr}"
    );
}

#[test]
fn translate_with_malformed_book_config_fails_with_e001() {
    let home = temp_dir();
    let book_path = book_with_chapter(&home, "book");
    std::fs::write(book_path.join("cipher.toml"), "this is not valid toml [[[[")
        .expect("write malformed book config");

    let output = isolated_command(&home)
        .arg("translate")
        .arg(&book_path)
        .output()
        .expect("translate failed");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("[E001]"), "got: {stderr}");
}

#[test]
fn glossary_import_invalid_json_fails_with_e004() {
    let home = temp_dir();
    let book_path = home.path().join("book");
    init_book(&mut isolated_command(&home), &book_path);
    let import_file = home.path().join("broken.json");
    std::fs::write(&import_file, "this is not json").expect("write broken import file");

    let output = isolated_command(&home)
        .arg("glossary")
        .arg("import")
        .arg("--file")
        .arg(&import_file)
        .arg(&book_path)
        .output()
        .expect("glossary import failed");

    assert_eq!(output.status.code(), Some(6));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("[E004]"), "got: {stderr}");
}

#[test]
fn profile_show_missing_profile_fails_with_e003() {
    let home = temp_dir();

    let output = isolated_command(&home)
        .arg("profile")
        .arg("show")
        .arg("nope")
        .output()
        .expect("profile show failed");

    assert_eq!(output.status.code(), Some(5));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("[E003]"), "got: {stderr}");
}

#[test]
fn verbose_dry_run_emits_per_chapter_reasons() {
    let home = temp_dir();
    let book_path = book_with_chapter(&home, "book");

    let output = isolated_command(&home)
        .arg("translate")
        .arg("--verbose")
        .arg("--dry-run")
        .arg(&book_path)
        .output()
        .expect("verbose dry-run failed");

    assert!(output.status.success(), "got: {:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Reason"),
        "verbose should include reasons: {stderr}"
    );
    assert!(
        stderr.contains("No output exists yet"),
        "verbose should include the preview reason: {stderr}"
    );
}

#[test]
fn status_json_on_corrupt_state_emits_e007_envelope() {
    let home = temp_dir();
    let book_path = book_with_chapter(&home, "book");

    let state_dir = book_path.join(".cipher");
    std::fs::create_dir_all(&state_dir).expect("create state dir");
    std::fs::write(state_dir.join("run.json"), "this is not json").expect("write corrupt state");

    let output = isolated_command(&home)
        .arg("status")
        .arg("--json")
        .arg(&book_path)
        .output()
        .expect("status --json failed");

    assert_eq!(output.status.code(), Some(70));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("error envelope should be valid JSON");
    assert_eq!(parsed["error"]["code"], "E007");
    assert_eq!(parsed["error"]["exit_code"], 70);
}

#[test]
fn translate_json_provider_failure_marks_chapter_failed_and_exits_2() {
    let home = temp_dir();
    let book_path = book_with_chapter(&home, "book");

    write_global_config(
        &home,
        r#"
        default_profile = "badbase"

        [providers.badbase]
        kind = "openai_compatible"
        base_url = "://not-a-url"
        keys = [{ value = "fake-key" }]

        [profiles.badbase]
        provider = "badbase"
        model = "test-model"
        "#,
    );

    let output = isolated_command(&home)
        .arg("translate")
        .arg("--json")
        .arg(&book_path)
        .output()
        .expect("translate --json failed");

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("report should be valid JSON");
    assert_eq!(parsed["summary"]["failed"], 1, "got: {parsed}");
    assert_eq!(parsed["chapters"][0]["status"], "failed", "got: {parsed}");
    assert_eq!(parsed["exit_code"], 2, "got: {parsed}");
}
