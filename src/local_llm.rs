use anyhow::{bail, Context, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::llama::{Cache, Llama, LlamaConfig};
use hf_hub::{api::sync::Api, Repo, RepoType};
use std::fs;
use std::path::Path;
use tokenizers::Tokenizer;

const MAX_TOKENS: usize = 256;
const TEMPERATURE: f64 = 0.7;
const TOP_P: f64 = 0.9;

pub fn run(file: &Path, model_id: &str, _force_download: bool, verbose: u8) -> Result<()> {
    if verbose > 0 {
        eprintln!("Summarizing: {}", file.display());
    }

    // Read file content
    let content = fs::read_to_string(file)
        .with_context(|| format!("Failed to read file: {}", file.display()))?;

    // Truncate content if too long (keep first ~2000 chars for context)
    let truncated = if content.len() > 2000 {
        format!("{}...\n[truncated]", &content[..2000])
    } else {
        content.clone()
    };

    // Build prompt
    let prompt = format!(
        r#"<|begin_of_text|><|start_header_id|>system<|end_header_id|>
You are a code analysis assistant. Provide a 2-line technical summary of the given code. Be concise and focus on:
1. What the code does (purpose/functionality)
2. Key implementation details (patterns, algorithms, dependencies)
<|eot_id|><|start_header_id|>user<|end_header_id|>
Summarize this code in exactly 2 lines:

```
{}
```
<|eot_id|><|start_header_id|>assistant<|end_header_id|>
"#,
        truncated
    );

    if verbose > 1 {
        eprintln!("Loading model: {}", model_id);
    }

    // Download and load model
    let device = Device::Cpu;
    let (model, tokenizer, config) = load_model(model_id, &device, verbose)?;

    // Generate summary
    let summary = generate(&model, &tokenizer, &config, &prompt, &device, verbose)?;

    // Output only the summary (first 2 non-empty lines)
    let lines: Vec<&str> = summary
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .take(2)
        .collect();

    for line in lines {
        println!("{}", line);
    }

    Ok(())
}

fn load_model(
    model_id: &str,
    device: &Device,
    verbose: u8,
) -> Result<(Llama, Tokenizer, candle_transformers::models::llama::Config)> {
    let api = Api::new().context("Failed to create HuggingFace API client")?;
    let repo = api.repo(Repo::new(model_id.to_string(), RepoType::Model));

    if verbose > 0 {
        eprintln!("Downloading model files from HuggingFace...");
    }

    // Download required files
    let config_path = repo
        .get("config.json")
        .context("Failed to download config.json")?;
    let tokenizer_path = repo
        .get("tokenizer.json")
        .context("Failed to download tokenizer.json")?;

    // For smaller models, try to get a single safetensors file
    let weights_path = repo
        .get("model.safetensors")
        .or_else(|_| repo.get("pytorch_model.bin"))
        .context("Failed to download model weights")?;

    if verbose > 1 {
        eprintln!("Config: {}", config_path.display());
        eprintln!("Tokenizer: {}", tokenizer_path.display());
        eprintln!("Weights: {}", weights_path.display());
    }

    // Load config
    let config_str = fs::read_to_string(&config_path)?;
    let llama_config: LlamaConfig = serde_json::from_str(&config_str)?;
    let config = llama_config.into_config(false);

    // Load tokenizer
    let tokenizer = Tokenizer::from_file(&tokenizer_path)
        .map_err(|e| anyhow::anyhow!("Tokenizer error: {}", e))?;

    // Load model weights
    if verbose > 0 {
        eprintln!("Loading model into memory...");
    }

    let vb = if weights_path
        .extension()
        .map(|e| e == "safetensors")
        .unwrap_or(false)
    {
        unsafe { VarBuilder::from_mmaped_safetensors(&[weights_path], DType::F32, device)? }
    } else {
        bail!("Only safetensors format is supported");
    };

    let model = Llama::load(vb, &config)?;

    Ok((model, tokenizer, config))
}

fn generate(
    model: &Llama,
    tokenizer: &Tokenizer,
    config: &candle_transformers::models::llama::Config,
    prompt: &str,
    device: &Device,
    verbose: u8,
) -> Result<String> {
    // Tokenize input
    let tokens = tokenizer
        .encode(prompt, true)
        .map_err(|e| anyhow::anyhow!("Tokenization error: {}", e))?;

    let input_ids: Vec<u32> = tokens.get_ids().to_vec();
    let mut all_tokens = input_ids.clone();

    if verbose > 1 {
        eprintln!("Input tokens: {}", input_ids.len());
    }

    // Create logits processor
    let mut logits_processor = LogitsProcessor::new(42, Some(TEMPERATURE), Some(TOP_P));

    // Create KV cache
    let mut cache = Cache::new(false, DType::F32, config, device)?;

    // Generate tokens
    let mut pos = 0usize;

    for _ in 0..MAX_TOKENS {
        let input = if pos == 0 {
            Tensor::new(&input_ids[..], device)?.unsqueeze(0)?
        } else {
            let last_token = *all_tokens.last().unwrap();
            Tensor::new(&[last_token], device)?.unsqueeze(0)?
        };

        let logits = model.forward(&input, pos, &mut cache)?;
        let logits = logits.squeeze(0)?;
        let logits = logits.get(logits.dim(0)? - 1)?;

        let next_token = logits_processor.sample(&logits)?;

        // Check for EOS token
        if next_token == 128001 || next_token == 128009 {
            // Llama 3 EOS tokens
            break;
        }

        all_tokens.push(next_token);

        if pos == 0 {
            pos = input_ids.len();
        } else {
            pos += 1;
        }

        if verbose > 2 {
            eprint!(".");
        }
    }

    if verbose > 2 {
        eprintln!();
    }

    // Decode output (only the generated part)
    let output_tokens = &all_tokens[input_ids.len()..];
    let output = tokenizer
        .decode(output_tokens, true)
        .map_err(|e| anyhow::anyhow!("Decoding error: {}", e))?;

    Ok(output)
}

#[cfg(test)]
mod tests {
    // Integration tests would require model download
    // Skip in CI, run manually with: cargo test -- --ignored

    #[test]
    #[ignore]
    fn test_model_loading() {
        // This test requires network access and ~2GB download
        // Run with: cargo test test_model_loading -- --ignored
    }
}
