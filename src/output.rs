use std::fmt::Display;

pub fn detail(message: impl Display) {
    println!("- {}", message);
}

pub fn detail_kv(label: &str, value: impl Display) {
    println!("- {}: {}", label, value);
}

pub fn stderr_detail(message: impl Display) {
    eprintln!("- {}", message);
}

pub fn warn(message: impl Display) {
    println!("- Warning: {}", message);
}

pub fn stderr_warn(message: impl Display) {
    eprintln!("- Warning: {}", message);
}

pub fn stderr_error(message: impl Display) {
    eprintln!("Error: {}", message);
}

pub fn section(header: impl Display) {
    println!();
    println!("{}", header);
}
