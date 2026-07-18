use std::process::Command;

fn cipher_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cipher"))
}

fn temp_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("Failed to create temp dir")
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
        stdout.contains("No glossary entries found"),
        "got: {stdout}"
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
