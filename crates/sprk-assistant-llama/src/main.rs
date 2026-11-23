//! The assistant helper with the llama.cpp engine. Same pipe protocol as every helper
//! (`sprk_assistant::host`); no network code, nothing written to disk.
mod engine;

use sprk_assistant::engine::AssistantEngine;
use sprk_assistant::host::{serve, Factory, LoadParams};
use std::io::{stdin, stdout, BufReader};

fn factory() -> Box<Factory> {
    Box::new(|p: &LoadParams| {
        engine::LlamaEngine::new(p).map(|e| Box::new(e) as Box<dyn AssistantEngine>)
    })
}

fn main() {
    serve(BufReader::new(stdin()), stdout(), factory());
}
