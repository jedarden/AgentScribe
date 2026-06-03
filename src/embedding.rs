//! Embedding model clients for vector index
//!
//! Supports both local (Ollama) and cloud (OpenAI) embedding models.

use crate::config::VectorConfig;
use crate::error::{AgentScribeError, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Embedding model type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingModel {
    /// Local Ollama model (nomic-embed-text, 768-dim)
    Ollama { dim: usize },
    /// OpenAI text-embedding-3-small (1536-dim)
    OpenAI { dim: usize },
}

impl EmbeddingModel {
    /// Get the embedding dimension
    pub fn dimension(&self) -> usize {
        match self {
            EmbeddingModel::Ollama { dim } => *dim,
            EmbeddingModel::OpenAI { dim } => *dim,
        }
    }

    /// Parse from config string
    pub fn from_config(s: &str) -> Result<Self> {
        if s.starts_with("openai:") {
            Ok(EmbeddingModel::OpenAI { dim: 1536 })
        } else if s == "nomic-embed-text" || s.starts_with("ollama:") {
            Ok(EmbeddingModel::Ollama { dim: 768 })
        } else {
            // Default to Ollama for unknown models
            Ok(EmbeddingModel::Ollama { dim: 768 })
        }
    }
}

/// Embedding client
pub trait EmbeddingClient: Send + Sync {
    /// Embed a single text
    fn embed(&self, text: &str) -> Result<Vec<f32>>;

    /// Embed multiple texts (batch)
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;

    /// Get the embedding dimension
    fn dimension(&self) -> usize;
}

/// Ollama embedding client (local)
pub struct OllamaClient {
    base_url: String,
    model: String,
    dimension: usize,
    client: reqwest::blocking::Client,
}

impl OllamaClient {
    /// Create a new Ollama client
    pub fn new(base_url: String, model: String) -> Result<Self> {
        let dimension = 768; // nomic-embed-text default dimension

        Ok(OllamaClient {
            base_url,
            model,
            dimension,
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .map_err(|e| {
                    AgentScribeError::VectorIndex(format!("Failed to create HTTP client: {}", e))
                })?,
        })
    }

    /// Check if the model is available
    pub fn check_model(&self) -> Result<bool> {
        let url = format!("{}/api/tags", self.base_url.trim_end_matches('/'));

        let response = self.client.get(&url).send().map_err(|e| {
            AgentScribeError::VectorIndex(format!("Failed to connect to Ollama: {}", e))
        })?;

        if !response.status().is_success() {
            return Ok(false);
        }

        let tags: OllamaTagsResponse = response.json().map_err(|e| {
            AgentScribeError::VectorIndex(format!("Failed to parse Ollama response: {}", e))
        })?;

        Ok(tags
            .models
            .iter()
            .any(|m| m.name == self.model || m.name.starts_with(&format!("{}/", self.model))))
    }
}

impl EmbeddingClient for OllamaClient {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/api/embeddings", self.base_url.trim_end_matches('/'));

        let request = OllamaEmbedRequest {
            model: self.model.clone(),
            input: text.to_string(),
        };

        let response = self.client.post(&url).json(&request).send().map_err(|e| {
            AgentScribeError::VectorIndex(format!("Ollama embed request failed: {}", e))
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .unwrap_or_else(|_| "unable to read error".to_string());
            return Err(AgentScribeError::VectorIndex(format!(
                "Ollama returned error {}: {}",
                status, error_text
            )));
        }

        let embed_response: OllamaEmbedResponse = response.json().map_err(|e| {
            AgentScribeError::VectorIndex(format!("Failed to parse Ollama embed response: {}", e))
        })?;

        Ok(embed_response.embedding)
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut embeddings = Vec::with_capacity(texts.len());
        for text in texts {
            embeddings.push(self.embed(text)?);
        }
        Ok(embeddings)
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

/// OpenAI embedding client (cloud)
pub struct OpenAIClient {
    api_key: String,
    model: String,
    dimension: usize,
    client: reqwest::blocking::Client,
}

impl OpenAIClient {
    /// Create a new OpenAI client
    pub fn new(api_key: String, model: String) -> Result<Self> {
        let dimension = 1536; // text-embedding-3-small dimension

        Ok(OpenAIClient {
            api_key,
            model,
            dimension,
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .map_err(|e| {
                    AgentScribeError::VectorIndex(format!("Failed to create HTTP client: {}", e))
                })?,
        })
    }
}

impl EmbeddingClient for OpenAIClient {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let url = "https://api.openai.com/v1/embeddings";

        let request = OpenAIEmbedRequest {
            model: self.model.clone(),
            input: OpenAIEmbedInput::Single(text.to_string()),
        };

        let response = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .map_err(|e| {
                AgentScribeError::VectorIndex(format!("OpenAI embed request failed: {}", e))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .unwrap_or_else(|_| "unable to read error".to_string());
            return Err(AgentScribeError::VectorIndex(format!(
                "OpenAI returned error {}: {}",
                status, error_text
            )));
        }

        let embed_response: OpenAIEmbedResponse = response.json().map_err(|e| {
            AgentScribeError::VectorIndex(format!("Failed to parse OpenAI embed response: {}", e))
        })?;

        Ok(embed_response.data[0].embedding.clone())
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.len() == 1 {
            return Ok(vec![self.embed(&texts[0])?]);
        }

        let url = "https://api.openai.com/v1/embeddings";

        let input_strings: Vec<String> = texts.to_vec();
        let request = OpenAIEmbedRequest {
            model: self.model.clone(),
            input: OpenAIEmbedInput::Multiple(input_strings),
        };

        let response = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .map_err(|e| {
                AgentScribeError::VectorIndex(format!("OpenAI batch embed failed: {}", e))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .unwrap_or_else(|_| "unable to read error".to_string());
            return Err(AgentScribeError::VectorIndex(format!(
                "OpenAI batch embed error {}: {}",
                status, error_text
            )));
        }

        let embed_response: OpenAIEmbedResponse = response.json().map_err(|e| {
            AgentScribeError::VectorIndex(format!("Failed to parse OpenAI batch response: {}", e))
        })?;

        Ok(embed_response
            .data
            .iter()
            .map(|d| d.embedding.clone())
            .collect())
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

/// Create an embedding client from config
pub fn create_client(config: &VectorConfig) -> Result<Box<dyn EmbeddingClient>> {
    let model_type = EmbeddingModel::from_config(&config.embedding_model)?;

    match model_type {
        EmbeddingModel::Ollama { .. } => {
            // Try OpenAI if API key is available and config requests it
            if config.embedding_model.starts_with("openai:") {
                if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
                    let client = OpenAIClient::new(
                        api_key,
                        config
                            .embedding_model
                            .strip_prefix("openai:")
                            .unwrap_or("text-embedding-3-small")
                            .to_string(),
                    )?;
                    return Ok(Box::new(client));
                }
            }

            // Default to Ollama
            let model = if config.embedding_model.starts_with("ollama:") {
                config
                    .embedding_model
                    .strip_prefix("ollama:")
                    .unwrap_or("nomic-embed-text")
                    .to_string()
            } else {
                config.embedding_model.clone()
            };

            let client = OllamaClient::new(config.ollama_url.clone(), model)?;
            Ok(Box::new(client))
        }
        EmbeddingModel::OpenAI { .. } => {
            let api_key = std::env::var("OPENAI_API_KEY")
                .map_err(|_| AgentScribeError::VectorIndex("OPENAI_API_KEY not set".to_string()))?;

            let model = config
                .embedding_model
                .strip_prefix("openai:")
                .unwrap_or("text-embedding-3-small")
                .to_string();

            let client = OpenAIClient::new(api_key, model)?;
            Ok(Box::new(client))
        }
    }
}

// ── Ollama API types ─────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct OllamaEmbedRequest {
    model: String,
    input: String,
}

#[derive(Debug, Deserialize)]
struct OllamaEmbedResponse {
    embedding: Vec<f32>,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaModel {
    name: String,
}

// ── OpenAI API types ─────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct OpenAIEmbedRequest {
    model: String,
    input: OpenAIEmbedInput,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum OpenAIEmbedInput {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbedResponse {
    data: Vec<OpenAIEmbedData>,
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbedData {
    embedding: Vec<f32>,
    #[allow(dead_code)]
    index: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedding_model_ollama() {
        let model = EmbeddingModel::from_config("nomic-embed-text").unwrap();
        assert_eq!(model.dimension(), 768);
    }

    #[test]
    fn test_embedding_model_openai() {
        let model = EmbeddingModel::from_config("openai:text-embedding-3-small").unwrap();
        assert_eq!(model.dimension(), 1536);
    }

    #[test]
    fn test_embedding_model_default() {
        let model = EmbeddingModel::from_config("unknown-model").unwrap();
        assert_eq!(model.dimension(), 768);
    }
}
