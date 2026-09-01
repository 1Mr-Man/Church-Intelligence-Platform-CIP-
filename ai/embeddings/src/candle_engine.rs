//! Real local `EmbeddingEngine` backend: `sentence-transformers/all-MiniLM-L6-v2`
//! run through `candle`/`candle-transformers`'s BERT implementation, entirely
//! on-device - no network call, no subscription, matching
//! `cip_ai_speech::WhisperSpeechEngine`'s "fully local, fully free" contract
//! exactly (the constraint that drove Phase 4.4's architecture choice over
//! any cloud embedding API).
//!
//! Only two operator-supplied files are needed: `model.safetensors` and
//! `tokenizer.json` (both published on the model's Hugging Face page, and
//! provisioned the same way as the Whisper model - see
//! `docs/phase-4-4-semantic-bible-search.md`). No `config.json` is required:
//! all-MiniLM-L6-v2's architecture hyperparameters are fixed and reproduced
//! in [`all_mini_lm_l6_v2_config`] below, copied field-for-field from
//! `candle_transformers::models::bert::Config`'s own private
//! `_all_mini_lm_l6_v2()` constructor (that constructor isn't `pub`, so it
//! can't be called directly, but the values it hardcodes are simply
//! `sentence-transformers/all-MiniLM-L6-v2`'s published `config.json`,
//! which is a stable part of that model's identity, not an implementation
//! detail of candle's).

use std::path::Path;

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config, HiddenAct, PositionEmbeddingType};
use tokenizers::Tokenizer;

use cip_core_ai::{EmbeddingEngine, EmbeddingEngineError};

use crate::pooling::{l2_normalize, mean_pool};

/// `sentence-transformers/all-MiniLM-L6-v2`'s published `config.json`,
/// reproduced verbatim - see this module's doc comment for why.
fn all_mini_lm_l6_v2_config() -> Config {
    Config {
        vocab_size: 30522,
        hidden_size: 384,
        num_hidden_layers: 6,
        num_attention_heads: 12,
        intermediate_size: 1536,
        hidden_act: HiddenAct::Gelu,
        hidden_dropout_prob: 0.1,
        max_position_embeddings: 512,
        type_vocab_size: 2,
        initializer_range: 0.02,
        layer_norm_eps: 1e-12,
        pad_token_id: 0,
        position_embedding_type: PositionEmbeddingType::Absolute,
        use_cache: true,
        classifier_dropout: None,
        model_type: Some("bert".to_string()),
    }
}

/// `all-MiniLM-L6-v2`'s sentence-embedding width - the model card's own
/// number, not derived from anything at runtime.
const DIMENSIONS: usize = 384;

/// The stable identifier `bible_verse_embeddings` rows are keyed by (see
/// `EmbeddingEngine::model_id`'s doc comment for why this must never be a
/// display name or a path).
const MODEL_ID: &str = "all-MiniLM-L6-v2";

pub struct CandleEmbeddingEngine {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
}

impl CandleEmbeddingEngine {
    /// Loads the model from an operator-supplied `model.safetensors` and
    /// `tokenizer.json`. CPU-only (`Device::Cpu`): Bible verse embedding is
    /// small enough (single sentences, run interactively, not in bulk) that
    /// GPU acceleration isn't worth the extra build/runtime dependency -
    /// mirrors `WhisperSpeechEngine`'s own CPU-only choice.
    pub fn load(model_path: &Path, tokenizer_path: &Path) -> Result<Self, EmbeddingEngineError> {
        if !model_path.is_file() {
            return Err(EmbeddingEngineError::ModelNotFound(
                model_path.display().to_string(),
            ));
        }
        if !tokenizer_path.is_file() {
            return Err(EmbeddingEngineError::ModelNotFound(
                tokenizer_path.display().to_string(),
            ));
        }

        let device = Device::Cpu;
        let config = all_mini_lm_l6_v2_config();

        // SAFETY: `from_mmaped_safetensors` is `unsafe` because the memory
        // map is invalidated if the file is modified/truncated while
        // mapped; `model_path` is a static, operator-provisioned model file
        // that CIP never writes to, matching the same assumption
        // `whisper-rs`'s own model loading already makes about the Whisper
        // model file.
        let var_builder = unsafe {
            VarBuilder::from_mmaped_safetensors(&[model_path], DType::F32, &device)
                .map_err(|e| EmbeddingEngineError::EmbeddingFailed(e.to_string()))?
        };
        let model = BertModel::load(var_builder, &config)
            .map_err(|e| EmbeddingEngineError::EmbeddingFailed(e.to_string()))?;

        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| EmbeddingEngineError::EmbeddingFailed(e.to_string()))?;

        Ok(Self {
            model,
            tokenizer,
            device,
        })
    }
}

impl EmbeddingEngine for CandleEmbeddingEngine {
    fn is_ready(&self) -> bool {
        true
    }

    fn model_id(&self) -> &str {
        MODEL_ID
    }

    fn dimensions(&self) -> usize {
        DIMENSIONS
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingEngineError> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| EmbeddingEngineError::EmbeddingFailed(e.to_string()))?;

        let token_ids = encoding.get_ids();
        let attention_mask = encoding.get_attention_mask();

        let token_ids_tensor = Tensor::new(token_ids, &self.device)
            .and_then(|t| t.unsqueeze(0))
            .map_err(|e| EmbeddingEngineError::EmbeddingFailed(e.to_string()))?;
        let token_type_ids = token_ids_tensor
            .zeros_like()
            .map_err(|e| EmbeddingEngineError::EmbeddingFailed(e.to_string()))?;
        let attention_mask_tensor = Tensor::new(attention_mask, &self.device)
            .and_then(|t| t.unsqueeze(0))
            .map_err(|e| EmbeddingEngineError::EmbeddingFailed(e.to_string()))?;

        let output = self
            .model
            .forward(
                &token_ids_tensor,
                &token_type_ids,
                Some(&attention_mask_tensor),
            )
            .map_err(|e| EmbeddingEngineError::EmbeddingFailed(e.to_string()))?;

        // [batch, seq_len, hidden_size], batch is always 1 here.
        let token_embeddings = output
            .to_vec3::<f32>()
            .map_err(|e| EmbeddingEngineError::EmbeddingFailed(e.to_string()))?
            .into_iter()
            .next()
            .ok_or_else(|| {
                EmbeddingEngineError::EmbeddingFailed("model produced an empty batch".to_string())
            })?;

        let mut pooled = mean_pool(&token_embeddings, attention_mask);
        l2_normalize(&mut pooled);
        Ok(pooled)
    }
}
