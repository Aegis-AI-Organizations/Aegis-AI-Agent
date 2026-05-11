use ndarray::Array2;
use tokenizers::Tokenizer;
use tracing::{info, warn};
use tract_onnx::prelude::*;

pub struct NlpEngine {
    model: RunnableModel<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>,
    tokenizer: Tokenizer,
}

impl NlpEngine {
    pub fn new() -> anyhow::Result<Self> {
        let model_path =
            std::env::var("AEGIS_MODEL_PATH").unwrap_or_else(|_| "models/model.onnx".to_string());
        let tokenizer_path = std::env::var("AEGIS_TOKENIZER_PATH")
            .unwrap_or_else(|_| "models/tokenizer.json".to_string());

        if !std::path::Path::new(&model_path).exists()
            || !std::path::Path::new(&tokenizer_path).exists()
        {
            anyhow::bail!(
                "NLP Model or Tokenizer not found at {} and {}",
                model_path,
                tokenizer_path
            );
        }

        info!("Loading NLP model from {}...", model_path);
        let model = tract_onnx::onnx()
            .model_for_path(model_path)?
            .into_optimized()?
            .into_runnable()?;

        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| anyhow::anyhow!("Failed to load tokenizer: {}", e))?;

        info!("NLP engine initialized successfully");
        Ok(Self { model, tokenizer })
    }

    pub fn redact(&self, input: &str) -> String {
        match self.process(input) {
            Ok(redacted) => redacted,
            Err(e) => {
                warn!("NLP redaction failed, returning original input: {}", e);
                input.to_string()
            }
        }
    }

    fn process(&self, input: &str) -> anyhow::Result<String> {
        let encoding = self
            .tokenizer
            .encode(input, true)
            .map_err(|e| anyhow::anyhow!("Tokenization failed: {}", e))?;

        let ids = encoding.get_ids();
        let input_ids =
            Array2::from_shape_vec((1, ids.len()), ids.iter().map(|&x| x as i64).collect())?;

        // Tract expects Tensors
        let tensor = tract_ndarray::Array::from(input_ids).into_tensor();

        let result = self.model.run(tvec!(tensor.into()))?;
        let logits = result[0].to_array_view::<f32>()?;

        // Simplified post-processing: map B-PER, I-PER etc to <REDACTED_PERSON>
        // In a real DistilBERT-NER model:
        // 0: O, 1: B-PER, 2: I-PER, 3: B-ORG, 4: I-ORG, 5: B-LOC, 6: I-LOC, 7: B-MISC, 8: I-MISC

        let mut redacted_indices = Vec::new();

        for i in 0..logits.shape()[1] {
            let mut max_val = f32::MIN;
            let mut max_idx = 0;
            for j in 0..logits.shape()[2] {
                let val = logits[[0, i, j]];
                if val > max_val {
                    max_val = val;
                    max_idx = j;
                }
            }

            match max_idx {
                1 | 2 => redacted_indices.push((i, "<REDACTED_PERSON>")),
                3 | 4 => redacted_indices.push((i, "<REDACTED_ORG>")),
                5 | 6 => redacted_indices.push((i, "<REDACTED_LOC>")),
                _ => {}
            }
        }

        // Reconstruct string (very simplified)
        if redacted_indices.is_empty() {
            return Ok(input.to_string());
        }

        // In a real implementation, we would use the offsets from the tokenizer
        // to replace the exact characters in the input string.
        let offsets = encoding.get_offsets();
        let mut last_pos = 0;
        let mut new_text = String::new();
        let mut current_redaction: Option<&str> = None;

        for (i, label) in redacted_indices {
            let (start, end) = offsets[i];
            if start == 0 && end == 0 {
                continue;
            } // Skip special tokens

            if let Some(prev_label) = current_redaction {
                if prev_label == label {
                    // Continue current redaction, don't add new tag yet
                    continue;
                }
            }

            new_text.push_str(&input[last_pos..start]);
            new_text.push_str(label);
            last_pos = end;
            current_redaction = Some(label);
        }
        new_text.push_str(&input[last_pos..]);

        Ok(new_text)
    }
}
