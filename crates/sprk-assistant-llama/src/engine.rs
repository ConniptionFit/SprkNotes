//! llama.cpp behind `AssistantEngine`. One context per request (so memory for the working state
//! exists only while answering), the model itself memory-mapped so the OS can reclaim it.
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::sampling::LlamaSampler;
use sprk_assistant::engine::{
    AssistantEngine, Control, EngineError, GenRequest, GenStats, RunControl,
};
use sprk_assistant::host::LoadParams;
use sprk_assistant::stream::{StopFilter, Utf8Pending};
use sprk_models::Manifest;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Tokens decoded per step while reading the prompt. Control is checked between steps, so a
/// hold or cancel takes effect within one step even during a long prompt.
const PREFILL_CHUNK: usize = 256;
const SEED: u32 = 1234;

pub struct LlamaEngine {
    weights: PathBuf,
    threads: i32,
    context_tokens: u32,
    // The model borrows nothing from the backend, but the backend must outlive it.
    loaded: Option<Loaded>,
}

struct Loaded {
    model: LlamaModel,
    backend: LlamaBackend,
}

impl LlamaEngine {
    /// Reads `model.json` in the pack folder to find the weights file. Nothing is loaded yet.
    pub fn new(p: &LoadParams) -> Result<Self, EngineError> {
        let dir = PathBuf::from(&p.model_dir);
        let json =
            std::fs::read_to_string(dir.join("model.json")).map_err(|_| EngineError::LoadFailed)?;
        let manifest = Manifest::from_json(&json).map_err(|_| EngineError::LoadFailed)?;
        let name = manifest
            .files
            .get("weights")
            .ok_or(EngineError::LoadFailed)?;
        let spec = manifest.assistant.ok_or(EngineError::LoadFailed)?;
        Ok(Self {
            weights: dir.join(name),
            threads: i32::try_from(p.threads.max(1)).unwrap_or(1),
            context_tokens: p.context_tokens.clamp(512, spec.context_tokens),
            loaded: None,
        })
    }
}

fn wait_while_held(control: &dyn RunControl) -> Result<(), EngineError> {
    loop {
        match control.state() {
            Control::Run => return Ok(()),
            Control::Cancel => return Err(EngineError::Cancelled),
            Control::Hold => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

impl AssistantEngine for LlamaEngine {
    fn is_loaded(&self) -> bool {
        self.loaded.is_some()
    }

    fn load(&mut self) -> Result<(), EngineError> {
        if self.loaded.is_some() {
            return Ok(());
        }
        let mut backend = LlamaBackend::init().map_err(|_| EngineError::LoadFailed)?;
        backend.void_logs();
        let params = LlamaModelParams::default().with_use_mmap(true);
        let model = LlamaModel::load_from_file(&backend, &self.weights, &params)
            .map_err(|_| EngineError::LoadFailed)?;
        self.loaded = Some(Loaded { model, backend });
        Ok(())
    }

    fn unload(&mut self) {
        self.loaded = None;
    }

    fn generate(
        &mut self,
        req: &GenRequest,
        control: &dyn RunControl,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<GenStats, EngineError> {
        let Some(l) = self.loaded.as_ref() else {
            return Err(EngineError::NotLoaded);
        };
        let vocab = l.model.vocab();
        // The prompt format already holds the special tokens, so they are parsed, not added.
        let tokens = vocab.tokenize(req.prompt.as_bytes(), false, true);
        if tokens.is_empty() {
            return Err(EngineError::Failed);
        }
        let need = tokens.len() + req.max_tokens as usize + 8;
        if need > self.context_tokens as usize {
            return Err(EngineError::Failed);
        }
        let n_ctx = NonZeroU32::new(u32::try_from(need).unwrap_or(u32::MAX));
        let cparams = LlamaContextParams::default()
            .with_n_ctx(n_ctx)
            .with_n_batch(PREFILL_CHUNK as u32)
            .with_n_ubatch(PREFILL_CHUNK as u32)
            .with_n_threads(self.threads)
            .with_n_threads_batch(self.threads);
        let mut ctx = l
            .model
            .new_context(&l.backend, cparams)
            .map_err(|_| EngineError::Failed)?;
        let started = Instant::now();
        let mut batch = LlamaBatch::new(PREFILL_CHUNK, 1);

        // Read the prompt in steps, yielding to hold and cancel between steps.
        let last = tokens.len() - 1;
        for (ci, chunk) in tokens.chunks(PREFILL_CHUNK).enumerate() {
            wait_while_held(control)?;
            batch.clear();
            for (i, t) in chunk.iter().enumerate() {
                let pos = ci * PREFILL_CHUNK + i;
                let pos_i = i32::try_from(pos).map_err(|_| EngineError::Failed)?;
                batch
                    .add(*t, pos_i, &[0], pos == last)
                    .map_err(|_| EngineError::Failed)?;
            }
            ctx.decode(&mut batch).map_err(|_| EngineError::Failed)?;
        }

        let mut sampler = if req.temperature <= 0.01 {
            LlamaSampler::greedy()
        } else {
            LlamaSampler::chain_simple([
                LlamaSampler::top_k(40),
                LlamaSampler::top_p(0.9, 1),
                LlamaSampler::temp(req.temperature),
                LlamaSampler::dist(SEED),
            ])
        };
        let mut utf8 = Utf8Pending::default();
        let mut stops = StopFilter::new(&req.stop);
        let mut produced = 0u32;
        let mut pos = tokens.len();
        let mut at = batch.n_tokens() - 1;
        while produced < req.max_tokens {
            wait_while_held(control)?;
            let token = sampler.sample(&ctx, at);
            if vocab.is_eog(token) {
                break;
            }
            produced += 1;
            let text = utf8.push(&vocab.token_to_piece(token, false, None));
            let shown = stops.push(&text);
            if !shown.is_empty() {
                on_text(&shown);
            }
            if stops.is_done() {
                break;
            }
            batch.clear();
            let pos_i = i32::try_from(pos).map_err(|_| EngineError::Failed)?;
            batch
                .add(token, pos_i, &[0], true)
                .map_err(|_| EngineError::Failed)?;
            pos += 1;
            ctx.decode(&mut batch).map_err(|_| EngineError::Failed)?;
            at = 0;
        }
        let rest = stops.finish();
        if !rest.is_empty() {
            on_text(&rest);
        }
        Ok(GenStats {
            prompt_tokens: u32::try_from(tokens.len()).unwrap_or(u32::MAX),
            output_tokens: produced,
            load_ms: 0,
            generate_ms: u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX),
        })
    }
}
