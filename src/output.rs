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
