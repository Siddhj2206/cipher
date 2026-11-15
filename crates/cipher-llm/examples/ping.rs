//! Manual check against a real Endpoint. Not run by tests.
//!
//! CIPHER_BASE_URL=https://openrouter.ai/api/v1 CIPHER_MODEL=... CIPHER_API_KEY=... \
//!   cargo run -p cipher-llm --example ping

use std::env;
use std::io::Write;

use cipher_llm::{ChatCompletions, Effort, Model, Request, Timeouts};

fn main() {
    let var = |name: &str| {
        env::var(name).unwrap_or_else(|_| {
            eprintln!("set {name}");
            std::process::exit(2);
        })
    };
    let model = ChatCompletions::new(
        &var("CIPHER_BASE_URL"),
        var("CIPHER_MODEL"),
        env::var("CIPHER_API_KEY").ok(),
        Timeouts::default(),
    );
    let req = Request {
        instructions: "You are a helpful assistant.".into(),
        input: "Say hi".into(),
        max_output: 512,
        reasoning: Effort::ProviderDefault,
    };
    let result = model.complete(&req, &mut |piece| {
        print!("{piece}");
        let _ = std::io::stdout().flush();
    });
    println!();
    match result {
        Ok(c) => {
            println!("finished: {}", c.finished);
            println!("usage: {:?}", c.usage);
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
